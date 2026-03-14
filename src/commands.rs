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

/// Return commands whose name matches the prefix typed after `/`.
///
/// Once the user has typed a space (entering the argument phase) only exact
/// command-name matches are returned, so the popup becomes a single-row
/// hint rather than a filtering list.
pub fn completions_for(input: &str) -> Vec<&'static SlashCommand> {
    let Some(rest) = input.strip_prefix('/') else {
        return vec![];
    };
    let (prefix, exact_only) = match rest.find(' ') {
        Some(pos) => (&rest[..pos], true),
        None => (rest, false),
    };
    COMMANDS
        .iter()
        .filter(|c| {
            if exact_only {
                c.name == prefix
            } else {
                c.name.starts_with(prefix)
            }
        })
        .collect()
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
