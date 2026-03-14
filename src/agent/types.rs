use std::{collections::HashMap, sync::Arc};

// ── Tool result ───────────────────────────────────────────────────────────────

/// The output produced by a tool execution.
#[derive(Debug, Clone)]
pub struct ToolResult {
    /// Text content returned to the model.
    pub content: String,
    /// True when the tool encountered an error.
    pub is_error: bool,
}

impl ToolResult {
    pub fn ok(content: impl Into<String>) -> Self {
        Self { content: content.into(), is_error: false }
    }

    pub fn err(content: impl Into<String>) -> Self {
        Self { content: content.into(), is_error: true }
    }
}

// ── Tool trait ────────────────────────────────────────────────────────────────

/// A tool the agent can invoke.
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    /// Emoji / short label used for display in the UI (defaults to `name()`).
    fn label(&self) -> &str { self.name() }
    /// JSON Schema object describing the tool's input parameters.
    fn parameters_schema(&self) -> serde_json::Value;
    /// Execute the tool with the given arguments (JSON object).
    fn execute(
        &self,
        args: serde_json::Value,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ToolResult> + Send + '_>>;
}

/// A registry mapping tool names to their implementations.
pub type ToolRegistry = HashMap<String, Arc<dyn Tool>>;

// ── Agent events ──────────────────────────────────────────────────────────────

/// Events emitted by the agent loop to `App` over a tokio channel.
#[derive(Debug)]
pub enum AgentEvent {
    // ── LLM streaming ─────────────────────────────────────────────────────────
    /// A text token chunk from the model's answer.
    TextToken(String),
    /// A token chunk from the model's thinking / chain-of-thought block.
    ThinkingToken(String),
    // ── Tool lifecycle ─────────────────────────────────────────────────────────
    /// The model requested a tool call; execution is about to begin.
    ToolCallStart { id: String, name: String, args: serde_json::Value },
    /// A tool call finished; contains the result.
    ToolCallEnd { id: String, name: String, result: ToolResult },
    // ── Loop lifecycle ─────────────────────────────────────────────────────────
    /// One LLM turn (assistant response + any tool calls) is complete.
    TurnEnd,
    /// The agent loop finished successfully.
    Done,
    /// The agent loop encountered a fatal error.
    Error(String),
}

// ── Agent loop configuration ──────────────────────────────────────────────────

/// Configuration passed to `run_agent_loop`.
pub struct AgentLoopConfig {
    /// Tools available to the model.
    pub tools: ToolRegistry,
    /// Optional hook called before each tool execution.
    /// Return `false` to block the tool call (an error result is returned instead).
    pub before_tool_call: Option<Box<dyn Fn(&str, &serde_json::Value) -> bool + Send + Sync>>,
    /// Optional hook called after each tool execution.
    /// Return `Some(result)` to override the tool's result.
    pub after_tool_call: Option<Box<dyn Fn(&str, &ToolResult) -> Option<ToolResult> + Send + Sync>>,
    /// Maximum number of LLM turns before the loop stops with an error.
    pub max_turns: usize,
}
