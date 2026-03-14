#![allow(dead_code)]

use std::collections::HashMap;

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};

use super::{LlmEvent, LlmProvider, LlmStream, Message, ModelListFuture, Role, ToolDefinition};

pub struct OpenAiProvider {
    base_url: String,
    model: String,
    api_key: String,
    extra_headers: Vec<(String, String)>,
    client: reqwest::Client,
}

impl OpenAiProvider {
    pub fn new(
        base_url: impl Into<String>,
        model: impl Into<String>,
        api_key: impl Into<String>,
    ) -> Self {
        Self::new_with_headers(base_url, model, api_key, vec![])
    }

    pub fn new_with_headers(
        base_url: impl Into<String>,
        model: impl Into<String>,
        api_key: impl Into<String>,
        extra_headers: Vec<(String, String)>,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            model: model.into(),
            api_key: api_key.into(),
            extra_headers,
            client: reqwest::Client::new(),
        }
    }

    /// Build from environment variables, named presets, and/or auth.json.
    ///
    /// Resolution order:
    ///   - `PIRS_PRESET`   selects a named preset (openrouter, groq) which
    ///                     sets the default base URL and the API-key env var.
    ///   - `OPENAI_BASE_URL` overrides the preset base URL.
    ///   - `OPENAI_MODEL`    overrides the default model (gpt-4o).
    ///   - Preset key env var (e.g. `OPENROUTER_API_KEY`) is tried first,
    ///     then `OPENAI_API_KEY`, then `openai-codex.access` from
    ///     `~/.pi/agent/auth.json`.
    pub fn from_env() -> anyhow::Result<Self> {
        let preset = std::env::var("PIRS_PRESET").unwrap_or_default();

        let (default_base_url, preset_key_var): (&str, &str) = match preset.as_str() {
            "openrouter" => ("https://openrouter.ai/api/v1", "OPENROUTER_API_KEY"),
            "groq"       => ("https://api.groq.com/openai/v1", "GROQ_API_KEY"),
            _            => ("https://api.openai.com/v1", "OPENAI_API_KEY"),
        };

        let base_url =
            std::env::var("OPENAI_BASE_URL").unwrap_or_else(|_| default_base_url.to_string());
        let model =
            std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o".to_string());

        // API key: preset-specific env var → OPENAI_API_KEY → auth.json
        let api_key = std::env::var(preset_key_var)
            .or_else(|_| std::env::var("OPENAI_API_KEY"))
            .unwrap_or_else(|_| String::new());

        let api_key = if api_key.is_empty() {
            read_auth_json_token()?
        } else {
            api_key
        };

        Ok(Self::new(base_url, model, api_key))
    }

    /// Clone this provider, replacing only the model name.
    pub fn with_model(&self, model: impl Into<String>) -> Self {
        Self {
            base_url: self.base_url.clone(),
            model: model.into(),
            api_key: self.api_key.clone(),
            extra_headers: self.extra_headers.clone(),
            client: self.client.clone(),
        }
    }

    fn stream_inner(&self, messages: Vec<Message>, tools: Vec<ToolDefinition>) -> LlmStream {
        let url = format!("{}/chat/completions", self.base_url);
        let model = self.model.clone();
        let api_key = self.api_key.clone();
        let extra_headers = self.extra_headers.clone();
        let client = self.client.clone();
        let debug = std::env::var("PIRS_DEBUG").is_ok();

        Box::pin(async_stream::stream! {
            let oai_messages: Vec<OaiMessage> = messages.iter().map(to_oai_message).collect();

            let body = ChatRequest {
                model,
                messages: oai_messages,
                stream: true,
                tools: if tools.is_empty() {
                    None
                } else {
                    Some(tools.iter().map(to_oai_tool).collect())
                },
            };

            if debug {
                if let Ok(json) = serde_json::to_string_pretty(&body) {
                    eprintln!("[PIRS_DEBUG] → request:\n{json}");
                }
            }

            let mut req = client
                .post(&url)
                .bearer_auth(&api_key)
                .json(&body);
            for (k, v) in &extra_headers {
                req = req.header(k.as_str(), v.as_str());
            }
            let response = match req
                .send()
                .await
                .map_err(|e| format!("Failed to connect to OpenAI at {url}: {e}"))
            {
                Ok(r) => r,
                Err(e) => {
                    yield LlmEvent::Error(e);
                    return;
                }
            };

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                yield LlmEvent::Error(format!("OpenAI returned {status}: {text}"));
                return;
            }

            let mut byte_stream = response.bytes_stream();
            let mut buf = String::new();
            // Accumulate partial tool-call deltas keyed by index.
            let mut tool_calls: HashMap<u32, PartialToolCall> = HashMap::new();
            let mut line_num = 0usize;

            'outer: while let Some(chunk) = byte_stream.next().await {
                let bytes = match chunk {
                    Ok(b) => b,
                    Err(e) => {
                        yield LlmEvent::Error(e.to_string());
                        return;
                    }
                };
                buf.push_str(&String::from_utf8_lossy(&bytes));

                while let Some(pos) = buf.find('\n') {
                    let raw = buf[..pos].trim().to_string();
                    buf.drain(..=pos);

                    if raw.is_empty() || raw.starts_with(':') {
                        continue; // blank line or SSE comment
                    }

                    // Strip "data: " prefix.
                    let line = if let Some(rest) = raw.strip_prefix("data: ") {
                        rest.trim()
                    } else {
                        continue; // non-data SSE field (event:, id:, retry:)
                    };

                    if line == "[DONE]" {
                        // Flush any accumulated tool calls.
                        let mut calls: Vec<PartialToolCall> = {
                            let mut v: Vec<(u32, PartialToolCall)> = tool_calls.drain().collect();
                            v.sort_by_key(|(i, _)| *i);
                            v.into_iter().map(|(_, tc)| tc).collect()
                        };
                        for (i, tc) in calls.iter_mut().enumerate() {
                            let args: serde_json::Value =
                                serde_json::from_str(&tc.arguments).unwrap_or(serde_json::Value::Null);
                            yield LlmEvent::ToolCall {
                                id: tc.id.clone().unwrap_or_else(|| format!("call_{i}")),
                                name: tc.name.clone(),
                                args,
                            };
                        }
                        yield LlmEvent::Done;
                        break 'outer;
                    }

                    if debug && !line.is_empty() {
                        eprintln!("[PIRS_DEBUG] ← chunk {line_num}: {line}");
                        line_num += 1;
                    }

                    let chunk: ChatChunk = match serde_json::from_str(line) {
                        Ok(c) => c,
                        Err(e) => {
                            yield LlmEvent::Error(format!("Parse error: {e}"));
                            return;
                        }
                    };

                    for choice in chunk.choices {
                        let delta = choice.delta;

                        // Text tokens.
                        if let Some(content) = delta.content {
                            if !content.is_empty() {
                                yield LlmEvent::Token(content);
                            }
                        }

                        // Tool-call delta fragments — merge into accumulator.
                        for tc_delta in delta.tool_calls {
                            let entry = tool_calls
                                .entry(tc_delta.index)
                                .or_insert_with(PartialToolCall::default);
                            if let Some(id) = tc_delta.id {
                                entry.id = Some(id);
                            }
                            if let Some(name) = tc_delta.function.name {
                                entry.name.push_str(&name);
                            }
                            if let Some(args) = tc_delta.function.arguments {
                                entry.arguments.push_str(&args);
                            }
                        }

                        // When finish_reason == "tool_calls" the arguments are
                        // complete.  We flush at [DONE] above, but also handle
                        // it here for providers that set finish_reason before
                        // [DONE] on the same or next chunk.
                        if choice.finish_reason.as_deref() == Some("stop") && tool_calls.is_empty() {
                            // Normal text finish — Done will be emitted at [DONE].
                        }
                    }
                }
            }
        })
    }
}

