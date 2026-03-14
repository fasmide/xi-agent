# PoC Cleanup Plan — 2026-03-14

Addresses all findings from the first PoC review. Broken into four phases,
each of which leaves the project in a compilable, working state and is
committed independently.

---

## Phase 1 — Dependency & cosmetic cleanup
*Effort: ~30 min. Entirely mechanical. No design decisions.*

### 1.1 Remove `async-trait`

`async fn` in traits is stable since Rust 1.75. We are on 1.90 + edition 2024.

- Remove `async-trait = "…"` from `Cargo.toml`.
- Remove `use async_trait::async_trait;` from `llm/mod.rs`.
- Remove `#[async_trait]` from the trait definition and the `OllamaProvider`
  `impl` in `llm/ollama.rs`.

### 1.2 Scope Tokio features

Replace:
```toml
tokio = { version = "1.50.0", features = ["full"] }
```
With:
```toml
tokio = { version = "1", features = ["rt-multi-thread", "macros", "sync", "time"] }
```
Only `rt-multi-thread` (for `#[tokio::main]`), `macros`, `sync` (for the
`mpsc` channel), and `time` (for `Duration`) are actually used.

### 1.3 Rename `AppEvent` → `LlmEvent`

`AppEvent` is defined inside `llm/` and describes LLM streaming events, not
general application events. Rename throughout:

- `llm/mod.rs`: `pub enum AppEvent` → `pub enum LlmEvent`
- `llm/ollama.rs`: all references
- `main.rs`: `use llm::{AppEvent, …}` → `use llm::{LlmEvent, …}`, and the
  match arms in `apply_events()`

### 1.4 Add `Role::System`

Add a `System` variant to `Role` and a `system_prompt: Option<String>` field
to `App`. Update `Role::as_str()` and the `stream_chat` history builder in
`submit()` to prepend a system message when set.

This is a one-line change in the enum now; it becomes invasive after multiple
providers exist.

---

## Phase 2 — Structure
*Effort: ~45 min. Moves code, no logic changes.*

### 2.1 Extract `src/app.rs`

Move the `App` struct, all its `impl` methods (`new`, `submit`,
`scroll_up`, `scroll_up_lines`, `scroll_down_lines`, `scroll_down`,
`apply_events`), and the `use` imports they need into a new `src/app.rs`
module.

`main.rs` becomes thin:
```
#[tokio::main] main()     — terminal setup/teardown only
async fn run(…)           — event loop only
mod app; mod ui; mod llm; — module declarations
```

### 2.2 Fix `App<'a>` — use `TextArea<'static>`

`App` gains a lifetime parameter `'a` only because `TextArea<'a>` has one.
`TextArea` holds owned `String` data; there is no borrowed content inside.

Replace every `TextArea<'a>` with `TextArea<'static>` and remove the `'a`
parameter from `App`, its `impl` block, and all call sites (`run`, `draw`,
etc.).

### 2.3 Fix the `ui` ↔ `App` coupling

`App::new()` currently calls `ui::make_textarea()`, making application state
depend on the rendering module. Fix this:

- Move the bare `TextArea::default()` construction into `App::new()` in
  `app.rs`; only set structural properties there (e.g. `insert_newline` mode).
- Move `make_textarea()` (or inline it) into `ui.rs` as a private helper that
  applies visual styles — called only at render time if needed, or eliminated
  entirely if `tui-textarea`'s default styling is acceptable.
- `app.rs` must not `use crate::ui`.

### 2.4 Document the `submit()` invariant

The line:
```rust
let history = self.messages[..self.messages.len() - 1].to_vec();
```
is correct but relies on the implicit invariant that the last element is
always the freshly-pushed, empty assistant message. Add a comment explaining
this, or extract it into a named helper `history_for_submission()` that panics
with a descriptive message if the invariant is violated.

---

## Phase 3 — Text rendering
*Effort: ~1 hour. Isolated to `ui.rs`.*

### 3.1 Add `textwrap` and use `unicode-width`

Add to `Cargo.toml`:
```toml
textwrap = "0.16"
unicode-width = "0.2"
```
(`unicode-width` is already a transitive dependency via `tui-textarea`; pin
it explicitly so we can use it directly.)

### 3.2 Replace `wrap_str` with a correct implementation

The current `wrap_str` calls `split_whitespace()`, which:
- Collapses runs of spaces and tabs into a single space.
- Strips leading and trailing whitespace per line.
- Silently destroys indentation in code blocks.

