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

### Provider authentication redesign (initial)
Auth is tau-owned and no longer depends on `~/.pi` credentials.
Implemented pieces include:
- `/login` flows for `copilot` (device flow) and `codex` (browser OAuth callback)
- provider startup checks with clear guidance (e.g. `Run /login <provider>`)
- token refresh support in auth modules plus one in-app retry path on auth `401`

---

## 🔴 High priority

### 1. Authentication hardening and coverage
Initial auth redesign is implemented; remaining work is reliability and test coverage:
- extend refresh/retry handling to remaining auth edge paths (notably non-TUI/print mode)
- tighten recovery UX and first-run messaging around expired/missing credentials
- add focused integration tests for auth and provider startup/retry paths

Reference design: [provider auth redesign](plans/2026-03-15-provider-auth-redesign-design.md).

---

## 🟡 Medium priority

### 2. OS keyring / credential storage
After the auth redesign lands, move secrets out of `auth.toml` and into the
platform credential store:
- macOS Keychain
- Windows Credential Manager
- Linux Secret Service / keyring

Keep only non-secret metadata in the tau app config directory. This improves
security without coupling tau back to `~/.pi`.

### 3. Context window management
Long agentic sessions silently degrade when the conversation history exceeds
the model's context window. Implement a soft-limit warning and a truncation
strategy (drop oldest non-system messages, or summarise) before the window
fills.

See [plan](plans/2026-03-14-context-management.md).

### 4. Tests
A small unit-test baseline exists, but integration coverage is still missing.
Add integration-shaped tests for the agent loop and provider behavior to reduce
refactor risk.

See [plan](plans/2026-03-14-tests.md).

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
