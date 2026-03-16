use std::{env, fs, path::PathBuf};

/// Metadata parsed from the YAML frontmatter of a `SKILL.md` file.
#[derive(Debug, Clone)]
pub struct SkillMeta {
    pub name: String,
    pub description: String,
    /// Absolute path to the `SKILL.md` file.
    pub path: PathBuf,
    /// Directory containing the `SKILL.md` file (base for relative references).
    pub base_dir: PathBuf,
}

/// Scan `$XDG_CONFIG_HOME/tau/skills/` (or `~/.config/tau/skills/`) for
/// subdirectories that contain a `SKILL.md` with YAML frontmatter.
///
/// Returns an empty vec when the directory does not exist or cannot be read.
pub fn load_skills() -> Vec<SkillMeta> {
    let Some(dir) = skills_dir() else {
        return vec![];
    };
    if !dir.exists() {
        return vec![];
    }

    let Ok(entries) = fs::read_dir(&dir) else {
        return vec![];
    };

    let mut skills: Vec<SkillMeta> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_dir() {
                return None;
            }
            let skill_file = path.join("SKILL.md");
            let content = fs::read_to_string(&skill_file).ok()?;
            parse_skill_meta(&content, skill_file)
        })
        .collect();

    // Deterministic order.
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    skills
}

fn skills_dir() -> Option<PathBuf> {
    if let Some(xdg) = env::var_os("XDG_CONFIG_HOME").filter(|s| !s.is_empty()) {
        return Some(PathBuf::from(xdg).join("tau").join("skills"));
    }
    env::var_os("HOME")
        .filter(|s| !s.is_empty())
        .map(|h| PathBuf::from(h).join(".config").join("tau").join("skills"))
}

/// Parse `name` and `description` from the YAML frontmatter block (`---` … `---`)
/// at the start of a `SKILL.md` file.
fn parse_skill_meta(content: &str, path: PathBuf) -> Option<SkillMeta> {
    let mut lines = content.lines();

    // First line must be `---`
    if lines.next()?.trim() != "---" {
        return None;
    }

    let mut name: Option<String> = None;
    let mut description: Option<String> = None;

    for line in lines {
        if line.trim() == "---" {
            break;
        }
        if let Some(v) = line.strip_prefix("name:") {
            name = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("description:") {
            description = Some(v.trim().to_string());
        }
    }

    let base_dir = path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_default();

    Some(SkillMeta {
        name: name?,
        description: description?,
        path,
        base_dir,
    })
}

/// Expand a skill invocation into the `<skill>` XML block that is submitted to
/// the model, following the Agent Skills specification format used by pi.
///
/// Format:
/// ```text
/// <skill name="{name}" location="{path}">
/// References are relative to {base_dir}.
///
/// {body — SKILL.md with frontmatter stripped}
/// </skill>
///
/// {optional args}
/// ```
pub fn expand_skill(skill: &SkillMeta, args: &str) -> anyhow::Result<String> {
    let content = fs::read_to_string(&skill.path)?;
    let body = strip_frontmatter(&content).trim();

    let skill_block = format!(
        "<skill name=\"{}\" location=\"{}\">\nReferences are relative to {}.\n\n{}\n</skill>",
        skill.name,
        skill.path.display(),
        skill.base_dir.display(),
        body,
    );

    if args.is_empty() {
        Ok(skill_block)
    } else {
        Ok(format!("{skill_block}\n\n{args}"))
    }
}

/// Strip YAML frontmatter (`---` … `---`) from the start of a file and return
/// the body text.  Returns the original string unchanged if no frontmatter is
/// found.
fn strip_frontmatter(content: &str) -> &str {
    let mut pos: usize = 0;
    let mut fence_seen = false;

    for line in content.split('\n') {
        // Handle CRLF gracefully.
        let trimmed = line.trim_end_matches('\r');
        // Byte length including the '\n' separator we split on.
        let advance = line.len() + 1;

        if !fence_seen {
            if trimmed == "---" {
                fence_seen = true;
                pos += advance;
                continue;
            } else {
                // Content does not start with a frontmatter fence.
                return content;
            }
        }

        pos += advance;

        if trimmed == "---" {
            // `pos` now points to the first byte after the closing `---\n`.
            return if pos <= content.len() {
                &content[pos..]
            } else {
                ""
            };
        }
    }

    content
}

#[cfg(test)]
mod tests {
    use super::{expand_skill, parse_skill_meta, strip_frontmatter};
    use std::path::PathBuf;

    fn dummy_path() -> PathBuf {
        PathBuf::from("/tmp/skills/work-loop/SKILL.md")
    }

    #[test]
    fn parses_standard_frontmatter() {
        let content = "\
---
name: work-loop
description: guides most non-trivial coding work.
---

# Work loop
";
        let meta = parse_skill_meta(content, dummy_path()).expect("should parse");
        assert_eq!(meta.name, "work-loop");
        assert_eq!(meta.description, "guides most non-trivial coding work.");
        assert_eq!(meta.base_dir, PathBuf::from("/tmp/skills/work-loop"));
    }

    #[test]
    fn returns_none_without_opening_fence() {
        let content = "name: foo\ndescription: bar\n";
        assert!(parse_skill_meta(content, dummy_path()).is_none());
    }

    #[test]
    fn returns_none_when_name_missing() {
        let content = "---\ndescription: bar\n---\n";
        assert!(parse_skill_meta(content, dummy_path()).is_none());
    }

    #[test]
    fn returns_none_when_description_missing() {
        let content = "---\nname: foo\n---\n";
        assert!(parse_skill_meta(content, dummy_path()).is_none());
    }

    #[test]
    fn strip_frontmatter_removes_fence() {
        let content = "---\nname: foo\n---\n\n# Body\n";
        assert_eq!(strip_frontmatter(content), "\n# Body\n");
    }

    #[test]
    fn strip_frontmatter_no_fence_returns_original() {
        let content = "# Just a doc\nno frontmatter here\n";
        assert_eq!(strip_frontmatter(content), content);
    }

    #[test]
    fn expand_skill_wraps_body() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let skill_dir = dir.path().join("my-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        let path = skill_dir.join("SKILL.md");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "---\nname: my-skill\ndescription: test.\n---\n\n# My skill\nDo the thing.").unwrap();

        let meta = parse_skill_meta(
            &std::fs::read_to_string(&path).unwrap(),
            path.clone(),
        )
        .unwrap();

        let expanded = expand_skill(&meta, "").unwrap();
        assert!(expanded.contains("<skill name=\"my-skill\""));
        assert!(expanded.contains("References are relative to"));
        assert!(expanded.contains("# My skill"));
        assert!(expanded.contains("</skill>"));
    }

    #[test]
    fn expand_skill_appends_args() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let skill_dir = dir.path().join("my-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        let path = skill_dir.join("SKILL.md");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "---\nname: my-skill\ndescription: test.\n---\n\n# Body").unwrap();

        let meta = parse_skill_meta(
            &std::fs::read_to_string(&path).unwrap(),
            path.clone(),
        )
        .unwrap();

        let expanded = expand_skill(&meta, "implement the feature").unwrap();
        assert!(expanded.ends_with("\n\nimplement the feature"));
    }
}
