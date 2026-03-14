# Agent Loop — Implementation Plan

**Date:** 2026-03-14
**Design:** [2026-03-14-agent-loop-design.md](2026-03-14-agent-loop-design.md)

Ordered by dependency. Each task is self-contained and buildable before the next.

---

## Task 1 — `Cargo.toml`: add `tokio/process` feature

The bash tool needs `tokio::process::Command`. Add `"process"` to tokio's feature list.

---

## Task 2 — `llm/mod.rs`: extend types for tool-calling

**a. Extend `LlmEvent`** — add `ToolCall { id: String, name: String, args: serde_json::Value }`.

**b. Add `ToolDefinition`** — a plain struct holding `name`, `description`,
and `parameters` (a `serde_json::Value` JSON Schema object). Derive
`Clone` so it can be moved into `Box::pin(async_stream::stream! {...})`.

**c. Add `stream_chat_with_tools` to `LlmProvider`** — takes
`Vec<Message>` and `Vec<ToolDefinition>` (owned, not borrowed, to keep the
return type `'static`). Default impl ignores tools and delegates to
`stream_chat`.

**d. Extend `Role`** — add `ToolCall` and `ToolResult` variants.

**e. Extend `Message`** — add optional fields:
```rust
pub tool_call_id: Option<String>,   // for ToolCall + ToolResult
pub tool_name:    Option<String>,   // for ToolCall
pub tool_args:    Option<serde_json::Value>, // for ToolCall
pub is_error:     bool,             // for ToolResult
```
Add constructor helpers `Message::user()`, `Message::assistant()`,
`Message::system()`, `Message::tool_call()`, `Message::tool_result()`.

---

## Task 3 — `llm/ollama.rs`: implement `stream_chat_with_tools`

**a. Extend `OllamaMessage`** — add optional `tool_calls` field for
serialisation of assistant tool-call turns.

**b. Add Ollama request/response serde types** for tools:
- `OllamaToolDef` / `OllamaFunction` (request)
- `ToolCallChunk` / `ToolCallFunction` (response chunk)
- Extend `ChunkMessage` with `tool_calls: Option<Vec<ToolCallChunk>>`

**c. Update `to_ollama_message`** — handle `Role::ToolCall` (→ assistant
with `tool_calls` array) and `Role::ToolResult` (→ role `"tool"`).

**d. Add `ChatRequestWithTools`** serde struct (same as `ChatRequest` plus
`tools` field).

**e. Implement `stream_chat_with_tools`** — same streaming approach as
`stream_chat` but:
- Uses `ChatRequestWithTools` with tools serialised as Ollama's
  `{ type: "function", function: { name, description, parameters } }` shape.
- On each parsed chunk: if `message.tool_calls` is non-empty, yield
  `LlmEvent::ToolCall` events; otherwise yield `LlmEvent::Token` as normal.

---

## Task 4 — `agent/types.rs`: core agent types

Create `src/agent/types.rs` with:
- `ToolResult { content: String, is_error: bool }`
- `Tool` trait: `name()`, `description()`, `parameters_schema()`,
  `async fn execute(&self, args: serde_json::Value) -> ToolResult`
  (uses stable async fn in traits, Rust edition 2024, no `async-trait` crate)
- `type ToolRegistry = std::collections::HashMap<String, std::sync::Arc<dyn Tool>>`
- `AgentEvent` enum (TextToken, ThinkingToken, ToolCallStart, ToolCallEnd,
  TurnEnd, Done, Error)
- `AgentLoopConfig` struct (tools, before_tool_call hook, after_tool_call
  hook, max_turns)

---

## Task 5 — `agent/tools/read.rs`: ReadFileTool

Parameters (JSON Schema): `path: string`, `offset: integer (optional)`,
`limit: integer (optional)`.

Reads the file at `path`. If `offset` is given, skips that many lines (1-indexed).
If `limit` is given, caps at that many lines. Returns the content as a string
with a header `[lines X-Y of Z]` when truncated.

Errors: file not found, permission denied, invalid UTF-8.

---

## Task 6 — `agent/tools/write.rs`: WriteTool

Parameters: `path: string`, `content: string`.

Creates parent directories as needed (`tokio::fs::create_dir_all`), then
writes the file. Returns a short confirmation string.

Errors: permission denied, invalid path.

---

## Task 7 — `agent/tools/edit.rs`: EditTool

Parameters: `path: string`, `old_text: string`, `new_text: string`.

