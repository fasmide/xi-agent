use std::sync::Arc;
use tui_textarea::{CursorMove, TextArea};

use crate::{
    commands::{self, CompletionItem},
    llm::{LlmEvent, LlmProvider, Message, Role},
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

    // ── Async channels ────────────────────────────────────────────────────────
    /// Receives LlmEvents forwarded from the active streaming task.
    pub(crate) event_rx: tokio::sync::mpsc::UnboundedReceiver<LlmEvent>,
    event_tx: tokio::sync::mpsc::UnboundedSender<LlmEvent>,
    /// Receives model lists forwarded from `list_models` tasks.
    pub(crate) models_rx: tokio::sync::mpsc::UnboundedReceiver<Vec<String>>,
    models_tx: tokio::sync::mpsc::UnboundedSender<Vec<String>>,
}

impl App {
    pub fn new(initial_model: impl Into<String>) -> Self {
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
            completions: Vec::new(),
            completion_selected: 0,
            available_models: None,
            models_loading: false,
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

    // ── Conversation management ───────────────────────────────────────────────

    /// Clear the conversation history and reset the input area.
    pub fn new_conversation(&mut self) {
        self.messages.clear();
        self.reset_textarea();
        self.auto_scroll = true;
    }

    // ── LLM submission ────────────────────────────────────────────────────────

    /// Take the textarea content and start an LLM streaming request.
    pub fn submit<P: LlmProvider + Send + Sync + 'static>(&mut self, provider: &Arc<P>) {
        let lines: Vec<String> = self.textarea.lines().to_vec();
        let text = lines.join("\n");
        let trimmed = text.trim().to_string();
        if trimmed.is_empty() || self.streaming {
            return;
        }

        self.messages.push(Message { role: Role::User, content: trimmed });
        self.messages.push(Message { role: Role::Assistant, content: String::new() });

        self.reset_textarea();
        self.auto_scroll = true;
        self.streaming = true;

        let history: Vec<Message> = self
            .system_prompt
            .iter()
            .map(|s| Message { role: Role::System, content: s.clone() })
            .chain(self.messages[..self.messages.len() - 1].iter().cloned())
            .collect();

        let mut stream = provider.stream_chat(history);
        let tx = self.event_tx.clone();
        tokio::spawn(async move {
            use futures_util::StreamExt;
            while let Some(event) = stream.next().await {
                let done = matches!(event, LlmEvent::Done | LlmEvent::Error(_));
                let _ = tx.send(event);
                if done {
                    break;
                }
            }
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

    // ── LLM event handling ────────────────────────────────────────────────────

    pub fn apply_event(&mut self, ev: LlmEvent) {
        match ev {
            LlmEvent::Token(token) => {
                if let Some(last) = self.messages.last_mut() {
                    last.content.push_str(&token);
                }
            }
            LlmEvent::Done => {
                self.streaming = false;
            }
            LlmEvent::Error(e) => {
                if let Some(last) = self.messages.last_mut() {
                    last.content = format!("[Error: {e}]");
                }
                self.streaming = false;
            }
        }
    }

    pub fn apply_events(&mut self) {
        while let Ok(ev) = self.event_rx.try_recv() {
            self.apply_event(ev);
        }
    }
}
