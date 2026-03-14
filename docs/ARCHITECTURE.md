# Architecture

## Purpose

`pirs` is a terminal AI agent harness. It provides a streaming TUI for
conversational interaction with LLMs and runs the full agentic loop: user
message → model response → tool call → tool result → model continues, for
up to `max_turns` iterations.

## Module Map

```
src/
  main.rs            — tokio entry point, CLI parsing, outer provider loop
  app.rs             — App state, event handling, submission, scroll
  ui.rs              — all ratatui rendering, pre-wrapping, scroll logic
  commands.rs        — slash-command registry and completion items
  provider.rs        — ProviderKind enum, build_provider(), context-window table
  agent/
    mod.rs           — run_agent_loop: the multi-turn agentic loop
    types.rs         — Tool trait, ToolRegistry, AgentEvent, AgentLoopConfig
    system_prompt.rs — build_system_prompt: dynamic system prompt
    tools/
      mod.rs         — register_builtin_tools()
      bash.rs        — BashTool  (💻 run shell command)
      read.rs        — ReadFileTool (👀 read file with offset/limit)
      write.rs       — WriteTool (✍️ write/overwrite file)
      edit.rs        — EditTool  (📝 replace exact text in file)
      find.rs        — FindTool  (🔍 search by name glob or content pattern)
  llm/
    mod.rs           — LlmProvider trait, Message/Role/LlmEvent/ToolDefinition types
    openai.rs        — OpenAiProvider (OpenAI Chat Completions, tool-calling)
    copilot.rs       — CopilotProvider (thin wrapper around OpenAiProvider)
    codex.rs         — CodexProvider (chatgpt.com backend)
    ollama.rs        — OllamaProvider (streaming NDJSON via /api/chat)
```

## Data Flow

```
User keystroke → App::submit
  └─ spawns tokio task: run_agent_loop(messages, config, provider, tx)
       └─ for each turn:
            provider.stream_chat_with_tools(messages, tool_defs)
              └─ yields LlmEvent::{Token, ThinkingToken, ToolCall, Done, Error}
            if ToolCall → tool.execute(args) → ToolResult
            loop until no tool calls or max_turns
            sends AgentEvent::{TextToken, ThinkingToken, ToolCallStart,
                               ToolCallEnd, TurnEnd, Done, Error} on tx
  App::apply_event drains tx on each draw tick → updates messages vec
  ui::draw renders messages vec to terminal
```

## Key Types

### `llm/mod.rs`

```rust
pub struct Message {
    pub role: Role,                      // System | User | Assistant | ToolCall | ToolResult
    pub content: String,
    pub thinking: Option<String>,        // chain-of-thought block
    pub tool_call_id: Option<String>,
    pub tool_name: Option<String>,
    pub tool_args: Option<serde_json::Value>,
    pub is_error: bool,
}

pub enum LlmEvent {
    Token(String),
    ThinkingToken(String),
    ToolCall { id: String, name: String, args: serde_json::Value },
    Done,
    Error(String),
}

pub trait LlmProvider: Send + Sync {
    fn stream_chat(&self, messages: Vec<Message>) -> LlmStream;
    fn stream_chat_with_tools(&self, messages: Vec<Message>, tools: Vec<ToolDefinition>) -> LlmStream;
    fn list_models(&self) -> ModelListFuture;  // default: returns []
}
```

### `agent/types.rs`

```rust
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn label(&self) -> &str;                          // emoji / display label
    fn parameters_schema(&self) -> serde_json::Value; // JSON Schema object
    fn execute(&self, args: serde_json::Value) -> Pin<Box<dyn Future<Output = ToolResult> + Send + '_>>;
}

pub enum AgentEvent {
    TextToken(String),
    ThinkingToken(String),
    ToolCallStart { id, name, args },
    ToolCallEnd   { id, name, result: ToolResult },
    TurnEnd,
    Done,
    Error(String),
}

pub struct AgentLoopConfig {
    pub tools: ToolRegistry,
    pub before_tool_call: Option<Box<dyn Fn(&str, &Value) -> bool + Send + Sync>>,
    pub after_tool_call:  Option<Box<dyn Fn(&str, &ToolResult) -> Option<ToolResult> + Send + Sync>>,
    pub max_turns: usize,
}
```

## Key Design Decisions

**Pre-wrapping instead of ratatui Wrap** — ratatui's `Wrap` widget wraps at
render time and cannot easily pad individual lines to full width. By
pre-wrapping in `build_log_lines` we know the exact row count before
rendering, which makes scroll arithmetic exact and lets us apply per-row
background styles (e.g. the grey user-message highlight).

**Channel-based LLM events** — `tokio::mpsc` decouples the async HTTP
streaming task from the synchronous draw loop. The draw loop never awaits;
it drains the channel non-blockingly on each tick, keeping the TUI
responsive during long model responses.

**`LlmProvider` trait** — all provider-specific wire formats are contained
in `llm/*.rs`. `agent/mod.rs`, `app.rs`, and `ui.rs` are provider-agnostic.
New backends implement the trait and are registered in `provider.rs`.

**`AgentLoopConfig` hooks** — `before_tool_call` and `after_tool_call` are
optional function pointers passed in at construction time. This keeps the
agent loop itself free of UI concerns; a future tool-confirmation UI will
wire a user-approval step through `before_tool_call` without touching the
loop logic.

**Outer provider loop in `main.rs`** — `run()` returns a `RunResult` enum
(`Quit | ChangeModel | ChangeProvider`) rather than mutating global state.
The outer loop in `main` rebuilds the provider and re-enters `run()` on
every model/provider switch, so `App` and `ui` never depend on which
provider is active.

## What Is Not Here Yet

- **Provider authentication** — no `/login` command; credentials must be
  placed manually; missing credentials cause a silent fallback; 401 responses
  are not retried. See [plan](plans/2026-03-14-provider-auth.md).
- **`ask_user` tool** — no built-in tool for the model to ask the user a
  question mid-task; the agent cannot pause for user decisions. See
  [plan](plans/2026-03-14-ask-user-tool.md).
- **Config file** — provider selection and API keys are env-var only; no
  persistent `~/.config/pirs/config.toml`. See
  [plan](plans/2026-03-14-config-file.md).
- **Context window management** — no truncation or summarisation when
  conversation history exceeds the model's context window. See
  [plan](plans/2026-03-14-context-management.md).
- **Anthropic and Gemini providers** — only Copilot, OpenAI, Codex, and
  Ollama are implemented.
- **Tests** — no unit or integration tests exist yet. See
  [plan](plans/2026-03-14-tests.md).

See [ROADMAP](ROADMAP.md) for prioritised work items.
