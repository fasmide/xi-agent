# OpenAI Provider Design

**Date:** 2026-03-14  
**Status:** Approved

## Goal

Add an `OpenAiProvider` that implements the existing `LlmProvider` trait using
the OpenAI Chat Completions API, covering OpenAI proper and OpenAI-compatible
endpoints (OpenRouter, Groq, Ollama, etc.). Replace the hardcoded
`OllamaProvider` in `main.rs` with `OpenAiProvider`.

## Architecture

### New file: `src/llm/openai.rs`

```rust
pub struct OpenAiProvider {
    base_url: String,   // e.g. "https://api.openai.com"
    model:    String,
    api_key:  String,
    client:   reqwest::Client,
}
```

#### Construction — `OpenAiProvider::from_env()`

Resolution order for each field:

| Field       | Env var            | Fallback                                      |
|-------------|--------------------|-----------------------------------------------|
| `model`     | `OPENAI_MODEL`     | `gpt-4o`                                      |
| `base_url`  | `OPENAI_BASE_URL`  | preset default or `https://api.openai.com`    |
| `api_key`   | `OPENAI_API_KEY`   | `openai-codex.access` in `~/.pi/agent/auth.json` |

**Named presets** are selected via `TAU_PRESET` env var and override the
base URL and the env var used for the API key:

| `TAU_PRESET`  | `base_url`                          | API key env var       |
|----------------|-------------------------------------|-----------------------|
| *(unset)*      | `https://api.openai.com`            | `OPENAI_API_KEY`      |
| `openrouter`   | `https://openrouter.ai/api`         | `OPENROUTER_API_KEY`  |
| `groq`         | `https://api.groq.com/openai`       | `GROQ_API_KEY`        |

For all presets (and the default), if the env-var API key is absent, the
provider falls back to reading `openai-codex.access` from
`~/.pi/agent/auth.json`. If neither source yields a key, `from_env()` returns
an error.

No automatic token refresh — if the token from `auth.json` is expired, the
first request will return a 401, which is surfaced as an `LlmEvent::Error`.

#### `LlmProvider::stream_chat` / `stream_chat_with_tools`

POST to `{base_url}/v1/chat/completions` with `stream: true`.

Response format is SSE — lines of the form `data: {…}`, terminated by
`data: [DONE]`. The SSE parser:

1. Reads the byte stream line-by-line.
2. Strips the `data: ` prefix; skips blank lines and comment lines.
3. Parses each JSON object into a `ChatCompletionChunk`.
4. Dispatches delta fields to `LlmEvent` variants:
   - `delta.content` → `LlmEvent::Token`
   - `delta.tool_calls` → accumulated across chunks (arguments arrive
     incrementally), emitted as `LlmEvent::ToolCall` once the chunk with
     `finish_reason: "tool_calls"` arrives.
   - `finish_reason: "stop"` or `[DONE]` → `LlmEvent::Done`.

Tool-call accumulation: maintain a `HashMap<u32, PartialToolCall>` keyed by
the delta's `index` field. Merge `id`, `name`, and `arguments` fragments
across chunks; emit all accumulated calls when `finish_reason` is
`"tool_calls"`.

Message serialisation for the request body follows the OpenAI schema directly:

| `Role`       | serialised `role` | extra fields                                 |
|--------------|-------------------|----------------------------------------------|
| `System`     | `"system"`        | —                                            |
| `User`       | `"user"`          | —                                            |
| `Assistant`  | `"assistant"`     | —                                            |
| `ToolCall`   | `"assistant"`     | `tool_calls: [{id, type:"function", function:{name, arguments}}]` |
| `ToolResult` | `"tool"`          | `tool_call_id`                               |

#### `LlmProvider::list_models`

GET `{base_url}/v1/models` → deserialise `{"data": [{"id": "…"}, …]}` →
return `Vec<String>` of model IDs. On any error, return an empty list.

### Changes to `main.rs`

- Remove all Ollama-specific env var reads (`OLLAMA_HOST`, `OLLAMA_MODEL`) and
  `OllamaProvider` construction.
- Call `OpenAiProvider::from_env()` at startup; surface any error to stderr and
  exit.
- Keep the model-change loop (`RunResult::ChangeModel`) — it now rebuilds
  `OpenAiProvider` with the new model name while preserving the same base URL
  and API key.

### No new Cargo dependencies

`reqwest` with `stream` and `json` features (already present) handles SSE
natively — lines are just read from the byte stream. `serde_json` handles all
JSON.

## Error Handling

- Connection failure → `LlmEvent::Error("Failed to connect to …: {e}")`
- Non-2xx HTTP → `LlmEvent::Error("OpenAI returned {status}: {body}")`
- JSON parse failure → `LlmEvent::Error("Parse error: {e}")`
- Missing API key at startup → `eprintln!` + process exit (not a streaming error)

## Out of Scope

- Automatic token refresh
- Config file (`~/.config/tau/config.toml`)
- Anthropic / Gemini providers
- Thinking / reasoning token support (no OpenAI equivalent at this time)
