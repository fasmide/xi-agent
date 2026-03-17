use futures_util::StreamExt;
use std::collections::HashMap;

use super::{
    AssistantPhase, LlmEvent, LlmProvider, LlmStream, Message, ModelListFuture, Role,
    ToolDefinition,
};

pub struct AnthropicProvider {
    base_url: String,
    model: String,
    api_key: String,
    extra_headers: Vec<(String, String)>,
    /// When true, authenticate with `Authorization: Bearer …` (GitHub Copilot
    /// proxy).  When false, use `x-api-key` (direct Anthropic API).
    bearer_auth: bool,
    client: reqwest::Client,
}

impl AnthropicProvider {
    pub fn new_with_headers(
        base_url: impl Into<String>,
        model: impl Into<String>,
        api_key: impl Into<String>,
        bearer_auth: bool,
        extra_headers: Vec<(String, String)>,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            model: model.into(),
            api_key: api_key.into(),
            bearer_auth,
            extra_headers,
            client: reqwest::Client::new(),
        }
    }

    fn stream_inner(&self, messages: Vec<Message>, tools: Vec<ToolDefinition>) -> LlmStream {
        let url = format!("{}/v1/messages", self.base_url.trim_end_matches('/'));
        let model = self.model.clone();
        let api_key = self.api_key.clone();
        let extra_headers = self.extra_headers.clone();
        let bearer_auth = self.bearer_auth;
        let client = self.client.clone();

        Box::pin(async_stream::stream! {
            let system: Option<String> = messages
                .iter()
                .find(|m| m.role == Role::System)
                .map(|m| m.content.clone());

            let anthropic_messages = to_anthropic_messages(&messages);

            let mut body = serde_json::json!({
                "model": model,
                "messages": anthropic_messages,
                "max_tokens": 8192,
                "stream": true,
            });

            if let Some(sys) = system {
                body["system"] = serde_json::Value::String(sys);
            }

            if !tools.is_empty() {
                body["tools"] = serde_json::json!(convert_tools(&tools));
            }

            log::debug!(
                "[TAU_DEBUG] → anthropic request:\n{}",
                serde_json::to_string_pretty(&body).unwrap_or_default()
            );

            let mut req = client.post(&url).json(&body);
            if bearer_auth {
                req = req.bearer_auth(&api_key);
            } else {
                req = req.header("x-api-key", &api_key);
            }
            req = req.header("anthropic-version", "2023-06-01");

            let use_dynamic_initiator = extra_headers
                .iter()
                .any(|(k, _)| k.eq_ignore_ascii_case("X-Initiator"));
            for (k, v) in &extra_headers {
                if !k.eq_ignore_ascii_case("X-Initiator") {
                    req = req.header(k.as_str(), v.as_str());
                }
            }
            if use_dynamic_initiator {
                req = req.header("X-Initiator", infer_initiator(&messages));
            }

            let response = match req
                .send()
                .await
                .map_err(|e| format!("Failed to connect to Anthropic at {url}: {e}"))
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
                let preview: String = text.chars().take(1000).collect();
                log::warn!("anthropic api error: status={} body={}", status, preview);
                yield LlmEvent::Error(format!("Anthropic returned {status}: {text}"));
                return;
            }

            log::debug!("← HTTP {} from anthropic", response.status());

            let mut byte_stream = response.bytes_stream();
            let mut buf = String::new();
            // Track streaming tool_use blocks: content index → accumulated state.
            let mut tool_blocks: HashMap<u64, ToolBlock> = HashMap::new();
            let mut emitted_tool_intent = false;
            let mut line_num = 0usize;

            while let Some(chunk) = byte_stream.next().await {
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

                    // Skip blank lines and `event:` type lines — event type is
                    // redundant because each data payload carries a `type` field.
                    if raw.is_empty() || raw.starts_with(':') || raw.starts_with("event:") {
                        continue;
                    }

                    let data = match raw.strip_prefix("data:") {
                        Some(d) => d.trim(),
                        None => continue,
                    };

                    if data == "[DONE]" {
                        yield LlmEvent::Done;
                        return;
                    }

                    if !data.is_empty() {
                        log::debug!("[TAU_DEBUG] ← anthropic chunk {line_num}: {data}");
                        line_num += 1;
                    }

                    let ev: serde_json::Value = match serde_json::from_str(data) {
                        Ok(v) => v,
                        Err(e) => {
                            yield LlmEvent::Error(format!("Parse error: {e}"));
                            return;
                        }
                    };

                    match ev["type"].as_str() {
                        Some("content_block_start") => {
                            let index = ev["index"].as_u64().unwrap_or(0);
                            let block = &ev["content_block"];
                            if block["type"].as_str() == Some("tool_use") {
                                if !emitted_tool_intent {
                                    emitted_tool_intent = true;
                                    yield LlmEvent::ToolIntentStart;
                                }
                                tool_blocks.insert(
                                    index,
                                    ToolBlock {
                                        id: block["id"].as_str().unwrap_or("").to_string(),
                                        name: block["name"].as_str().unwrap_or("").to_string(),
                                        partial_json: String::new(),
                                    },
                                );
                            }
                        }

                        Some("content_block_delta") => {
                            let index = ev["index"].as_u64().unwrap_or(0);
                            let delta = &ev["delta"];
                            match delta["type"].as_str() {
                                Some("text_delta") => {
                                    if let Some(text) = delta["text"].as_str()
                                        && !text.is_empty()
                                    {
                                        yield LlmEvent::Token {
                                            text: text.to_string(),
                                            phase: if emitted_tool_intent {
                                                AssistantPhase::Provisional
                                            } else {
                                                AssistantPhase::Unknown
                                            },
                                        };
                                    }
                                }
                                Some("thinking_delta") => {
                                    if let Some(t) = delta["thinking"].as_str()
                                        && !t.is_empty()
                                    {
                                        yield LlmEvent::ThinkingToken(t.to_string());
                                    }
                                }
                                Some("input_json_delta") => {
                                    if let Some(partial) = delta["partial_json"].as_str()
                                        && let Some(block) = tool_blocks.get_mut(&index)
                                    {
                                        block.partial_json.push_str(partial);
                                    }
                                }
                                _ => {}
                            }
                        }

                        // When a tool_use block finishes, emit the accumulated call.
                        Some("content_block_stop") => {
                            let index = ev["index"].as_u64().unwrap_or(0);
                            if let Some(block) = tool_blocks.remove(&index) {
                                let args: serde_json::Value =
                                    serde_json::from_str(&block.partial_json)
                                        .unwrap_or(serde_json::Value::Object(Default::default()));
                                yield LlmEvent::ToolCall {
                                    id: block.id,
                                    name: block.name,
                                    args,
                                };
                            }
                        }

                        Some("message_stop") => {
                            yield LlmEvent::Done;
                            return;
                        }

                        Some("error") => {
                            let msg = ev["error"]["message"]
                                .as_str()
                                .unwrap_or("Anthropic API error")
                                .to_string();
                            yield LlmEvent::Error(msg);
                            return;
                        }

                        _ => {}
                    }
                }
            }

            yield LlmEvent::Done;
        })
    }
}

