# Thinking Output Support — 2026-03-14

## Overview

Add support for LLM "thinking" / chain-of-thought output. Thinking content is
streamed and displayed in the chat log as dim text before the assistant's
answer, separated by a blank line. No toggle, no separate pane — always
visible, visually subdued.

Designed for Ollama today (DeepSeek-R1, QwQ, etc. which emit `<think>…</think>`
inline), but generalises to OpenAI-compatible APIs that expose reasoning as a
structured field.

---

## Data Model (`llm/mod.rs`)

Add an optional `thinking` field to `Message`:

```rust
pub struct Message {
    pub role: Role,
    pub content: String,
    pub thinking: Option<String>,   // new
}
```

Add `ThinkingToken` to `LlmEvent`:

```rust
pub enum LlmEvent {
    ThinkingToken(String),   // new
    Token(String),
    Done,
    Error(String),
}
```

---

## Ollama Provider (`llm/ollama.rs`)

`stream_chat` runs a small two-state parser over the incoming text stream:

- **`Thinking` state** — active while inside `<think>…</think>`; tokens are
  emitted as `LlmEvent::ThinkingToken`.
- **`Responding` state** — default; tokens are emitted as `LlmEvent::Token`.

Tag boundaries (`<think>`, `</think>`) that span multiple NDJSON chunks are
handled by carrying a partial-match prefix in the parse buffer.

When building `OllamaMessage` for the request history, any `message.thinking`
content is re-injected into the content string wrapped in `<think>…</think>` so
that DeepSeek-R1 sees its prior reasoning in multi-turn conversations.

---

## App State (`app.rs`)

`apply_event` is extended:

```rust
LlmEvent::ThinkingToken(token) => {
    if let Some(last) = self.messages.last_mut() {
        last.thinking.get_or_insert_with(String::new).push_str(&token);
    }
}
```

No other app-level changes.

---

## UI (`ui.rs`)

In `build_log_lines`, for `Role::Assistant` messages with a non-empty
`thinking` field:

1. Render the thinking content as dim (`Color::DarkGray`) pre-wrapped lines.
2. Append one blank line.
3. Render the answer text in the normal style.

During streaming the thinking cursor (`▋`) tracks whichever buffer is
currently being filled (thinking or content). After thinking is complete and
response tokens start arriving, the `▋` moves to the answer section.

---

## What Does Not Change

- `LlmProvider` trait signature — providers emit the new event type where
  applicable; others simply never emit `ThinkingToken`.
- `Role` enum.
- Layout, scroll, completion popup, all other features.

---

## Non-Goals

- Collapsible/expandable thinking blocks (YAGNI for now).
- Stripping thinking from history for providers that forbid it (not a current
  concern with Ollama).
