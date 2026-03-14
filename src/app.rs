use std::sync::Arc;
use tui_textarea::{CursorMove, TextArea};

use crate::{
    agent::{run_agent_loop, types::AgentEvent, AgentLoopConfig},
    commands::{self, CompletionItem},
    llm::{Message, Role, LlmProvider},
};

// ── App state ─────────────────────────────────────────────────────────────────

pub struct App {
    pub messages: Vec<Message>,
    pub textarea: TextArea<'static>,
    pub log_scroll: usize,
    /// When true, the view always follows the bottom (auto-scrolls).
    pub auto_scroll: bool,
    /// Height of the log pane from the last draw — used as page-size scrolling.
    pub last_log_height: usize,
    pub streaming: bool,
    /// Optional system prompt prepended to every request.
    pub system_prompt: Option<String>,
    /// Currently active model name (mirrors the provider; updated on `/model`).
    pub current_model: String,
    /// Agent loop configuration (tools, hooks, max_turns).
    pub agent_config: AgentLoopConfig,

    // ── Completion popup ──────────────────────────────────────────────────────
    /// Items to display in the completion popup (empty = popup hidden).
    pub completions: Vec<CompletionItem>,
    /// Index of the currently highlighted completion row.
    pub completion_selected: usize,

    // ── Available models (for /model completions) ─────────────────────────────
    /// Cached model list from the provider; `None` until first fetch.
    pub available_models: Option<Vec<String>>,
    /// True while a `list_models` task is in flight.
    pub models_loading: bool,

    // ── Model selection menu ───────────────────────────────────────────────────
    /// True when the full-screen model picker is active.
    pub selection_mode: bool,
    /// Items shown in the selection menu.
    pub selection_items: Vec<CompletionItem>,
    /// Index of the currently highlighted selection row.
    pub selection_selected: usize,

    // ── Async channels ────────────────────────────────────────────────────────
    /// Receives AgentEvents forwarded from the active agent loop task.
    pub(crate) event_rx: tokio::sync::mpsc::UnboundedReceiver<AgentEvent>,
    event_tx: tokio::sync::mpsc::UnboundedSender<AgentEvent>,
    /// Receives model lists forwarded from `list_models` tasks.
    pub(crate) models_rx: tokio::sync::mpsc::UnboundedReceiver<Vec<String>>,
    models_tx: tokio::sync::mpsc::UnboundedSender<Vec<String>>,
}

impl App {
    pub fn new(initial_model: impl Into<String>, agent_config: AgentLoopConfig) -> Self {
        let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
        let (models_tx, models_rx) = tokio::sync::mpsc::unbounded_channel();
        Self {
            messages: Vec::new(),
            textarea: Self::make_textarea(),
            log_scroll: 0,
            auto_scroll: true,
            last_log_height: 0,
            streaming: false,
            system_prompt: None,
            current_model: initial_model.into(),
            agent_config,
            completions: Vec::new(),
            completion_selected: 0,
            available_models: None,
            models_loading: false,
            selection_mode: false,
            selection_items: Vec::new(),
            selection_selected: 0,
            event_rx,
            event_tx,
            models_rx,
            models_tx,
        }
    }

