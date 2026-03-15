use std::pin::Pin;

use serde_json::Value;

use crate::agent::types::{Tool, ToolResult};

pub struct EditTool;

impl Tool for EditTool {
    fn name(&self) -> &str {
        "edit_file"
    }

    fn description(&self) -> &str {
        "Edit a file by replacing an exact text occurrence with new text. \
         `old_text` must match exactly (including whitespace and newlines) and \
         must appear exactly once in the file — the call fails if zero or \
         multiple matches are found."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to edit"
                },
                "old_text": {
                    "type": "string",
                    "description": "Exact text to find (must appear exactly once)"
                },
                "new_text": {
                    "type": "string",
                    "description": "Text to replace old_text with"
                }
            },
            "required": ["path", "old_text", "new_text"]
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
            let old_text = match args.get("old_text").and_then(|v| v.as_str()) {
                Some(t) => t.to_string(),
                None => return ToolResult::err("Missing required parameter: old_text"),
            };
            let new_text = match args.get("new_text").and_then(|v| v.as_str()) {
                Some(t) => t.to_string(),
                None => return ToolResult::err("Missing required parameter: new_text"),
            };

            let content = match tokio::fs::read_to_string(&path).await {
                Ok(c) => c,
                Err(e) => return ToolResult::err(format!("Failed to read {path}: {e}")),
            };

            let count = content.matches(old_text.as_str()).count();
            if count == 0 {
                return ToolResult::err(format!(
                    "old_text not found in {path}. \
                     Verify the text matches exactly, including whitespace."
                ));
            }
            if count > 1 {
                return ToolResult::err(format!(
                    "old_text found {count} times in {path}. \
                     old_text must be unique to avoid ambiguous edits."
                ));
            }

            let updated = content.replacen(old_text.as_str(), new_text.as_str(), 1);

            if let Err(e) = tokio::fs::write(&path, updated.as_bytes()).await {
                return ToolResult::err(format!("Failed to write {path}: {e}"));
            }

            ToolResult::ok(format!("Successfully edited {path}"))
        })
    }
}
