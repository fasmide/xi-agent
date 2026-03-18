# Info-line context window utilization

**Date:** 2026-03-18  
**Status:** Implemented (verified + accepted)  
**Scope:** provider stream usage capture, app state propagation, info-line rendering

## Chosen direction

Add provider-agnostic token-usage events to the streaming pipeline, store latest usage in app state, and extend the info line to display context utilization when both usage and model context-window size are known.

When usage is unavailable for a provider/model/API path, preserve current behavior and show only context window size (or `unknown`).

## Goals

1. Show context utilization in the info bar as `used / max (percent)` when possible.
2. Support all existing providers with graceful per-provider fallbacks.
3. Avoid breaking current transcript behavior, streaming flow, or persistence.
4. Keep UI output stable when usage is missing or partial.

## Non-goals

- Perfect token accounting independent of provider-reported usage.
- Historical per-turn usage timeline UI.
- New slash commands or settings for usage reporting.

## Scope boundaries

In scope:

- `llm` event model: add usage event shape.
- Provider adapters: parse usage from streaming payloads where exposed.
- Agent loop: forward usage events to app.
- App state: retain latest usage for current/most recent turn.
- UI info line: render utilization string with fallback.
- Targeted tests for formatting and propagation.

Out of scope:

- Reworking model context-window lookup strategy.
- Billing/cost estimation.
- Persisting usage history in session store.

## Proposed design

### 1) Add normalized usage event types

Introduce a small normalized structure in `src/llm/mod.rs`:

- `UsageStats { input_tokens: Option<usize>, output_tokens: Option<usize>, total_tokens: Option<usize> }`
- `LlmEvent::Usage(UsageStats)`

Mirror at agent layer:

- `AgentEvent::Usage(UsageStats)`

Design notes:

- Keep fields optional because APIs differ.
- Prefer provider-reported totals when available.
- If only input/output are present, compute used tokens from available fields in UI/app helper.

### 2) Parse usage in providers where available

- `src/llm/openai.rs` (chat completions SSE): parse final usage chunk if present.
  - If required by API behavior, include request flag to emit stream usage.
- `src/llm/codex.rs` (responses SSE): parse usage from response completion events.
- `src/llm/anthropic.rs` (messages SSE): parse usage from message delta/stop usage fields.
- `src/llm/ollama.rs`: if stream payload includes counts, map them; otherwise emit nothing.
- `src/llm/copilot.rs`: no direct parsing (delegates to routed inner providers).

Emit `LlmEvent::Usage` at most once per turn per provider path when stable/final usage is known.

### 3) Propagate through agent loop and app

- In `src/agent/mod.rs`, forward `LlmEvent::Usage` as `AgentEvent::Usage`.
- In `src/app.rs`, add state field (e.g. `latest_usage: Option<UsageStats>`).
- Update `apply_event` to store latest usage when received.
- Reset usage state at start of a new user-submitted turn to avoid stale display.

### 4) Extend info-line rendering

- In `src/ui.rs`, augment `build_info_line` inputs with optional usage-derived utilized token count.
- Display logic:
  - If `context_max` and `used` known: `context <used_fmt> / <max_fmt> (<pct>%)`
  - If only `context_max` known: current `context <max_fmt>`
  - If unknown max: `context unknown`
- Keep current right-side hint (`Ctrl+I`) unchanged.

Formatting conventions:

- Reuse or extend existing `format_context_size` helper.
- Add helper for percentage with clamp and integer rounding.

## Ordered implementation steps

1. Add usage types/event variants to `src/llm/mod.rs` and `src/agent/types.rs`.
2. Update `src/agent/mod.rs` event forwarding for usage.
3. Implement per-provider usage parsing in:
   - `src/llm/openai.rs`
   - `src/llm/codex.rs`
   - `src/llm/anthropic.rs`
   - `src/llm/ollama.rs` (if available)
4. Add `App` usage state and event handling in `src/app.rs`.
5. Update `src/ui.rs` info-line construction and formatting helpers.
6. Add/adjust tests for info-line formatting and any provider parsing helpers that are unit-testable.
7. Run quality gates:
   - `cargo fmt`
   - `cargo clippy --all-targets --all-features`
   - `cargo test`

## Affected files

Expected:

- `src/llm/mod.rs`
- `src/llm/openai.rs`
- `src/llm/codex.rs`
- `src/llm/anthropic.rs`
- `src/llm/ollama.rs` (possibly)
- `src/agent/types.rs`
- `src/agent/mod.rs`
- `src/app.rs`
- `src/ui.rs`
- `docs/plans/2026-03-18-context-window-utilization-plan.md`

Possible doc updates after acceptance (Reconcile):

- `docs/ARCHITECTURE.md`
- `docs/USER-INTERFACE-SPEC.md`

## Assumptions

1. At least one provider route already exposes usage in its stream payload.
2. Optional usage fields are acceptable as first-class behavior.
3. Info-line width constraints can be handled with existing fill/truncation behavior.

## Risks and mitigations

1. **Provider payload variance / drift**
   - Mitigation: defensive parsing and emit usage only when confidently parsed.
2. **Stale usage shown across turns**
   - Mitigation: clear usage state at turn start.
3. **UI overcrowding on narrow terminals**
   - Mitigation: compact formatting; rely on existing saturation behavior and add tests.

## Verification approach

Technical evidence required:

1. Build compiles and lint is clean:
   - `cargo clippy --all-targets --all-features`
2. All tests pass:
   - `cargo test`
3. UI unit checks for info-line text:
   - with usage + max => includes ratio and percent
   - without usage => preserves existing context display
4. Manual smoke test (if feasible):
   - run a prompt on at least one provider known to emit usage and confirm info line updates.

## Locked decisions before build

1. Usage is represented as optional fields in a normalized struct.
2. Info line shows utilization only when `used` and `max` are both known.
3. Missing usage is non-fatal and does not alter existing UX beyond omission of utilization.
