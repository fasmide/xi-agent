# User Interface Specification

## Command line interface

- Start in interactive mode by default.
- Support non-interactive mode with `-p/--print` flag, which prints the result of each command to stdout instead of rendering a UI.

# Interactive UI

- Render a scrollable, paginated view of the session history, with the most recent command at the bottom.
- Tool invocations are rendered as "<icon> <args>", where <icon> is a visual representation of the tool (e.g. a terminal icon for `bash`, a pencil for `edit`, etc.).
- User input is a text input field at the bottom of the screen, where users can type commands and submit them by pressing Enter.