Replace it using `textwrap::wrap_algorithms` (or `textwrap::fill` /
`textwrap::wrap` with a custom `WordSeparator`) so that:
- Whitespace within a line is preserved.
- `textwrap` uses `unicode-width` internally to measure column width, so CJK
  and emoji are handled correctly.
- The `append_message` helper can be simplified: it no longer needs to
  manually compute `text_len` and `padding` in scalar characters.

If `textwrap` is insufficient for the user-message background-pad use case,
use `unicode_width::UnicodeWidthStr::width()` (not `.chars().count()`) when
computing the padding for user messages.

### 3.3 Remove the duplicated hard-break logic

Until Phase 3.2 is done, if any manual `wrap_str` survives, extract the
duplicated oversized-word loop into a private `fn hard_break(…)` helper.

---

## Phase 4 — LlmProvider API + event loop
*Effort: ~2 hours. The most invasive change; touches four files.*

### 4.1 Change `LlmProvider` to return a `Stream`

The current signature forces every implementor and every caller to wire up a
Tokio channel internally:
```rust
async fn stream_chat(&self, messages: &[Message], tx: UnboundedSender<LlmEvent>)
    -> anyhow::Result<()>;
```

Replace with a stream-returning signature:
```rust
fn stream_chat(
    &self,
    messages: Vec<Message>,
) -> impl Stream<Item = anyhow::Result<LlmEvent>> + Send + 'static;
```

Benefits:
- No channel plumbing inside the trait or its implementors.
- Implementors are independently testable (collect the stream in a test).
- The caller (the event loop) decides how to drive the stream.
- Removes the `tx: UnboundedSender` from the public API surface.

Add `async-stream = "0.3"` or `futures-core` (already transitive) to
support writing `stream!{ … }` blocks in implementors.

### 4.2 Adapt `OllamaProvider`

Rewrite `stream_chat` as a `stream!` block (using `async-stream`) that
`yield`s `Ok(LlmEvent::Token(…))`, `Ok(LlmEvent::Done)`, or
`Err(anyhow_error)` for each NDJSON line, removing the channel send calls.

### 4.3 Adapt `App::submit`

`submit()` currently clones a `tx` and passes it into the spawned task.
After Phase 4.1 it instead:
1. Calls `provider.stream_chat(history)` to get a `Stream`.
2. Spawns a task that drives the stream and sends results over the internal
   `UnboundedSender<LlmEvent>` (the one owned by `App`).

The `App`-internal channel (`event_tx` / `event_rx`) is kept — it bridges the
async streaming task to the synchronous-style `apply_events()` drain in the
event loop.

### 4.4 Fix the event loop — replace busy-poll with `tokio::select!`

The current loop polls crossterm every 10 ms unconditionally, spinning at 100
fps even when idle:
```rust
loop {
    app.apply_events();
    terminal.draw(…)?;
    if event::poll(Duration::from_millis(10))? { … }
}
```

Replace with an async loop driven by `tokio::select!` over two sources:
- `crossterm`'s async event stream (`EventStream` from the `crossterm` crate).
- The `event_rx` channel receiver.

```rust
loop {
    tokio::select! {
        Some(ev) = crossterm_stream.next() => { /* handle input */ }
        Some(ev) = app.event_rx.recv()     => { app.apply_event(ev); }
    }
    terminal.draw(|f| ui::draw(f, app))?;
}
```

The loop now only wakes on real activity: a keypress, a mouse event, or an
LLM token arriving. CPU usage during idle periods drops to near zero.

Note: `crossterm`'s `EventStream` requires the `event-stream` feature:
```toml
crossterm = { version = "0.28", features = ["event-stream"] }
```

---

## Commit plan

| Phase | Suggested commit message |
|-------|--------------------------|
| 1.1   | `chore: remove async-trait (stable since Rust 1.75)` |
| 1.2   | `chore: scope tokio features to what is actually used` |
| 1.3   | `refactor: rename AppEvent → LlmEvent` |
| 1.4   | `feat: add Role::System and App::system_prompt` |
| 2     | `refactor: extract app.rs, fix App lifetime, fix ui/app coupling` |
| 3     | `fix: replace wrap_str with textwrap + unicode-width` |
| 4     | `refactor: LlmProvider returns Stream; replace busy-poll with select!` |

Each commit must leave the project in a fully compiling, runnable state.
