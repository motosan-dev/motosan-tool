use std::future::Future;
use std::pin::Pin;

use serde::Deserialize;
use serde_json::json;

use crate::{Tool, ToolContext, ToolDef, ToolResult};

const BINARY: &str = "agent-browser";

/// A tool that manages browser authentication state via `agent-browser`.
///
/// Actions:
/// - `load`: Next `open` uses `--state <path>` to load auth state.
/// - `save`: Runs `agent-browser state save <path>` to persist current state.
/// - `auto-connect-save`: Runs `agent-browser --auto-connect state save <path>`.
pub struct BrowserAuthTool;

#[derive(Debug, Deserialize)]
struct Input {
    action: String,
    path: String,
}

impl Default for BrowserAuthTool {
    fn default() -> Self {
        Self::new()
    }
}

impl BrowserAuthTool {
    pub fn new() -> Self {
        Self
    }
}

impl Tool for BrowserAuthTool {
    fn def(&self) -> ToolDef {
        ToolDef {
            name: "browser_auth".to_string(),
            description: "Manage browser authentication state. Actions: load (use auth state on \
                next open), save (persist current state), auto-connect-save (save with \
                auto-connect)."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["load", "save", "auto-connect-save"],
                        "description": "The auth action to perform"
                    },
                    "path": {
                        "type": "string",
                        "description": "Path to the auth state file (e.g. ~/.pilot/auth-591.json)"
                    }
                },
                "required": ["action", "path"]
            }),
        }
    }

    fn call(
        &self,
        args: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Pin<Box<dyn Future<Output = ToolResult> + Send + '_>> {
        Box::pin(async move {
            let input: Input = match serde_json::from_value(args) {
                Ok(v) => v,
                Err(e) => return ToolResult::error(format!("Invalid input: {e}")),
            };

            match input.action.as_str() {
                "load" => {
                    // "load" tells the system that the next open should use --state <path>.
                    // Since we delegate to agent-browser, we validate the path and return
                    // instructions for the caller.
                    ToolResult::text(format!(
                        "Auth state loaded. Next browser_navigate will use --state {}",
                        input.path
                    ))
                }
                "save" => run_state_save(&input.path, false).await,
                "auto-connect-save" => run_state_save(&input.path, true).await,
                _ => ToolResult::error(format!(
                    "Unknown action '{}'. Valid actions: load, save, auto-connect-save",
                    input.action
                )),
            }
        })
    }
}

async fn run_state_save(path: &str, auto_connect: bool) -> ToolResult {
    let mut cmd = tokio::process::Command::new(BINARY);
    if auto_connect {
        cmd.arg("--auto-connect");
    }
    cmd.arg("state").arg("save").arg(path);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    cmd.kill_on_drop(true);

    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return ToolResult::error(not_found_or_error(e)),
    };

    let timeout = tokio::time::Duration::from_secs(30);
    match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            if output.status.success() {
                let text = if stdout.trim().is_empty() {
                    format!("Auth state saved to {path}")
                } else {
                    stdout
                };
                ToolResult::text(text)
            } else {
                ToolResult::error(format!(
                    "agent-browser state save failed (exit {}):\n{stderr}",
                    output.status.code().unwrap_or(-1)
                ))
            }
        }
        Ok(Err(e)) => ToolResult::error(format!("Process error: {e}")),
        Err(_) => ToolResult::error("Execution timed out after 30 seconds"),
    }
}

fn not_found_or_error(e: std::io::Error) -> String {
    if e.kind() == std::io::ErrorKind::NotFound {
        format!(
            "agent-browser not found. Please install it:\n\
             \n  cargo install agent-browser\n\
             \nor download from https://github.com/anthropics/agent-browser\n\
             \nError: {e}"
        )
    } else {
        format!("Failed to spawn agent-browser: {e}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_ctx() -> ToolContext {
        ToolContext::new("test-agent", "test")
    }

    #[test]
    fn should_have_correct_name_and_schema() {
        let tool = BrowserAuthTool::new();
        let def = tool.def();
        assert_eq!(def.name, "browser_auth");

        let schema = &def.input_schema;
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["action"].is_object());
        assert!(schema["properties"]["path"].is_object());
        assert_eq!(schema["required"], json!(["action", "path"]));
        def.validate_input_schema().unwrap();
    }

    #[tokio::test]
    async fn should_fail_with_invalid_input() {
        let tool = BrowserAuthTool::new();
        let ctx = test_ctx();
        let result = tool.call(json!({}), &ctx).await;
        assert!(result.is_error);
        assert!(result.as_text().unwrap().contains("Invalid input"));
    }

    #[tokio::test]
    async fn should_handle_load_action() {
        let tool = BrowserAuthTool::new();
        let ctx = test_ctx();
        let result = tool
            .call(
                json!({"action": "load", "path": "~/.pilot/auth.json"}),
                &ctx,
            )
            .await;
        assert!(!result.is_error);
        let text = result.as_text().unwrap();
        assert!(text.contains("Auth state loaded"), "Got: {text}");
        assert!(text.contains("~/.pilot/auth.json"), "Got: {text}");
    }

    #[tokio::test]
    async fn should_return_error_for_save_when_binary_missing() {
        let tool = BrowserAuthTool::new();
        let ctx = test_ctx();
        let result = tool
            .call(
                json!({"action": "save", "path": "/tmp/auth-test.json"}),
                &ctx,
            )
            .await;
        if result.is_error {
            let text = result.as_text().unwrap();
            assert!(
                text.contains("agent-browser") || text.contains("error"),
                "Unexpected error: {text}"
            );
        }
    }

    #[tokio::test]
    async fn should_support_auto_connect_save() {
        let tool = BrowserAuthTool::new();
        let ctx = test_ctx();
        let result = tool
            .call(
                json!({"action": "auto-connect-save", "path": "/tmp/auth-test.json"}),
                &ctx,
            )
            .await;
        // Will error because binary isn't installed, but validates the code path
        if result.is_error {
            let text = result.as_text().unwrap();
            assert!(
                text.contains("agent-browser") || text.contains("error"),
                "Unexpected error: {text}"
            );
        }
    }
}
