use crate::tools::{extract_string_arg, get_global_rate_limiter};
use crate::tools::security::validate_workspace_path;
use crate::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;
use tokio::fs;

pub struct FileWriteTool {
    workspace: std::path::PathBuf,
    rate_limiter: std::sync::Arc<crate::tools::security::RateLimiter>,
}

impl FileWriteTool {
    pub fn new(workspace: impl AsRef<std::path::Path>) -> Self {
        Self {
            workspace: workspace.as_ref().to_path_buf(),
            rate_limiter: get_global_rate_limiter(),
        }
    }
}

#[async_trait]
impl Tool for FileWriteTool {
    fn name(&self) -> &str {
        "file_write"
    }

    fn description(&self) -> &str {
        "Write content to a file in the workspace"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
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

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        if !self.rate_limiter.check_and_record() {
            return Ok(ToolResult::error(
                "Rate limit exceeded: too many file writes. Please wait a moment.",
            ));
        }

        let path = extract_string_arg(&args, "path")?;
        let content = extract_string_arg(&args, "content")?;

        let full_path = match validate_workspace_path(&path, &self.workspace) {
            Ok(p) => p,
            Err(e) => return Ok(ToolResult::error(e)),
        };

        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).await?;
        }

        match fs::write(&full_path, content).await {
            Ok(_) => Ok(ToolResult::success("File written successfully")),
            Err(e) => Ok(ToolResult::error(format!("Failed to write file: {}", e))),
        }
    }
}
