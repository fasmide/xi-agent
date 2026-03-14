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
mod provider;
mod ui;

use app::{App, SelectionResult};
use agent::{build_system_prompt, tools::register_builtin_tools, AgentLoopConfig};
use commands::CommandAction;
use llm::LlmProvider;
use provider::{build_provider, ProviderKind};

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> io::Result<()> {
    // Determine the initial provider from the PIRS_PROVIDER env var, falling
    // back to Copilot (the default credential source in auth.json).
    let mut current_kind = std::env::var("PIRS_PROVIDER")
        .ok()
        .and_then(|s| ProviderKind::from_name(&s))
        .unwrap_or(ProviderKind::Copilot);

    let mut current_model = std::env::var("COPILOT_MODEL")
        .or_else(|_| std::env::var("OPENAI_MODEL"))
        .unwrap_or_else(|_| current_kind.default_model().to_string());

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

    let mut app = App::new(&current_model, &current_kind, AgentLoopConfig {
        tools,
        before_tool_call: None,
        after_tool_call: None,
        max_turns: 20,
    });
    app.system_prompt = Some(system_prompt);

    loop {
        // Build (or re-build) the provider for the current kind + model.
        let provider = match build_provider(&current_kind, &current_model) {
            Ok(p) => p,
            Err(e) => {
                // If the chosen provider can't initialise, fall back to Copilot
                // and inform the user via a system message in the chat log.
                app.messages.push(llm::Message::assistant(
                    format!("[provider error: {e} — falling back to copilot]")
                ));
                current_kind = ProviderKind::Copilot;
                current_model = current_kind.default_model().to_string();
                match build_provider(&current_kind, &current_model) {
                    Ok(p) => p,
                    Err(e2) => {
                        eprintln!("pirs: cannot initialise any provider: {e2}");
                        break;
                    }
                }
            }
        };

        match run(&mut terminal, &mut app, &provider).await {
            Ok(RunResult::Quit) | Err(_) => break,

            Ok(RunResult::ChangeModel(name)) => {
                app.current_model = name.clone();
                current_model = name;
                // Invalidate cached model list so the next fetch is fresh.
                app.available_models = None;
            }

            Ok(RunResult::ChangeProvider(name)) => {
                if let Some(kind) = ProviderKind::from_name(&name) {
                    current_kind = kind;
                    // Reset to the new provider's default model.
                    current_model = current_kind.default_model().to_string();
                    app.current_model = current_model.clone();
                    app.current_provider = current_kind.name().to_string();
                    app.available_models = None;
                }
                // Unknown name: silently ignore and loop (provider unchanged).
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
    ChangeProvider(String),
}

// ── Inner event loop ──────────────────────────────────────────────────────────

async fn run(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    provider: &Arc<dyn LlmProvider + Send + Sync>,
) -> io::Result<RunResult> {
    let mut crossterm_events = EventStream::new();

    loop {
        terminal.draw(|f| ui::draw(f, app))?;

        tokio::select! {
            // ── Terminal input ────────────────────────────────────────────────
            Some(Ok(ev)) = crossterm_events.next() => {
                match ev {
                    Event::Key(key) => {
                        if key.code == KeyCode::Char('c')
                            && key.modifiers.contains(KeyModifiers::CONTROL)
                        {
                            return Ok(RunResult::Quit);
                        }

                        // ── Selection menu mode ───────────────────────────────
                        if app.selection_mode {
                            match key.code {
                                KeyCode::Up   => app.selection_select_prev(),
                                KeyCode::Down => app.selection_select_next(),
                                KeyCode::Enter if key.modifiers.is_empty() => {
                                    match app.apply_selection() {
                                        Some(SelectionResult::Model(m)) => {
                                            return Ok(RunResult::ChangeModel(m));
                                        }
                                        Some(SelectionResult::Provider(p)) => {
                                            return Ok(RunResult::ChangeProvider(p));
                                        }
                                        None => {}
                                    }
                                }
                                KeyCode::Esc => app.exit_selection_mode(),
                                _ => {}
                            }
                            continue;
                        }

                        if key.code == KeyCode::Esc {
                            if app.in_slash_mode() {
                                app.reset_textarea();
                            } else {
                                return Ok(RunResult::Quit);
                            }
                            continue;
                        }

                        match key.code {
                            KeyCode::PageUp   => app.scroll_up(),
                            KeyCode::PageDown => app.scroll_down(),

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

                            KeyCode::Tab => {
                                if !app.completions.is_empty() {
                                    app.apply_completion();
                                }
                            }

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
                                            app.enter_model_selection_mode();
                                            if app.should_fetch_models_for_selection() {
                                                app.start_model_fetch(provider);
                                            }
                                        }
                                        Some(CommandAction::Provider(name)) => {
                                            return Ok(RunResult::ChangeProvider(name));
                                        }
                                        Some(CommandAction::ProviderNoArg) => {
                                            app.enter_provider_selection_mode();
                                        }
                                        None => {}
                                    }
                                } else {
                                    app.submit(provider);
                                }
                            }
                            KeyCode::Enter if key.modifiers == KeyModifiers::SHIFT => {
                                app.textarea.insert_newline();
                                app.update_completions();
                            }

                            _ => {
                                app.textarea.input(Event::Key(key));
                                app.update_completions();
                                if app.should_fetch_models() {
                                    app.start_model_fetch(provider);
                                }
                            }
                        }
                    }
                    Event::Mouse(mouse) => match mouse.kind {
                        MouseEventKind::ScrollUp   => app.scroll_up_lines(3),
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
