# Output phase classification from provider stream

**Date:** 2026-03-17  
**Status:** Proposed plan  
**Scope:** LLM stream events, agent loop events, UI icon mapping

## Chosen direction

Preserve provider stream semantics so tau can distinguish:

- thinking output
- provisional assistant output (non-final because tool intent is known)
- final assistant output

without relying only on post-hoc UI inference.

## Goals

1. Detect tool intent as early as provider APIs allow.
2. Represent assistant text phase explicitly in tau’s internal event model.
3. Render icons consistently:
   - 🧠 thinking
   - 💭 provisional/internal monologue
   - 💬 final output
4. Keep behavior stable for providers with weaker signals.

## Non-goals

- Changing provider HTTP APIs.
- Reworking tool execution order in the agent loop.
- Altering persisted message schema beyond what is required for display.

## Scope boundaries

In scope:

- `llm` event model changes
- provider adapters (`codex`, `openai`, `anthropic`, `ollama`) emitting early tool-intent events
- agent loop propagation of phase/intention signals
- UI rendering changes for icon mapping
- tests for event ordering and icon behavior

Out of scope (for this feature):

- New slash commands
- Full transcript redesign
- Backfill migration of existing saved sessions

## Proposed design

### 1) Add explicit stream semantics

Extend `LlmEvent` with a tool-intent signal and phase-aware text delta shape.

Proposed additions (exact naming to confirm during build):

- `ToolIntentStart` (emitted immediately when provider indicates a pending function/tool call)
- `Token { text, phase }` where `phase ∈ {Unknown, Provisional, Final}`

Compatibility rule:

- Providers that cannot determine phase at emission time use `Unknown`.

### 2) Map provider-native events early

- **Codex (Responses):** emit `ToolIntentStart` at `response.output_item.added` with `function_call`.
- **Anthropic:** emit at `content_block_start` with `tool_use`.
- **OpenAI Chat Completions:** emit when first `delta.tool_calls` fragment appears.
- **Ollama:** emit when first chunk contains `message.tool_calls`.

### 3) Agent-loop propagation

Update `run_agent_loop` to forward early intent to app state before tool execution begins.

Add corresponding `AgentEvent` variant(s), e.g.:

- `AssistantPhaseChanged(Provisional|Final)` or `ToolIntentStart`

Ensure ordering invariants:

1. zero or more thinking/text deltas
2. optional early intent event
3. stream `Done`
4. tool execution events (if any)

### 4) App state model for rendering

Track current assistant turn phase while streaming.

- Default: `Unknown`
- On tool-intent: `Provisional`
- On turn completion with no tool calls: `Final`

Persist enough metadata on assistant messages to render the correct icon after streaming completes.

### 5) UI icon mapping

In `build_log_lines`:

- thinking lines: `🧠`
- assistant text with phase `Provisional`: `💭`
- assistant text with phase `Final`: `💬`
- `Unknown` fallback:
  - while streaming: `💭` (conservative)
  - after turn end without tools: `💬`

## Ordered implementation steps

1. Introduce new phase/intent types in `src/llm/mod.rs` and `src/agent/types.rs`.
2. Update provider adapters to emit early tool-intent semantics.
3. Update `src/agent/mod.rs` to propagate new events and preserve ordering.
4. Update `src/app.rs` state transitions and assistant-message metadata.
5. Update `src/ui.rs` rendering logic and tests for icon selection.
6. Update architecture docs (`docs/ARCHITECTURE.md`) to reflect new event model.
7. Run formatting/lint/tests:
   - `cargo fmt`
   - `cargo clippy --all-targets --all-features`
   - `cargo test`

## Affected files

Expected primary edits:

- `src/llm/mod.rs`
- `src/llm/codex.rs`
- `src/llm/openai.rs`
- `src/llm/anthropic.rs`
- `src/llm/ollama.rs`
- `src/agent/types.rs`
- `src/agent/mod.rs`
- `src/app.rs`
- `src/ui.rs`
- `docs/ARCHITECTURE.md`

Likely test updates:

- `src/agent/tests.rs`
- `src/ui.rs` (unit tests at file bottom)

## Assumptions

- Provider streams continue to include the currently mapped tool-intent markers.
- Introducing new internal event variants is acceptable across the codebase.
- Session persistence can tolerate additive assistant metadata changes (or we gate metadata to in-memory only).

## Risks

1. **Event-order regressions** causing UI flicker or wrong icon transitions.
2. **Provider drift** in SSE payload shapes, especially Responses API event names.
3. **State mismatch** between streaming and persisted transcript if phase metadata is incomplete.

Mitigations:

- Add unit tests for event ordering and phase transitions.
- Keep conservative fallback (`Unknown` -> current behavior).
- Validate with at least one tool-calling scenario per provider adapter test/mocked parse path.

## Verification approach

Minimum acceptance checks:

1. Non-tool answer:
   - streaming assistant line ends as `💬`.
2. Tool-calling turn:
   - assistant line switches/lands on `💭` before tool execution entries appear.
3. Thinking-capable model:
   - thinking still renders with `🧠` and does not alter assistant phase incorrectly.
4. Regression:
   - existing tool-call start/end behavior and transcript order unchanged.
5. Full quality gates pass:
   - `cargo clippy --all-targets --all-features`
   - `cargo test`

## Open decisions to confirm before build

1. Should phase live on `Message` (persisted) or only in transient UI state?
2. Should we introduce a distinct `InternalMonologueToken` event, or treat it as `Token + phase`?
3. Exact fallback icon for `Unknown` during streaming (`💭` vs current `💬`).
