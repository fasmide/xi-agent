use std::sync::Arc;

use crate::llm::{
    LlmProvider,
    copilot,
    openai::OpenAiProvider,
    codex::CodexProvider,
    ollama::OllamaProvider,
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
            Self::OpenAi  => "openai",
            Self::Codex   => "codex",
            Self::Ollama  => "ollama",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Copilot => "GitHub Copilot",
            Self::OpenAi  => "OpenAI API",
            Self::Codex   => "OpenAI Codex (chatgpt.com)",
            Self::Ollama  => "Ollama (local)",
        }
    }

    pub fn all() -> &'static [ProviderKind] {
        &[Self::Copilot, Self::OpenAi, Self::Codex, Self::Ollama]
    }

    pub fn from_name(s: &str) -> Option<Self> {
        match s {
            "copilot" | "github-copilot" => Some(Self::Copilot),
            "openai"                     => Some(Self::OpenAi),
            "codex"                      => Some(Self::Codex),
            "ollama"                     => Some(Self::Ollama),
            _                            => None,
        }
    }

    /// Sensible default model for this provider.
    pub fn default_model(&self) -> &'static str {
        match self {
            Self::Copilot => "gpt-4o",
            Self::OpenAi  => "gpt-4o",
            Self::Codex   => "gpt-5.4",
            Self::Ollama  => "llama3.1",
        }
    }
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
            let p = copilot::from_env()?.with_model(model);
            Ok(Arc::new(p))
        }
        ProviderKind::OpenAi => {
            let p = OpenAiProvider::from_env()?.with_model(model);
            Ok(Arc::new(p))
        }
        ProviderKind::Codex => {
            let p = CodexProvider::from_env()?.with_model(model);
            Ok(Arc::new(p))
        }
        ProviderKind::Ollama => {
            let base = std::env::var("OLLAMA_BASE_URL")
                .unwrap_or_else(|_| "http://localhost:11434".to_string());
            Ok(Arc::new(OllamaProvider::new(base, model.to_string())))
        }
    }
}
