# Architecture

## Purpose

`pirs` is a terminal AI agent harness. Its job is to provide a TUI for
conversational interaction with LLMs and, once tool-calling is wired in, to
run the full agentic loop: user message → model response → tool call →
tool result → model continues.

## Module Map

```
src/
  main.rs          — tokio entry point, App state, event loop
  ui.rs            — all ratatui rendering, pre-wrapping, scroll logic
  llm/
    mod.rs         — LlmProvider trait, Message/Role/AppEvent types
    ollama.rs      — OllamaProvider (streaming NDJSON via /api/chat)
```

### `main.rs`

Owns `App` (the single shared mutable state) and the async event loop.
The loop does three things on every tick:
1. Drain `event_rx` and apply pending LLM events to `App`.
2. Call `terminal.draw(|f| ui::draw(f, app))`.
3. Poll crossterm for input and handle keyboard + mouse events.

LLM requests are spawned as detached `tokio::task`s that send `AppEvent`
values back over an `UnboundedSender<AppEvent>`.

### `ui.rs`

All rendering lives here. The chat log is drawn as a `Paragraph` with a
manual scroll offset — **no ratatui `Wrap`**. Instead, `build_log_lines`
pre-wraps every message into individual `Line` values that each fit exactly
one terminal column width. This gives precise control over padding (user
messages get a full-width grey background) and avoids ratatui's line-count
inaccuracies when mixing styled spans of different lengths.

Scroll state:
- `app.log_scroll` is a line offset into the pre-wrapped line vec.
- `app.auto_scroll = true` snaps `log_scroll` to `max_scroll` every draw.
- Auto-scroll is disabled on scroll-up and re-enabled when the user reaches
  the bottom (wheel or PageDown).

### `llm/mod.rs`

Defines the three core types:

```rust
pub struct Message { pub role: Role, pub content: String }
pub enum AppEvent  { Token(String), Done, Error(String) }

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn stream_chat(&self, messages: &[Message], tx: UnboundedSender<AppEvent>)
        -> anyhow::Result<()>;
}
```

`LlmProvider` is the primary extension point. New backends (OpenAI,
Anthropic, Gemini) implement this trait; the rest of the codebase is
provider-agnostic.

### `llm/ollama.rs`

Streams NDJSON from Ollama's `/api/chat` endpoint. Configured via
`OLLAMA_HOST` and `OLLAMA_MODEL` environment variables.

## Key Design Decisions

**Pre-wrapping instead of ratatui Wrap** — ratatui's `Wrap` widget wraps at
render time and cannot easily pad individual lines to full width. By
pre-wrapping in `build_log_lines` we know the exact pixel-row count before
rendering, which makes scroll arithmetic exact and lets us apply per-row
background styles (e.g. the grey user-message highlight).

**Channel-based LLM events** — `tokio::mpsc` decouples the async HTTP
streaming task from the synchronous draw loop. The draw loop never awaits;
it drains the channel non-blockingly on each tick. This keeps the TUI
responsive during long model responses.

**`LlmProvider` trait over a concrete struct** — future backends slot in
without touching `App` or `ui.rs`. Provider selection will be wired to
config/env vars once multiple backends exist.

## What Is Not Here Yet

- Tool-calling: `LlmProvider`, `AppEvent`, and `Message` do not yet carry
  tool-call or tool-result payloads.
- Multiple providers: only `OllamaProvider` is implemented.
- Config file: provider selection and API keys are env-var only.
- Built-in tools: no `Tool` trait or tool registry exists yet.

See [ROADMAP](ROADMAP.md) for the planned additions.
