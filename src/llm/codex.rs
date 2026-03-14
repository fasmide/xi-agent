
use std::collections::HashMap;
use futures_util::StreamExt;

use super::{LlmEvent, LlmProvider, LlmStream, Message, ModelListFuture, Role, ToolDefinition};

const DEFAULT_BASE_URL: &str = "https://chatgpt.com/backend-api";

pub struct CodexProvider {
    base_url: String,
    model: String,
    api_key: String,
    account_id: String,
    client: reqwest::Client,
}

impl CodexProvider {
    pub fn new(
        base_url: impl Into<String>,
        model: impl Into<String>,
        api_key: impl Into<String>,
        account_id: impl Into<String>,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            model: model.into(),
            api_key: api_key.into(),
            account_id: account_id.into(),
            client: reqwest::Client::new(),
        }
    }

    pub fn from_env() -> anyhow::Result<Self> {
        let base_url = std::env::var("CODEX_BASE_URL")
            .unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());
        let model = std::env::var("OPENAI_MODEL")
            .unwrap_or_else(|_| "gpt-5.4".to_string());
        let (api_key, account_id) = read_codex_auth()?;
        Ok(Self::new(base_url, model, api_key, account_id))
    }

    pub fn with_model(&self, model: impl Into<String>) -> Self {
        Self {
            base_url: self.base_url.clone(),
            model: model.into(),
            api_key: self.api_key.clone(),
            account_id: self.account_id.clone(),
            client: self.client.clone(),
        }
    }

    fn stream_inner(&self, messages: Vec<Message>, tools: Vec<ToolDefinition>) -> LlmStream {
        let url = resolve_codex_url(&self.base_url);
        let model = self.model.clone();
        let api_key = self.api_key.clone();
        let account_id = self.account_id.clone();
        let client = self.client.clone();
        let debug = std::env::var("PIRS_DEBUG").is_ok();

        Box::pin(async_stream::stream! {
            // Separate system prompt from the rest.
            let instructions: Option<String> = messages.iter()
                .find(|m| m.role == Role::System)
                .map(|m| m.content.clone());

            let input: Vec<serde_json::Value> = convert_messages(&messages);

            let mut body = serde_json::json!({
                "model": model,
                "store": false,
                "stream": true,
                "input": input,
                "text": { "verbosity": "medium" },
                "include": ["reasoning.encrypted_content"],
                "tool_choice": "auto",
                "parallel_tool_calls": true,
            });

            if let Some(sys) = instructions {
                body["instructions"] = serde_json::Value::String(sys);
            }

            if !tools.is_empty() {
                body["tools"] = serde_json::json!(convert_tools(&tools));
            }

            if debug {
                eprintln!("[PIRS_DEBUG] → codex request:\n{}", serde_json::to_string_pretty(&body).unwrap_or_default());
            }

            let response = match client
                .post(&url)
                .header("Authorization", format!("Bearer {api_key}"))
                .header("chatgpt-account-id", &account_id)
                .header("originator", "pi")
                .header("OpenAI-Beta", "responses=experimental")
                .header("accept", "text/event-stream")
                .header("content-type", "application/json")
                .json(&body)
                .send()
                .await
                .map_err(|e| format!("Failed to connect to Codex at {url}: {e}"))
            {
                Ok(r) => r,
                Err(e) => { yield LlmEvent::Error(e); return; }
            };

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                yield LlmEvent::Error(format!("Codex returned {status}: {text}"));
                return;
            }

            let mut byte_stream = response.bytes_stream();
            let mut buf = String::new();
            // Track pending function calls keyed by item index or call_id.
            let mut pending_calls: HashMap<String, PendingCall> = HashMap::new();
            let mut current_call_item_id: Option<String> = None;
            let mut line_num = 0usize;

            while let Some(chunk) = byte_stream.next().await {
                let bytes = match chunk {
                    Ok(b) => b,
                    Err(e) => { yield LlmEvent::Error(e.to_string()); return; }
                };
                buf.push_str(&String::from_utf8_lossy(&bytes));

                // SSE uses double-newline as event separator, but we also
                // handle single-newline `data:` lines for robustness.
                loop {
                    // Find next complete SSE event (terminated by \n\n or \n).
                    let line_end = buf.find('\n');
                    if line_end.is_none() { break; }
                    let pos = line_end.unwrap();
                    let raw = buf[..pos].trim().to_string();
                    buf.drain(..=pos);

                    if raw.is_empty() || raw.starts_with(':') { continue; }

                    let data = match raw.strip_prefix("data:") {
                        Some(d) => d.trim(),
                        None => continue,
                    };

                    if data == "[DONE]" {
                        yield LlmEvent::Done;
                        return;
                    }

                    if debug {
                        eprintln!("[PIRS_DEBUG] ← chunk {line_num}: {data}");
                        line_num += 1;
                    }

                    let ev: serde_json::Value = match serde_json::from_str(data) {
                        Ok(v) => v,
                        Err(e) => { yield LlmEvent::Error(format!("Parse error: {e}")); return; }
                    };

                    let ev_type = ev["type"].as_str().unwrap_or("");

                    match ev_type {
                        // ── Text token ────────────────────────────────────────
                        "response.output_text.delta" => {
                            if let Some(delta) = ev["delta"].as_str()
                                && !delta.is_empty() {
                                    yield LlmEvent::Token(delta.to_string());
                                }
                        }

                        // ── Thinking / reasoning token ────────────────────────
                        "response.reasoning_summary_text.delta" => {
                            if let Some(delta) = ev["delta"].as_str()
                                && !delta.is_empty() {
                                    yield LlmEvent::ThinkingToken(delta.to_string());
                                }
                        }

                        // ── Function call started ─────────────────────────────
                        "response.output_item.added" => {
                            let item = &ev["item"];
                            if item["type"].as_str() == Some("function_call") {
                                let item_id = item["id"].as_str()
                                    .unwrap_or("")
                                    .to_string();
                                pending_calls.insert(item_id.clone(), PendingCall {
                                    arguments: String::new(),
                                });
                                current_call_item_id = Some(item_id);
                            }
                        }

                        // ── Function call arguments delta ─────────────────────
                        "response.function_call_arguments.delta" => {
                            let item_id = ev["item_id"].as_str().unwrap_or("");
                            if let Some(call) = pending_calls.get_mut(item_id) {
                                if let Some(delta) = ev["delta"].as_str() {
                                    call.arguments.push_str(delta);
                                }
                            } else if let Some(ref id) = current_call_item_id.clone()
                                && let Some(call) = pending_calls.get_mut(id)
                                    && let Some(delta) = ev["delta"].as_str() {
                                        call.arguments.push_str(delta);
                                    }
                        }

                        // ── Item completed ────────────────────────────────────
                        "response.output_item.done" => {
                            let item = &ev["item"];
                            if item["type"].as_str() == Some("function_call") {
                                let call_id = item["call_id"].as_str()
                                    .unwrap_or("")
                                    .to_string();
                                let name = item["name"].as_str()
                                    .unwrap_or("")
                                    .to_string();
                                let args_str = item["arguments"].as_str()
                                    .unwrap_or("{}");
                                let args: serde_json::Value = serde_json::from_str(args_str)
                                    .unwrap_or(serde_json::Value::Object(Default::default()));

                                // Remove from pending.
                                let item_id = item["id"].as_str().unwrap_or("");
                                pending_calls.remove(item_id);
                                if current_call_item_id.as_deref() == Some(item_id) {
                                    current_call_item_id = None;
                                }

                                yield LlmEvent::ToolCall { id: call_id, name, args };
                            }
                        }

                        // ── Stream complete ───────────────────────────────────
                        "response.completed" | "response.done" | "response.incomplete" => {
                            yield LlmEvent::Done;
                            return;
                        }

                        // ── Errors ────────────────────────────────────────────
                        "response.failed" => {
                            let msg = ev["response"]["error"]["message"]
                                .as_str()
                                .unwrap_or("Codex response failed")
                                .to_string();
                            yield LlmEvent::Error(msg);
                            return;
                        }
                        "error" => {
                            let msg = ev["message"].as_str()
                                .or_else(|| ev["code"].as_str())
                                .unwrap_or("Unknown error")
                                .to_string();
                            yield LlmEvent::Error(format!("Codex error: {msg}"));
                            return;
                        }

                        _ => {} // ignore other event types
                    }
                }
            }

            yield LlmEvent::Done;
        })
    }
}