    fn make_textarea() -> TextArea<'static> {
        TextArea::default()
    }

    /// Reset the input area to a blank state between submissions.
    /// Also clears any active completion state.
    pub fn reset_textarea(&mut self) {
        self.textarea = Self::make_textarea();
        self.completions.clear();
        self.completion_selected = 0;
    }

    /// True when the input is a single line beginning with `/`.
    pub fn in_slash_mode(&self) -> bool {
        let lines = self.textarea.lines();
        lines.len() == 1 && lines[0].trim_start().starts_with('/')
    }

    // ── Completion helpers ────────────────────────────────────────────────────

    /// Recompute the completion list from the current textarea content and
    /// cached model list. Call this after every keystroke.
    pub fn update_completions(&mut self) {
        let lines = self.textarea.lines().to_vec();
        let input = if lines.len() == 1 {
            lines[0].trim().to_string()
        } else {
            String::new()
        };
        let available = self.available_models.as_deref();
        let loading = self.models_loading;
        let new = commands::completions_for(&input, available, loading);

        if new.len() != self.completions.len() {
            self.completion_selected = 0;
        }
        self.completions = new;
    }

    /// Returns true if a model-list fetch should be triggered now.
    /// This is the case when the user has typed `/model ` but the list has
    /// not been fetched yet and no fetch is currently in flight.
    pub fn should_fetch_models(&self) -> bool {
        if self.available_models.is_some() || self.models_loading {
            return false;
        }
        let lines = self.textarea.lines();
        lines.len() == 1 && lines[0].trim_start().starts_with("/model ")
    }

    /// Spawn a background task to fetch the model list from the provider.
    pub fn start_model_fetch<P: LlmProvider + Send + Sync + 'static>(
        &mut self,
        provider: &Arc<P>,
    ) {
        self.models_loading = true;
        let future = provider.list_models();
        let tx = self.models_tx.clone();
        tokio::spawn(async move {
            let models = future.await;
            let _ = tx.send(models);
        });
    }

    /// Store a freshly fetched model list and refresh completions.
    pub fn apply_model_list(&mut self, models: Vec<String>) {
        self.available_models = Some(models);
        self.models_loading = false;
        self.update_completions();
        // Populate (or refresh) the selection menu if it is currently shown.
        if self.selection_mode {
            self.selection_items = self
                .available_models
                .as_ref()
                .unwrap()
                .iter()
                .map(|m| commands::CompletionItem::from_model(m))
                .collect();
            // Keep the selected index in bounds.
            if self.selection_selected >= self.selection_items.len() {
                self.selection_selected = 0;
            }
        }
    }

    /// Navigate the completion selection down (wraps around).
    pub fn completion_select_next(&mut self) {
        let len = self.completions.len();
        if len > 0 {
            self.completion_selected = (self.completion_selected + 1) % len;
        }
    }

    /// Navigate the completion selection up (wraps around).
    pub fn completion_select_prev(&mut self) {
        let len = self.completions.len();
        if len > 0 {
            self.completion_selected = (self.completion_selected + len - 1) % len;
        }
    }

    /// Replace the textarea with the selected item's `complete_to` text and
    /// move the cursor to the end of the line. No-ops on loading indicators.
    pub fn apply_completion(&mut self) {
        let item = match self.completions.get(self.completion_selected) {
            Some(i) if !i.loading && !i.complete_to.is_empty() => i,
            _ => return,
        };
        let text = item.complete_to.clone();
        self.textarea = TextArea::new(vec![text]);
        self.textarea.move_cursor(CursorMove::End);
        self.update_completions();
    }

    // ── Selection menu ────────────────────────────────────────────────────────

    /// Enter the model selection menu, pre-populating items from the cache if
    /// available or showing a loading indicator otherwise.
    pub fn enter_selection_mode(&mut self) {
        self.reset_textarea();
        self.selection_mode = true;
        self.selection_selected = 0;
        self.selection_items = if let Some(models) = &self.available_models {
            models
                .iter()
                .map(|m| commands::CompletionItem::from_model(m))
                .collect()
        } else {
            vec![commands::CompletionItem::loading_indicator()]
        };
    }

    /// Dismiss the selection menu without applying a choice.
    pub fn exit_selection_mode(&mut self) {
        self.selection_mode = false;
        self.selection_items.clear();
        self.selection_selected = 0;
    }

    /// Returns true when a model fetch should be triggered for the selection
    /// menu (models not yet loaded, no fetch in flight).
    pub fn should_fetch_models_for_selection(&self) -> bool {
        self.selection_mode && self.available_models.is_none() && !self.models_loading
    }

    /// Navigate the selection menu down (wraps; skips loading rows).
    pub fn selection_select_next(&mut self) {
        let len = self.selection_items.len();
        if len > 0 {
            self.selection_selected = (self.selection_selected + 1) % len;
        }
    }

    /// Navigate the selection menu up (wraps; skips loading rows).
    pub fn selection_select_prev(&mut self) {
        let len = self.selection_items.len();
        if len > 0 {
            self.selection_selected = (self.selection_selected + len - 1) % len;
        }
    }

    /// Confirm the currently highlighted selection.
    /// Returns the chosen model name, or `None` if the row is a loading
    /// indicator or no valid item is highlighted.
    pub fn apply_selection(&mut self) -> Option<String> {
        let item = self.selection_items.get(self.selection_selected)?;
        if item.loading || item.label.is_empty() {
            return None;
        }
        let model = item.label.clone();
        self.exit_selection_mode();
        Some(model)
    }

    // ── Conversation management ───────────────────────────────────────────────

    /// Clear the conversation history and reset the input area.
    pub fn new_conversation(&mut self) {
        self.messages.clear();
        self.reset_textarea();
        self.auto_scroll = true;
    }

    // ── LLM submission ────────────────────────────────────────────────────────

    /// Take the textarea content and start the agent loop.
    pub fn submit<P: LlmProvider + Send + Sync + 'static>(&mut self, provider: &Arc<P>) {
        let lines: Vec<String> = self.textarea.lines().to_vec();
        let text = lines.join("\n");
        let trimmed = text.trim().to_string();
        if trimmed.is_empty() || self.streaming {
            return;
        }

        self.messages.push(Message::user(trimmed));
        self.reset_textarea();
        self.auto_scroll = true;
        self.streaming = true;

        // Build the LLM history: system prompt (if any) + all display messages.
        let mut llm_messages: Vec<Message> = self
            .system_prompt
            .iter()
            .map(|s| Message::system(s))
            .chain(self.messages.iter().cloned())
            .collect();

        // The agent loop manages assistant/tool messages internally;
        // strip any trailing assistant placeholder that might exist.
        if matches!(llm_messages.last().map(|m| &m.role), Some(Role::Assistant))
            && llm_messages.last().map(|m| m.content.is_empty()).unwrap_or(false)
        {
            llm_messages.pop();
        }

        let config = AgentLoopConfig {
            tools: self.agent_config.tools.clone(),
            before_tool_call: None,
            after_tool_call: None,
            max_turns: self.agent_config.max_turns,
        };

        let provider = Arc::clone(provider);
        let tx = self.event_tx.clone();
        tokio::spawn(async move {
            run_agent_loop(llm_messages, config, provider, tx).await;
        });
    }

    // ── Scrolling ─────────────────────────────────────────────────────────────

    pub fn scroll_up(&mut self) {
        self.scroll_up_lines(self.last_log_height.max(1));
    }

    pub fn scroll_up_lines(&mut self, n: usize) {
        self.auto_scroll = false;
        self.log_scroll = self.log_scroll.saturating_sub(n);
    }

    pub fn scroll_down_lines(&mut self, n: usize) {
        self.log_scroll = self.log_scroll.saturating_add(n);
    }

    pub fn scroll_down(&mut self) {
        self.auto_scroll = true;
    }

    // ── Agent event handling ──────────────────────────────────────────────────

    pub fn apply_event(&mut self, ev: AgentEvent) {
        match ev {
            AgentEvent::ThinkingToken(token) => {
                self.ensure_assistant_message();
                if let Some(last) = self.messages.last_mut() {
                    last.thinking.get_or_insert_with(String::new).push_str(&token);
                }
            }
            AgentEvent::TextToken(token) => {
                self.ensure_assistant_message();
                if let Some(last) = self.messages.last_mut() {
                    last.content.push_str(&token);
                }
            }
            AgentEvent::ToolCallStart { name, args } => {
                self.messages.push(Message::tool_call("", name, args));
            }
            AgentEvent::ToolCallEnd { result, .. } => {
                self.messages.push(Message::tool_result("", result.content, result.is_error));
            }
            AgentEvent::TurnEnd => {
                // Reserved for future use (e.g. visual separator between turns).
            }
            AgentEvent::Done => {
                self.streaming = false;
            }
            AgentEvent::Error(e) => {
                self.messages.push(Message::assistant(format!("[Error: {e}]")));
                self.streaming = false;
            }
        }
    }

    /// Ensure the last message is an (possibly empty) assistant message.
    /// Pushes a new one if the last message is not an assistant message.
    fn ensure_assistant_message(&mut self) {
        match self.messages.last().map(|m| &m.role) {
            Some(Role::Assistant) => {}
            _ => self.messages.push(Message::assistant("")),
        }
    }

    pub fn apply_events(&mut self) {
        while let Ok(ev) = self.event_rx.try_recv() {
            self.apply_event(ev);
        }
    }
}
