use crate::tools::{extract_string_arg, get_global_rate_limiter};
use crate::tools::security::validate_workspace_path;
use crate::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;
use tokio::fs;

pub struct FileReadTool {
    workspace: std::path::PathBuf,
    rate_limiter: std::sync::Arc<crate::tools::security::RateLimiter>,
}

impl FileReadTool {
    pub fn new(workspace: impl AsRef<std::path::Path>) -> Self {
        Self {
            workspace: workspace.as_ref().to_path_buf(),
            rate_limiter: get_global_rate_limiter(),
        }
    }
}

#[async_trait]
impl Tool for FileReadTool {
    fn name(&self) -> &str {
        "file_read"
    }

    fn description(&self) -> &str {
        "Read the contents of a file from the workspace"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to read"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        if !self.rate_limiter.check_and_record() {
            return Ok(ToolResult::error(
                "Rate limit exceeded: too many file reads. Please wait a moment.",
            ));
        }

        let path = extract_string_arg(&args, "path")?;

        let full_path = match validate_workspace_path(&path, &self.workspace) {
            Ok(p) => p,
            Err(e) => return Ok(ToolResult::error(e)),
        };

        match fs::read_to_string(&full_path).await {
            Ok(content) => Ok(ToolResult::success(content)),
            Err(e) => Ok(ToolResult::error(format!("Failed to read file: {}", e))),
        }
    }
}
