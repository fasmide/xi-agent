use tokio::sync::mpsc::UnboundedSender;

/// A single message in the conversation history.
#[derive(Debug, Clone)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Role {
    System,
    User,
    Assistant,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
        }
    }
}

/// Events sent from a streaming LLM task back to the UI.
#[derive(Debug)]
pub enum LlmEvent {
    /// A token chunk from the model.
    Token(String),
    /// The stream finished successfully.
    Done,
    /// The request failed.
    Error(String),
}

/// Trait every LLM backend must implement.
pub trait LlmProvider: Send + Sync {
    fn stream_chat(
        &self,
        messages: &[Message],
        tx: UnboundedSender<LlmEvent>,
    ) -> impl std::future::Future<Output = anyhow::Result<()>> + Send;
}

pub mod ollama;
