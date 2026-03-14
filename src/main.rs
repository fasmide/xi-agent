use crossterm::{
    event::{
        DisableMouseCapture, EnableMouseCapture, Event, EventStream, KeyCode, KeyModifiers,
        KeyboardEnhancementFlags, MouseEventKind, PopKeyboardEnhancementFlags,
        PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures_util::StreamExt;
use ratatui::{backend::CrosstermBackend, Terminal};
use std::{io, sync::Arc};

mod app;
mod llm;
mod ui;

use app::App;
use llm::{ollama::OllamaProvider, LlmProvider};

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> io::Result<()> {
    let provider = Arc::new(OllamaProvider::from_env());

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableMouseCapture,
        PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES),
    )?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
    let res = run(&mut terminal, &mut app, &provider).await;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        PopKeyboardEnhancementFlags,
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    res
}

async fn run<P: LlmProvider + Send + Sync + 'static>(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    provider: &Arc<P>,
) -> io::Result<()> {
    let mut crossterm_events = EventStream::new();

    loop {
        terminal.draw(|f| ui::draw(f, app))?;

        // Wait for either a terminal input event or an LLM token — whichever
        // arrives first. The loop only wakes when there is real work to do;
        // CPU usage is near zero while idle.
        tokio::select! {
            // ── Terminal input ────────────────────────────────────────────────
            Some(Ok(ev)) = crossterm_events.next() => {
                match ev {
                    Event::Key(key) => {
                        if key.code == KeyCode::Char('c')
                            && key.modifiers.contains(KeyModifiers::CONTROL)
                        {
                            return Ok(());
                        }
                        if key.code == KeyCode::Esc {
                            return Ok(());
                        }

                        match key.code {
                            KeyCode::PageUp => app.scroll_up(),
                            KeyCode::PageDown => app.scroll_down(),
                            KeyCode::Enter if key.modifiers.is_empty() => {
                                app.submit(provider);
                            }
                            KeyCode::Enter if key.modifiers == KeyModifiers::SHIFT => {
                                app.textarea.insert_newline();
                            }
                            _ => {
                                app.textarea.input(Event::Key(key));
                            }
                        }
                    }
                    Event::Mouse(mouse) => match mouse.kind {
                        MouseEventKind::ScrollUp => app.scroll_up_lines(3),
                        MouseEventKind::ScrollDown => app.scroll_down_lines(3),
                        _ => {}
                    },
                    _ => {}
                }
            }

            // ── LLM streaming events ──────────────────────────────────────────
            Some(ev) = app.event_rx.recv() => {
                app.apply_event(ev);
                // Drain any further tokens that arrived in the same batch.
                app.apply_events();
            }
        }
    }
}
