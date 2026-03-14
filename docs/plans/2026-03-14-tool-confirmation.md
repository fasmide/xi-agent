# Tool Confirmation UI

**Date:** 2026-03-14  
**Status:** Planned  
**Priority:** High (safety gap)

## Problem

The agent loop executes tool calls silently and immediately. When a model
decides to run `bash rm -rf …` or overwrite a file, there is no checkpoint
where the user can review and approve or reject the action. The
`before_tool_call` hook in `AgentLoopConfig` exists precisely for this, but
it is always `None` in `main.rs`.

## Goal

Display a non-blocking confirmation prompt in the TUI before each tool
invocation. The user presses `y` to allow or `n`/`Esc` to block. Auto-approve
read-only tools so the confirmation only appears where it matters.

## Design

### Which tools require confirmation

| Tool        | Category    | Requires approval? |
|-------------|-------------|--------------------|
| `read_file` | read-only   | no                 |
| `find`      | read-only   | no                 |
| `bash`      | destructive | **yes**            |
| `write`     | destructive | **yes**            |
| `edit`      | destructive | **yes**            |

The set of tools requiring approval should be configurable (see config file
plan), but the above is a safe default.

### UX flow

1. Agent loop calls `before_tool_call("bash", &args)`.
2. The hook sends a `ConfirmRequest { id, name, args }` to `App` over a
   new `confirm_tx` channel and then **blocks** waiting for a
   `ConfirmResponse { id, allowed: bool }` on a paired `oneshot` channel.
3. `App` enters a new `confirm_mode: bool` state. The UI renders a compact
   confirmation bar above the input panel:
   ```
   ┌─────────────────────────────────────────────────────────────┐
   │ 💻 bash   ls -la src/                             [y] allow  │
   │                                                   [n] block  │
   └─────────────────────────────────────────────────────────────┘
   ```
4. The user presses `y` or `n` (or `Esc` as alias for `n`).
5. `App` sends the response on the oneshot channel and clears `confirm_mode`.
6. The hook unblocks and returns `true` (allow) or `false` (block).
7. If blocked, `run_agent_loop` injects a `ToolResult::err("blocked by user")`
   into the conversation and continues — the model can explain the failure.

### Why blocking-in-a-hook works

`before_tool_call` is called from inside a `tokio::spawn`ed task. Blocking
that task (via a `tokio::sync::oneshot` await) is safe: it does not block the
TUI draw loop, which runs in its own select branch on the main task.

### New types

```rust
// In agent/types.rs or a new agent/confirm.rs

pub struct ConfirmRequest {
    pub id:   String,
    pub name: String,        // display label (emoji)
    pub args: serde_json::Value,
}

pub type ConfirmTx = tokio::sync::mpsc::UnboundedSender<
    (ConfirmRequest, tokio::sync::oneshot::Sender<bool>)
>;
```

### App state additions

```rust
pub struct App {
    // …
    /// Pending tool-confirmation request, if any.
    pub confirm_request: Option<ConfirmRequest>,
    /// Oneshot sender to unblock the agent loop after user responds.
    confirm_reply: Option<tokio::sync::oneshot::Sender<bool>>,
    /// Channel that receives confirmation requests from agent tasks.
    pub(crate) confirm_rx: tokio::sync::mpsc::UnboundedReceiver<(ConfirmRequest, oneshot::Sender<bool>)>,
    confirm_tx: tokio::sync::mpsc::UnboundedSender<(ConfirmRequest, oneshot::Sender<bool>)>,
}
```

### Wiring in `main.rs`

```rust
let confirm_tx = app.confirm_tx.clone();

app.agent_config.before_tool_call = Some(Box::new(move |name, args| {
    if READ_ONLY_TOOLS.contains(&name) {
        return true;
    }
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    let req = ConfirmRequest { id: uuid(), name: name.to_string(), args: args.clone() };
    let _ = confirm_tx.send((req, reply_tx));
    // Block this task until the user responds.
    tokio::runtime::Handle::current().block_on(reply_rx).unwrap_or(false)
}));
```

## Implementation Tasks

1. Add `ConfirmRequest`, `ConfirmTx` types to `agent/types.rs`.
2. Add `confirm_rx` / `confirm_tx` / `confirm_request` / `confirm_reply`
   fields to `App`. Wire channel in `App::new`.
3. Update `App::apply_event` to drain `confirm_rx` alongside `event_rx`.
4. Add `App::confirm_allow` / `App::confirm_block` helpers that send on
   the oneshot and clear state.
5. Add `confirm_mode` rendering to `ui.rs` — a slim bar above the input.
6. Handle `y` / `n` / `Esc` in the keyboard branch of `run()` when
   `app.confirm_request.is_some()`.
7. Wire `before_tool_call` in `main.rs` as shown above.
8. Update the keybindings table in README.