fn infer_initiator(messages: &[Message]) -> &'static str {
    match messages.last().map(|m| &m.role) {
        Some(Role::User) | None => "user",
        _ => "agent",
    }
}

// ── LlmProvider impl ──────────────────────────────────────────────────────────

impl LlmProvider for AnthropicProvider {
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
        let models = known_models();
        Box::pin(async move { models })
    }
}

// ── Known Claude models available via GitHub Copilot ─────────────────────────

fn known_models() -> Vec<String> {
    vec![
        "claude-sonnet-4.6".to_string(),
        "claude-opus-4.6".to_string(),
        "claude-sonnet-4.5".to_string(),
        "claude-opus-4.5".to_string(),
        "claude-sonnet-4".to_string(),
        "claude-haiku-4.5".to_string(),
    ]
}

// ── Message conversion ────────────────────────────────────────────────────────

fn normalize_tool_name(name: &str) -> &str {
    match name {
        "👀" => "read_file",
        "✍️" => "write_file",
        "📝" => "edit_file",
        "💻" => "bash",
        "🔍" => "find_files",
        other => other,
    }
}

/// Convert tau `Message` history into the Anthropic Messages API format.
///
/// Key differences from OpenAI:
/// - System messages are excluded (sent as a separate `system` top-level field).
/// - Tool calls live in `tool_use` content blocks inside the assistant turn.
/// - Tool results live in `tool_result` content blocks inside a user turn.
fn to_anthropic_messages(messages: &[Message]) -> Vec<serde_json::Value> {
    let mut result: Vec<serde_json::Value> = Vec::new();
    let mut i = 0;

    while i < messages.len() {
        let msg = &messages[i];

        match msg.role {
            Role::System => {
                i += 1;
            }

            Role::User => {
                result.push(serde_json::json!({
                    "role": "user",
                    "content": msg.content,
                }));
                i += 1;
            }

            Role::Assistant => {
                let mut content: Vec<serde_json::Value> = Vec::new();

                if !msg.content.is_empty() {
                    content.push(serde_json::json!({
                        "type": "text",
                        "text": msg.content,
                    }));
                }

                i += 1;

                // Collect tool calls and their results from this turn.
                let mut tool_results: Vec<serde_json::Value> = Vec::new();
                while i < messages.len() && messages[i].role == Role::ToolCall {
                    let tc = &messages[i];
                    content.push(serde_json::json!({
                        "type": "tool_use",
                        "id": tc.tool_call_id.as_deref().unwrap_or("call_0"),
                        "name": normalize_tool_name(tc.tool_name.as_deref().unwrap_or("")),
                        "input": tc.tool_args.clone().unwrap_or_default(),
                    }));
                    i += 1;

                    if i < messages.len() && messages[i].role == Role::ToolResult {
                        let tr = &messages[i];
                        tool_results.push(serde_json::json!({
                            "type": "tool_result",
                            "tool_use_id": tr.tool_call_id.as_deref().unwrap_or("call_0"),
                            "content": tr.content,
                            "is_error": tr.is_error,
                        }));
                        i += 1;
                    }
                }

                if content.is_empty() {
                    continue;
                }

                result.push(serde_json::json!({
                    "role": "assistant",
                    "content": content,
                }));

                if !tool_results.is_empty() {
                    result.push(serde_json::json!({
                        "role": "user",
                        "content": tool_results,
                    }));
                }
            }

            // Standalone ToolCall without a preceding assistant turn.
            Role::ToolCall => {
                let tc = msg;
                result.push(serde_json::json!({
                    "role": "assistant",
                    "content": [{
                        "type": "tool_use",
                        "id": tc.tool_call_id.as_deref().unwrap_or("call_0"),
                        "name": normalize_tool_name(tc.tool_name.as_deref().unwrap_or("")),
                        "input": tc.tool_args.clone().unwrap_or_default(),
                    }],
                }));
                i += 1;
            }

            // Standalone ToolResult without a preceding tool call in this pass.
            Role::ToolResult => {
                let tr = msg;
                result.push(serde_json::json!({
                    "role": "user",
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": tr.tool_call_id.as_deref().unwrap_or("call_0"),
                        "content": tr.content,
                        "is_error": tr.is_error,
                    }],
                }));
                i += 1;
            }
        }
    }

    result
}

fn convert_tools(tools: &[ToolDefinition]) -> Vec<serde_json::Value> {
    tools
        .iter()
        .map(|t| {
            serde_json::json!({
                "name": t.name,
                "description": t.description,
                "input_schema": t.parameters,
            })
        })
        .collect()
}

// ── Pending tool-use block accumulator ────────────────────────────────────────

struct ToolBlock {
    id: String,
    name: String,
    partial_json: String,
}
