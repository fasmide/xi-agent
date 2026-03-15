# Provider authentication redesign — implementation plan

**Date:** 2026-03-15
**Design:** [2026-03-15-provider-auth-redesign-design.md](2026-03-15-provider-auth-redesign-design.md)
**Scope:** `copilot`, `codex`

Ordered by dependency. Each phase should compile before moving on.

---

## Phase 1 — Auth foundation (this step)

### 1.1 Add platform path dependency
- Update `Cargo.toml` to add `directories`.

### 1.2 Add auth module skeleton
Create:
- `src/auth/mod.rs`
- `src/auth/paths.rs`
- `src/auth/types.rs`
- `src/auth/store.rs`

### 1.3 Implement platform auth path resolution
- `auth/paths.rs` resolves pirs config dir using `ProjectDirs`.
- Expose helper for `auth.json` path.

### 1.4 Implement auth schema types
- versioned auth file envelope
- provider records for `copilot` and `codex`

### 1.5 Implement file-backed auth store
- load auth file if present
- return empty state if missing
- atomic write via temp file + rename
- preserve unrelated providers when updating one provider
- best-effort restrictive permissions on Unix

### 1.6 Wire module in binary crate
- add `mod auth;` in `src/main.rs`

Deliverable: foundational auth APIs compile and are usable by provider construction.

---

## Phase 2 — Provider decoupling from `~/.pi`

### 2.1 Refactor provider construction
- Update `src/provider.rs` to read credentials via `AuthStore` for:
  - `ProviderKind::Copilot`
  - `ProviderKind::Codex`
- Return explicit unauthenticated errors when credentials are missing.

### 2.2 Remove direct `~/.pi` reads from Copilot provider
- Update `src/llm/copilot.rs` to construct from supplied credentials/token.
- Keep token->base-url derivation in this module.
- Remove `HOME/.pi/agent/auth.json` logic.

### 2.3 Remove direct `~/.pi` reads from Codex provider
- Update `src/llm/codex.rs` to construct from supplied access token + account ID.
- Remove `HOME/.pi/agent/auth.json` logic.

Deliverable: no runtime token reads from `~/.pi` for Copilot/Codex.

---

## Phase 3 — Interactive login command and UI mode

### 3.1 Slash command plumbing
- Add `/login [provider]` command in `src/commands.rs`.
- Add corresponding action handling in `src/main.rs` event loop.

### 3.2 App auth mode state
- Add login/auth state to `App`.
- Add cancel handling (`Esc`) while auth is active.

### 3.3 Copilot interactive login flow
- Implement device flow module in `src/auth/copilot.rs`.
- Show URL/code + progress in TUI.
- Persist resulting credentials to `AuthStore`.

### 3.4 Codex interactive login flow
- Implement browser OAuth + localhost callback in `src/auth/codex.rs`.
- No manual token/code paste flow.
- Persist resulting credentials to `AuthStore`.

Deliverable: user can authenticate both providers from inside pirs.

---

## Phase 4 — Renewal and retry behavior

### 4.1 Pre-request expiry checks
- If auth data indicates expiry, refresh before request.

### 4.2 Refresh-on-401
- On 401, trigger one refresh attempt and retry once.

### 4.3 Failure UX
- On refresh failure, surface clear `/login <provider>` action.

Deliverable: automatic renewal when possible; explicit re-login path on failure.

---

## Phase 5 — Product polish and docs

### 5.1 Startup/auth error UX
- Remove silent fallback behavior.
- Show actionable auth messages.

### 5.2 Documentation
- Update `README.md` auth section.
- Keep roadmap links current.

### 5.3 Tests
- unit tests for paths/store/parsing
- integration-shaped tests for provider init and refresh retry behavior

---

## Verification gates per phase

Run after each phase:
- `cargo fmt`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test`

