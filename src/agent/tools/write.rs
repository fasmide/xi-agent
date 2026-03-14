use std::pin::Pin;

use serde_json::Value;

use crate::agent::types::{Tool, ToolResult};

pub struct WriteTool;

impl Tool for WriteTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn label(&self) -> &str {
        "✍️"
    }

    fn description(&self) -> &str {
        "Write content to a file at the given path, creating parent directories \
         as needed. Overwrites the file if it already exists."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to write"
                },
                "content": {
                    "type": "string",
                    "description": "Content to write to the file"
                }
            },
            "required": ["path", "content"]
        })
    }

    fn execute(
        &self,
        args: Value,
    ) -> Pin<Box<dyn std::future::Future<Output = ToolResult> + Send + '_>> {
        Box::pin(async move {
            let path = match args.get("path").and_then(|v| v.as_str()) {
                Some(p) => p.to_string(),
                None => return ToolResult::err("Missing required parameter: path"),
            };
            let content = match args.get("content").and_then(|v| v.as_str()) {
                Some(c) => c.to_string(),
                None => return ToolResult::err("Missing required parameter: content"),
            };

            // Create parent directories if needed.
            if let Some(parent) = std::path::Path::new(&path).parent()
                && !parent.as_os_str().is_empty()
                    && let Err(e) = tokio::fs::create_dir_all(parent).await {
                        return ToolResult::err(format!(
                            "Failed to create directories for {path}: {e}"
                        ));
                    }

            if let Err(e) = tokio::fs::write(&path, content.as_bytes()).await {
                return ToolResult::err(format!("Failed to write {path}: {e}"));
            }

            let line_count = content.lines().count();
            ToolResult::ok(format!("Written {line_count} lines to {path}"))
        })
    }
}
