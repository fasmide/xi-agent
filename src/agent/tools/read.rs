use std::pin::Pin;

use serde_json::Value;

use crate::agent::types::{Tool, ToolResult};

pub struct ReadFileTool;

impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "read"
    }

    fn description(&self) -> &str {
        "Read the contents of a file at the given path. \
         Optionally specify `offset` (1-indexed line number to start from) \
         and `limit` (maximum number of lines to return). \
         When the output is truncated a header `[lines X-Y of Z]` is prepended."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to read"
                },
                "offset": {
                    "type": "integer",
                    "description": "1-indexed line number to start reading from (optional)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of lines to return (optional)"
                }
            },
            "required": ["path"]
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
            let offset = args.get("offset").and_then(|v| v.as_u64()).map(|n| n as usize);
            let limit = args.get("limit").and_then(|v| v.as_u64()).map(|n| n as usize);

            let content = match tokio::fs::read_to_string(&path).await {
                Ok(c) => c,
                Err(e) => return ToolResult::err(format!("Failed to read {path}: {e}")),
            };

            let all_lines: Vec<&str> = content.lines().collect();
            let total = all_lines.len();

            // offset is 1-indexed; default to line 1
            let start = offset.unwrap_or(1).saturating_sub(1);
            let start = start.min(total);

            let end = match limit {
                Some(l) => (start + l).min(total),
                None => total,
            };

            let selected: Vec<&str> = all_lines[start..end].to_vec();
            let truncated = start > 0 || end < total;

            let mut result = String::new();
            if truncated {
                result.push_str(&format!("[lines {}-{} of {}]\n", start + 1, end, total));
            }
            result.push_str(&selected.join("\n"));

            ToolResult::ok(result)
        })
    }
}
