# Provider authentication redesign

**Date:** 2026-03-15  
**Status:** Approved design  
**Scope:** `copilot`, `codex`

## Summary

Redesign provider authentication so tau no longer reads credentials from
`~/.pi/agent/auth.json`.

Instead, tau will:

- store its own credentials in a platform-appropriate app directory
- support **interactive initial authentication** from inside the TUI
- renew expired tokens automatically when possible
- never migrate or import secrets from `~/.pi`
- keep the auth layer separate from provider transport code

This design uses ideas from `../pi-mono` for:

- dedicated auth storage abstraction
- provider-specific login / refresh helpers
- interactive login UX for device flow and browser OAuth
- single-retry refresh behavior after auth failures
- no manual token/code paste path

It intentionally does **not** preserve pi's auth file compatibility.

---

## Decisions

### Accepted

- Use a **tau-owned auth file** in a platform-specific app directory.
- Support **interactive initial authentication** for both `copilot` and `codex`.
- Scope this work to **`copilot` and `codex` only**.
- Do **not** read or migrate credentials from `~/.pi`.
- Add **OS keyring / credential storage** as a roadmap follow-up, not in the
  first implementation.

### Rejected

- Continuing to read `~/.pi/agent/auth.json`
- Automatic or one-time migration from `~/.pi`
- Designing around pi compatibility instead of tau ownership
- Manual token file editing as the primary UX for `copilot` or `codex`

---

## Goals

- Remove all direct token reads from `~/.pi`.
- Make first-time auth possible from inside tau.
- Make token renewal automatic and quiet when refresh tokens are valid.
- Keep auth logic testable and isolated from provider request/streaming code.
- Improve startup and runtime auth errors so users get clear next steps.

## Non-goals

- `openai` API-key auth redesign
- OS keychain storage in this phase
- Multi-process locking unless it becomes necessary
- Importing pi credentials for convenience

---

## Approach

Use a central auth subsystem in `src/auth/`.

### Why this approach

A minimal patch would just replace hardcoded paths, but that would leave auth
logic scattered across `src/llm/copilot.rs` and `src/llm/codex.rs`. A central
subsystem is slightly more work up front, but gives:

- clear ownership of storage and renewal
- better tests
- cleaner provider code
- an easier future migration to OS keyring storage

---

## Storage design

### Location

Store credentials in a tau-owned auth file resolved from a platform helper.

Expected locations:

- Linux: `$XDG_CONFIG_HOME/tau/auth.json` or `~/.config/tau/auth.json`
- macOS: `~/Library/Application Support/tau/auth.json`
- Windows: `%APPDATA%\\tau\\auth.json`

Implementation should use `directories::ProjectDirs` so the rest of the code
never hardcodes platform paths.

### File format

Use a tau-native JSON schema.

```json
{
  "version": 1,
  "providers": {
    "copilot": {
      "kind": "copilot",
      "access_token": "...",
      "refresh_token": "...",
      "expires_at": 1760000000,
      "base_url": "https://api.individual.githubcopilot.com"
    },
    "codex": {
      "kind": "codex",
      "access_token": "...",
      "refresh_token": "...",
      "expires_at": 1760000000,
      "account_id": "..."
    }
  }
}
```

### Persistence rules

- create parent directories as needed
- write atomically via temp file + rename
- use restrictive file permissions where supported
- preserve unrelated provider entries when updating a single provider
- preserve current credentials if a refresh attempt fails before new data is
  fully available

### Future-proofing

The auth store should sit behind a small interface so a later roadmap item can
move secrets into OS keyring storage while keeping the rest of the code stable.

---

## Module layout

```text
src/
  auth/
    mod.rs
    paths.rs
    types.rs
    store.rs
    copilot.rs
    codex.rs
```

### Responsibilities

- `auth/mod.rs`
  - top-level API used by the rest of the app
  - login dispatch
  - refresh dispatch
  - credential lookup

- `auth/paths.rs`
  - resolve the tau config directory and `auth.json` path

- `auth/types.rs`
  - credential structs
  - auth store schema
  - auth-specific error enums

- `auth/store.rs`
  - load/save auth file
  - atomic persistence
  - provider get/set/remove helpers

- `auth/copilot.rs`
  - GitHub device flow
  - Copilot token exchange
  - Copilot token refresh
  - base URL extraction from token

- `auth/codex.rs`
  - browser OAuth flow with localhost callback
  - access-token refresh
  - account ID extraction from token

---

## Interactive login UX

Add `/login [provider]`.

### Command behavior

- `/login` → show provider picker
- `/login copilot` → start Copilot device flow
- `/login codex` → start Codex browser OAuth flow

While login is active, the app enters a dedicated auth mode:

- show a modal or overlay over normal chat input
- disable regular chat submission
- allow `Esc` to cancel
- append concise success/failure status to the chat log when finished

### Copilot initial authentication

Use a GitHub device flow modeled after `pi-mono`.

Flow:

1. request device code from GitHub
2. show verification URL + one-time code in the TUI
3. try to open the browser
4. poll until the user completes login or cancels
5. exchange the result for a Copilot session token
6. persist credentials in tau auth storage