// ── Auth ──────────────────────────────────────────────────────────────────────

fn read_auth_json_token() -> anyhow::Result<String> {
    let home = std::env::var("HOME")
        .map_err(|_| anyhow::anyhow!("$HOME not set"))?;
    let path = std::path::Path::new(&home).join(".pi/agent/auth.json");
    let content = std::fs::read_to_string(&path)
        .map_err(|e| anyhow::anyhow!("Cannot read {}: {}", path.display(), e))?;
    let v: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| anyhow::anyhow!("Cannot parse auth.json: {}", e))?;
    v["openai-codex"]["access"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("No openai-codex.access token in ~/.pi/agent/auth.json"))
}

// ── Serde types ───────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<OaiMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OaiToolDef>>,
}

#[derive(Serialize)]
struct OaiMessage {
    role: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OaiToolCallItem>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Serialize)]
struct OaiToolCallItem {
    id: String,
    r#type: &'static str,
    function: OaiToolCallFunction,
}

#[derive(Serialize)]
struct OaiToolCallFunction {
    name: String,
    arguments: String, // JSON-encoded string
}

#[derive(Serialize)]
struct OaiToolDef {
    r#type: &'static str,
    function: OaiFunctionDef,
}

#[derive(Serialize)]
struct OaiFunctionDef {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

// ── SSE response types ────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ChatChunk {
    choices: Vec<ChunkChoice>,
}

