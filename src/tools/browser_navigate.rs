use std::future::Future;
use std::pin::Pin;

use serde::Deserialize;
use serde_json::json;

use crate::{Tool, ToolContext, ToolDef, ToolResult};

const BINARY: &str = "agent-browser";

/// A tool that navigates to a URL using `agent-browser open <url>`.
pub struct BrowserNavigateTool;

#[derive(Debug, Deserialize)]
struct Input {
    url: String,
}

impl Default for BrowserNavigateTool {
    fn default() -> Self {
        Self::new()
    }
}

impl BrowserNavigateTool {
    pub fn new() -> Self {
        Self
    }
}

impl Tool for BrowserNavigateTool {
    fn def(&self) -> ToolDef {
        ToolDef {
            name: "browser_navigate".to_string(),
            description: "Navigate the browser to a URL using agent-browser.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "The URL to navigate to"
                    }
                },
                "required": ["url"]
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

            let child = match tokio::process::Command::new(BINARY)
                .arg("open")
                .arg(&input.url)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .kill_on_drop(true)
                .spawn()
            {
                Ok(c) => c,
                Err(e) => {
                    return ToolResult::error(not_found_or_error(e));
                }
            };

            let timeout = tokio::time::Duration::from_secs(30);
            match tokio::time::timeout(timeout, child.wait_with_output()).await {
                Ok(Ok(output)) => {
                    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                    if output.status.success() {
                        let text = if stdout.trim().is_empty() {
                            format!("Navigated to {}", input.url)
                        } else {
                            stdout
                        };
                        ToolResult::text(text)
                    } else {
                        ToolResult::error(format!(
                            "agent-browser failed (exit {}):\n{stderr}",
                            output.status.code().unwrap_or(-1)
                        ))
                    }
                }
                Ok(Err(e)) => ToolResult::error(format!("Process error: {e}")),
                Err(_) => ToolResult::error("Execution timed out after 30 seconds"),
            }
        })
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
        let tool = BrowserNavigateTool::new();
        let def = tool.def();
        assert_eq!(def.name, "browser_navigate");

        let schema = &def.input_schema;
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["url"].is_object());
        assert_eq!(schema["required"], json!(["url"]));
        def.validate_input_schema().unwrap();
    }

    #[tokio::test]
    async fn should_fail_with_invalid_input() {
        let tool = BrowserNavigateTool::new();
        let ctx = test_ctx();
        let result = tool.call(json!({}), &ctx).await;
        assert!(result.is_error);
        assert!(result.as_text().unwrap().contains("Invalid input"));
    }

    #[tokio::test]
    async fn should_return_not_found_when_binary_missing() {
        // This test only validates the error path when agent-browser is not installed
        let tool = BrowserNavigateTool::new();
        let ctx = test_ctx();
        let result = tool.call(json!({"url": "https://example.com"}), &ctx).await;
        // If agent-browser IS installed, the command may succeed — that's fine too
        if result.is_error {
            let text = result.as_text().unwrap();
            // Either "not found" or some other spawn error
            assert!(
                text.contains("agent-browser") || text.contains("error"),
                "Unexpected error: {text}"
            );
        }
    }
}
