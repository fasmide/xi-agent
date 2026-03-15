use std::pin::Pin;

use serde_json::Value;

use crate::agent::types::{Tool, ToolResult};

/// Maximum bytes captured from stdout or stderr before truncation.
const MAX_OUTPUT_BYTES: usize = 8 * 1024; // 8 KiB

pub struct BashTool;

impl Tool for BashTool {
    fn name(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        "Run a shell command via `/bin/sh -c` and return its stdout, stderr, \
         and exit code. Both stdout and stderr are truncated to 8 KiB each. \
         The command is considered successful regardless of exit code — \
         check the exit code in the output to determine success."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Shell command to execute"
                }
            },
            "required": ["command"]
        })
    }

    fn execute(
        &self,
        args: Value,
    ) -> Pin<Box<dyn std::future::Future<Output = ToolResult> + Send + '_>> {
        Box::pin(async move {
            let command = match args.get("command").and_then(|v| v.as_str()) {
                Some(c) => c.to_string(),
                None => return ToolResult::err("Missing required parameter: command"),
            };

            let output = match tokio::process::Command::new("sh")
                .arg("-c")
                .arg(&command)
                .output()
                .await
            {
                Ok(o) => o,
                Err(e) => return ToolResult::err(format!("Failed to spawn shell: {e}")),
            };

            let exit_code = output.status.code().unwrap_or(-1);

            let stdout = truncate_bytes(&output.stdout, MAX_OUTPUT_BYTES);
            let stderr = truncate_bytes(&output.stderr, MAX_OUTPUT_BYTES);

            let mut result = format!("exit {exit_code}\n");

            if !stdout.is_empty() {
                result.push_str("stdout:\n");
                result.push_str(&stdout);
                if !stdout.ends_with('\n') {
                    result.push('\n');
                }
            }

            if !stderr.is_empty() {
                result.push_str("stderr:\n");
                result.push_str(&stderr);
                if !stderr.ends_with('\n') {
                    result.push('\n');
                }
            }

            // is_error = false: the model sees the exit code and decides.
            ToolResult::ok(result)
        })
    }
}

/// Convert raw bytes to a UTF-8 string, truncating to `max_bytes` if needed.
/// Appends a `[truncated]` marker when truncation occurs.
fn truncate_bytes(bytes: &[u8], max_bytes: usize) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    if bytes.len() <= max_bytes {
        String::from_utf8_lossy(bytes).into_owned()
    } else {
        let mut s = String::from_utf8_lossy(&bytes[..max_bytes]).into_owned();
        s.push_str("\n[truncated]");
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::types::Tool;

    #[tokio::test]
    async fn bash_captures_stdout() {
        let tool = BashTool;
        let args = serde_json::json!({"command": "echo hello"});
        let result = tool.execute(args).await;
        assert!(!result.is_error);
        assert!(result.content.contains("hello"), "stdout not captured: {}", result.content);
    }

    #[tokio::test]
    async fn bash_captures_stderr() {
        let tool = BashTool;
        let args = serde_json::json!({"command": "echo oops >&2"});
        let result = tool.execute(args).await;
        assert!(!result.is_error);
        assert!(result.content.contains("oops"), "stderr not captured: {}", result.content);
    }

    #[tokio::test]
    async fn bash_nonzero_exit_not_error() {
        let tool = BashTool;
        let args = serde_json::json!({"command": "exit 42"});
        let result = tool.execute(args).await;
        // is_error stays false; the exit code is embedded in the content
        assert!(!result.is_error);
        assert!(result.content.contains("exit 42"), "exit code not in output: {}", result.content);
    }

    #[tokio::test]
    async fn bash_truncates_large_output() {
        let tool = BashTool;
        // Generate >8 KiB of output.
        let args = serde_json::json!({"command": "head -c 16384 /dev/urandom | base64"});
        let result = tool.execute(args).await;
        assert!(!result.is_error);
        assert!(
            result.content.contains("[truncated]"),
            "expected truncation marker: {}",
            &result.content[..100.min(result.content.len())]
        );
    }

    #[tokio::test]
    async fn bash_missing_command_param_is_error() {
        let tool = BashTool;
        let args = serde_json::json!({});
        let result = tool.execute(args).await;
        assert!(result.is_error);
    }
}
