use crate::tools::extract_string_arg;
use crate::tools::security::{validate_workspace_path, RateLimiter};
use crate::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;
use std::sync::{Arc, OnceLock};

const RATE_LIMIT_MAX: u64 = 60;
const RATE_LIMIT_WINDOW_SECS: u64 = 3600;

static GLOBAL_RATE_LIMITER: OnceLock<Arc<RateLimiter>> = OnceLock::new();

pub struct FileReadTool {
    workspace: std::path::PathBuf,
    rate_limiter: Arc<RateLimiter>,
}

impl FileReadTool {
    pub fn new(workspace: impl AsRef<std::path::Path>) -> Self {
        let rate_limiter = GLOBAL_RATE_LIMITER
            .get_or_init(|| Arc::new(RateLimiter::new(RATE_LIMIT_MAX, RATE_LIMIT_WINDOW_SECS)))
            .clone();
        Self {
            workspace: workspace.as_ref().to_path_buf(),
            rate_limiter,
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

        match std::fs::read_to_string(&full_path) {
            Ok(content) => Ok(ToolResult::success(content)),
            Err(e) => Ok(ToolResult::error(format!("Failed to read file: {}", e))),
        }
    }
}
