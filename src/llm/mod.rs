use std::pin::Pin;

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

/// Events emitted by a streaming LLM response.
#[derive(Debug)]
pub enum LlmEvent {
    /// A token chunk from the model.
    Token(String),
    /// The stream finished successfully.
    Done,
    /// The request failed.
    Error(String),
}

/// A boxed, heap-allocated stream of `LlmEvent`s that is `Send` and `'static`,
/// suitable for passing across thread boundaries and storing in `App`.
pub type LlmStream = Pin<Box<dyn futures_util::Stream<Item = LlmEvent> + Send + 'static>>;

/// Trait every LLM backend must implement.
///
/// `stream_chat` returns an `LlmStream` rather than accepting a channel
/// sender. This decouples the trait from any specific async runtime primitive
/// and makes implementors independently testable by collecting the stream.
pub trait LlmProvider: Send + Sync {
    fn stream_chat(&self, messages: Vec<Message>) -> LlmStream;
}

pub mod ollama;
