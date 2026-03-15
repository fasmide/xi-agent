# pirs

An AI agent harness for the terminal. Chat with local and remote LLMs using
a streaming TUI, with full tool-calling support so the model can read files,
run shell commands, edit code, and feed results back into the conversation.

Built with Rust + [ratatui](https://github.com/ratatui/ratatui) +
[tui-textarea](https://github.com/rhysd/tui-textarea).

## Status

Active development. The core agentic loop is working end-to-end:
multi-provider streaming, tool-calling, inline tool display, thinking output,
and slash commands are all implemented. See [ROADMAP](docs/ROADMAP.md) for
what's coming next.

## Build & Run

```sh
cargo build --release
./target/release/pirs
```

## Providers

| Provider         | Key / credential                                      | Env var override     |
|------------------|-------------------------------------------------------|----------------------|
| `copilot`        | pirs auth store (`auth.json` in platform config dir) | —                    |
| `openai`         | `OPENAI_API_KEY`                                      | `OPENAI_MODEL`       |
| `codex`          | pirs auth store (`auth.json` in platform config dir) | `OPENAI_MODEL`       |
| `ollama`         | none (local)                                          | `OLLAMA_BASE_URL`, `OPENAI_MODEL` |

The default provider is `copilot`. Override at startup:

```sh
PIRS_PROVIDER=openai pirs
pirs --provider openai
pirs -P ollama -m llama3.2
```

## Configuration

| Variable           | Default                          | Description                        |
|--------------------|----------------------------------|------------------------------------|
| `PIRS_PROVIDER`    | `copilot`                        | Active provider                    |
| `COPILOT_MODEL`    | `gpt-4o`                         | Model for Copilot / OpenAI         |
| `OPENAI_API_KEY`   | —                                | API key for OpenAI provider        |
| `OPENAI_MODEL`     | `gpt-4o`                         | Model for OpenAI / Codex / Ollama  |
| `OLLAMA_BASE_URL`  | `http://localhost:11434`         | Ollama server base URL             |

## CLI flags

| Flag                    | Short | Description                                      |
|-------------------------|-------|--------------------------------------------------|
| `--provider <name>`     | `-P`  | Override provider (copilot / openai / codex / ollama) |
| `--model <name>`        | `-m`  | Override model name                              |
| `--print <prompt…>`     | `-p`  | Non-interactive: print response and exit         |

## Keybindings

| Key             | Action                          |
|-----------------|---------------------------------|
| `Enter`         | Submit message                  |
| `Shift+Enter`   | Insert newline in input         |
| `Page Up`       | Scroll chat up one page         |
| `Page Down`     | Scroll chat to bottom           |
| `Scroll wheel`  | Scroll chat (3 lines/tick)      |
| `Ctrl+I`        | Toggle provider/model info bar  |
| `Ctrl+C`        | Quit                            |
| `Esc`           | Quit (or cancel slash command)  |

## Slash commands

| Command              | Description                                      |
|----------------------|--------------------------------------------------|
| `/new`               | Start a new conversation                         |
| `/model`             | Open interactive model picker                    |
| `/model <name>`      | Switch to a named model                          |
| `/provider`          | Open interactive provider picker                 |
| `/provider <name>`   | Switch provider (copilot / openai / codex / ollama) |
| `/login`             | Open interactive auth provider picker (copilot / codex) |
| `/login <provider>`  | Authenticate provider                            |
| `/quit`              | Quit                                             |

## Built-in tools

The agent has access to five tools out of the box:

| Tool        | Emoji | Description                                      |
|-------------|-------|--------------------------------------------------|
| `read_file` | 👀    | Read a file (with optional offset/limit)         |
| `write`     | ✍️    | Write or overwrite a file                        |
| `edit`      | 📝    | Replace exact text in a file                     |
| `bash`      | 💻    | Run a shell command, return stdout/stderr         |
| `find`      | 🔍    | Search files by name glob or content pattern     |

## Non-interactive mode

Send a single prompt and stream the response to stdout:

```sh
pirs --print explain the Cargo.toml
pirs -p what does src/agent/mod.rs do
```

Tool calls are printed to stderr; final output goes to stdout, making it
pipeline-friendly.
