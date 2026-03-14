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

// Serde types for the Ollama /api/tags endpoint.
#[derive(Deserialize)]
struct TagsResponse {
    models: Vec<TagModel>,
}

#[derive(Deserialize)]
struct TagModel {
    name: String,
}

// ── <think> tag parser ────────────────────────────────────────────────────────

/// Build the Ollama message content for a past assistant message.
/// If the message has associated thinking content, re-wrap it in
/// `<think>…</think>` so that reasoning models see their prior chain of
/// thought in multi-turn conversations.
fn ollama_content(msg: &Message) -> String {
    match &msg.thinking {
        Some(thinking) if !thinking.is_empty() => {
            format!("<think>{}</think>{}", thinking, msg.content)
        }
        _ => msg.content.clone(),
    }
}

/// Two-state parser for `<think>…</think>` blocks.
#[derive(Clone, Copy, PartialEq)]
enum ParseState {
    /// Currently emitting normal response tokens.
    Responding,
    /// Currently inside a `<think>…</think>` block.
    Thinking,
}

/// Return the largest byte offset ≤ `index` that falls on a UTF-8 character
/// boundary in `s`.
fn floor_char_boundary(s: &str, index: usize) -> usize {
    if index >= s.len() {
        return s.len();
    }
    let mut i = index;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
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
                        content: ollama_content(m),
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

            // Think-tag parser state.
            let mut parse_state = ParseState::Responding;
            // Holds raw text that might be the start of a `<think>` or
            // `</think>` tag spanning a chunk boundary.
            let mut text_buf = String::new();

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

                    let chunk = match serde_json::from_str::<ChatChunk>(&line) {
                        Ok(c) => c,
                        Err(e) => {
                            yield LlmEvent::Error(format!("Parse error: {e}"));
                            return;
                        }
                    };

                    if !chunk.message.content.is_empty() {
                        text_buf.push_str(&chunk.message.content);

                        // Flush all complete tag-delimited regions from text_buf.
                        'flush: loop {
                            let tag = if parse_state == ParseState::Responding {
                                "<think>"
                            } else {
                                "</think>"
                            };

                            if let Some(pos) = text_buf.find(tag) {
                                // Emit everything before the tag.
                                if pos > 0 {
                                    let text = text_buf[..pos].to_string();
                                    text_buf.drain(..pos);
                                    if parse_state == ParseState::Responding {
                                        yield LlmEvent::Token(text);
                                    } else {
                                        yield LlmEvent::ThinkingToken(text);
                                    }
                                }
                                // Consume the tag itself.
                                text_buf.drain(..tag.len());
                                parse_state = match parse_state {
                                    ParseState::Responding => ParseState::Thinking,
                                    ParseState::Thinking  => ParseState::Responding,
                                };
                                // Loop again — there may be another tag.
                            } else {
                                // No complete tag. Hold back enough bytes to
                                // handle a tag that straddles this boundary.
                                let hold = tag.len() - 1;
                                if text_buf.len() > hold {
                                    let emit_len = floor_char_boundary(
                                        &text_buf,
                                        text_buf.len() - hold,
                                    );
                                    if emit_len > 0 {
                                        let text = text_buf[..emit_len].to_string();
                                        text_buf.drain(..emit_len);
                                        if parse_state == ParseState::Responding {
                                            yield LlmEvent::Token(text);
                                        } else {
                                            yield LlmEvent::ThinkingToken(text);
                                        }
                                    }
                                }
                                break 'flush;
                            }
                        }
                    }

                    if chunk.done {
                        // Flush any remaining buffered text.
                        if !text_buf.is_empty() {
                            let text = std::mem::take(&mut text_buf);
                            if parse_state == ParseState::Responding {
                                yield LlmEvent::Token(text);
                            } else {
                                yield LlmEvent::ThinkingToken(text);
                            }
                        }
                        yield LlmEvent::Done;
                        return;
                    }
                }
            }

            // Stream ended without a done=true — flush and finish.
            if !text_buf.is_empty() {
                let text = std::mem::take(&mut text_buf);
                if parse_state == ParseState::Responding {
                    yield LlmEvent::Token(text);
                } else {
                    yield LlmEvent::ThinkingToken(text);
                }
            }
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
