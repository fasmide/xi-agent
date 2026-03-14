# LLM Backend Design — 2026-03-14

## Overview
Add a streaming LLM backend to the chat TUI, starting with Ollama, designed for easy extension to other providers.

## Architecture

```
src/
  main.rs          ← entrypoint, tokio runtime, event loop
  app.rs           ← App state
  ui.rs            ← ratatui rendering
  llm/
    mod.rs         ← LlmProvider trait + Message/Role/AppEvent types
    ollama.rs      ← OllamaProvider implementation
```

## Data Model

```rust
pub enum Role { User, Assistant }
pub struct Message { pub role: Role, pub content: String }
pub enum AppEvent { Token(String), Done, Error(String) }

pub struct App {
    messages: Vec<Message>,
    textarea: TextArea,
    log_scroll: usize,
    streaming: bool,
    event_rx: UnboundedReceiver<AppEvent>,
    event_tx: UnboundedSender<AppEvent>,
}
```

## Event Loop

```
loop {
  if crossterm::event::poll(10ms) → handle key events
  drain event_rx:
    Token(t)  → append to last assistant message, scroll to bottom
    Done      → streaming = false
    Error(e)  → replace last assistant message with error text, streaming = false
  redraw
}
```

On Enter: push user Message, push empty assistant Message, set streaming = true, tokio::spawn(provider.stream_chat(history, tx)).

## Provider Trait

```rust
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn stream_chat(
        &self,
        messages: &[Message],
        tx: UnboundedSender<AppEvent>,
    ) -> anyhow::Result<()>;
}
```

## OllamaProvider

- POST `http://localhost:11434/api/chat` with `{"model":"llama3.1","messages":[...],"stream":true}`
- Parse NDJSON response, extract `message.content` per chunk
- Config: `OLLAMA_HOST` env var (default: `http://localhost:11434`), `OLLAMA_MODEL` env var (default: `llama3.1`)
