use chrono::Local;

use crate::agent::types::ToolRegistry;

/// Build the default system prompt for the agent loop.
///
/// Mirrors pi-mono's `buildSystemPrompt`: declares the agent's identity,
/// lists available tools with their descriptions, adds tool-use guidelines,
/// and stamps the current date and working directory so the model has
/// accurate context.
pub fn build_system_prompt(tools: &ToolRegistry, cwd: &str) -> String {
    let date = Local::now().format("%Y-%m-%d").to_string();

    // Build tool list sorted by name for deterministic output.
    let mut tool_names: Vec<&str> = tools.keys().map(String::as_str).collect();
    tool_names.sort_unstable();

    let tool_list = if tool_names.is_empty() {
        "(none)".to_string()
    } else {
        tool_names
            .iter()
            .map(|name| {
                let desc = tools.get(*name).map(|t| t.description()).unwrap_or(*name);
                // Trim the description to its first sentence for brevity.
                let short = first_sentence(desc);
                format!("- {name}: {short}")
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    // Build guidelines conditioned on which tools are present.
    let has = |name: &str| tool_names.contains(&name);
    let mut guidelines: Vec<&str> = Vec::new();

    if has("👀") && has("📝") {
        guidelines.push("Use 👀 to examine files before editing.");
    }
    if has("📝") {
        guidelines.push("Use 📝 for precise changes — old_text must match exactly.");
    }
    if has("✍️") {
        guidelines.push("Use ✍️ only for new files or complete rewrites.");
    }
    if has("🔍") || has("💻") {
        guidelines.push("Use 🔍 or 💻 to explore the filesystem rather than guessing paths.");
    }
    guidelines.push("Be concise. Show file paths clearly when working with files.");
    guidelines.push("Always use your tools to answer questions about files and the system — do not write code that the user would have to run themselves.");

    let guidelines_text = guidelines
        .iter()
        .map(|g| format!("- {g}"))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "You are an expert coding assistant and autonomous agent. \
You help users by reading files, executing commands, editing code, and writing new files. \
Use your tools proactively to answer questions — never write code as a substitute for calling a tool.\n\
\n\
Available tools:\n\
{tool_list}\n\
\n\
Guidelines:\n\
{guidelines_text}\n\
\n\
Current date: {date}\n\
Current working directory: {cwd}"
    )
}

/// Return the text up to and including the first `.`, `!`, or `?`,
/// or the whole string if none is found. Strips leading/trailing whitespace.
fn first_sentence(s: &str) -> &str {
    let end = s
        .char_indices()
        .find(|(_, c)| matches!(c, '.' | '!' | '?'))
        .map(|(i, _)| i + 1)
        .unwrap_or(s.len());
    s[..end].trim()
}
