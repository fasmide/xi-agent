use std::sync::Arc;

use crate::{
    auth::AuthStore,
    llm::{
        LlmProvider,
        codex::{CodexProvider, DEFAULT_BASE_URL as CODEX_DEFAULT_BASE_URL},
        copilot,
        ollama::OllamaProvider,
        openai::OpenAiProvider,
    },
};

/// All supported back-end providers, in display order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderKind {
    Copilot,
    OpenAi,
    Codex,
    Ollama,
}

impl ProviderKind {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Copilot => "copilot",
            Self::OpenAi => "openai",
            Self::Codex => "codex",
            Self::Ollama => "ollama",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Copilot => "GitHub Copilot",
            Self::OpenAi => "OpenAI API",
            Self::Codex => "OpenAI Codex (chatgpt.com)",
            Self::Ollama => "Ollama (local)",
        }
    }

    pub fn all() -> &'static [ProviderKind] {
        &[Self::Copilot, Self::OpenAi, Self::Codex, Self::Ollama]
    }

    pub fn from_name(s: &str) -> Option<Self> {
        match s {
            "copilot" | "github-copilot" => Some(Self::Copilot),
            "openai" => Some(Self::OpenAi),
            "codex" => Some(Self::Codex),
            "ollama" => Some(Self::Ollama),
            _ => None,
        }
    }

    /// Sensible default model for this provider.
    pub fn default_model(&self) -> &'static str {
        match self {
            Self::Copilot => "gpt-4o",
            Self::OpenAi => "gpt-4o",
            Self::Codex => "gpt-5.4",
            Self::Ollama => "llama3.1",
        }
    }
}

/// Return the context-window size (in tokens) for a known model name.
/// Returns `None` for unrecognised models.
pub fn context_window_for_model(model: &str) -> Option<usize> {
    // Normalise to lowercase for matching.
    let m = model.to_ascii_lowercase();
    // Check prefixes / substrings for common model families.
    if m.starts_with("o3-mini") {
        return Some(200_000);
    }
    if m.starts_with("o3") {
        return Some(200_000);
    }
    if m.starts_with("o1-mini") {
        return Some(128_000);
    }
    if m.starts_with("o1") {
        return Some(200_000);
    }
    if m.starts_with("gpt-4o") {
        return Some(128_000);
    }
    if m.starts_with("gpt-4-turbo") {
        return Some(128_000);
    }
    if m.starts_with("gpt-4") {
        return Some(8_192);
    }
    if m.starts_with("gpt-3.5-turbo") {
        return Some(16_385);
    }
    if m.starts_with("gpt-5") {
        return Some(200_000);
    }
    if m.contains("claude-3-5") || m.contains("claude-3.5") {
        return Some(200_000);
    }
    if m.contains("claude-3") {
        return Some(200_000);
    }
    if m.contains("claude-2") {
        return Some(100_000);
    }
    if m.contains("llama3") {
        return Some(128_000);
    }
    if m.contains("llama2") {
        return Some(4_096);
    }
    if m.contains("mistral") {
        return Some(32_000);
    }
    if m.contains("gemma") {
        return Some(8_192);
    }
    None
}

/// Build a boxed `LlmProvider` for `kind` with the given model name.
///
/// Returns an error if the required credentials or configuration are missing.
pub fn build_provider(
    kind: &ProviderKind,
    model: &str,
) -> anyhow::Result<Arc<dyn LlmProvider + Send + Sync>> {
    match kind {
        ProviderKind::Copilot => {
            let store = AuthStore::load_default()?;
            let creds = store.get_copilot().ok_or_else(|| {
                anyhow::anyhow!("Not authenticated for copilot. Run /login copilot.")
            })?;
            let p =
                copilot::from_access_token(&creds.access_token, model, creds.base_url.as_deref());
            Ok(Arc::new(p))
        }
        ProviderKind::OpenAi => {
            let p = OpenAiProvider::from_env()?.with_model(model);
            Ok(Arc::new(p))
        }
        ProviderKind::Codex => {
            let store = AuthStore::load_default()?;
            let creds = store
                .get_codex()
                .ok_or_else(|| anyhow::anyhow!("Not authenticated for codex. Run /login codex."))?;
            let base_url = std::env::var("CODEX_BASE_URL")
                .unwrap_or_else(|_| CODEX_DEFAULT_BASE_URL.to_string());
            let p = CodexProvider::new(base_url, model, creds.access_token, creds.account_id);
            Ok(Arc::new(p))
        }
        ProviderKind::Ollama => {
            let base = std::env::var("OLLAMA_BASE_URL")
                .unwrap_or_else(|_| "http://localhost:11434".to_string());
            Ok(Arc::new(OllamaProvider::new(base, model.to_string())))
        }
    }
}
