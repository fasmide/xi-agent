# Context Window Management

**Date:** 2026-03-14  
**Status:** Planned  
**Priority:** Medium

## Problem

The agent accumulates every message (user, assistant, tool call, tool result)
in `app.messages` and sends the full history on every LLM turn. Once the
conversation exceeds the model's context window the provider either returns
an error or silently truncates the input, producing degraded or nonsensical
responses. The info bar (Ctrl+I) shows the context window size for known
models, but nothing enforces it.

## Goal

1. **Warn** the user when the conversation is approaching the context limit.
2. **Truncate** gracefully when the limit is reached so the agent loop
   continues rather than failing.

Accurate token counting requires a tokenizer per model family (expensive).
This plan uses a character-based approximation: 1 token ≈ 4 characters.
That is a deliberate simplification — close enough for a warning threshold
and conservative truncation.

## Estimated token counting

```rust
/// Rough token estimate: 1 token ≈ 4 UTF-8 bytes.
pub fn estimate_tokens(messages: &[Message]) -> usize {
    messages.iter().map(|m| m.content.len() / 4 + 10).sum()
}
```

The +10 per message accounts for role tags and JSON overhead.

## Warning threshold

When `estimate_tokens(messages) > context_window * 0.85`, render a warning
line in the chat log (or in the info bar) so the user knows the session is
running long. No action is taken yet — the user can start a `/new`
conversation.

Suggested style: a dim amber line at the bottom of the log:
```
⚠ ~87k / 128k tokens used — consider /new
```

## Truncation strategy

When `estimate_tokens > context_window * 0.95`, apply truncation before
sending to the model:

1. Always keep: the system prompt message (index 0).
2. Always keep: the N most recent messages (default N = 6, i.e. ~3 turns).
3. Drop the oldest non-system messages until the estimate is below 80 % of
   the context window.
4. Insert a synthetic system message immediately after the real system prompt:
   ```
   [Note: earlier conversation history was omitted to fit the context window.]
   ```

This is conservative: it may drop more than strictly necessary, but it
guarantees the most recent context is preserved and the model always sees
the note.

## New function: `agent/context.rs`

```rust
/// Apply context-window truncation to a message list.
///
/// `window` is the model's context window in tokens.
/// Returns the (possibly shortened) list ready to send to the provider.
pub fn maybe_truncate(messages: Vec<Message>, window: usize) -> Vec<Message> { … }

/// Warn threshold: true when the history is over 85 % full.
pub fn is_near_limit(messages: &[Message], window: usize) -> bool { … }
```

## Integration points

### In `agent/mod.rs` — before each LLM call

```rust
let window = provider.context_window().unwrap_or(usize::MAX);
let messages_for_turn = context::maybe_truncate(messages.clone(), window);
let mut stream = provider.stream_chat_with_tools(messages_for_turn, tool_defs.clone());
```

`context_window()` is a new optional method on `LlmProvider` (default:
`None`). `provider.rs` has the lookup table `context_window_for_model()`
which already covers the common cases; the provider implementations call
it from their `context_window()` impl.

### In `app.rs` — for the warning indicator

```rust
pub fn context_warning(&self) -> Option<String> {
    let window = provider::context_window_for_model(&self.current_model)?;
    let used = context::estimate_tokens(&self.messages);
    if context::is_near_limit(&self.messages, window) {
        Some(format!("~{}k / {}k tokens used — consider /new",
            used / 1000, window / 1000))
    } else {
        None
    }
}
```

`ui.rs` renders this as a dim amber line at the bottom of the chat log when
`app.context_warning().is_some()`.

## New `LlmProvider` method

```rust
/// Return the context window size in tokens for this provider+model combo.
/// Default: None (unknown; no truncation applied).
fn context_window(&self) -> Option<usize> {
    None
}
```

Each provider overrides this by calling `context_window_for_model(&self.model)`.

## Implementation Tasks

1. Add `src/agent/context.rs` with `estimate_tokens`, `is_near_limit`,
   `maybe_truncate`.
2. Add `fn context_window(&self) -> Option<usize>` to `LlmProvider` trait
   (default `None`); implement in `OpenAiProvider`, `OllamaProvider`, etc.
3. Call `maybe_truncate` in `agent/mod.rs` before each `stream_chat_with_tools`.
4. Add `App::context_warning` in `app.rs`.
5. Render the warning line in `ui.rs` at the bottom of the chat log.
6. Update the `ARCHITECTURE.md` "Key Types" section to document
   `context_window()`.
