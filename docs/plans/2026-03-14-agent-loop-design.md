# Agent Loop Design

**Date:** 2026-03-14
**Status:** Approved

## Overview

Implement the agentic turn loop that lets the LLM either respond with text or
activate one or more tools, then continue the conversation with the tool
results. The design follows the pi-mono agent architecture: a separated
`agent/` module with a clean `Tool` trait, a typed `AgentEvent` channel, and
an `AgentLoopConfig` struct that carries hooks and settings — all decoupled
from the TUI.

---

## Module Structure

```
src/
  agent/
    mod.rs        — AgentLoop, AgentLoopConfig, run_agent_loop()
    types.rs      — Tool trait, ToolRegistry, AgentEvent, AgentMessage
    tools/
      mod.rs      — register_builtin_tools()
      read.rs     — ReadFileTool
      write.rs    — WriteTool
      edit.rs     — EditTool
      bash.rs     — BashTool
  llm/
    mod.rs        — LlmProvider extended with stream_chat_with_tools + ToolCall LlmEvent
    ollama.rs     — OllamaProvider updated for tool schemas + tool_call responses
  app.rs          — receives AgentEvent, updates display state
```

`agent/` is a new first-class module. `llm/` receives surgical additions only.

---

## Core Types (`agent/types.rs`)

### `Tool` trait

```rust
pub struct ToolResult {
    pub content: String,   // text returned to the model
    pub is_error: bool,
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> serde_json::Value;  // JSON Schema object
    async fn execute(&self, args: serde_json::Value) -> ToolResult;
}

pub type ToolRegistry = HashMap<String, Arc<dyn Tool>>;
```

### `AgentEvent`

Sent from the agent loop task to `App` over a tokio `UnboundedSender`.

```rust
pub enum AgentEvent {
    // LLM streaming
    TextToken(String),
    ThinkingToken(String),
    // Tool lifecycle
    ToolCallStart { name: String, args: serde_json::Value },
    ToolCallEnd   { name: String, result: ToolResult },
    // Loop lifecycle
    TurnEnd,
    Done,
    Error(String),
}
```

### `AgentLoopConfig`

```rust
pub struct AgentLoopConfig {
    /// Registered tools available to the model.
    pub tools: ToolRegistry,
    /// Optional pre-execution hook. Return false to block the call.
    pub before_tool_call: Option<Box<dyn Fn(&str, &serde_json::Value) -> bool + Send + Sync>>,
    /// Optional post-execution hook. Return Some(result) to override the result.
    pub after_tool_call: Option<Box<dyn Fn(&str, &ToolResult) -> Option<ToolResult> + Send + Sync>>,
    /// Safety cap: maximum number of LLM turns before stopping. Default: 20.
    pub max_turns: usize,
}
```

---

## LLM Layer Changes (`llm/mod.rs`)

### New `LlmEvent` variant

```rust
pub enum LlmEvent {
    ThinkingToken(String),
    Token(String),
    ToolCall { id: String, name: String, args: serde_json::Value },
    Done,
    Error(String),
}
```

### New `LlmProvider` method

```rust
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,  // JSON Schema object
}

pub trait LlmProvider: Send + Sync {
    fn stream_chat(&self, messages: Vec<Message>) -> LlmStream;             // unchanged

    fn stream_chat_with_tools(
        &self,
        messages: Vec<Message>,
        tools: &[ToolDefinition],
    ) -> LlmStream;                                                          // new

    fn list_models(&self) -> ModelListFuture;                               // unchanged
}
```

`OllamaProvider` implements `stream_chat_with_tools` by:
- Serialising `tools` into Ollama's `tools: [...]` request field.
- Handling the non-streaming `tool_calls` path in the response (Ollama
  returns tool calls in a single non-streamed message, then switches back to
  streaming for subsequent turns).
- Yielding `LlmEvent::ToolCall` for each call.

---

## Agent Loop Flow (`agent/mod.rs`)

`run_agent_loop` is an `async fn` spawned as a detached tokio task. It owns
the full conversation history for the duration of the run.

```
run_agent_loop(messages, config, provider, tx: AgentEventSender)
  loop (turn = 0..config.max_turns):
    stream = provider.stream_chat_with_tools(messages, tools_from_registry)
    pending_tool_calls = []

    for event in stream:
      TextToken(t)      → tx.send(TextToken), append to current assistant msg
      ThinkingToken(t)  → tx.send(ThinkingToken)
      ToolCall(c)       → push c onto pending_tool_calls
      Done              → break inner loop
      Error(e)          → tx.send(Error), return

    if pending_tool_calls.is_empty():
      tx.send(TurnEnd)
      tx.send(Done)
      return                        ← model gave a final text answer, stop

    // Execute tool calls sequentially
    for call in pending_tool_calls:
      tx.send(ToolCallStart { name, args })

      if let Some(f) = config.before_tool_call:
        if !f(&call.name, &call.args):
          result = ToolResult { content: "blocked", is_error: true }
          tx.send(ToolCallEnd { name, result })
          append ToolResult message to messages
          continue

      result = registry[call.name].execute(call.args).await
               or ToolResult { content: "tool not found", is_error: true }

      if let Some(f) = config.after_tool_call:
        result = f(&call.name, &result).unwrap_or(result)

      tx.send(ToolCallEnd { name, result.clone() })
      append ToolResult message to messages

    tx.send(TurnEnd)
    // loop continues for next LLM turn

  // Reached max_turns without a final answer
  tx.send(Error("max turns reached"))
```

---

## Built-in Tools (`agent/tools/`)

| Tool | File | Parameters | Behaviour |
|------|------|------------|-----------|
| **read** | `read.rs` | `path: String` | Read file contents; optional `offset` and `limit` (line numbers) |
| **write** | `write.rs` | `path: String, content: String` | Write file, creating parent directories as needed |
| **edit** | `edit.rs` | `path: String, old_text: String, new_text: String` | Find exact `old_text`, replace with `new_text`; error if not found |
| **bash** | `bash.rs` | `command: String` | Run via `/bin/sh -c`, return stdout + stderr + exit code |

All tools return `is_error: true` with a descriptive message on failure.

`register_builtin_tools()` in `tools/mod.rs` instantiates all four and returns a `ToolRegistry`.

---

## App Integration (`app.rs`)

Changes to `App` are minimal:

1. `event_rx` / `event_tx` change from `LlmEvent` to `AgentEvent`.
2. `apply_event` maps `AgentEvent` to message mutations:
   - `TextToken` / `ThinkingToken` → same as today (append to last assistant message).
   - `ToolCallStart` → push a `Message { role: Role::ToolCall, ... }`.
   - `ToolCallEnd` → push a `Message { role: Role::ToolResult, ... }`.
   - `Done` / `Error` → set `streaming = false` (same as today).
3. `submit()` calls `run_agent_loop(...)` instead of `provider.stream_chat(...)`.
4. Two new `Role` variants: `ToolCall` and `ToolResult`.

**Display:** tool call messages render as `[tool: bash] ls -la` and result
messages as `[result] exit 0 / 3 lines` (inline, collapsed; expandable later).

---

## Error Handling

- Tool `execute` never panics; all errors are captured as `ToolResult { is_error: true }`.
- `stream_chat_with_tools` never panics; failures yield `LlmEvent::Error`.
- `run_agent_loop` never panics; all errors are forwarded via `AgentEvent::Error`.
- `max_turns` prevents infinite tool-call loops.

---

## Out of Scope (this iteration)

- Parallel tool execution (sequential only for now).
- User confirmation prompts before tool execution.
- Tool output truncation / context-window management.
- Additional providers beyond Ollama.