### Codex initial authentication

Use browser OAuth with localhost callback, modeled after `pi-mono`'s
OpenAI Codex flow.

Flow:

1. create PKCE verifier/challenge and state
2. open browser to OpenAI auth URL
3. listen on localhost callback endpoint
4. complete login automatically if callback arrives
5. exchange authorization code for tokens
6. extract `account_id` from the access token
7. persist credentials in tau auth storage

If callback setup fails or callback never arrives, fail clearly and ask the
user to retry `/login codex` in a suitable environment.

---

## Runtime integration

### Startup behavior

Provider construction should no longer read secrets directly inside provider
modules.

Instead:

1. `main.rs` selects provider/model
2. `provider::build_provider(...)` asks the auth layer for required
   credentials
3. if credentials are missing, return a structured unauthenticated error
4. app shows a clear message and remains usable

Example user-facing messages:

- `Not authenticated for GitHub Copilot. Run /login copilot.`
- `Not authenticated for Codex. Run /login codex.`

There should be **no silent fallback** to another provider.

### Provider integration boundary

Current direct reads from `~/.pi` must be removed from:

- `src/llm/copilot.rs`
- `src/llm/codex.rs`

These modules should receive already-resolved credentials from the auth layer
or from provider construction.

The provider modules should focus on request/streaming behavior, not file I/O
or token storage.

---

## Renewal design

Token renewal should be owned by auth.

### Copilot renewal

When credentials are expired or a request returns `401`:

1. use stored refresh/OAuth token to request a fresh Copilot session token
2. update auth storage atomically
3. retry the failed request once

### Codex renewal

When credentials are expired or a request returns `401`:

1. use stored refresh token to request a fresh access token
2. re-extract or update `account_id` if necessary
3. update auth storage atomically
4. retry the failed request once

### Retry policy

- one refresh attempt per failing request
- one request retry after successful refresh
- if refresh fails, stop retrying and surface a clear re-login message

Example:

- `Authentication expired for codex. Run /login codex again.`

---

## Error handling

Introduce auth-specific error types rather than relying on generic string
errors.

Suggested categories:

- `MissingCredentials`
- `ExpiredCredentials`
- `RefreshFailed`
- `LoginCancelled`
- `CallbackBindFailed`
- `Unauthorized`
- `StoreReadFailed`
- `StoreWriteFailed`

Rules:

- cancel returns the app to normal chat mode
- failed login does not clobber working credentials
- failed refresh does not overwrite last known good credentials
- raw network or JSON errors should be mapped to concise user-facing messages
  at the app layer

---

## Testing

### Unit tests

#### Path resolution

- resolves the correct tau app dir on each platform
- builds the expected `auth.json` path

#### Store

- missing file loads as empty state
- auth file round-trips for Copilot and Codex credentials
- single-provider updates preserve other provider entries
- atomic persistence behavior works as expected

#### Copilot auth

- parse proxy/base URL from token
- device flow response parsing
- refresh success/failure behavior

#### Codex auth

- callback request parsing/state validation
- extract account ID from access token
- refresh success/failure behavior

### Integration-shaped tests

- `build_provider` returns an unauthenticated error when creds are absent
- provider build succeeds when auth returns valid creds
- `401` triggers exactly one refresh and one retry
- failed refresh produces a re-login path instead of silent fallback

---

## Roadmap impact

Update the roadmap to reflect:

1. provider auth now means **interactive** first-time login plus refresh for
   `copilot` and `codex`
2. tau owns its own credentials and does not reuse `~/.pi`
3. OS keyring / credential storage is a separate follow-up item

---

## Recommended implementation phases

### Phase 1: auth foundation

- add `directories` dependency
- add `src/auth/{mod,paths,types,store}.rs`
- define credential schema and auth API
- implement atomic file persistence

### Phase 2: provider decoupling

- remove direct `~/.pi` reads from `src/llm/copilot.rs`
- remove direct `~/.pi` reads from `src/llm/codex.rs`
- thread resolved credentials through provider construction

### Phase 3: interactive login

- add `/login`
- add auth UI mode / overlay
- implement Copilot device flow
- implement Codex browser OAuth + localhost callback

### Phase 4: renewal

- add refresh-on-expiry behavior
- add refresh-on-401 behavior
- retry requests once after successful refresh

### Phase 5: polish and docs

- improve startup/runtime auth messages
- update README and roadmap
- add tests

---

## Notes from pi-mono inspiration

Useful references in `../pi-mono`:

- `packages/coding-agent/src/core/auth-storage.ts`
  - auth abstraction shape
  - per-provider persistence
  - refresh-on-demand ownership

- `packages/ai/src/utils/oauth/github-copilot.ts`
  - GitHub device flow
  - Copilot token refresh
  - base URL derivation from token

- `packages/ai/src/utils/oauth/openai-codex.ts`
  - browser OAuth + PKCE
  - localhost callback server
  - account ID extraction from access token

The tau design intentionally copies the *shape* of these flows, but not the
storage choice of `~/.pi/agent/auth.json`.
son`.
