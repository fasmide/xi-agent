use std::sync::Arc;

use futures_util::StreamExt;
use tokio::sync::mpsc::UnboundedSender;

use crate::llm::{LlmEvent, LlmProvider, Message, ToolDefinition};

pub mod system_prompt;
pub mod tools;
pub mod types;

#[cfg(test)]
mod tests;

pub use system_prompt::build_system_prompt;
pub use types::{AgentEvent, AgentLoopConfig, ToolResult};

/// Run the agent loop: call the LLM, execute tool calls, repeat until the
/// model gives a final text answer.
///
/// All activity is reported back to `App` via `AgentEvent`s sent on `tx`.
pub async fn run_agent_loop(
    mut messages: Vec<Message>,
    config: AgentLoopConfig,
    provider: Arc<dyn LlmProvider>,
    tx: UnboundedSender<AgentEvent>,
) {
    // Build the tool definitions once from the registry.
    let tool_defs: Vec<ToolDefinition> = config
        .tools
        .values()
        .map(|t| ToolDefinition {
            name: t.name().to_string(),
            description: t.description().to_string(),
            parameters: t.parameters_schema(),
        })
        .collect();

    loop {
        // ── Stream the assistant response ─────────────────────────────────────
        let mut stream = provider.stream_chat_with_tools(messages.clone(), tool_defs.clone());

        // Accumulate text/thinking for the assistant message we'll push to
        // the display and to `messages` for history.
        let mut assistant_text = String::new();
        let mut assistant_thinking: Option<String> = None;
        let mut pending_tool_calls: Vec<(String, String, serde_json::Value)> = Vec::new(); // (id, name, args)

        let mut stream_error: Option<String> = None;

        while let Some(ev) = stream.next().await {
            match ev {
                LlmEvent::Token(t) => {
                    let _ = tx.send(AgentEvent::TextToken(t.clone()));
                    assistant_text.push_str(&t);
                }
                LlmEvent::ThinkingToken(t) => {
                    let _ = tx.send(AgentEvent::ThinkingToken(t.clone()));
                    assistant_thinking
                        .get_or_insert_with(String::new)
                        .push_str(&t);
                }
                LlmEvent::ToolCall { id, name, args } => {
                    pending_tool_calls.push((id, name, args));
                }
                LlmEvent::Done => break,
                LlmEvent::Error(e) => {
                    stream_error = Some(e);
                    break;
                }
            }
        }

        if let Some(e) = stream_error {
            let _ = tx.send(AgentEvent::Error(e));
            return;
        }

        // Append assistant message to history (even if empty when tools were called).
        let mut asst_msg = Message::assistant(&assistant_text);
        asst_msg.thinking = assistant_thinking;
        messages.push(asst_msg);

        // ── No tool calls → final answer ──────────────────────────────────────
        if pending_tool_calls.is_empty() {
            let _ = tx.send(AgentEvent::TurnEnd);
            let _ = tx.send(AgentEvent::Done);
            return;
        }

        // ── Execute tool calls sequentially ───────────────────────────────────
        for (id, name, args) in pending_tool_calls {
            let _ = tx.send(AgentEvent::ToolCallStart {
                id: id.clone(),
                name: name.clone(),
                args: args.clone(),
            });

            // before_tool_call hook
            let blocked = config
                .before_tool_call
                .as_ref()
                .map(|f| !f(&name, &args))
                .unwrap_or(false);

            let mut result = if blocked {
                ToolResult::err(format!("Tool call '{name}' was blocked"))
            } else {
                match config.tools.get(&name) {
                    Some(tool) => tool.execute(args.clone()).await,
                    None => ToolResult::err(format!("Unknown tool: '{name}'")),
                }
            };

            // after_tool_call hook
            if let Some(f) = &config.after_tool_call
                && let Some(override_result) = f(&name, &result)
            {
                result = override_result;
            }

            let _ = tx.send(AgentEvent::ToolCallEnd {
                id: id.clone(),
                name: name.clone(),
                result: result.clone(),
            });

            // Append tool call + result to history for the next LLM turn.
            messages.push(Message::tool_call(&id, &name, args));
            messages.push(Message::tool_result(&id, &result.content, result.is_error));
        }

        let _ = tx.send(AgentEvent::TurnEnd);
    }
}
