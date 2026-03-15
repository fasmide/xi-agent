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

---

## 🔴 High priority

### 1. Provider authentication / login redesign
Current tau reads subscription credentials from `~/.pi`, which is the wrong
ownership model and effectively reuses another app's tokens. Missing creds also
cause a poor startup experience.

Redesign auth around a tau-owned store in a platform-appropriate app config
location, with **interactive initial authentication** inside the TUI:
- **Copilot**: GitHub device flow — show verification URL and code, poll in
  the background, exchange for a Copilot session token, and store in tau's
  own auth file.
- **Codex**: browser OAuth + localhost callback only (no manual token/code
  paste path).

Also: refresh expired tokens automatically and retry once on `401`, with a
clear re-login path if refresh fails.

See [design](plans/2026-03-15-provider-auth-redesign-design.md).

### 2. CI quality gates
Automate the existing local quality policy in CI:
- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -D warnings`
- `cargo test`

This keeps the repository warning-free and prevents regressions.

---

## 🟡 Medium priority

### 3. OS keyring / credential storage
After the auth redesign lands, move secrets out of `auth.json` and into the
platform credential store:
- macOS Keychain
- Windows Credential Manager
- Linux Secret Service / keyring

Keep only non-secret metadata in the tau app config directory. This improves
security without coupling tau back to `~/.pi`.

### 4. Context window management
Long agentic sessions silently degrade when the conversation history exceeds
the model's context window. Implement a soft-limit warning and a truncation
strategy (drop oldest non-system messages, or summarise) before the window
fills.

See [plan](plans/2026-03-14-context-management.md).

### 5. Tests
A small unit-test baseline exists, but integration coverage is still missing.
Add integration-shaped tests for the agent loop and provider behavior to reduce
refactor risk.

See [plan](plans/2026-03-14-tests.md).

---

## 🟢 Lower priority

### 6. Anthropic provider
`AnthropicProvider` implementing `LlmProvider` against the Anthropic
Messages API (`/v1/messages`). Include native tool-calling support and
`thinking` block extraction.

### 7. Gemini provider
`GeminiProvider` implementing `LlmProvider` against the Google Gemini API.

### 8. Provider status discoverability
The active provider and model are hidden behind `Ctrl+I`. Show a minimal
status indicator (e.g. in the input panel border label) so new users know
which backend is active without having to discover the keybinding.

### 9. Fetch URL tool
`fetch` tool: HTTP GET a URL, return the response body (plain text or
truncated HTML). Useful for research tasks.

### 10. Session UX enhancements
Add richer session management on top of the initial persistence feature:
search/filter in the session picker, delete/rename sessions, and optional
preview metadata before loading.
