use super::openai::OpenAiProvider;

/// Build an `OpenAiProvider` configured for GitHub Copilot's proxy API.
///
/// Reads credentials from `~/.pi/agent/auth.json` (`github-copilot` entry).
/// The Copilot access token encodes the proxy endpoint in the format:
///   `...;proxy-ep=proxy.individual.githubcopilot.com;...`
/// which maps to the API base URL `https://api.individual.githubcopilot.com`.
pub fn from_env() -> anyhow::Result<OpenAiProvider> {
    let (token, base_url) = read_copilot_auth()?;
    let model = std::env::var("COPILOT_MODEL")
        .or_else(|_| std::env::var("OPENAI_MODEL"))
        .unwrap_or_else(|_| "gpt-4o".to_string());

    let extra_headers = vec![
        ("User-Agent".to_string(),            "GitHubCopilotChat/0.35.0".to_string()),
        ("Editor-Version".to_string(),        "vscode/1.107.0".to_string()),
        ("Editor-Plugin-Version".to_string(), "copilot-chat/0.35.0".to_string()),
        ("Copilot-Integration-Id".to_string(),"vscode-chat".to_string()),
        ("X-Initiator".to_string(),           "user".to_string()),
        ("Openai-Intent".to_string(),         "conversation-edits".to_string()),
    ];

    Ok(OpenAiProvider::new_with_headers(base_url, model, token, extra_headers))
}

fn read_copilot_auth() -> anyhow::Result<(String, String)> {
    let home = std::env::var("HOME").map_err(|_| anyhow::anyhow!("$HOME not set"))?;
    let path = std::path::Path::new(&home).join(".pi/agent/auth.json");
    let content = std::fs::read_to_string(&path)
        .map_err(|e| anyhow::anyhow!("Cannot read {}: {}", path.display(), e))?;
    let v: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| anyhow::anyhow!("Cannot parse auth.json: {}", e))?;

    let token = v["github-copilot"]["access"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No github-copilot.access in auth.json"))?
        .to_string();

    let base_url = extract_base_url(&token);
    Ok((token, base_url))
}

/// Parse `proxy-ep=proxy.X.Y.Z` from the token and return `https://api.X.Y.Z`.
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
