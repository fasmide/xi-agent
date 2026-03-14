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
mod agent;
mod llm;
mod ui;

use app::App;
use agent::{build_system_prompt, tools::register_builtin_tools, AgentLoopConfig};
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

    let tools = register_builtin_tools();
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| ".".to_string());
    let system_prompt = build_system_prompt(&tools, &cwd);

    let mut app = App::new(&current_model, AgentLoopConfig {
        tools,
        before_tool_call: None,
        after_tool_call: None,
        max_turns: 20,
    });
    app.system_prompt = Some(system_prompt);

    // The outer loop re-enters `run` when the user changes the model with
    // `/model <name>`, rebuilding the provider with the new model name while
    // preserving the rest of the App state.
    loop {
        let provider = Arc::new(OllamaProvider::new(&base_url, &current_model));
        match run(&mut terminal, &mut app, &provider).await {
            Ok(RunResult::Quit) | Err(_) => break,
            Ok(RunResult::ChangeModel(name)) => {
                app.current_model = name.clone();
                current_model = name;
                // Invalidate the cached model list so the next `/model ` fetch
                // reflects any changes since the last query.
                app.available_models = None;
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
    Quit,
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

                        // ── Selection menu mode ───────────────────────────────
                        if app.selection_mode {
                            match key.code {
                                KeyCode::Up => app.selection_select_prev(),
                                KeyCode::Down => app.selection_select_next(),
                                KeyCode::Enter if key.modifiers.is_empty() => {
                                    if let Some(model) = app.apply_selection() {
                                        return Ok(RunResult::ChangeModel(model));
                                    }
                                }
                                KeyCode::Esc => app.exit_selection_mode(),
                                _ => {}
                            }
                            continue;
                        }

                        // Esc: dismiss slash popup / clear input if in slash mode,
                        // otherwise quit.
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
                                if !app.completions.is_empty() {
                                    app.completion_select_prev();
                                } else {
                                    app.textarea.input(Event::Key(key));
                                }
                            }
                            KeyCode::Down => {
                                if !app.completions.is_empty() {
                                    app.completion_select_next();
                                } else {
                                    app.textarea.input(Event::Key(key));
                                }
                            }

                            // ── Tab: expand selected completion into textarea ───
                            KeyCode::Tab => {
                                if !app.completions.is_empty() {
                                    app.apply_completion();
                                }
                                // Intentionally ignored when no completions.
                            }

                            // ── Enter: execute command or submit to LLM ────────
                            KeyCode::Enter if key.modifiers.is_empty() => {
                                if app.in_slash_mode() {
                                    let input = app.textarea.lines().first()
                                        .cloned()
                                        .unwrap_or_default();
                                    let input = input.trim().to_string();
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
                                        Some(CommandAction::ModelNoArg) => {
                                            // No argument given — open the interactive
                                            // model selection menu.
                                            app.enter_selection_mode();
                                            if app.should_fetch_models_for_selection() {
                                                app.start_model_fetch(provider);
                                            }
                                        }
                                        None => {
                                            // Unknown command — discard silently.
                                        }
                                    }
                                } else {
                                    app.submit(provider);
                                }
                            }
                            KeyCode::Enter if key.modifiers == KeyModifiers::SHIFT => {
                                app.textarea.insert_newline();
                                app.update_completions();
                            }

                            // ── All other keys: forward to textarea ────────────
                            _ => {
                                app.textarea.input(Event::Key(key));
                                app.update_completions();
                                // Lazily fetch the model list the first time the
                                // user types "/model <space>".
                                if app.should_fetch_models() {
                                    app.start_model_fetch(provider);
                                }
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
                app.apply_events();
            }

            // ── Model list fetched ────────────────────────────────────────────
            Some(models) = app.models_rx.recv() => {
                app.apply_model_list(models);
            }
        }
    }
}
