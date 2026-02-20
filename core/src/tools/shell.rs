use crate::tools::extract_string_arg;
use crate::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;
use std::process::Command;

pub struct ShellTool {
    workspace: std::path::PathBuf,
}

impl ShellTool {
    pub fn new(workspace: impl AsRef<std::path::Path>) -> Self {
        Self {
            workspace: workspace.as_ref().to_path_buf(),
        }
    }
}

#[async_trait]
impl Tool for ShellTool {
    fn name(&self) -> &str {
        "shell"
    }

    fn description(&self) -> &str {
        "Execute a shell command in the workspace directory"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
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

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let command = extract_string_arg(&args, "command")?;

        let output = Command::new("sh")
            .arg("-c")
            .arg(&command)
            .current_dir(&self.workspace)
            .output();

        match output {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();

                if output.status.success() {
                    let result = if stdout.is_empty() { stderr } else { stdout };
                    Ok(ToolResult::success(result))
                } else {
                    let error = if stderr.is_empty() {
                        format!("Command failed with status: {}", output.status)
                    } else {
                        stderr
                    };
                    Ok(ToolResult::error(error))
                }
            }
            Err(e) => Ok(ToolResult::error(format!(
                "Failed to execute command: {}",
                e
            ))),
        }
    }
}
