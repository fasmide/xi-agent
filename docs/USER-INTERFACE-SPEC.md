# User Interface Specification

## Command line interface

- Start in interactive mode by default.
- Support non-interactive mode with `-p/--print` flag, which prints the result of each command to stdout instead of rendering a UI.

# Interactive UI

- Render a scrollable, paginated view of the session history, with the most recent command at the bottom.
- Tool invocations are rendered as "<icon> <args>", where <icon> is a visual representation of the tool (e.g. a terminal icon for `bash`, a pencil for `edit`, etc.).
- User input is a text input field at the bottom of the screen, where users can type commands and submit them by pressing Enter.

## Login panel

When `/login <provider>` is active the input area is replaced by a login panel
injected at the bottom of the screen (the same vertical layout slot as the
selection menu).

Layout:

```
┌─ header row ──────────────────────────────────── Esc cancel ─┐
│  <status/progress line>                                       │
│                                                               │
│  URL: [open in browser →]   ← OSC 8 hyperlink                │
│  Code: XXXX-YYYY            ← device flow only               │
└───────────────────────────────────────────────────────────────┘
```

- The browser is opened automatically via `xdg-open` / `open` / `start`.
- The `open in browser →` label is an OSC 8 terminal hyperlink; clicking it
  opens the URL in terminals that support hyperlinks (Kitty, WezTerm, iTerm2,
  GNOME Terminal ≥ 3.26, foot, …). In other terminals it renders as underlined
  text.
- `Esc` cancels the flow; success or failure is appended to the chat log.
- The panel height adjusts automatically to the number of content rows.