#[derive(Deserialize)]
struct ChunkChoice {
    delta: Delta,
    finish_reason: Option<String>,
}

#[derive(Deserialize, Default)]
struct Delta {
    content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<ToolCallDelta>,
}

#[derive(Deserialize)]
struct ToolCallDelta {
    index: u32,
    id: Option<String>,
    #[serde(default)]
    function: ToolCallFunctionDelta,
}

#[derive(Deserialize, Default)]
struct ToolCallFunctionDelta {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Default)]
struct PartialToolCall {
    id: Option<String>,
    name: String,
    arguments: String,
}

// ── Model list response ───────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ModelsResponse {
    data: Vec<ModelEntry>,
}

#[derive(Deserialize)]
struct ModelEntry {
    id: String,
}

// ── Serialisation helpers ─────────────────────────────────────────────────────

fn to_oai_message(msg: &Message) -> OaiMessage {
    match msg.role {
        Role::ToolCall => OaiMessage {
            role: "assistant",
            content: None,
            tool_calls: Some(vec![OaiToolCallItem {
                id: msg.tool_call_id.clone().unwrap_or_else(|| "call_0".to_string()),
                r#type: "function",
                function: OaiToolCallFunction {
                    name: msg.tool_name.clone().unwrap_or_default(),
                    arguments: msg
                        .tool_args
                        .as_ref()
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "{}".to_string()),
                },
            }]),
            tool_call_id: None,
        },
        Role::ToolResult => OaiMessage {
            role: "tool",
            content: Some(msg.content.clone()),
            tool_calls: None,
            tool_call_id: msg.tool_call_id.clone(),
        },
        Role::System => OaiMessage {
            role: "system",
            content: Some(msg.content.clone()),
            tool_calls: None,
            tool_call_id: None,
        },
        Role::User => OaiMessage {
            role: "user",
            content: Some(msg.content.clone()),
            tool_calls: None,
            tool_call_id: None,
        },
        Role::Assistant => OaiMessage {
            role: "assistant",
            content: Some(msg.content.clone()),
            tool_calls: None,
            tool_call_id: None,
        },
    }
}

fn to_oai_tool(tool: &ToolDefinition) -> OaiToolDef {
    OaiToolDef {
        r#type: "function",
        function: OaiFunctionDef {
            name: tool.name.clone(),
            description: tool.description.clone(),
            parameters: tool.parameters.clone(),
        },
    }
}

// ── LlmProvider impl ──────────────────────────────────────────────────────────

impl LlmProvider for OpenAiProvider {
    fn stream_chat(&self, messages: Vec<Message>) -> LlmStream {
        self.stream_inner(messages, vec![])
    }

    fn stream_chat_with_tools(
        &self,
        messages: Vec<Message>,
        tools: Vec<ToolDefinition>,
    ) -> LlmStream {
        self.stream_inner(messages, tools)
    }

    fn list_models(&self) -> ModelListFuture {
        let url = format!("{}/models", self.base_url);
        let api_key = self.api_key.clone();
        let extra_headers = self.extra_headers.clone();
        let client = self.client.clone();
        Box::pin(async move {
            let mut req = client.get(&url).bearer_auth(&api_key);
            for (k, v) in &extra_headers {
                req = req.header(k.as_str(), v.as_str());
            }
            let response = match req.send().await {
                Ok(r) => r,
                Err(_) => return vec![],
            };
            let models: ModelsResponse = match response.json().await {
                Ok(m) => m,
                Err(_) => return vec![],
            };
            let mut ids: Vec<String> = models.data.into_iter().map(|m| m.id).collect();
            ids.sort();
            ids
        })
    }
}
