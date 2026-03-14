use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
};
use tui_textarea::TextArea;

use crate::{App, llm::Role};

pub fn make_textarea<'a>() -> TextArea<'a> {
    let mut textarea = TextArea::default();
    textarea.set_block(Block::default().borders(Borders::NONE));
    textarea.set_style(Style::default().fg(Color::White));
    textarea.set_cursor_line_style(Style::default());
    textarea
}

pub fn draw(f: &mut ratatui::Frame, app: &mut App) {
    let terminal_height = f.area().height as usize;

    let input_line_count = app.textarea.lines().len().max(1);
    let max_input_height = (terminal_height * 40 / 100).max(1);
    let input_height = input_line_count.min(max_input_height) as u16;

    // Layout: chat | divider | input
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(input_height),
        ])
        .split(f.area());

    let log_area = chunks[0];
    let divider_area = chunks[1];
    let input_area = chunks[2];

    // ── Chat log ──────────────────────────────────────────────────────────────
    let inner_height = log_area.height as usize;
    let lines = build_log_lines(&app.messages, app.streaming);

    let total_lines = lines.len();
    let max_scroll = total_lines.saturating_sub(inner_height);
    if app.log_scroll == usize::MAX {
        app.log_scroll = max_scroll;
    } else {
        app.log_scroll = app.log_scroll.min(max_scroll);
    }
    let scroll_offset = app.log_scroll as u16;

    let log_paragraph = Paragraph::new(Text::from(lines))
        .block(Block::default().borders(Borders::NONE))
        .wrap(Wrap { trim: false })
        .scroll((scroll_offset, 0));

    f.render_widget(log_paragraph, log_area);

    // Scrollbar
    if total_lines > inner_height {
        let mut scrollbar_state =
            ScrollbarState::new(max_scroll + 1).position(app.log_scroll);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight);
        f.render_stateful_widget(scrollbar, log_area, &mut scrollbar_state);
    }

    // ── Divider ───────────────────────────────────────────────────────────────
    let divider = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(Color::DarkGray));
    f.render_widget(divider, divider_area);

    // ── Input box ─────────────────────────────────────────────────────────────
    f.render_widget(&app.textarea, input_area);
}

/// Convert the message history into ratatui `Line`s for rendering.
fn build_log_lines<'a>(
    messages: &'a [crate::llm::Message],
    streaming: bool,
) -> Vec<Line<'a>> {
    let mut lines: Vec<Line> = Vec::new();

    for (idx, msg) in messages.iter().enumerate() {
        let is_last = idx == messages.len() - 1;

        match msg.role {
            Role::User => {
                render_message(&mut lines, "You", Color::Green, &msg.content);
            }
            Role::Assistant => {
                let display = if streaming && is_last && msg.content.is_empty() {
                    "▋".to_string() // blinking cursor placeholder while waiting for first token
                } else {
                    msg.content.clone()
                };
                let suffix = if streaming && is_last && !msg.content.is_empty() {
                    "▋" // inline cursor while tokens arrive
                } else {
                    ""
                };
                render_message_with_suffix(&mut lines, "AI", Color::Cyan, &display, suffix);
            }
        }
    }

    lines
}

fn render_message<'a>(lines: &mut Vec<Line<'a>>, label: &'static str, color: Color, content: &str) {
    render_message_with_suffix(lines, label, color, content, "");
}

fn render_message_with_suffix<'a>(
    lines: &mut Vec<Line<'a>>,
    label: &'static str,
    color: Color,
    content: &str,
    suffix: &'static str,
) {
    let indent = " ".repeat(label.len() + 2); // "You: " → 5 chars, "AI: " → 4 chars, etc.
    let content_lines: Vec<&str> = if content.is_empty() {
        vec![""]
    } else {
        content.split('\n').collect()
    };

    for (i, part) in content_lines.iter().enumerate() {
        if i == 0 {
            let mut spans = vec![
                Span::styled(
                    format!("{label}: "),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::raw(part.to_string()),
            ];
            if suffix != "" && i == content_lines.len() - 1 {
                spans.push(Span::styled(suffix, Style::default().fg(Color::Yellow)));
            }
            lines.push(Line::from(spans));
        } else {
            let mut spans = vec![
                Span::raw(indent.clone()),
                Span::raw(part.to_string()),
            ];
            if suffix != "" && i == content_lines.len() - 1 {
                spans.push(Span::styled(suffix, Style::default().fg(Color::Yellow)));
            }
            lines.push(Line::from(spans));
        }
    }
}
