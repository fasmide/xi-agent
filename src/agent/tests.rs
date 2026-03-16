use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures_util::stream;
use tokio::sync::mpsc;

use crate::agent::types::AgentEvent;
use crate::agent::{AgentLoopConfig, run_agent_loop};
use crate::llm::{LlmEvent, LlmProvider, LlmStream, Message, ModelListFuture, ToolDefinition};

// ── MockProvider ──────────────────────────────────────────────────────────────

/// A fake LLM provider that returns pre-canned sequences of `LlmEvent`s,
/// one `Vec<LlmEvent>` per turn.
struct MockProvider {
    turns: Arc<Mutex<std::collections::VecDeque<Vec<LlmEvent>>>>,
}

impl MockProvider {
    fn new(turns: Vec<Vec<LlmEvent>>) -> Self {
        Self {
            turns: Arc::new(Mutex::new(turns.into())),
        }
    }
}

impl LlmProvider for MockProvider {
    fn stream_chat(&self, messages: Vec<Message>) -> LlmStream {
        self.stream_chat_with_tools(messages, vec![])
    }

    fn stream_chat_with_tools(
        &self,
        _messages: Vec<Message>,
        _tools: Vec<ToolDefinition>,
    ) -> LlmStream {
        let events = self.turns.lock().unwrap().pop_front().unwrap_or_default();
        Box::pin(stream::iter(events))
    }

    fn list_models(&self) -> ModelListFuture {
        Box::pin(async { vec![] })
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Run the agent loop with the given provider and collect all emitted events.
async fn run_and_collect(provider: MockProvider, max_turns: usize) -> Vec<AgentEvent> {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let config = AgentLoopConfig {
        tools: HashMap::new(),
        before_tool_call: None,
        after_tool_call: None,
        max_turns,
    };
    let messages = vec![Message::user("hi")];
    run_agent_loop(messages, config, Arc::new(provider), tx).await;
    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }
    events
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn agent_loop_single_text_turn() {
    let provider = MockProvider::new(vec![vec![
        LlmEvent::Token("hello".to_string()),
        LlmEvent::Done,
    ]]);
    let events = run_and_collect(provider, 10).await;

    // First event must be TextToken("hello").
    assert!(
        matches!(&events[0], AgentEvent::TextToken(t) if t == "hello"),
        "unexpected first event: {:?}",
        events[0]
    );
    // Last event must be Done.
    assert!(
        matches!(events.last().unwrap(), AgentEvent::Done),
        "expected Done as last event, got: {:?}",
        events.last()
    );
}

#[tokio::test]
async fn agent_loop_tool_call_then_text() {
    // Turn 1: the model requests a tool call.
    // The tool is unknown so the loop returns an error result to the model.
    // Turn 2: the model gives a plain text answer.
    let provider = MockProvider::new(vec![
        vec![
            LlmEvent::ToolCall {
                id: "call_1".to_string(),
                name: "bash".to_string(),
                args: serde_json::json!({"command": "echo hi"}),
            },
            LlmEvent::Done,
        ],
        vec![LlmEvent::Token("result".to_string()), LlmEvent::Done],
    ]);
    let events = run_and_collect(provider, 10).await;

    let has_tool_start = events
        .iter()
        .any(|e| matches!(e, AgentEvent::ToolCallStart { .. }));
    let has_tool_end = events
        .iter()
        .any(|e| matches!(e, AgentEvent::ToolCallEnd { .. }));
    let has_text = events
        .iter()
        .any(|e| matches!(e, AgentEvent::TextToken(t) if t == "result"));
    let ends_done = matches!(events.last().unwrap(), AgentEvent::Done);

    assert!(has_tool_start, "expected ToolCallStart in events");
    assert!(has_tool_end, "expected ToolCallEnd in events");
    assert!(has_text, "expected TextToken('result') in events");
    assert!(ends_done, "expected Done as last event");
}

#[tokio::test]
async fn agent_loop_max_turns_reached() {
    // Provider always emits a tool call, so the loop never reaches a plain
    // text answer.  With max_turns=2, it should stop with Error after 2 turns.
    let turns: Vec<Vec<LlmEvent>> = (0..10)
        .map(|_| {
            vec![
                LlmEvent::ToolCall {
                    id: "call_1".to_string(),
                    name: "bash".to_string(),
                    args: serde_json::json!({"command": "echo hi"}),
                },
                LlmEvent::Done,
            ]
        })
        .collect();
    let provider = MockProvider::new(turns);
    let events = run_and_collect(provider, 2).await;

    assert!(
        matches!(events.last().unwrap(), AgentEvent::Error(_)),
        "expected Error as last event, got: {:?}",
        events.last()
    );
}

#[tokio::test]
async fn agent_loop_before_hook_blocks_tool() {
    // Turn 1: model requests a tool call; `before_tool_call` returns false.
    // Turn 2: model gives a plain text answer after seeing the error result.
    let provider = MockProvider::new(vec![
        vec![
            LlmEvent::ToolCall {
                id: "call_1".to_string(),
                name: "bash".to_string(),
                args: serde_json::json!({"command": "echo hi"}),
            },
            LlmEvent::Done,
        ],
        vec![LlmEvent::Token("ok".to_string()), LlmEvent::Done],
    ]);

    let (tx, mut rx) = mpsc::unbounded_channel();
    let config = AgentLoopConfig {
        tools: HashMap::new(),
        before_tool_call: Some(Box::new(|_name, _args| false)), // block everything
        after_tool_call: None,
        max_turns: 10,
    };
    let messages = vec![Message::user("hi")];
    run_agent_loop(messages, config, Arc::new(provider), tx).await;

    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }

    let has_blocked_result = events
        .iter()
        .any(|ev| matches!(ev, AgentEvent::ToolCallEnd { result, .. } if result.is_error));
    assert!(
        has_blocked_result,
        "expected a blocked (is_error) ToolCallEnd"
    );

    // Loop should still complete (not hang or error out).
    assert!(
        matches!(events.last().unwrap(), AgentEvent::Done),
        "expected Done after blocked tool call"
    );
}
