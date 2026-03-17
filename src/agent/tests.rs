use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures_util::stream;
use tokio::sync::mpsc;

use crate::agent::types::AgentEvent;
use crate::agent::{AgentLoopConfig, run_agent_loop};
use crate::llm::{
    AssistantPhase, LlmEvent, LlmProvider, LlmStream, Message, ModelListFuture, ToolDefinition,
};

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
async fn run_and_collect(provider: MockProvider) -> Vec<AgentEvent> {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let config = AgentLoopConfig {
        tools: HashMap::new(),
        before_tool_call: None,
        after_tool_call: None,
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
        LlmEvent::Token {
            text: "hello".to_string(),
            phase: AssistantPhase::Unknown,
        },
        LlmEvent::Done,
    ]]);
    let events = run_and_collect(provider).await;

    // First event must be TextToken("hello").
    assert!(
        matches!(&events[0], AgentEvent::TextToken { text, .. } if text == "hello"),
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
        vec![
            LlmEvent::Token {
                text: "result".to_string(),
                phase: AssistantPhase::Final,
            },
            LlmEvent::Done,
        ],
    ]);
    let events = run_and_collect(provider).await;

    let has_tool_start = events
        .iter()
        .any(|e| matches!(e, AgentEvent::ToolCallStart { .. }));
    let has_tool_end = events
        .iter()
        .any(|e| matches!(e, AgentEvent::ToolCallEnd { .. }));
    let has_text = events
        .iter()
        .any(|e| matches!(e, AgentEvent::TextToken { text, .. } if text == "result"));
    let ends_done = matches!(events.last().unwrap(), AgentEvent::Done);

    assert!(has_tool_start, "expected ToolCallStart in events");
    assert!(has_tool_end, "expected ToolCallEnd in events");
    assert!(has_text, "expected TextToken('result') in events");
    assert!(ends_done, "expected Done as last event");
}

#[tokio::test]
async fn agent_loop_forwards_tool_intent_before_tool_start() {
    let provider = MockProvider::new(vec![vec![
        LlmEvent::ToolIntentStart,
        LlmEvent::ToolCall {
            id: "call_1".to_string(),
            name: "bash".to_string(),
            args: serde_json::json!({"command": "echo hi"}),
        },
        LlmEvent::Done,
    ]]);

    let events = run_and_collect(provider).await;

    let intent_idx = events
        .iter()
        .position(|e| matches!(e, AgentEvent::ToolIntentStart))
        .expect("expected ToolIntentStart");
    let tool_start_idx = events
        .iter()
        .position(|e| matches!(e, AgentEvent::ToolCallStart { .. }))
        .expect("expected ToolCallStart");

    assert!(
        intent_idx < tool_start_idx,
        "expected ToolIntentStart before ToolCallStart"
    );
}

#[tokio::test]
async fn agent_loop_stream_error_is_reported() {
    let provider = MockProvider::new(vec![vec![LlmEvent::Error("boom".to_string())]]);
    let events = run_and_collect(provider).await;

    assert!(
        matches!(events.last().unwrap(), AgentEvent::Error(e) if e == "boom"),
        "expected Error('boom') as last event, got: {:?}",
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
        vec![
            LlmEvent::Token {
                text: "ok".to_string(),
                phase: AssistantPhase::Final,
            },
            LlmEvent::Done,
        ],
    ]);

    let (tx, mut rx) = mpsc::unbounded_channel();
    let config = AgentLoopConfig {
        tools: HashMap::new(),
        before_tool_call: Some(Box::new(|_name, _args| false)), // block everything
        after_tool_call: None,
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

#[test]
fn test_read_agents_md() {
    use std::fs;

    use tempfile::tempdir;

    // Create a temporary directory structure.
    let temp_home = tempdir().unwrap();
    let home_path = temp_home.path();
    let temp_working = tempdir().unwrap();
    let working_path = temp_working.path();

    // Simulate ~/.tau/AGENTS.md
    let tau_agents_md = home_path.join(".tau/AGENTS.md");
    fs::create_dir_all(tau_agents_md.parent().unwrap()).unwrap();
    fs::write(&tau_agents_md, "Global agents configuration\n").unwrap();

    // Simulate AGENTS.md at cwd.
    let cwd_agents_md = working_path.join("AGENTS.md");
    fs::write(&cwd_agents_md, "Local agents configuration\n").unwrap();

    // Mock the home and current directory paths.
    let cwd = working_path.display().to_string();
    let concatenated = crate::agent::system_prompt::read_agents_md(&cwd, Some(home_path));

    assert!(concatenated.contains("Global agents configuration"));
    assert!(concatenated.contains("Local agents configuration"));

    // Clean up temporary directories.
    temp_home.close().unwrap();
    temp_working.close().unwrap();
}

#[test]
fn test_read_agents_md_from_nested_cwd_includes_parent_chain_in_order() {
    use std::fs;

    use tempfile::tempdir;

    let temp_home = tempdir().unwrap();
    let home_path = temp_home.path();

    // Simulate ~/.tau/AGENTS.md
    let tau_agents_md = home_path.join(".tau/AGENTS.md");
    fs::create_dir_all(tau_agents_md.parent().unwrap()).unwrap();
    fs::write(&tau_agents_md, "Global config\n").unwrap();

    // Build nested workspace: <root>/project/subdir
    let root = tempdir().unwrap();
    let project = root.path().join("project");
    let subdir = project.join("subdir");
    fs::create_dir_all(&subdir).unwrap();

    fs::write(project.join("AGENTS.md"), "Project-level config\n").unwrap();
    fs::write(subdir.join("AGENTS.md"), "Subdir-level config\n").unwrap();

    let cwd = subdir.display().to_string();
    let concatenated = crate::agent::system_prompt::read_agents_md(&cwd, Some(home_path));

    let global_idx = concatenated.find("Global config").unwrap();
    let subdir_idx = concatenated.find("Subdir-level config").unwrap();
    let project_idx = concatenated.find("Project-level config").unwrap();

    // read_agents_md appends in this order: global, cwd, then each parent up to root.
    assert!(global_idx < subdir_idx);
    assert!(subdir_idx < project_idx);

    temp_home.close().unwrap();
    root.close().unwrap();
}
