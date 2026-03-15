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
behind the `LlmProvider` trait. Provider selected via `PIRS_PROVIDER` env
var or `--provider` / `-P` CLI flag.

### Provider and model configuration (partial)
Active provider and model shown in the Ctrl+I info bar. `/provider` and
`/model` slash commands with interactive picker menus. `--provider` and
`--model` CLI flags.

### Built-in tools
`bash` (💻), `read_file` (👀), `write` (✍️), `edit` (📝), `find` (🔍).

### Non-interactive mode
`--print` / `-p` flag streams the agent response to stdout and exits.

### Thinking output
`ThinkingToken` events rendered as dim text above the assistant answer.

---

## 🔴 High priority

### 1. Provider authentication / login redesign
Current pirs reads subscription credentials from `~/.pi`, which is the wrong
ownership model and effectively reuses another app's tokens. Missing creds also
cause a poor startup experience.

Redesign auth around a pirs-owned store in a platform-appropriate app config
location, with **interactive initial authentication** inside the TUI:
- **Copilot**: GitHub device flow — show verification URL and code, poll in
  the background, exchange for a Copilot session token, and store in pirs's
  own auth file.
- **Codex**: browser OAuth + localhost callback only (no manual token/code
  paste path).

Also: refresh expired tokens automatically and retry once on `401`, with a
clear re-login path if refresh fails.

See [design](plans/2026-03-15-provider-auth-redesign-design.md).

### 2. `ask_user` tool
Add a sixth built-in tool the model can call when it reaches a genuine
decision point — choosing between approaches, resolving an ambiguous
filename, obtaining a value it cannot infer. The model decides when to call
it; it is not an automatic gate before every operation.

The tool pauses the agent loop, surfaces the question in the TUI, accepts
the user's typed response via the normal input field, and returns the answer
as the tool result. The system prompt tells the model to use it only when
truly necessary.

See [plan](plans/2026-03-14-ask-user-tool.md).

### 3. Fix clippy warnings
`cargo clippy` reports 19 warnings — mostly collapsible `if` chains,
redundant closures, and simplifiable expressions. `codex.rs` suppresses
additional issues with `#![allow(dead_code)]`.

This violates the `AGENTS.md` coding policy ("fix all clippy issues before
committing"). Run `cargo clippy --fix` for the automatable subset; address
the rest manually. Remove the `#![allow(dead_code)]` from `codex.rs` and
either use or remove the dead items.

---

## 🟡 Medium priority

### 3. OS keyring / credential storage
After the auth redesign lands, move secrets out of `auth.json` and into the
platform credential store:
- macOS Keychain
- Windows Credential Manager
- Linux Secret Service / keyring

Keep only non-secret metadata in the pirs app config directory. This improves
security without coupling pirs back to `~/.pi`.

### 4. Config file
Provide a persistent `~/.config/pirs/config.toml` for API keys, default
provider, and per-provider default model. Env vars and CLI flags override
the config file. Eliminates the need to set env vars on every launch.

See [plan](plans/2026-03-14-config-file.md).

### 5. Context window management
Long agentic sessions silently degrade when the conversation history exceeds
the model's context window. Implement a soft-limit warning and a truncation
strategy (drop oldest non-system messages, or summarise) before the window
fills.

See [plan](plans/2026-03-14-context-management.md).

### 6. Tests
No unit or integration tests exist. Adds fragility risk for refactors and
new provider work.

See [plan](plans/2026-03-14-tests.md).

---

## 🟢 Lower priority

### 7. Anthropic provider
`AnthropicProvider` implementing `LlmProvider` against the Anthropic
Messages API (`/v1/messages`). Include native tool-calling support and
`thinking` block extraction.

### 8. Gemini provider
`GeminiProvider` implementing `LlmProvider` against the Google Gemini API.

### 9. Provider status discoverability
The active provider and model are hidden behind `Ctrl+I`. Show a minimal
status indicator (e.g. in the input panel border label) so new users know
which backend is active without having to discover the keybinding.

### 10. Fetch URL tool
`fetch` tool: HTTP GET a URL, return the response body (plain text or
truncated HTML). Useful for research tasks.

### 11. Conversation persistence
Save and restore conversation history across sessions. A simple JSONL log
per session under `~/.local/share/pirs/` would suffice.
