# Provider Authentication / Login

**Date:** 2026-03-14  
**Status:** Superseded  
**Priority:** High

> Superseded by `docs/plans/2026-03-15-provider-auth-redesign-design.md`.
> This earlier plan assumed `~/.pi` compatibility and a non-interactive Codex
> flow, which no longer matches the approved design.

## Problem

Every provider requires credentials that currently must be obtained and placed
manually before tau can be used:

| Provider | What is needed | Where it is read |
|----------|----------------|------------------|
| `copilot` | Copilot session token (GitHub OAuth device flow) | `~/.pi/agent/auth.json` → `github-copilot.access` |
| `openai`  | API key | `OPENAI_API_KEY` env var or `~/.pi/agent/auth.json` → `openai-codex.access` |
| `codex`   | chatgpt.com session token + account ID | `~/.pi/agent/auth.json` → `openai-codex` |
| `ollama`  | none | — |

If the credentials are absent, `build_provider` returns an error and tau
falls back to Copilot with a terse inline message. There is no way to
authenticate from inside tau itself.

## Goal

- Add a `/login [provider]` slash command.
- Implement the per-provider auth flows in a new `src/auth/` module.
- Store credentials in `~/.pi/agent/auth.json` (compatible with pi's format).
- On startup, when a provider has no credentials, show a clear actionable
  message instead of a crash/fallback.
- On API 401 responses, prompt the user to re-login rather than silently
  failing.

## Scope

| Provider | Flow | In scope |
|----------|------|----------|
| `copilot` | GitHub OAuth device flow + Copilot token exchange | ✅ yes |
| `openai`  | Prompt for API key, store in `auth.json` | ✅ yes |
| `codex`   | Requires browser-extracted chatgpt.com session; not automatable | ❌ out of scope — document manual steps |
| `ollama`  | No auth | n/a |

---

## Auth storage format

Credentials are stored in `~/.pi/agent/auth.json`. This file is shared with
pi and must remain compatible with its format. The relevant keys:

```json
{
  "github-copilot": {
    "access": "<copilot-session-token>",
    "refresh": "<github-oauth-token>"
  },
  "openai-codex": {
    "access": "<openai-api-key-or-codex-session>",
    "accountId": "<chatgpt-account-id>"
  }
}
```

`auth.json` is read/written atomically (write to a temp file, rename).
Existing keys are preserved when only one provider's credentials are updated.

A new `src/auth/store.rs` module owns all reads and writes:

```rust
pub struct AuthStore {
    path: PathBuf,   // ~/.pi/agent/auth.json
}

impl AuthStore {
    pub fn load() -> Self;
    pub fn get_copilot(&self) -> Option<CopilotCreds>;
    pub fn set_copilot(&mut self, creds: CopilotCreds) -> anyhow::Result<()>;
    pub fn get_openai(&self) -> Option<String>;       // API key
    pub fn set_openai(&mut self, key: String) -> anyhow::Result<()>;
}

pub struct CopilotCreds {
    pub access:  String,   // Copilot session token (contains proxy-ep=…)
    pub refresh: String,   // GitHub OAuth token used to refresh the session
}
```

---

## Copilot: GitHub OAuth device flow

### Overview

```
tau                      GitHub                     Copilot API
 │                            │                           │
 ├─ POST /login/device/code ──►                           │
 │◄─ device_code, user_code ──┤                           │
 │                            │                           │
 │  show user_code + URL       │                           │
 │  (poll loop)                │                           │
 │                            │                           │
 ├─ POST /login/oauth/access_token (polling) ─────────────►
 │◄─ access_token ────────────┤                           │
 │                            │                           │
 ├─ GET /copilot_internal/v2/token (Bearer access_token) ─►
 │◄─ copilot session token ───────────────────────────────┤
 │                            │                           │
 │  store { access: session, refresh: oauth_token }        │
```

### Endpoints

| Step | Method | URL |
|------|--------|-----|
| Request device code | POST | `https://github.com/login/device/code` |
| Poll for OAuth token | POST | `https://github.com/login/oauth/access_token` |
| Exchange for Copilot session | GET | `https://api.github.com/copilot_internal/v2/token` |

Request device code body:
```json
{ "client_id": "<github-app-client-id>", "scope": "read:user" }
```

> The `client_id` is the GitHub OAuth App registered for Copilot access.
> tau should use its own registered OAuth App; during development the
> VS Code Copilot extension's public `client_id` can be used as a
> placeholder (`Iv1.b507a08c87ecfe98`).

### TUI during device flow

A new `LoginState::DeviceFlow` variant in `App`:

```rust
pub enum LoginState {
    Idle,
    DeviceFlow {
        provider: ProviderKind,
        user_code: String,
        verification_uri: String,
        expires_at: std::time::Instant,
    },
    ApiKeyPrompt {
        provider: ProviderKind,
    },
    Success(ProviderKind),
    Failed { provider: ProviderKind, reason: String },
}
```

While `DeviceFlow` is active, `ui.rs` renders an overlay above the input:

```
┌─────────────────────────────────────────────────────────────────┐
│  🔑 GitHub Copilot login                                        │
│                                                                 │
│  Open:  https://github.com/login/device                        │
│  Code:  ABCD-1234                                               │
│                                                                 │
│  Waiting for authorisation… (expires in 14:32)                 │
└─────────────────────────────────────────────────────────────────┘
```

Input is disabled during the flow. `Esc` cancels.

Polling runs in a background `tokio::spawn` task; progress is sent to `App`
via the existing `event_rx` channel using a new `AgentEvent::LoginProgress`
variant, or a dedicated `login_rx` channel (preferred, to keep concerns
separate).

### Copilot token refresh

Copilot session tokens expire (typically within 30 minutes). When
`stream_chat_with_tools` returns a 401, `App` should:

1. Use the stored `refresh` (GitHub OAuth token) to call
   `GET /copilot_internal/v2/token` again.
2. Update the session token in `AuthStore`.
3. Rebuild the provider and retry the last request automatically.

A `max_refresh_attempts = 1` guard prevents infinite retry loops.

---

## OpenAI: API key prompt

`/login openai` sets `app.login_state = LoginState::ApiKeyPrompt { provider: OpenAi }`.

The input border label changes to `" OpenAI API key "` and the textarea
content is masked (rendered as `•` characters). On `Enter`:

1. The key is written to `AuthStore` (`openai-codex.access`).
2. `build_provider` is called with the new key.
3. Provider switches to `openai` and the conversation continues.

Masking implementation: `tui-textarea` does not natively mask input. Options:
- Intercept every character event and store plaintext in a separate `String`
  while rendering `•` into the textarea.
- Or use a separate `masked_input: String` field in `App` and render it
  separately (bypassing `tui-textarea` entirely for this mode).

The second approach is simpler and avoids fighting `tui-textarea`.

---

## Startup behaviour when credentials are missing

Currently `build_provider` errors out silently. The new behaviour:

1. `build_provider` returns a new `ProviderError::Unauthenticated(ProviderKind)` variant.
2. `main.rs` catches this and calls `app.prompt_login(kind)` instead of
   falling back to Copilot.
3. The chat log shows:
   ```
   Not logged in to Copilot. Run /login to authenticate.
   ```
4. The app remains usable — the user can `/login` or switch provider.

---

## `/login` slash command

Add to `commands.rs`:

```rust
SlashCommand { name: "login", usage: "/login [provider]", description: "Authenticate with a provider", takes_arg: true }
```

`CommandAction::Login(Option<String>)` — if no provider given, open the
provider picker first, then start the login flow for the selection.

Add `CommandAction::Login` handling to the `Enter` branch in `run()`.

---

## New module layout

```
src/
  auth/
    mod.rs       — re-exports; top-level login dispatch
    store.rs     — AuthStore: read/write auth.json
    copilot.rs   — GitHub device-flow + Copilot token exchange
    openai.rs    — API key prompt helpers
```

---

## Implementation tasks

1. `src/auth/store.rs` — `AuthStore`, `CopilotCreds`; atomic JSON read/write.
2. `src/auth/copilot.rs` — device-flow async function returning a stream of
   `DeviceFlowEvent::{Pending{user_code, uri, expires_in}, Success, Expired, Error}`.
3. `src/auth/openai.rs` — `save_api_key(key: &str)` using `AuthStore`.
4. `src/auth/mod.rs` — `login(provider, tx)` dispatcher.
5. `App` — add `login_state: LoginState`, `login_rx`, `masked_input` fields;
   `App::prompt_login(kind)`, `App::apply_login_event(ev)`.
6. `ui.rs` — render `LoginState::DeviceFlow` overlay; mask API key input.
7. `commands.rs` / `main.rs` — wire `/login` command and `CommandAction::Login`.
8. `provider.rs` — `build_provider` returns `ProviderError::Unauthenticated`
   when credentials are absent.
9. `llm/openai.rs` — detect 401, emit a new `LlmEvent::Unauthorized` that
   `App` handles by triggering the Copilot token refresh path.
10. Update `README.md` with the `/login` command and first-run instructions.
