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

### 1. Tool confirmation UI
The agent currently executes `bash`, `write`, and `edit` calls silently,
without asking the user. This is the most significant safety gap.

Wire a user-approval step through the existing `before_tool_call` hook:
display a confirmation prompt in the TUI before each destructive tool
invocation, require an explicit `y` / `n` keystroke, and block or allow the
call accordingly. Read-only tools (`read_file`, `find`) can be auto-approved.

See [plan](plans/2026-03-14-tool-confirmation.md).

### 2. Fix clippy warnings
`cargo clippy` reports 19 warnings — mostly collapsible `if` chains,
redundant closures, and simplifiable expressions. `codex.rs` suppresses
additional issues with `#![allow(dead_code)]`.

This violates the `AGENTS.md` coding policy ("fix all clippy issues before
committing"). Run `cargo clippy --fix` for the automatable subset; address
the rest manually. Remove the `#![allow(dead_code)]` from `codex.rs` and
either use or remove the dead items.

---

## 🟡 Medium priority

### 3. Config file
Provide a persistent `~/.config/pirs/config.toml` for API keys, default
provider, and per-provider default model. Env vars and CLI flags override
the config file. Eliminates the need to set env vars on every launch.

See [plan](plans/2026-03-14-config-file.md).

### 4. Context window management
Long agentic sessions silently degrade when the conversation history exceeds
the model's context window. Implement a soft-limit warning and a truncation
strategy (drop oldest non-system messages, or summarise) before the window
fills.

See [plan](plans/2026-03-14-context-management.md).

### 5. Tests
No unit or integration tests exist. Adds fragility risk for refactors and
new provider work.

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

### 10. Conversation persistence
Save and restore conversation history across sessions. A simple JSONL log
per session under `~/.local/share/pirs/` would suffice.
