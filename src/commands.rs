/// A slash command supported by the application.
pub struct SlashCommand {
    pub name: &'static str,
    /// Full usage string shown in the completion popup (e.g. `/model <name>`).
    pub usage: &'static str,
    pub description: &'static str,
    /// Whether this command accepts a required argument after its name.
    pub takes_arg: bool,
}

/// All supported slash commands, in display order.
pub static COMMANDS: &[SlashCommand] = &[
    SlashCommand {
        name: "new",
        usage: "/new",
        description: "Start a new conversation",
        takes_arg: false,
    },
    SlashCommand {
        name: "model",
        usage: "/model <name>",
        description: "Switch to a different model",
        takes_arg: true,
    },
    SlashCommand {
        name: "quit",
        usage: "/quit",
        description: "Quit the application",
        takes_arg: false,
    },
];

// ── Completion items ──────────────────────────────────────────────────────────

/// A single entry in the completion popup.
pub struct CompletionItem {
    /// Primary text shown in the left column (command usage or model name).
    pub label: String,
    /// Secondary text shown in the right column (description or empty).
    pub detail: String,
    /// Text to place in the textarea when this item is selected via Tab/Enter.
    /// Empty for non-interactive items (e.g. the loading indicator).
    pub complete_to: String,
    /// When true the item is a non-interactive status row (e.g. "fetching…").
    pub loading: bool,
}

impl CompletionItem {
    fn from_command(cmd: &SlashCommand) -> Self {
        Self {
            label: cmd.usage.to_string(),
            detail: cmd.description.to_string(),
            complete_to: if cmd.takes_arg {
                format!("/{} ", cmd.name)
            } else {
                format!("/{}", cmd.name)
            },
            loading: false,
        }
    }

    fn from_model(name: &str) -> Self {
        Self {
            label: name.to_string(),
            detail: String::new(),
            complete_to: format!("/model {}", name),
            loading: false,
        }
    }

    fn loading_indicator() -> Self {
        Self {
            label: "fetching models…".to_string(),
            detail: String::new(),
            complete_to: String::new(),
            loading: true,
        }
    }
}

// ── Completion matching ───────────────────────────────────────────────────────

/// Build the completion list for the current textarea `input`.
///
/// **Phase 1 — command name** (no space yet): filter `COMMANDS` by prefix.
/// **Phase 2 — argument** (space present after `/model`): filter available
/// model names by the typed prefix, or show a loading indicator while the
/// model list is being fetched.
pub fn completions_for(
    input: &str,
    available_models: Option<&[String]>,
    models_loading: bool,
) -> Vec<CompletionItem> {
    let Some(rest) = input.strip_prefix('/') else {
        return vec![];
    };

    match rest.find(' ') {
        // ── Phase 2: argument ─────────────────────────────────────────────────
        Some(space_pos) => {
            let cmd = &rest[..space_pos];
            let arg = rest[space_pos + 1..].trim_start();

            match cmd {
                "model" => {
                    if models_loading {
                        vec![CompletionItem::loading_indicator()]
                    } else if let Some(models) = available_models {
                        models
                            .iter()
                            .filter(|m| m.starts_with(arg))
                            .map(|m| CompletionItem::from_model(m))
                            .collect()
                    } else {
                        // Models not fetched yet — will trigger a fetch via
                        // App::should_fetch_models(); show nothing for now.
                        vec![]
                    }
                }
                _ => vec![],
            }
        }
        // ── Phase 1: command name ─────────────────────────────────────────────
        None => COMMANDS
            .iter()
            .filter(|c| c.name.starts_with(rest))
            .map(CompletionItem::from_command)
            .collect(),
    }
}

// ── Command parsing ───────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum CommandAction {
    New,
    Quit,
    /// Switch model to the given name.
    Model(String),
    /// `/model` typed with no argument — show usage hint.
    ModelNoArg,
}

/// Parse a complete slash command input string into an action.
/// Returns `None` if the input is not a recognised slash command.
pub fn parse(input: &str) -> Option<CommandAction> {
    let rest = input.strip_prefix('/')?;
    let (name, arg) = match rest.find(' ') {
        Some(pos) => (&rest[..pos], rest[pos + 1..].trim()),
        None => (rest, ""),
    };
    match name {
        "new" => Some(CommandAction::New),
        "quit" => Some(CommandAction::Quit),
        "model" if !arg.is_empty() => Some(CommandAction::Model(arg.to_string())),
        "model" => Some(CommandAction::ModelNoArg),
        _ => None,
    }
}
