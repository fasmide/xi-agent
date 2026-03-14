# Chat TUI Design — 2026-03-14

## Overview
A terminal chat UI built with Rust + ratatui + tui-textarea.

## Layout
Vertical split, full terminal height:
- **Chat log** (top, `Min(1)`): scrollable list of past messages rendered as `Paragraph` with `Wrap`.
- **Input box** (bottom, `Length(line_count + 2)`): `tui-textarea` widget, grows with content, clamped to 40% of terminal height.

## Data Model
```rust
struct App {
    messages: Vec<String>,  // submitted chat entries
    textarea: TextArea,     // live input state
    log_scroll: usize,      // scroll offset for chat log (usize::MAX = snap to bottom)
}
```

## Event Handling
| Key | Action |
|-----|--------|
| `Enter` (no modifiers) | Submit textarea → push to messages, reset textarea |
| `Esc` / `Ctrl-C` | Quit |
| Everything else | Forwarded to `tui-textarea` |

## Dependencies
- `ratatui = "0.29"` — TUI framework
- `crossterm = "0.28"` — backend (pinned to match tui-textarea)
- `tui-textarea = "0.7"` — growing multi-line input widget
