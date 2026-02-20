use crate::tools::extract_string_arg;
use crate::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;

pub struct FileReadTool {
    workspace: std::path::PathBuf,
}

impl FileReadTool {
    pub fn new(workspace: impl AsRef<std::path::Path>) -> Self {
        Self {
            workspace: workspace.as_ref().to_path_buf(),
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
        let path = extract_string_arg(&args, "path")?;
        let full_path = self.workspace.join(&path);

        match std::fs::read_to_string(&full_path) {
            Ok(content) => Ok(ToolResult::success(content)),
            Err(e) => Ok(ToolResult::error(format!("Failed to read file: {}", e))),
        }
    }
}