Reads the file, finds the first occurrence of `old_text` (exact match,
including whitespace), replaces with `new_text`, writes back.

Errors: file not found, `old_text` not found (with a hint), multiple
occurrences found (also an error — `old_text` must be unambiguous).

---

## Task 8 — `agent/tools/bash.rs`: BashTool

Parameters: `command: string`.

Runs `command` via `tokio::process::Command::new("sh").arg("-c").arg(command)`,
captures stdout and stderr, waits for exit. Returns:
```
exit 0
stdout:
<stdout>
stderr:
<stderr>
```
Truncates stdout/stderr to 8 KiB each before returning. `is_error: false`
regardless of exit code (the model sees the exit code and decides).

Errors: failed to spawn (e.g., `sh` not found).

---

## Task 9 — `agent/tools/mod.rs`: `register_builtin_tools`

```rust
pub fn register_builtin_tools() -> ToolRegistry { ... }
```
Instantiates all four tools and inserts into a `ToolRegistry`.

---

## Task 10 — `agent/mod.rs`: `run_agent_loop`

```rust
pub async fn run_agent_loop(
    mut messages: Vec<llm::Message>,
    config: AgentLoopConfig,
    provider: Arc<dyn LlmProvider>,
    tx: UnboundedSender<AgentEvent>,
)
```

Turn loop as described in the design doc:
1. Build `Vec<ToolDefinition>` from `config.tools` registry.
2. Call `provider.stream_chat_with_tools(messages.clone(), tool_defs)`.
3. Collect stream: text tokens → emit + append to current assistant message;
   tool calls → collect into `pending`.
4. If `pending` is empty → emit `TurnEnd` + `Done`, return.
5. For each pending tool call:
   - Emit `ToolCallStart`.
   - Run `before_tool_call` hook; block if returns false.
   - Look up tool in registry; execute.
   - Run `after_tool_call` hook; override result if hook returns `Some`.
   - Emit `ToolCallEnd`.
   - Append `Message::tool_call(...)` + `Message::tool_result(...)` to `messages`.
6. Emit `TurnEnd`, continue loop.
7. After `max_turns` → emit `Error("max turns reached")`.

---

## Task 11 — `app.rs`: wire AgentEvent

**a.** Change `event_rx` / `event_tx` types from `LlmEvent` to `AgentEvent`.

**b.** Remove the pre-appended empty assistant `Message` from `submit()`.
The agent loop drives message creation via events.

**c.** Change `submit()` to call `agent::run_agent_loop` instead of
`provider.stream_chat`.

`submit()` takes the provider and a pre-built `AgentLoopConfig` (passed in
from `main.rs` or stored in `App`). For now, store `AgentLoopConfig` in `App`.

**d.** Update `apply_event` to handle `AgentEvent`:
- `TextToken(t)` → if last message is `Role::Assistant`, append `t`; else
  push a new empty assistant message then append.
- `ThinkingToken(t)` → same pattern with `.thinking`.
- `ToolCallStart { name, args }` → push `Message::tool_call("", &name, args)`.
- `ToolCallEnd { name, result }` → push `Message::tool_result("", &result.content, result.is_error)`.
- `TurnEnd` → no-op (reserved for future).
- `Done` → `self.streaming = false`.
- `Error(e)` → push error message (or amend last), `self.streaming = false`.

---

## Task 12 — `ui.rs`: render tool roles

In `build_log_lines`, add arms for `Role::ToolCall` and `Role::ToolResult`:

- `ToolCall` → dim cyan line: `⚙ <name>(<args_preview>)` where `args_preview`
  is the JSON args truncated to ~60 chars.
- `ToolResult` → dim green (or red if `is_error`) line: `↳ <content_preview>`,
  content truncated to ~200 chars; wrap normally.

---

## Task 13 — `main.rs`: build `AgentLoopConfig` and pass to `App`

In `main`, after constructing `App`, build the config:
```rust
let config = AgentLoopConfig {
    tools: register_builtin_tools(),
    before_tool_call: None,
    after_tool_call: None,
    max_turns: 20,
};
app.agent_config = config;
```

`App` stores `agent_config: AgentLoopConfig` (added in Task 11). Add `mod agent` to `main.rs`.

---

## Build order

```
1 → 2 → 3 → 4 → 5 → 6 → 7 → 8 → 9 → 10 → 11 → 12 → 13
```

Tasks 5-9 can be done in any order after Task 4. All compile-check after
Task 13 is complete.
