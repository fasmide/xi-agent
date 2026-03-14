use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::{io, sync::Arc, time::Duration};
use tokio::sync::mpsc::unbounded_channel;
use tui_textarea::TextArea;

mod llm;
mod ui;

use llm::ollama::OllamaProvider;
use llm::{AppEvent, LlmProvider, Message, Role};

// ── App state ─────────────────────────────────────────────────────────────────

pub struct App<'a> {
    pub messages: Vec<Message>,
    pub textarea: TextArea<'a>,
    pub log_scroll: usize,
    /// When true, the view always follows the bottom (auto-scrolls).
    /// Set to false when the user scrolls up; restored on PageDown or new submit.
    pub auto_scroll: bool,
    /// Height of the log pane from the last draw — used as the page size for scrolling.
    pub last_log_height: usize,
    pub streaming: bool,
    event_rx: tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    event_tx: tokio::sync::mpsc::UnboundedSender<AppEvent>,
}

impl<'a> App<'a> {
    pub fn new() -> Self {
        let (event_tx, event_rx) = unbounded_channel();
        Self {
            messages: Vec::new(),
            textarea: ui::make_textarea(),
            log_scroll: 0,
            auto_scroll: true,
            last_log_height: 0,
            streaming: false,
            event_rx,
            event_tx,
        }
    }

    /// Take the textarea content and start an LLM streaming request.
    pub fn submit(&mut self, provider: &Arc<dyn LlmProvider>) {
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

        self.textarea = ui::make_textarea();
        self.auto_scroll = true;
        self.streaming = true;

        let provider = Arc::clone(provider);
        let tx = self.event_tx.clone();
        let history: Vec<Message> = self.messages[..self.messages.len() - 1].to_vec();
        tokio::spawn(async move {
            if let Err(e) = provider.stream_chat(&history, tx.clone()).await {
                let _ = tx.send(AppEvent::Error(e.to_string()));
            }
        });
    }

    /// Scroll up by one page. Disables auto-scroll.
    pub fn scroll_up(&mut self) {
        self.auto_scroll = false;
        self.log_scroll = self
            .log_scroll
            .saturating_sub(self.last_log_height.max(1));
    }

    /// Scroll down by one page. Snaps to bottom and re-enables auto-scroll.
    pub fn scroll_down(&mut self) {
        self.auto_scroll = true;
    }

    /// Drain the event channel and apply any pending LLM events.
    pub fn apply_events(&mut self) {
        while let Ok(ev) = self.event_rx.try_recv() {
            match ev {
                AppEvent::Token(token) => {
                    if let Some(last) = self.messages.last_mut() {
                        last.content.push_str(&token);
                    }
                }
                AppEvent::Done => {
                    self.streaming = false;
                }
                AppEvent::Error(e) => {
                    if let Some(last) = self.messages.last_mut() {
                        last.content = format!("[Error: {e}]");
                    }
                    self.streaming = false;
                }
            }
        }
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> io::Result<()> {
    let provider: Arc<dyn LlmProvider> = Arc::new(OllamaProvider::from_env());

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
    let res = run(&mut terminal, &mut app, &provider).await;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    res
}

async fn run(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App<'_>,
    provider: &Arc<dyn LlmProvider>,
) -> io::Result<()> {
    loop {
        app.apply_events();
        terminal.draw(|f| ui::draw(f, app))?;

        if event::poll(Duration::from_millis(10))? {
            if let Event::Key(key) = event::read()? {
                if key.code == KeyCode::Char('c')
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                {
                    return Ok(());
                }
                if key.code == KeyCode::Esc {
                    return Ok(());
                }

                match key.code {
                    KeyCode::PageUp => {
                        app.scroll_up();
                        continue;
                    }
                    KeyCode::PageDown => {
                        app.scroll_down();
                        continue;
                    }
                    KeyCode::Enter if key.modifiers.is_empty() => {
                        app.submit(provider);
                        continue;
                    }
                    _ => {}
                }

                app.textarea.input(Event::Key(key));
            }
        }
    }
}
