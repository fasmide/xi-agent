use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers,
        KeyboardEnhancementFlags, MouseEventKind, PopKeyboardEnhancementFlags,
        PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::{io, sync::Arc, time::Duration};

mod app;
mod llm;
mod ui;

use app::App;
use llm::{LlmProvider, ollama::OllamaProvider};

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
    loop {
        app.apply_events();
        terminal.draw(|f| ui::draw(f, app))?;

        if event::poll(Duration::from_millis(10))? {
            match event::read()? {
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
                        KeyCode::Enter if key.modifiers == KeyModifiers::SHIFT => {
                            app.textarea.insert_newline();
                            continue;
                        }
                        _ => {}
                    }

                    app.textarea.input(Event::Key(key));
                }
                Event::Mouse(mouse) => match mouse.kind {
                    MouseEventKind::ScrollUp => app.scroll_up_lines(3),
                    MouseEventKind::ScrollDown => app.scroll_down_lines(3),
                    _ => {}
                },
                _ => {}
            }
        }
    }
}
