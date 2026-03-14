# ask_user Tool

**Date:** 2026-03-14  
**Status:** Planned  
**Priority:** High

## Concept

Add `ask_user` as a sixth built-in tool. The model calls it proactively when
it reaches a decision point that genuinely requires user input — e.g. which
of two approaches to take, what name to give a new file, whether to proceed
with a potentially significant change. It is not an automatic safety gate;
the model decides when the question is necessary.

The system prompt guides the model toward using `ask_user` only for real
decisions, not as a politeness reflex.

## Tool definition

```
name:        ask_user
label:       ❓
description: Ask the user a question and wait for their answer.
             Use this only when you genuinely need a decision or piece of
             information from the user before you can proceed — not as a
             courtesy check before routine operations.
```

Parameters:

```json
{
  "type": "object",
  "properties": {
    "question": {
      "type": "string",
      "description": "The question to ask the user"
    }
  },
  "required": ["question"]
}
```

Returns: the user's verbatim typed response as a plain string.

## Architecture

The tool cannot be implemented like the others (a simple `async fn execute`)
because it needs to pause the agent loop and yield control back to the TUI
event loop. The mechanism is a shared channel pair, the same pattern used for
the main `AgentEvent` channel.

### New channel in `App`

```rust
pub struct App {
    // …
    /// Pending ask_user question from the agent, if any.
    pub ask_request: Option<String>,
    /// Oneshot sender used to return the user's answer to the agent task.
    ask_reply: Option<tokio::sync::oneshot::Sender<String>>,
    /// Channel on which AskUserTool sends its requests.
    pub(crate) ask_rx: tokio::sync::mpsc::UnboundedReceiver<AskRequest>,
    ask_tx:            tokio::sync::mpsc::UnboundedSender<AskRequest>,
}

pub struct AskRequest {
    pub question: String,
    pub reply:    tokio::sync::oneshot::Sender<String>,
}
```

`ask_tx` is cloned and passed into `AskUserTool` at construction time, before
the agent loop starts.

### `AskUserTool`

```rust
pub struct AskUserTool {
    tx: tokio::sync::mpsc::UnboundedSender<AskRequest>,
}

impl Tool for AskUserTool {
    fn execute(&self, args: Value) -> Pin<Box<dyn Future<Output = ToolResult> + Send + '_>> {
        Box::pin(async move {
            let question = args["question"].as_str().unwrap_or("").to_string();
            let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
            let _ = self.tx.send(AskRequest { question, reply: reply_tx });
            match reply_rx.await {
                Ok(answer) => ToolResult::ok(answer),
                Err(_)     => ToolResult::err("ask_user: channel closed"),
            }
        })
    }
}
```

The agent task blocks on `reply_rx.await`. Because this runs inside a
`tokio::spawn`ed task — separate from the TUI draw loop — blocking it does
not freeze the UI.

### App event handling

Drain `ask_rx` alongside `event_rx` in the main select:

```rust
Some(req) = app.ask_rx.recv() => {
    app.ask_request = Some(req.question.clone());
    app.ask_reply   = Some(req.reply);
    // Optionally push a display message so the question appears in the log.
    app.messages.push(Message::ask_question(&req.question));
}
```

When the user submits their answer (Enter in the input field while
`app.ask_request.is_some()`), instead of calling `app.submit()`:

```rust
if let Some(reply) = app.ask_reply.take() {
    let answer = app.textarea.lines().join("\n");
    app.reset_textarea();
    // Show the user's answer in the log.
    app.messages.push(Message::user(&answer));
    let _ = reply.send(answer);
    app.ask_request = None;
}
```

### Visual treatment

While `ask_request.is_some()`, render the question prominently in the chat
log (it is already pushed as a `Message` of a new `Role::AskQuestion` or
displayed as part of the tool-call block), and style the input panel border
differently to signal that the user's next Enter goes to the agent, not as
a new user turn:

```
╔═ Answer ════════════════════════════════════════════════════════╗
║ │                                                               ║
╚════════════════════════════════════════════════════════════════╝
```

A simple approach: change the input block's border label from the default to
`" Answer "` while `ask_request.is_some()`.

## System prompt guidance

Add to `build_system_prompt` when `ask_user` is in the registry:

> Use ask_user only when the task genuinely requires a decision or piece of
> information that only the user can provide — for example, choosing between
> two valid approaches, confirming an ambiguous filename, or obtaining a value
> you cannot infer. Do not use it as a courtesy check before routine
> operations.

## Wiring in `main.rs` / `register_builtin_tools`

`AskUserTool` is not stateless so it cannot be constructed without the
channel. The registration call gains the sender:

```rust
pub fn register_builtin_tools(ask_tx: AskTx) -> ToolRegistry {
    // …
    Arc::new(AskUserTool::new(ask_tx)),
}
```

`main.rs` creates `App` first (which allocates the channel), then calls
`register_builtin_tools(app.ask_tx.clone())`, then sets
`app.agent_config.tools`.

## Implementation Tasks

1. Add `AskRequest` struct and `AskTx` type alias to `agent/types.rs`.
2. Add `ask_rx` / `ask_tx` / `ask_request` / `ask_reply` to `App`; wire
   channel in `App::new`.
3. Implement `src/agent/tools/ask_user.rs` with `AskUserTool`.
4. Register `AskUserTool` in `register_builtin_tools`; thread `ask_tx` from
   `App` through to the call site in `main.rs`.
5. Drain `ask_rx` in the main `select!` in `run()`.
6. Branch on `app.ask_request.is_some()` in the `Enter` key handler to
   send the answer instead of submitting a new turn.
7. Change input panel border label to `" Answer "` in `ui.rs` when a
   request is pending.
8. Add the `ask_user` guidance paragraph to `build_system_prompt`.
9. Add `ask_user` to the tools table in `README.md`.
