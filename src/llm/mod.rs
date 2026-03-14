use async_trait::async_trait;
use tokio::sync::mpsc::UnboundedSender;

/// A single message in the conversation history.
#[derive(Debug, Clone)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Role {
    User,
    Assistant,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::User => "user",
            Role::Assistant => "assistant",
        }
    }
}

/// Events sent from a streaming LLM task back to the UI.
#[derive(Debug)]
pub enum AppEvent {
    /// A token chunk from the model.
    Token(String),
    /// The stream finished successfully.
    Done,
    /// The request failed.
    Error(String),
}

/// Trait every LLM backend must implement.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn stream_chat(
        &self,
        messages: &[Message],
        tx: UnboundedSender<AppEvent>,
    ) -> anyhow::Result<()>;
}

pub mod ollama;
