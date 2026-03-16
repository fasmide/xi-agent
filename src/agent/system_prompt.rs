use chrono::Local;
use directories::BaseDirs;
use std::{fs, path::Path};

use crate::agent::types::ToolRegistry;
use crate::skills::SkillMeta;

/// Helper to read and combine AGENTS.md content.
pub fn read_agents_md(cwd: &str, test_home: Option<&Path>) -> String {
    let mut content = String::new();

    // Check for ~/.tau/AGENTS.md
    let home_dir_buf = test_home.map(|p| p.to_path_buf()).or_else(|| BaseDirs::new().map(|bd| bd.home_dir().to_path_buf()));
    if let Some(home_dir) = home_dir_buf.as_deref() {
        let global_agents_md = home_dir.join(".tau/AGENTS.md");
        if global_agents_md.exists() && let Ok(file_content) = fs::read_to_string(&global_agents_md) {
            content.push_str(&file_content);
            content.push('\n');
        }
    }

    // Check cwd and its parent directories for AGENTS.md
    let mut current_dir = Path::new(cwd);
    loop {
        let agents_md_path = current_dir.join("AGENTS.md");
        if agents_md_path.exists() && let Ok(file_content) = fs::read_to_string(&agents_md_path) {
            content.push_str(&file_content);
            content.push('\n');
        }

        match current_dir.parent() {
            Some(parent) if parent != current_dir => current_dir = parent,
            _ => break,
        }
    }

    content
}

/// Build the default system prompt for the agent loop.
///
/// Mirrors pi-mono's `buildSystemPrompt`: declares the agent's identity,
/// lists available tools with their descriptions, adds tool-use guidelines,
/// stamps the current date and working directory, and optionally appends an
/// `<available_skills>` block when skill files are present.
pub fn build_system_prompt(tools: &ToolRegistry, cwd: &str, skills: &[SkillMeta]) -> String {
    let date = Local::now().format("%Y-%m-%d").to_string();

    // Include AGENTS.md content
    let agents_md_content = read_agents_md(cwd, None);

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

    if has("read_file") && has("edit_file") {
        guidelines.push("Use read_file to examine files before editing.");
    }
    if has("edit_file") {
        guidelines.push("Use edit_file for precise changes — old_text must match exactly.");
    }
    if has("write_file") {
        guidelines.push("Use write_file only for new files or complete rewrites.");
    }
    if has("find_files") || has("bash") {
        guidelines
            .push("Use find_files or bash to explore the filesystem rather than guessing paths.");
    }
    if has("ask_user") {
        guidelines.push("Use ask_user only when the task requires a user decision or information you cannot infer.");
        guidelines.push("Before calling ask_user, gather relevant context with your other tools and include a short summary in the context field.");
    }
    guidelines.push("Be concise. Show file paths clearly when working with files.");
    guidelines.push("Always use your tools to answer questions about files and the system — do not write code that the user would have to run themselves.");

    let guidelines_text = guidelines
        .iter()
        .map(|g| format!("- {g}"))
        .collect::<Vec<_>>()
        .join("\n");

    let skills_section = render_skills_block(skills);

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
{agents_md_content}\n\
Current date: {date}\n\
Current working directory: {cwd}{skills_section}"
    )
}

/// Render the `<available_skills>` prompt block from a slice of skill metadata.
/// Returns an empty string when `skills` is empty.
fn render_skills_block(skills: &[SkillMeta]) -> String {
    if skills.is_empty() {
        return String::new();
    }

    let entries: String = skills
        .iter()
        .map(|s| {
            format!(
                "  <skill>\n    <name>{}</name>\n    <description>{}</description>\n    <location>{}</location>\n  </skill>",
                s.name,
                s.description,
                s.path.display()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "\n\
\nThe following skills provide specialized instructions for specific tasks.\n\
Use the read tool to load a skill's file when the task matches its description.\n\
When a skill file references a relative path, resolve it against the skill directory \
(parent of SKILL.md / dirname of the path) and use that absolute path in tool commands.\n\
\n<available_skills>\n{entries}\n</available_skills>"
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