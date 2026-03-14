# pirs

An AI agent harness for the terminal. Chat with local and remote LLMs, with
tool-calling support so the model can take actions and feed results back into
the conversation.

Built with Rust + [ratatui](https://github.com/ratatui/ratatui) +
[tui-textarea](https://github.com/rhysd/tui-textarea).

## Status

Early development. Streaming chat with Ollama works. Tool-calling and
multi-provider support are in progress — see [ROADMAP](docs/ROADMAP.md).

## Build & Run

```sh
cargo build --release
./target/release/pirs
```

Requires a running [Ollama](https://ollama.com) instance by default.

## Configuration

| Environment variable | Default                    | Description                  |
|----------------------|----------------------------|------------------------------|
| `OLLAMA_HOST`        | `http://localhost:11434`   | Ollama server base URL       |
| `OLLAMA_MODEL`       | `llama3.1`                 | Model name to use            |

## Keybindings

| Key            | Action                        |
|----------------|-------------------------------|
| `Enter`        | Submit message                |
| `Shift+Enter`  | Insert newline in input       |
| `Page Up`      | Scroll chat up one page       |
| `Page Down`    | Scroll chat to bottom         |
| `Scroll wheel` | Scroll chat (3 lines/tick)    |
| `Esc`          | Quit                          |
| `Ctrl+C`       | Quit                          |
