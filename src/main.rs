use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
    Terminal,
};
use std::io;
use tui_textarea::TextArea;

struct App<'a> {
    messages: Vec<String>,
    textarea: TextArea<'a>,
    log_scroll: usize,
}

fn make_textarea<'a>() -> TextArea<'a> {
    let mut textarea = TextArea::default();
    textarea.set_block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Input ")
            .border_style(Style::default().fg(Color::Cyan)),
    );
    textarea.set_style(Style::default().fg(Color::White));
    textarea.set_cursor_line_style(Style::default());
    textarea
}

impl<'a> App<'a> {
    fn new() -> Self {
        Self {
            messages: Vec::new(),
            textarea: make_textarea(),
            log_scroll: 0,
        }
    }

    /// Submit the current textarea content to the chat log.
    fn submit(&mut self) {
        let lines: Vec<String> = self.textarea.lines().to_vec();
        let text = lines.join("\n");
        let trimmed = text.trim().to_string();
        if !trimmed.is_empty() {
            self.messages.push(trimmed);
        }
        self.textarea = make_textarea();
        // Signal: scroll to bottom next frame
        self.log_scroll = usize::MAX;
    }
}

fn main() -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
    let res = run(&mut terminal, &mut app);

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    res
}

fn run(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> io::Result<()> {
    loop {
        terminal.draw(|f| ui(f, app))?;

        let ev = event::read()?;
        if let Event::Key(key) = ev {
            // Quit
            if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                return Ok(());
            }
            if key.code == KeyCode::Esc {
                return Ok(());
            }

            // Plain Enter = submit
            if key.code == KeyCode::Enter && key.modifiers.is_empty() {
                app.submit();
                continue;
            }

            // Forward to textarea (re-wrap as Event so tui-textarea gets the right type)
            app.textarea.input(Event::Key(key));
        }
    }
}

fn ui(f: &mut ratatui::Frame, app: &mut App) {
    let terminal_height = f.area().height as usize;

    // Textarea height: line count + 2 borders, clamped to 40% of terminal
    let input_line_count = app.textarea.lines().len().max(1);
    let max_input_height = (terminal_height * 40 / 100).max(3);
    let input_height = (input_line_count + 2).min(max_input_height) as u16;

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(input_height)])
        .split(f.area());

    // ── Chat log ──────────────────────────────────────────────────────────────
    let log_area = chunks[0];
    let inner_height = log_area.height.saturating_sub(2) as usize;

    let mut lines: Vec<Line> = Vec::new();
    for msg in &app.messages {
        for (i, part) in msg.lines().enumerate() {
            if i == 0 {
                lines.push(Line::from(vec![
                    Span::styled(
                        "You: ",
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(part.to_string()),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::raw("      "),
                    Span::raw(part.to_string()),
                ]));
            }
        }
    }

    let total_lines = lines.len();
    let max_scroll = total_lines.saturating_sub(inner_height);
    if app.log_scroll == usize::MAX {
        app.log_scroll = max_scroll;
    } else {
        app.log_scroll = app.log_scroll.min(max_scroll);
    }
    let scroll_offset = app.log_scroll as u16;

    let log_paragraph = Paragraph::new(Text::from(lines))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Chat ")
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .wrap(Wrap { trim: false })
        .scroll((scroll_offset, 0));

    f.render_widget(log_paragraph, log_area);

    // Scrollbar
    if total_lines > inner_height {
        let mut scrollbar_state = ScrollbarState::new(max_scroll + 1).position(app.log_scroll);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight);
        f.render_stateful_widget(scrollbar, log_area, &mut scrollbar_state);
    }

    // ── Input box ─────────────────────────────────────────────────────────────
    f.render_widget(&app.textarea, chunks[1]);
}
