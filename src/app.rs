use std::sync::Arc;
use tui_textarea::TextArea;

use crate::llm::{LlmEvent, LlmProvider, Message, Role};

// ── App state ─────────────────────────────────────────────────────────────────

pub struct App {
    pub messages: Vec<Message>,
    pub textarea: TextArea<'static>,
    pub log_scroll: usize,
    /// When true, the view always follows the bottom (auto-scrolls).
    /// Set to false when the user scrolls up; restored on PageDown or new submit.
    pub auto_scroll: bool,
    /// Height of the log pane from the last draw — used as the page size for scrolling.
    pub last_log_height: usize,
    pub streaming: bool,
    /// Optional system prompt prepended to every request.
    pub system_prompt: Option<String>,
    /// Receives LlmEvents forwarded from the active streaming task.
    pub(crate) event_rx: tokio::sync::mpsc::UnboundedReceiver<LlmEvent>,
    event_tx: tokio::sync::mpsc::UnboundedSender<LlmEvent>,
}

impl App {
    pub fn new() -> Self {
        let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
        Self {
            messages: Vec::new(),
            textarea: Self::make_textarea(),
            log_scroll: 0,
            auto_scroll: true,
            last_log_height: 0,
            streaming: false,
            system_prompt: None,
            event_rx,
            event_tx,
        }
    }

    /// Create a fresh, unstyled textarea. Visual styles are applied by `ui::draw`.
    fn make_textarea() -> TextArea<'static> {
        TextArea::default()
    }

    /// Reset the input area to a blank state between submissions.
    pub fn reset_textarea(&mut self) {
        self.textarea = Self::make_textarea();
    }

    /// Take the textarea content and start an LLM streaming request.
    pub fn submit<P: LlmProvider + Send + Sync + 'static>(&mut self, provider: &Arc<P>) {
        let lines: Vec<String> = self.textarea.lines().to_vec();
        let text = lines.join("\n");
        let trimmed = text.trim().to_string();
        if trimmed.is_empty() || self.streaming {
            return;
        }

        self.messages.push(Message {
            role: Role::User,
            content: trimmed,
        });
        self.messages.push(Message {
            role: Role::Assistant,
            content: String::new(),
        });

        self.reset_textarea();
        self.auto_scroll = true;
        self.streaming = true;

        // Build the history to send: optional system message first, then all
        // conversation messages except the trailing empty assistant placeholder
        // that was just pushed above.
        let history: Vec<Message> = self
            .system_prompt
            .iter()
            .map(|s| Message { role: Role::System, content: s.clone() })
            .chain(self.messages[..self.messages.len() - 1].iter().cloned())
            .collect();

        // Obtain a stream from the provider — no channel required by the trait.
        // We bridge it to our internal channel here so the event loop can drain
        // it non-blockingly with try_recv().
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

    /// Scroll up by one page. Disables auto-scroll.
    pub fn scroll_up(&mut self) {
        self.scroll_up_lines(self.last_log_height.max(1));
    }

    /// Scroll up by `n` lines. Disables auto-scroll.
    pub fn scroll_up_lines(&mut self, n: usize) {
        self.auto_scroll = false;
        self.log_scroll = self.log_scroll.saturating_sub(n);
    }

    /// Scroll down by `n` lines. Re-enables auto-scroll when reaching bottom.
    pub fn scroll_down_lines(&mut self, n: usize) {
        self.log_scroll = self.log_scroll.saturating_add(n);
        // auto_scroll is re-enabled by draw() once log_scroll reaches max_scroll
    }

    /// Scroll down by one page. Snaps to bottom and re-enables auto-scroll.
    pub fn scroll_down(&mut self) {
        self.auto_scroll = true;
    }

    /// Apply a single LLM event to the application state.
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

    /// Drain the event channel and apply all pending LLM events.
    pub fn apply_events(&mut self) {
        while let Ok(ev) = self.event_rx.try_recv() {
            self.apply_event(ev);
        }
    }
}
