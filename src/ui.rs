use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};
use tui_textarea::TextArea;

use crate::{llm::Role, App};

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
    let pane_width = log_area.width as usize;

    // Pre-wrapped lines: each Line is exactly one visual row.
    let mut lines = build_log_lines(&app.messages, app.streaming, pane_width);

    // Store log height for use as page size in the event loop.
    app.last_log_height = inner_height;

    // Pad the top with empty lines so content is anchored to the bottom.
    if lines.len() < inner_height {
        let padding = inner_height - lines.len();
        let mut padded = vec![Line::default(); padding];
        padded.append(&mut lines);
        lines = padded;
    }

    let total_lines = lines.len();
    // max_scroll and scroll offset are now purely in visual (== logical) lines.
    let max_scroll = total_lines.saturating_sub(inner_height);

    if app.auto_scroll {
        app.log_scroll = max_scroll;
    } else {
        app.log_scroll = app.log_scroll.min(max_scroll);
        // Re-enable auto-scroll if the user has scrolled back to the bottom.
        if app.log_scroll >= max_scroll {
            app.auto_scroll = true;
        }
    }

    let scroll_offset = app.log_scroll as u16;

    // No Wrap needed: every Line already fits within pane_width.
    let log_paragraph = Paragraph::new(Text::from(lines))
        .block(Block::default().borders(Borders::NONE))
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

// ── Line building + pre-wrapping ─────────────────────────────────────────────

/// Build all visual lines for the chat log, pre-wrapped to `width` columns.
/// Each returned `Line` occupies exactly one terminal row.
fn build_log_lines(
    messages: &[crate::llm::Message],
    streaming: bool,
    width: usize,
) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();

    for (idx, msg) in messages.iter().enumerate() {
        let is_last = idx == messages.len() - 1;

        match msg.role {
            Role::User => {
                append_message(&mut lines, &msg.content, "", width, true);
            }
            Role::System => {
                // System messages are not displayed in the chat log.
            }
            Role::Assistant => {
                let content = if streaming && is_last && msg.content.is_empty() {
                    "▋".to_string()
                } else {
                    msg.content.clone()
                };
                let suffix = if streaming && is_last && !msg.content.is_empty() {
                    "▋"
                } else {
                    ""
                };
                append_message(&mut lines, &content, suffix, width, false);
            }
        }
    }

    lines
}

/// Append pre-wrapped visual lines for one message to `out`.
///
/// `user` — when true each line is padded to `width` and given a grey
/// background so it stands out from assistant replies.
fn append_message(
    out: &mut Vec<Line<'static>>,
    content: &str,
    suffix: &'static str,
    width: usize,
    user: bool,
) {
    let user_bg = Style::default().bg(Color::Rgb(50, 50, 50));

    // Split on explicit newlines first, then wrap each segment to width.
    let segments: Vec<&str> = if content.is_empty() {
        vec![""]
    } else {
        content.split('\n').collect()
    };

    let last_seg = segments.len() - 1;

    for (seg_idx, segment) in segments.iter().enumerate() {
        let is_last_seg = seg_idx == last_seg;
        let chunks = wrap_str(segment, width);
        let last_chunk = chunks.len() - 1;

        for (chunk_idx, chunk) in chunks.iter().enumerate() {
            let is_last_chunk = chunk_idx == last_chunk;
            let show_suffix = !suffix.is_empty() && is_last_seg && is_last_chunk;

            if user {
                // Pad to `width` so the background spans the full line.
                let text_len = chunk.chars().count();
                let padding = width.saturating_sub(text_len);
                let padded = format!("{}{}", chunk, " ".repeat(padding));
                out.push(Line::from(Span::styled(padded, user_bg)));
            } else {
                let mut spans: Vec<Span<'static>> = vec![Span::raw(chunk.clone())];
                if show_suffix {
                    spans.push(Span::styled(suffix, Style::default().fg(Color::Yellow)));
                }
                out.push(Line::from(spans));
            }
        }
    }
}

/// Split `text` into lines of at most `width` characters, breaking at word
/// boundaries.  Words longer than `width` are hard-broken as a last resort.
/// Always returns at least one chunk (empty string when `text` is empty).
fn wrap_str(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    if text.is_empty() {
        return vec![String::new()];
    }

    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_len: usize = 0;

    for word in text.split_whitespace() {
        let word_len = word.chars().count();

        if current.is_empty() {
            // Start of a new line — always place the word here, hard-breaking
            // if the word itself is longer than the available width.
            if word_len <= width {
                current.push_str(word);
                current_len = word_len;
            } else {
                // Hard-break the oversized word across multiple lines.
                let mut remaining = word;
                while !remaining.is_empty() {
                    let take = remaining
                        .char_indices()
                        .nth(width)
                        .map(|(i, _)| i)
                        .unwrap_or(remaining.len());
                    let (head, tail) = remaining.split_at(take);
                    if tail.is_empty() {
                        // Last fragment — continue accumulating normally.
                        current.push_str(head);
                        current_len = head.chars().count();
                    } else {
                        lines.push(head.to_string());
                    }
                    remaining = tail;
                }
            }
        } else {
            // There is already content on the current line.
            let needed = 1 + word_len; // space + word
            if current_len + needed <= width {
                current.push(' ');
                current.push_str(word);
                current_len += needed;
            } else {
                // Word doesn't fit — flush the current line and start fresh.
                lines.push(current.clone());
                current.clear();
                current_len = 0;

                if word_len <= width {
                    current.push_str(word);
                    current_len = word_len;
                } else {
                    // Hard-break the oversized word.
                    let mut remaining = word;
                    while !remaining.is_empty() {
                        let take = remaining
                            .char_indices()
                            .nth(width)
                            .map(|(i, _)| i)
                            .unwrap_or(remaining.len());
                        let (head, tail) = remaining.split_at(take);
                        if tail.is_empty() {
                            current.push_str(head);
                            current_len = head.chars().count();
                        } else {
                            lines.push(head.to_string());
                        }
                        remaining = tail;
                    }
                }
            }
        }
    }

    if !current.is_empty() || lines.is_empty() {
        lines.push(current);
    }
    lines
}
