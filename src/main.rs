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
mod commands;
mod llm;
mod ui;

use app::App;
use commands::CommandAction;
use llm::{ollama::OllamaProvider, LlmProvider};

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> io::Result<()> {
    let base_url = std::env::var("OLLAMA_HOST")
        .unwrap_or_else(|_| "http://localhost:11434".to_string());
    let mut current_model = std::env::var("OLLAMA_MODEL")
        .unwrap_or_else(|_| "llama3.1".to_string());

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

    let mut app = App::new(&current_model);

    // The outer loop re-enters `run` when the user changes the model with
    // `/model <name>`, rebuilding the provider with the new model name.
    loop {
        let provider = Arc::new(OllamaProvider::new(&base_url, &current_model));
        match run(&mut terminal, &mut app, &provider).await {
            Ok(RunResult::Quit) | Err(_) => break,
            Ok(RunResult::ChangeModel(name)) => {
                app.current_model = name.clone();
                current_model = name;
                // Continue the loop — run() will be called again with a new provider.
            }
        }
    }

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        PopKeyboardEnhancementFlags,
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}

// ── Result type returned by the inner run loop ────────────────────────────────

enum RunResult {
    /// The user quit the application.
    Quit,
    /// The user switched to a different model; restart with a new provider.
    ChangeModel(String),
}

// ── Inner event loop ──────────────────────────────────────────────────────────

async fn run<P: LlmProvider + Send + Sync + 'static>(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    provider: &Arc<P>,
) -> io::Result<RunResult> {
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
                        // Global quit shortcuts (always active).
                        if key.code == KeyCode::Char('c')
                            && key.modifiers.contains(KeyModifiers::CONTROL)
                        {
                            return Ok(RunResult::Quit);
                        }

                        // Esc: dismiss slash popup if open, otherwise quit.
                        if key.code == KeyCode::Esc {
                            if app.in_slash_mode() {
                                app.reset_textarea();
                            } else {
                                return Ok(RunResult::Quit);
                            }
                            continue;
                        }

                        match key.code {
                            KeyCode::PageUp => app.scroll_up(),
                            KeyCode::PageDown => app.scroll_down(),

                            // ── Up / Down: navigate completions or move cursor ──
                            KeyCode::Up => {
                                if !app.slash_completions.is_empty() {
                                    app.slash_select_prev();
                                } else {
                                    app.textarea.input(Event::Key(key));
                                }
                            }
                            KeyCode::Down => {
                                if !app.slash_completions.is_empty() {
                                    app.slash_select_next();
                                } else {
                                    app.textarea.input(Event::Key(key));
                                }
                            }

                            // ── Tab: complete to selected command ──────────────
                            KeyCode::Tab => {
                                if !app.slash_completions.is_empty() {
                                    app.slash_complete();
                                }
                                // Tab is intentionally ignored when not in slash mode.
                            }

                            // ── Enter: execute command or submit to LLM ────────
                            KeyCode::Enter if key.modifiers.is_empty() => {
                                if app.in_slash_mode() {
                                    let input = app.textarea.lines().first()
                                        .cloned()
                                        .unwrap_or_default();
                                    let input = input.trim().to_string();

                                    // Always clear the input regardless of outcome.
                                    app.reset_textarea();

                                    match commands::parse(&input) {
                                        Some(CommandAction::New) => {
                                            app.new_conversation();
                                        }
                                        Some(CommandAction::Quit) => {
                                            return Ok(RunResult::Quit);
                                        }
                                        Some(CommandAction::Model(name)) => {
                                            return Ok(RunResult::ChangeModel(name));
                                        }
                                        Some(CommandAction::ModelNoArg) | None => {
                                            // Unknown or incomplete command — just discard.
                                        }
                                    }
                                } else {
                                    app.submit(provider);
                                }
                            }
                            KeyCode::Enter if key.modifiers == KeyModifiers::SHIFT => {
                                app.textarea.insert_newline();
                                app.update_slash_state();
                            }

                            // ── All other keys: forward to textarea ────────────
                            _ => {
                                app.textarea.input(Event::Key(key));
                                app.update_slash_state();
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
