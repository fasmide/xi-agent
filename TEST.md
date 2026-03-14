# Test Specification

Describes how to verify the application manually until automated tests
are in place. See [docs/plans/2026-03-14-tests.md](docs/plans/2026-03-14-tests.md)
for the automated test plan.

---

## 1. Build

```sh
cargo build
cargo clippy            # must produce zero warnings
```

---

## 2. Startup

| Step | Expected |
|------|----------|
| `./target/debug/pirs` | TUI opens; no crash; input field is focused |
| Provider/model in info bar (`Ctrl+I`) | Shows correct provider (`copilot`) and model (`gpt-4o`) |
| `--help` | Prints usage including `--provider`, `--model`, `--print` flags |

---

## 3. Basic chat

| Step | Expected |
|------|----------|
| Type a message, press `Enter` | Message appears in log; streaming begins |
| Streaming in progress | Tokens appear incrementally; auto-scroll follows |
| Response finishes | `streaming` indicator stops; input is enabled |
| `Shift+Enter` | Inserts newline, does not submit |
| Empty input + `Enter` | No submission |

---

## 4. Scrolling

| Step | Expected |
|------|----------|
| `Page Up` | Scrolls log up; auto-scroll disabled |
| `Page Down` | Snaps to bottom; auto-scroll re-enabled |
| Mouse scroll up | Scrolls up 3 lines per tick |
| Mouse scroll to bottom | Re-enables auto-scroll |

---

## 5. Slash commands

| Command | Expected |
|---------|----------|
| `/` | Completion popup appears with all commands |
| `/new` + `Enter` | Chat log clears; fresh conversation |
| `/quit` + `Enter` | Application exits |
| `/model` + `Enter` | Full-screen model picker opens; models load async |
| Up/Down in picker | Selection highlights move |
| `Enter` in picker | Switches model; picker closes |
| `Esc` in picker | Picker closes without change |
| `/model gpt-4o-mini` + `Enter` | Model switches directly |
| `/provider` + `Enter` | Provider picker opens with 4 entries |
| `/provider openai` + `Enter` | Provider switches |

---

## 6. Providers

For each provider that has credentials available:

| Step | Expected |
|------|----------|
| `/provider <name>`, ask a simple question | Response streams successfully |
| `--provider <name>` flag at launch | Correct provider active on startup |

---

## 7. Tool calls

Send prompts that exercise each built-in tool:

| Prompt | Tool expected | Pass criteria |
|--------|--------------|---------------|
| "List the files in the current directory" | `bash` or `find` | Lists files correctly |
| "Read the Cargo.toml" | `read_file` | Shows contents |
| "What is in src/main.rs lines 1-20?" | `read_file` (offset/limit) | Shows correct lines |
| "Create a file /tmp/pirs-test.txt with content 'hello'" | `write` | File exists and contains 'hello' |
| "Edit /tmp/pirs-test.txt: replace 'hello' with 'world'" | `edit` | File now contains 'world' |
| "Find all .rs files in src/" | `find` | Lists .rs files |

---

## 8. Non-interactive mode

```sh
./target/debug/pirs --print what is 2 plus 2
./target/debug/pirs -p list the files in the current directory
./target/debug/pirs --provider ollama --print say hello
```

Expected: response streamed to stdout, tool calls printed to stderr, exits with code 0.

---

## 9. Info bar

| Step | Expected |
|------|----------|
| `Ctrl+I` | Info bar appears below input showing provider, model, context tokens |
| `Ctrl+I` again | Info bar hides |

---

## 10. Quit

| Step | Expected |
|------|----------|
| `Ctrl+C` | Application exits cleanly |
| `Esc` (outside slash mode) | Application exits cleanly |
| `Esc` (in slash mode, e.g. typed `/new`) | Clears input, does not quit |
