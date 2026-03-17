# Steering queue during active agent loop

**Date:** 2026-03-17  
**Status:** Implemented (verified + accepted)  
**Scope:** App input handling, agent loop steering injection, UI pinned queued-message rendering, tests

## Goal

Allow users to type and submit messages while the agent loop is running, with these semantics:

1. Submitted messages are queued as steering messages.
2. Queued steering is shown at the bottom of the output with `🕹️` until consumed.
3. At the first safe opportunity, queued steering is inserted into the model message sequence.
4. After insertion, the message appears in normal chat sequence as a user message.

## Success criteria

1. During streaming, Enter on non-empty input enqueues steering instead of no-op.
2. UI displays queued steering rows at bottom with `🕹️` icon.
3. Queued rows remain visible until the agent loop consumes them.
4. Consumed steering message is inserted as `Role::User` in transcript order before the next assistant turn.
5. If steering arrives during a multi-tool turn, remaining tool calls are skipped with a clear tool-result error message.
6. `cargo clippy --all-targets --all-features` and `cargo test` pass.

## Design

### 1) Agent-loop steering channel

Add a queue receiver to `run_agent_loop` (e.g. `UnboundedReceiver<String>` for raw text steering payloads).

Loop behavior:
- Check queue before each assistant streaming turn and inject all pending steering messages into loop history.
- After each tool execution, check queue again.
- If queue is non-empty during a tool batch, skip remaining tool calls in that assistant tool list and return error tool results (`"Skipped due to queued user message."`).

### 2) Agent events for steering lifecycle

Extend `AgentEvent` with:
- `SteeringConsumed { text: String }` (or equivalent) so App can remove it from pinned queue UI.

Consumption event is emitted when the loop actually inserts the queued message into `messages` for the next LLM call.

### 3) App state + submission behavior

Add app state:
- sender handle for steering messages (to the active loop)
- `queued_steering: Vec<String>` for pinned UI rows

Input handling changes:
- In `run()` Enter handling, when `app.streaming` and not slash/ask/login mode, call a new enqueue method instead of ignoring submit.
- enqueue method:
  - trims input
  - appends to `queued_steering`
  - sends to steering channel
  - clears textarea
  - keeps auto-scroll behavior

On `AgentEvent::SteeringConsumed`, remove first matching queued item (FIFO-safe handling).

### 4) UI rendering

Update `build_log_lines` signature to accept queued steering list.

Rendering rule:
- Append one rendered row block per queued steering item at end of log, style as normal message text with `🕹️` prefix.
- These rows appear after all transcript entries, so they are visually at bottom.

When consumed and removed from queue, rows disappear from pinned section and appear in transcript where inserted.

### 5) Ordering and persistence

- App should persist messages on turn end/done as today.
- Inserted steering messages should become normal `Message::user` entries and persist normally.
- Pinned queue state is transient UI state and should not be session-persisted.

## Ordered implementation steps

1. Extend `AgentEvent` and `run_agent_loop` API for steering support.
2. Implement steering queue drain helpers in `src/agent/mod.rs`.
3. Add tool-batch interruption/skip behavior when steering appears mid-batch.
4. Wire steering sender/receiver creation in `App::submit`, `submit_with_text`, `retry_last_request`.
5. Add app enqueue method for streaming-time Enter handling.
6. Update `main.rs` Enter key path to route streaming input to steering enqueue.
7. Update `ui.rs` log builder and draw call to include queued steering rows with `🕹️`.
8. Add/adjust tests in `src/agent/tests.rs` and `src/ui.rs`.
9. Run format/lint/tests.

## Affected files

- `src/agent/types.rs`
- `src/agent/mod.rs`
- `src/agent/tests.rs`
- `src/app.rs`
- `src/main.rs`
- `src/ui.rs`
- `docs/ARCHITECTURE.md` (event/update semantics)

## Risks and mitigations

1. **Race/ordering bugs in queue consumption**
   - Mitigate with deterministic queue-drain points and event tests.
2. **UI confusion between queued and committed messages**
   - Mitigate with distinct icon (`🕹️`) and clear transition on consume.
3. **Stale steering sender after loop ends**
   - Mitigate by resetting sender on `Done`, `Error`, and abort paths.

## Verification approach

- Unit tests:
  - steering consumed before next assistant turn
  - steering during tool batch skips remaining tools
  - queued UI rows rendered at bottom with `🕹️`
- Manual check:
  - submit prompt causing tool usage, type steering while running, observe pinned queue + sequence handoff
- Quality gates:
  - `cargo fmt`
  - `cargo clippy --all-targets --all-features`
  - `cargo test`
