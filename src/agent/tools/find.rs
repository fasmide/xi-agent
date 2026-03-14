use std::pin::Pin;

use globset::Glob;
use ignore::WalkBuilder;
use serde_json::Value;

use crate::agent::types::{Tool, ToolResult};

const DEFAULT_LIMIT: usize = 1000;

pub struct FindTool;

impl Tool for FindTool {
    fn name(&self) -> &str {
        "find_files"
    }

    fn label(&self) -> &str {
        "🔍"
    }

    fn description(&self) -> &str {
        "Search for files matching a glob pattern. Returns file paths relative \
         to the search directory, one per line, sorted alphabetically. \
         Respects .gitignore. Output is capped at `limit` results (default \
         1000); a notice is appended when the cap is reached."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern to match file paths, \
                                    e.g. '*.rs', '**/*.json', 'src/**/*.rs'"
                },
                "path": {
                    "type": "string",
                    "description": "Directory to search in (default: current directory)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of results to return (default: 1000)"
                }
            },
            "required": ["pattern"]
        })
    }

    fn execute(
        &self,
        args: Value,
    ) -> Pin<Box<dyn std::future::Future<Output = ToolResult> + Send + '_>> {
        Box::pin(async move {
            let pattern = match args.get("pattern").and_then(|v| v.as_str()) {
                Some(p) => p.to_string(),
                None => return ToolResult::err("Missing required parameter: pattern"),
            };
            let search_dir = args
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or(".")
                .to_string();
            let limit = args
                .get("limit")
                .and_then(|v| v.as_u64())
                .map(|n| n as usize)
                .unwrap_or(DEFAULT_LIMIT);

            // Compile the glob pattern up front so we can report errors early.
            // We use `**/<pattern>` anchoring so bare patterns like `*.rs`
            // match anywhere in the tree, consistent with pi-mono behaviour.
            let matcher = match Glob::new(&pattern) {
                Ok(g) => g.compile_matcher(),
                Err(e) => {
                    return ToolResult::err(format!(
                        "Invalid glob pattern '{pattern}': {e}"
                    ))
                }
            };

            // Verify the search directory exists before spawning the walker.
            if !std::path::Path::new(&search_dir).exists() {
                return ToolResult::err(format!("Path not found: {search_dir}"));
            }

            // The ignore::Walk API is synchronous; run it on the blocking thread pool.
            let result = tokio::task::spawn_blocking(move || {
                let mut matches: Vec<String> = Vec::new();

                let walker = WalkBuilder::new(&search_dir)
                    // Include hidden files (dotfiles); gitignore handles exclusions.
                    .hidden(false)
                    // Respect .gitignore, .ignore, and global gitignore.
                    .git_ignore(true)
                    .git_global(true)
                    .git_exclude(true)
                    .sort_by_file_path(std::cmp::Ord::cmp)
                    .build();

                for entry in walker {
                    let entry = match entry {
                        Ok(e) => e,
                        Err(_) => continue,
                    };

                    // Skip directory entries themselves; we only report files.
                    if entry.file_type().is_some_and(|t| t.is_dir()) {
                        continue;
                    }

                    // Build a forward-slash relative path for matching.
                    let abs = entry.path();
                    let rel = match abs.strip_prefix(&search_dir) {
                        Ok(p) => p.to_string_lossy().replace('\\', "/"),
                        Err(_) => abs.to_string_lossy().replace('\\', "/"),
                    };

                    if matcher.is_match(&rel) {
                        matches.push(rel);
                        // Collect one extra to detect whether the limit was hit.
                        if matches.len() > limit {
                            break;
                        }
                    }
                }

                matches
            })
            .await;

            let mut matches = match result {
                Ok(m) => m,
                Err(e) => return ToolResult::err(format!("Find failed: {e}")),
            };

            let limit_reached = matches.len() > limit;
            if limit_reached {
                matches.truncate(limit);
            }

            if matches.is_empty() {
                return ToolResult::ok("No files found matching pattern");
            }

            let mut output = matches.join("\n");

            if limit_reached {
                output.push_str(&format!(
                    "\n\n[{limit} result limit reached — use limit=N for more or refine the pattern]"
                ));
            }

            ToolResult::ok(output)
        })
    }
}
