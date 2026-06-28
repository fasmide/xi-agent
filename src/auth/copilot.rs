use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use serde::Deserialize;

use crate::auth::types::CopilotCredentials;

const CLIENT_ID: &str = "Iv1.b507a08c87ecfe98";

#[derive(Debug, Clone)]
pub enum CopilotLoginEvent {
    DeviceCode {
        verification_uri: String,
        user_code: String,
    },
    Progress(String),
}

#[derive(Debug, Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    interval: u64,
    expires_in: u64,
}

#[derive(Debug, Deserialize)]
struct DeviceTokenResponse {
    access_token: Option<String>,
    error: Option<String>,
    interval: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct CopilotTokenResponse {
    token: String,
    expires_at: i64,
}

fn cancelled(cancel: &Arc<AtomicBool>) -> bool {
    cancel.load(Ordering::Relaxed)
}

pub async fn login(
    on_event: impl Fn(CopilotLoginEvent),
    cancel: Arc<AtomicBool>,
    device_code_url_override: Option<&str>,
    access_token_url_override: Option<&str>,
    copilot_token_url_override: Option<&str>,
) -> anyhow::Result<CopilotCredentials> {
    let client = reqwest::Client::new();
    let device_code_url =
        device_code_url_override.unwrap_or("https://github.com/login/device/code");
    let access_token_url =
        access_token_url_override.unwrap_or("https://github.com/login/oauth/access_token");
    let copilot_token_url =
        copilot_token_url_override.unwrap_or("https://api.github.com/copilot_internal/v2/token");

    let device_body = format!("client_id={CLIENT_ID}&scope=read%3Auser");
    log::debug!("→ POST {device_code_url}");
    let device: DeviceCodeResponse = client
        .post(device_code_url)
        .header("Accept", "application/json")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(device_body)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    on_event(CopilotLoginEvent::DeviceCode {
        verification_uri: device.verification_uri.clone(),
        user_code: device.user_code.clone(),
    });
    log::debug!(
        "copilot device flow started: uri={} interval={} expires_in={}",
        device.verification_uri,
        device.interval,
        device.expires_in
    );

    let mut interval = device.interval.max(2);
    let deadline = std::time::Instant::now() + Duration::from_secs(device.expires_in);

    let github_access = loop {
        if cancelled(&cancel) {
            anyhow::bail!("Login cancelled");
        }
        if std::time::Instant::now() >= deadline {
            anyhow::bail!("GitHub device login timed out");
        }

        tokio::time::sleep(Duration::from_secs(interval)).await;

        let token_body = format!(
            "client_id={CLIENT_ID}&device_code={}&grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Adevice_code",
            urlencoding::encode(&device.device_code)
        );
        log::debug!("→ POST {access_token_url}");
        let token_resp: DeviceTokenResponse = client
            .post(access_token_url)
            .header("Accept", "application/json")
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(token_body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        if let Some(access_token) = token_resp.access_token {
            break access_token;
        }

        match token_resp.error.as_deref() {
            Some("authorization_pending") => {
                on_event(CopilotLoginEvent::Progress(
                    "Waiting for authorization…".to_string(),
                ));
            }
            Some("slow_down") => {
                interval = token_resp
                    .interval
                    .unwrap_or(interval + 5)
                    .max(interval + 1);
                on_event(CopilotLoginEvent::Progress(
                    "GitHub asked to slow down polling…".to_string(),
                ));
            }
            Some(err) => {
                log::debug!("copilot device flow failed: {}", err);
                anyhow::bail!("GitHub device flow failed: {err}")
            }
            None => anyhow::bail!("GitHub device flow failed without an error code"),
        }
    };

    log::debug!("→ GET {copilot_token_url}");
    let copilot: CopilotTokenResponse = client
        .get(copilot_token_url)
        .header("Accept", "application/json")
        .header("Authorization", format!("Bearer {github_access}"))
        .header("User-Agent", "GitHubCopilotChat/0.35.0")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let base_url = extract_base_url(&copilot.token);

    log::debug!("copilot login token exchange succeeded");
    Ok(CopilotCredentials {
        access_token: copilot.token,
        refresh_token: github_access,
        expires_at: copilot.expires_at,
        base_url,
    })
}

pub async fn refresh(
    refresh_token: &str,
    copilot_token_url_override: Option<&str>,
) -> anyhow::Result<CopilotCredentials> {
    let client = reqwest::Client::new();
    let copilot_token_url =
        copilot_token_url_override.unwrap_or("https://api.github.com/copilot_internal/v2/token");
    log::debug!("→ GET {copilot_token_url} (refresh)");
    let copilot: CopilotTokenResponse = client
        .get(copilot_token_url)
        .header("Accept", "application/json")
        .header("Authorization", format!("Bearer {refresh_token}"))
        .header("User-Agent", "GitHubCopilotChat/0.35.0")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    log::debug!("copilot token refresh succeeded");
    Ok(CopilotCredentials {
        base_url: extract_base_url(&copilot.token),
        access_token: copilot.token,
        refresh_token: refresh_token.to_string(),
        expires_at: copilot.expires_at,
    })
}

fn extract_base_url(token: &str) -> Option<String> {
    let domain = token
        .split(';')
        .find_map(|segment| segment.strip_prefix("proxy-ep="))?;

    let api_domain = domain
        .strip_prefix("proxy.")
        .map(|rest| format!("api.{rest}"))
        .unwrap_or_else(|| domain.to_string());
    Some(format!("https://{api_domain}"))
}

#[cfg(test)]
mod tests {
    use super::extract_base_url;

    #[test]
    fn extract_base_url_maps_proxy_prefix_to_api_subdomain() {
        let token = "abc;proxy-ep=proxy.business.githubcopilot.com;xyz";
        let base = extract_base_url(token);
        assert_eq!(
            base,
            Some("https://api.business.githubcopilot.com".to_string())
        );
    }

    #[test]
    fn extract_base_url_uses_domain_as_is_when_not_proxy_prefixed() {
        let token = "k=v;proxy-ep=enterprise.githubcopilot.com";
        let base = extract_base_url(token);
        assert_eq!(
            base,
            Some("https://enterprise.githubcopilot.com".to_string())
        );
    }

    #[test]
    fn extract_base_url_returns_none_when_segment_missing() {
        assert_eq!(extract_base_url("foo=bar;baz=qux"), None);
    }

    // ── Wiremock refresh tests ────────────────────────────────────────────────

    use super::refresh;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };

    fn mock_copilot_token_response(access_token: &str, expires_at: i64) -> String {
        format!(
            r#"{{"token":"{}","expires_at":{}}}"#,
            access_token, expires_at
        )
    }

    #[tokio::test]
    async fn copilot_refresh_success() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/copilotjsdkf/token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(mock_copilot_token_response("fresh-token", 9999999999)),
            )
            .mount(&mock_server)
            .await;

        let url = format!("{}/copilotjsdkf/token", mock_server.uri());
        let result = refresh("old-refresh-token", Some(&url)).await;
        assert!(result.is_ok(), "refresh should succeed: {:?}", result.err());

        let creds = result.unwrap();
        assert_eq!(creds.access_token, "fresh-token");
        assert_eq!(creds.expires_at, 9999999999);
    }

    #[tokio::test]
    async fn copilot_refresh_http_500_returns_error() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/copilotjsdkf/token"))
            .respond_with(ResponseTemplate::new(500).set_body_string("internal error"))
            .mount(&mock_server)
            .await;

        let url = format!("{}/copilotjsdkf/token", mock_server.uri());
        let result = refresh("old-refresh-token", Some(&url)).await;
        assert!(result.is_err(), "refresh should fail on 500");
    }

    #[tokio::test]
    async fn copilot_refresh_malformed_json_returns_error() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/copilotjsdkf/token"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not valid json {{{"))
            .mount(&mock_server)
            .await;

        let url = format!("{}/copilotjsdkf/token", mock_server.uri());
        let result = refresh("old-refresh-token", Some(&url)).await;
        assert!(result.is_err(), "refresh should fail on malformed JSON");
    }
}
