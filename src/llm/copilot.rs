use super::codex::CodexProvider;
use super::openai::OpenAiProvider;

/// Build an `OpenAiProvider` configured for GitHub Copilot's proxy API.
///
/// The Copilot access token encodes the proxy endpoint in the format:
/// `...;proxy-ep=proxy.individual.githubcopilot.com;...`
/// which maps to the API base URL `https://api.individual.githubcopilot.com`.
pub fn from_access_token(
    access_token: &str,
    model: &str,
    base_url: Option<&str>,
) -> OpenAiProvider {
    let resolved_base_url = base_url
        .map(|s| s.to_string())
        .unwrap_or_else(|| extract_base_url(access_token));

    let extra_headers = vec![
        (
            "User-Agent".to_string(),
            "GitHubCopilotChat/0.35.0".to_string(),
        ),
        ("Editor-Version".to_string(), "vscode/1.107.0".to_string()),
        (
            "Editor-Plugin-Version".to_string(),
            "copilot-chat/0.35.0".to_string(),
        ),
        (
            "Copilot-Integration-Id".to_string(),
            "vscode-chat".to_string(),
        ),
        ("X-Initiator".to_string(), "user".to_string()),
        (
            "Openai-Intent".to_string(),
            "conversation-edits".to_string(),
        ),
    ];

    OpenAiProvider::new_with_headers(resolved_base_url, model, access_token, extra_headers)
}

/// Build a `CodexProvider` (Responses API) configured for GitHub Copilot's proxy.
///
/// Codex models (e.g. `gpt-5.3-codex`) are not accessible via `/chat/completions`
/// through the Copilot proxy; they require the OpenAI Responses API (`/v1/responses`).
pub fn codex_from_access_token(
    access_token: &str,
    model: &str,
    base_url: Option<&str>,
) -> CodexProvider {
    let base = base_url
        .map(|s| s.to_string())
        .unwrap_or_else(|| extract_base_url(access_token));

    // Append the Responses API path so CodexProvider hits the right endpoint.
    let responses_url = format!("{}/v1/responses", base.trim_end_matches('/'));

    let extra_headers = vec![
        (
            "User-Agent".to_string(),
            "GitHubCopilotChat/0.35.0".to_string(),
        ),
        ("Editor-Version".to_string(), "vscode/1.107.0".to_string()),
        (
            "Editor-Plugin-Version".to_string(),
            "copilot-chat/0.35.0".to_string(),
        ),
        (
            "Copilot-Integration-Id".to_string(),
            "vscode-chat".to_string(),
        ),
        ("X-Initiator".to_string(), "user".to_string()),
        (
            "Openai-Intent".to_string(),
            "conversation-edits".to_string(),
        ),
    ];

    CodexProvider::new_with_headers(responses_url, model, access_token, extra_headers)
}


/// Falls back to the known default if the field is absent.
fn extract_base_url(token: &str) -> String {
    if let Some(domain) = token
        .split(';')
        .find_map(|seg| seg.strip_prefix("proxy-ep="))
    {
        // "proxy.individual.githubcopilot.com" → "api.individual.githubcopilot.com"
        let api_domain = domain
            .strip_prefix("proxy.")
            .map(|rest| format!("api.{rest}"))
            .unwrap_or_else(|| domain.to_string());
        return format!("https://{api_domain}");
    }
    "https://api.individual.githubcopilot.com".to_string()
}
