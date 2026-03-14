use anyhow::Context;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::UnboundedSender;

use super::{LlmEvent, LlmProvider, Message};

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
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<OllamaMessage<'a>>,
    stream: bool,
}

#[derive(Serialize)]
struct OllamaMessage<'a> {
    role: &'a str,
    content: &'a str,
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
    async fn stream_chat(
        &self,
        messages: &[Message],
        tx: UnboundedSender<LlmEvent>,
    ) -> anyhow::Result<()> {
        let url = format!("{}/api/chat", self.base_url);

        let body = ChatRequest {
            model: &self.model,
            messages: messages
                .iter()
                .map(|m| OllamaMessage {
                    role: m.role.as_str(),
                    content: &m.content,
                })
                .collect(),
            stream: true,
        };

        let response = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .with_context(|| format!("Failed to connect to Ollama at {url}"))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("Ollama returned {status}: {text}");
        }

        let mut stream = response.bytes_stream();

        // Ollama streams NDJSON: one JSON object per line.
        let mut buf = String::new();
        while let Some(chunk) = stream.next().await {
            let bytes = chunk.context("Error reading stream chunk")?;
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
                            let _ = tx.send(LlmEvent::Token(chunk.message.content));
                        }
                        if chunk.done {
                            let _ = tx.send(LlmEvent::Done);
                            return Ok(());
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(LlmEvent::Error(format!("Parse error: {e}")));
                        return Ok(());
                    }
                }
            }
        }

        // Stream ended without a done=true (shouldn't happen with Ollama, but be safe).
        let _ = tx.send(LlmEvent::Done);
        Ok(())
    }
}