// ── Auth ──────────────────────────────────────────────────────────────────────

fn read_codex_auth() -> anyhow::Result<(String, String)> {
    let home = std::env::var("HOME").map_err(|_| anyhow::anyhow!("$HOME not set"))?;
    let path = std::path::Path::new(&home).join(".pi/agent/auth.json");
    let content = std::fs::read_to_string(&path)
        .map_err(|e| anyhow::anyhow!("Cannot read {}: {}", path.display(), e))?;
    let v: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| anyhow::anyhow!("Cannot parse auth.json: {}", e))?;

    let entry = &v["openai-codex"];
    let access = entry["access"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No openai-codex.access in auth.json"))?
        .to_string();
    let account_id = entry["accountId"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No openai-codex.accountId in auth.json"))?
        .to_string();
    Ok((access, account_id))
}

fn resolve_codex_url(base_url: &str) -> String {
    let normalized = base_url.trim_end_matches('/');
    if normalized.ends_with("/codex/responses") {
        normalized.to_string()
    } else if normalized.ends_with("/codex") {
        format!("{normalized}/responses")
    } else {
        format!("{normalized}/codex/responses")
    }
}

// ── Message conversion ────────────────────────────────────────────────────────

/// Convert pirs `Message` history to the Responses API `input` array.
/// System messages are excluded (they go in `instructions`).
fn convert_messages(messages: &[Message]) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    let mut msg_idx = 0usize;

    for msg in messages {
        match msg.role {
            Role::System => {} // handled as `instructions`

            Role::User => {
                out.push(serde_json::json!({
                    "role": "user",
                    "content": [{ "type": "input_text", "text": msg.content }]
                }));
                msg_idx += 1;
            }

            Role::Assistant => {
                out.push(serde_json::json!({
                    "type": "message",
                    "role": "assistant",
                    "status": "completed",
                    "id": format!("msg_{msg_idx}"),
                    "content": [{ "type": "output_text", "text": msg.content, "annotations": [] }]
                }));
                msg_idx += 1;
            }

            Role::ToolCall => {
                let call_id = msg.tool_call_id.as_deref().unwrap_or("call_0");
                let name = msg.tool_name.as_deref().unwrap_or("");
                let args = msg.tool_args.as_ref()
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "{}".to_string());
                out.push(serde_json::json!({
                    "type": "function_call",
                    "call_id": call_id,
                    "name": name,
                    "arguments": args,
                }));
            }

            Role::ToolResult => {
                let call_id = msg.tool_call_id.as_deref().unwrap_or("call_0");
                out.push(serde_json::json!({
                    "type": "function_call_output",
                    "call_id": call_id,
                    "output": msg.content,
                }));
            }
        }
    }

    out
}

