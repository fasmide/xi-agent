# Roadmap

Items are grouped by status and then by priority within each group.

---

## ✅ Done

### Tool-calling plumbing
`ToolCall` / `ToolResult` roles in `Message`; `Tool` trait; `ToolRegistry`;
`LlmProvider::stream_chat_with_tools`; multi-turn `run_agent_loop` in
`agent/mod.rs`.

### Inline tool display
`Role::ToolCall` and `Role::ToolResult` rendered in the chat log with
distinct styles (yellow call block, green result block).

### Multi-provider support
`OpenAiProvider`, `CopilotProvider`, `CodexProvider`, `OllamaProvider` all
behind the `LlmProvider` trait. Provider selected via `TAU_PROVIDER` env
var or `--provider` / `-P` CLI flag.

### Provider and model configuration
Active provider and model shown in the Ctrl+I info bar. `/provider` and
`/model` slash commands with interactive picker menus. `--provider` and
`--model` CLI flags.

### Config file (initial)
Optional config file at `$XDG_CONFIG_HOME/tau/config.toml` (fallback
`~/.config/tau/config.toml`) supports default provider and per-provider
settings (`[openai]`, `[copilot]`, `[codex]`, `[ollama]`). Resolution order:
CLI flags > env vars > config file > built-in defaults.

### Built-in tools
`bash` (💻), `read_file` (👀), `write` (✍️), `edit` (📝), `find` (🔍),
`ask_user` (❓).

### Non-interactive mode
`--print` / `-p` flag streams the agent response to stdout and exits.

### Thinking output
`ThinkingToken` events rendered as dim text above the assistant answer.

### Session persistence (initial)
Conversation history is persisted to session files under the tau data dir,
keyed by working folder metadata. `/resume` opens a picker for local + foreign
sessions, and `Ctrl+R` resumes the latest session for the current folder.

### Provider authentication redesign
Auth is tau-owned and no longer depends on `~/.pi` credentials.
- `src/auth/` subsystem: paths, types, atomic store (temp+rename, 0o600), copilot and codex modules
- `/login` flows for `copilot` (device flow) and `codex` (browser OAuth + localhost callback)
- TUI login overlay with cancel (Esc), progress events, and success/failure feedback in chat log
- provider startup checks with clear guidance (`Not authenticated… Run /login <provider>`)
- 401 → silent refresh → one retry in TUI mode; refresh failure surfaces a re-login prompt

---

## 🔴 High priority

### 1. Tests ✓ done
Tool implementations (`bash`, `read_file`, `edit_file`, `write_file`), agent
loop (`MockProvider` + 4 scenarios), `Role`/`Message` serialisation, and
`wrap_str` UI arithmetic are all covered. Auth store was done in a prior pass.

Remaining gap (tracked separately as #2 below):
- **Agent loop 401 path**: 401 triggers exactly one refresh and one retry; failed
  refresh produces a re-login prompt — this logic lives in `App::apply_event`
  and is tested implicitly via manual QA. Unit tests require a heavier `App`
  harness or extraction of the 401 logic.

### 2. HTTP-dependent auth tests

---

## 🟡 Medium priority

### 2. Auth edge-case hardening
The TUI refresh/retry path works. Two small gaps remain:

- **Print mode (`--print`)**: a 401 mid-stream is surfaced as a plain error
  with no refresh+retry; add the same one-shot refresh logic that the TUI loop
  already has.
- **Proactive expiry check**: `build_provider` hands expired credentials
  straight to the provider, causing an avoidable 401 round-trip; check
  `expires_at` at provider-build time and refresh before connecting.

### 3. OS keyring / credential storage
Move secrets out of `auth.toml` and into the platform credential store:
- macOS Keychain
- Windows Credential Manager
- Linux Secret Service / keyring

Keep only non-secret metadata in the tau app config directory. This improves
security and is a natural follow-on now that the auth subsystem has a clean
storage interface.

### 4. Context window management
Long agentic sessions silently degrade when the conversation history exceeds
the model's context window. Implement a soft-limit warning and a truncation
strategy (drop oldest non-system messages, or summarise) before the window
fills.

See [plan](plans/2026-03-14-context-management.md).

---

## 🟢 Lower priority

### 5. Anthropic provider
`AnthropicProvider` implementing `LlmProvider` against the Anthropic
Messages API (`/v1/messages`). Include native tool-calling support and
`thinking` block extraction.

### 6. Gemini provider
`GeminiProvider` implementing `LlmProvider` against the Google Gemini API.

### 7. Provider status discoverability
The active provider and model are hidden behind `Ctrl+I`. Show a minimal
status indicator (e.g. in the input panel border label) so new users know
which backend is active without having to discover the keybinding.

### 8. Fetch URL tool
`fetch` tool: HTTP GET a URL, return the response body (plain text or
truncated HTML). Useful for research tasks.

### 9. Session UX enhancements
Add richer session management on top of the initial persistence feature.
Preview metadata is already shown in the picker; remaining work includes
search/filter plus delete/rename operations.
