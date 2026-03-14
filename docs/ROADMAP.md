# Roadmap

Near-term features in rough priority order.

## 1. Tool-calling plumbing

Extend the core types to support the agentic loop:

- Add `ToolCall` and `ToolResult` variants to `AppEvent` and `Message`.
- Define a `Tool` trait (name, description, JSON schema for parameters,
  `invoke` method).
- Implement the loop in `App`: when the model returns a tool call, invoke
  the matching tool, append the result as a `ToolResult` message, and
  continue the conversation.
- Extend `LlmProvider::stream_chat` (or add a companion method) to send
  the tool schema to the model and parse tool-call responses.

## 2. Inline tool display

Render tool activity in the chat log without a separate pane:

- New `Role` variants: `ToolCall` and `ToolResult`.
- Distinct visual style for each: e.g. dim yellow for the call (function
  name + args), dim green for the result.
- Tool messages are pre-wrapped and scrollable like any other message.

## 3. Multi-provider support

Add backends for the major providers behind the existing `LlmProvider` trait:

- `OpenAiProvider` — OpenAI Chat Completions API (also covers
  OpenAI-compatible endpoints).
- `AnthropicProvider` — Anthropic Messages API.
- `GeminiProvider` — Google Gemini API.
- Provider selected via a `PIRS_PROVIDER` env var (or CLI flag).

## 4. Provider and model configuration

Make provider/model selection ergonomic without hardcoding env vars:

- Optional config file (`~/.config/pirs/config.toml` or similar) for API
  keys, default provider, default model per provider.
- Env vars override config file values.
- Show active provider + model in the UI (e.g. status bar or divider label).

## 5. Built-in tools

Ship a small set of first-party tools the agent can use out of the box:

- **Shell** — run a shell command, return stdout/stderr.
- **Read file** — read a local file path, return contents.
- **Write file** — write content to a local file path.
- **Fetch URL** — HTTP GET a URL, return the response body.

Tools enabled/disabled via config or a command-line flag.