fn convert_tools(tools: &[ToolDefinition]) -> Vec<serde_json::Value> {
    tools.iter().map(|t| serde_json::json!({
        "type": "function",
        "name": t.name,
        "description": t.description,
        "parameters": t.parameters,
    })).collect()
}

// ── Pending call accumulator ──────────────────────────────────────────────────

struct PendingCall {
    arguments: String,
}

// ── Known models ──────────────────────────────────────────────────────────────

fn known_models() -> Vec<String> {
    vec![
        "gpt-5.4".to_string(),
        "gpt-5.4-pro".to_string(),
        "gpt-5.3-codex".to_string(),
        "gpt-5.3-codex-spark".to_string(),
        "gpt-5.2".to_string(),
        "gpt-5.2-pro".to_string(),
        "gpt-5.2-codex".to_string(),
        "gpt-5.1".to_string(),
        "gpt-5.1-codex".to_string(),
        "gpt-5.1-codex-max".to_string(),
        "gpt-5.1-codex-mini".to_string(),
        "gpt-5".to_string(),
        "gpt-5-codex".to_string(),
        "gpt-5-mini".to_string(),
        "codex-mini-latest".to_string(),
    ]
}

// ── LlmProvider impl ──────────────────────────────────────────────────────────

impl LlmProvider for CodexProvider {
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
