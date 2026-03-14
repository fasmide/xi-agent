use anyhow::Context;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};

use super::{LlmEvent, LlmProvider, LlmStream, Message, ModelListFuture};

pub struct OllamaProvider {
    pub base_url: String,
    pub model: String,
    client: reqwest::Client,
}

impl OllamaProvider {
    pub fn new(base_url: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            model: model.into(),
            client: reqwest::Client::new(),
        }
    }

    /// Build from environment variables, falling back to defaults.
    #[allow(dead_code)]
    pub fn from_env() -> Self {
        let host = std::env::var("OLLAMA_HOST")
            .unwrap_or_else(|_| "http://localhost:11434".to_string());
        let model = std::env::var("OLLAMA_MODEL")
            .unwrap_or_else(|_| "llama3.1".to_string());
        Self::new(host, model)
    }
}

// ── Serde types ───────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<OllamaMessage>,
    stream: bool,
}

#[derive(Serialize)]
struct OllamaMessage {
    role: &'static str,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<String>,
}

#[derive(Deserialize)]
struct ChatChunk {
    message: ChunkMessage,
    #[serde(default)]
    done: bool,
}

#[derive(Deserialize)]
struct ChunkMessage {
    #[serde(default)]
    content: String,
    #[serde(default)]
    thinking: String,
}

// Serde types for the Ollama /api/tags endpoint.
#[derive(Deserialize)]
struct TagsResponse {
    models: Vec<TagModel>,
}

#[derive(Deserialize)]
struct TagModel {
    name: String,
}

// ── History serialisation ─────────────────────────────────────────────────────

/// Build the Ollama message for a past assistant turn.
/// If the message has associated thinking content, pass it in the `thinking`
/// field so reasoning models see their prior chain of thought in multi-turn
/// conversations.
fn to_ollama_message(msg: &Message) -> OllamaMessage {
    OllamaMessage {
        role: msg.role.as_str(),
        content: msg.content.clone(),
        thinking: msg.thinking.clone().filter(|t| !t.is_empty()),
    }
}

// ── Provider implementation ───────────────────────────────────────────────────

impl LlmProvider for OllamaProvider {
    fn stream_chat(&self, messages: Vec<Message>) -> LlmStream {
        let url = format!("{}/api/chat", self.base_url);
        let model = self.model.clone();
        let client = self.client.clone();

        Box::pin(async_stream::stream! {
            let body = ChatRequest {
                model,
                messages: messages.iter().map(to_ollama_message).collect(),
                stream: true,
            };

            let response = match client
                .post(&url)
                .json(&body)
                .send()
                .await
                .with_context(|| format!("Failed to connect to Ollama at {url}"))
            {
                Ok(r) => r,
                Err(e) => {
                    yield LlmEvent::Error(e.to_string());
                    return;
                }
            };

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                yield LlmEvent::Error(format!("Ollama returned {status}: {text}"));
                return;
            }

            let mut byte_stream = response.bytes_stream();

            // Ollama streams NDJSON: one JSON object per line.
            let mut buf = String::new();
            while let Some(chunk) = byte_stream.next().await {
                let bytes = match chunk.context("Error reading stream chunk") {
                    Ok(b) => b,
                    Err(e) => {
                        yield LlmEvent::Error(e.to_string());
                        return;
                    }
                };
                buf.push_str(&String::from_utf8_lossy(&bytes));

                // Process every complete NDJSON line in the buffer.
                while let Some(pos) = buf.find('\n') {
                    let line = buf[..pos].trim().to_string();
                    buf.drain(..=pos);

                    if line.is_empty() {
                        continue;
                    }

                    match serde_json::from_str::<ChatChunk>(&line) {
                        Ok(chunk) => {
                            if !chunk.message.thinking.is_empty() {
                                yield LlmEvent::ThinkingToken(chunk.message.thinking);
                            }
                            if !chunk.message.content.is_empty() {
                                yield LlmEvent::Token(chunk.message.content);
                            }
                            if chunk.done {
                                yield LlmEvent::Done;
                                return;
                            }
                        }
                        Err(e) => {
                            yield LlmEvent::Error(format!("Parse error: {e}"));
                            return;
                        }
                    }
                }
            }

            // Stream ended without done=true.
            yield LlmEvent::Done;
        })
    }

    fn list_models(&self) -> ModelListFuture {
        let url = format!("{}/api/tags", self.base_url);
        let client = self.client.clone();
        Box::pin(async move {
            let response = match client.get(&url).send().await {
                Ok(r) => r,
                Err(_) => return vec![],
            };
            let tags: TagsResponse = match response.json().await {
                Ok(t) => t,
                Err(_) => return vec![],
            };
            tags.models.into_iter().map(|m| m.name).collect()
        })
    }
}
