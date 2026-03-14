use anyhow::Context;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};

use super::{LlmEvent, LlmProvider, LlmStream, Message};

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

// ── Serde types for the Ollama /api/chat endpoint ────────────────────────────

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
}

#[derive(Deserialize)]
struct ChatChunk {
    message: ChunkMessage,
    #[serde(default)]
    done: bool,
}

#[derive(Deserialize)]
struct ChunkMessage {
    content: String,
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
                messages: messages
                    .iter()
                    .map(|m| OllamaMessage {
                        role: m.role.as_str(),
                        content: m.content.clone(),
                    })
                    .collect(),
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

                // Process every complete line in the buffer.
                while let Some(pos) = buf.find('\n') {
                    let line = buf[..pos].trim().to_string();
                    buf.drain(..=pos);

                    if line.is_empty() {
                        continue;
                    }

                    match serde_json::from_str::<ChatChunk>(&line) {
                        Ok(chunk) => {
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

            // Stream ended without a done=true (shouldn't happen with Ollama, but be safe).
            yield LlmEvent::Done;
        })
    }
}
