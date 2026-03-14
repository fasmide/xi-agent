use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};
use unicode_width::UnicodeWidthStr;

use crate::{app::App, llm::Role};

/// Background colour of the input panel.
const INPUT_BG: Color = Color::Rgb(30, 30, 40);

/// Background colour of user message blocks in the chat log.
const USER_BG: Color = Color::Rgb(50, 50, 60);

/// Apply visual styles to the textarea at render time.
/// The textarea itself is owned by `App` with no styling baked in;
/// all rendering concerns live here.
fn style_textarea(app: &mut App) {
    app.textarea
        .set_block(Block::default().borders(Borders::NONE));
    app.textarea
        .set_style(Style::default().fg(Color::White).bg(INPUT_BG));
    // Highlight the active cursor line with a slightly brighter shade.
    app.textarea
        .set_cursor_line_style(Style::default().bg(Color::Rgb(50, 50, 65)));
}

/// Render a full-width row of halfblock characters in `color` so that a
/// coloured panel appears to have a smooth sub-character edge against the
/// default terminal background.
///
/// - Top edge: `▄` (lower-half block) — upper half = bg, lower half = color
/// - Bottom edge: `▀` (upper-half block) — upper half = color, lower half = bg
fn halfblock_line(width: usize, ch: char, color: Color) -> Line<'static> {
    Line::from(Span::styled(
        ch.to_string().repeat(width),
        Style::default().fg(color),
    ))
}

pub fn draw(f: &mut ratatui::Frame, app: &mut App) {
    style_textarea(app);

    let terminal_height = f.area().height as usize;

    let input_line_count = app.textarea.lines().len().max(1);
    let max_input_height = (terminal_height * 40 / 100).max(1);
    let input_height = input_line_count.min(max_input_height) as u16;

    // Layout: chat log | top halfblock | input | bottom halfblock
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(1),             // ▄  top edge of input panel
            Constraint::Length(input_height),  // input textarea
            Constraint::Length(1),             // ▀  bottom edge of input panel
        ])
        .split(f.area());

    let log_area    = chunks[0];
    let top_hb_area = chunks[1];
    let input_area  = chunks[2];
    let bot_hb_area = chunks[3];

    // ── Chat log ──────────────────────────────────────────────────────────────
    let inner_height = log_area.height as usize;
    let pane_width   = log_area.width as usize;

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
    let max_scroll  = total_lines.saturating_sub(inner_height);

    if app.auto_scroll {
        app.log_scroll = max_scroll;
    } else {
        app.log_scroll = app.log_scroll.min(max_scroll);
        if app.log_scroll >= max_scroll {
            app.auto_scroll = true;
        }
    }

    let log_paragraph = Paragraph::new(Text::from(lines))
        .block(Block::default().borders(Borders::NONE))
        .scroll((app.log_scroll as u16, 0));

    f.render_widget(log_paragraph, log_area);

    if total_lines > inner_height {
        let mut scrollbar_state =
            ScrollbarState::new(max_scroll + 1).position(app.log_scroll);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            log_area,
            &mut scrollbar_state,
        );
    }

    // ── Halfblock edges ───────────────────────────────────────────────────────
    let width = f.area().width as usize;

    f.render_widget(
        Paragraph::new(halfblock_line(width, '▄', INPUT_BG)),
        top_hb_area,
    );
    f.render_widget(
        Paragraph::new(halfblock_line(width, '▀', INPUT_BG)),
        bot_hb_area,
    );

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
    let user_bg_style = Style::default().bg(USER_BG);

    // Split on explicit newlines first, then wrap each segment to width.
    let segments: Vec<&str> = if content.is_empty() {
        vec![""]
    } else {
        content.split('\n').collect()
    };

    let last_seg = segments.len() - 1;

    if user {
        out.push(halfblock_line(width, '▄', USER_BG));
    }

    for (seg_idx, segment) in segments.iter().enumerate() {
        let is_last_seg = seg_idx == last_seg;
        let chunks = wrap_str(segment, width);
        let last_chunk = chunks.len() - 1;

        for (chunk_idx, chunk) in chunks.iter().enumerate() {
            let is_last_chunk = chunk_idx == last_chunk;
            let show_suffix = !suffix.is_empty() && is_last_seg && is_last_chunk;

            if user {
                // Pad to `width` so the background spans the full line.
                // Use unicode-aware column width, not scalar char count.
                let text_cols = chunk.as_str().width();
                let padding = width.saturating_sub(text_cols);
                let padded = format!("{}{}", chunk, " ".repeat(padding));
                out.push(Line::from(Span::styled(padded, user_bg_style)));
            } else {
                let mut spans: Vec<Span<'static>> = vec![Span::raw(chunk.clone())];
                if show_suffix {
                    spans.push(Span::styled(suffix, Style::default().fg(Color::Yellow)));
                }
                out.push(Line::from(spans));
            }
        }
    }

    if user {
        out.push(halfblock_line(width, '▀', USER_BG));
    }
}

/// Split `text` into lines of at most `width` display columns, preserving
/// internal whitespace. Uses `textwrap` for word-boundary splitting and
/// `unicode-width` for correct column measurement of CJK / emoji characters.
/// Always returns at least one element (empty string when `text` is empty).
fn wrap_str(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    if text.is_empty() {
        return vec![String::new()];
    }
    textwrap::wrap(text, width)
        .into_iter()
        .map(|cow| cow.into_owned())
        .collect()
}
