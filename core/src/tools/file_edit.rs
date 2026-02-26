use crate::tools::extract_string_arg;
use crate::tools::security::RateLimiter;
use crate::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;
use std::path::Path;
use std::sync::{Arc, OnceLock};

const RATE_LIMIT_MAX: u64 = 60;
const RATE_LIMIT_WINDOW_SECS: u64 = 3600;

static GLOBAL_RATE_LIMITER: OnceLock<Arc<RateLimiter>> = OnceLock::new();

pub struct FileEditTool {
    workspace: std::path::PathBuf,
    rate_limiter: Arc<RateLimiter>,
}

impl FileEditTool {
    pub fn new(workspace: impl AsRef<Path>) -> Self {
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
impl Tool for FileEditTool {
    fn name(&self) -> &str {
        "file_edit"
    }

    fn description(&self) -> &str {
        "Edit a file by replacing an exact string match with new content"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Relative path to the file within the workspace"
                },
                "old_string": {
                    "type": "string",
                    "description": "The exact text to find and replace (must appear exactly once in the file)"
                },
                "new_string": {
                    "type": "string",
                    "description": "The replacement text (empty string to delete the matched text)"
                }
            },
            "required": ["path", "old_string", "new_string"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        if !self.rate_limiter.check_and_record() {
            return Ok(ToolResult::error(
                "Rate limit exceeded: too many edits. Please wait a moment.",
            ));
        }

        let path = extract_string_arg(&args, "path")?;
        let old_string = extract_string_arg(&args, "old_string")?;
        let new_string = args
            .get("new_string")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if old_string.is_empty() {
            return Ok(ToolResult::error("old_string must not be empty"));
        }

        if std::path::Path::new(&path).is_absolute() {
            return Ok(ToolResult::error(
                "Absolute paths are not allowed. Use a relative path.",
            ));
        }

        if path.contains("../") || path.contains("..\\") || path == ".." {
            return Ok(ToolResult::error(
                "Path traversal ('..') is not allowed in file path.",
            ));
        }

        let full_path = self.workspace.join(&path);

        let Some(parent) = full_path.parent() else {
            return Ok(ToolResult::error("Invalid path: missing parent directory"));
        };

        let resolved_parent = match tokio::fs::canonicalize(parent).await {
            Ok(p) => p,
            Err(e) => {
                return Ok(ToolResult::error(format!(
                    "Failed to resolve file path: {e}"
                )));
            }
        };

        let workspace_canon = match std::fs::canonicalize(&self.workspace) {
            Ok(p) => p,
            Err(e) => {
                return Ok(ToolResult::error(format!(
                    "Cannot resolve workspace directory: {e}"
                )));
            }
        };

        if !resolved_parent.starts_with(&workspace_canon) {
            return Ok(ToolResult::error(
                "Resolved path is outside the allowed workspace.",
            ));
        }

        let Some(file_name) = full_path.file_name() else {
            return Ok(ToolResult::error("Invalid path: missing file name"));
        };

        let resolved_target = resolved_parent.join(file_name);

        if let Ok(meta) = tokio::fs::symlink_metadata(&resolved_target).await
            && meta.file_type().is_symlink()
        {
            return Ok(ToolResult::error(format!(
                "Refusing to edit through symlink: {}",
                resolved_target.display()
            )));
        }

        let content = match tokio::fs::read_to_string(&resolved_target).await {
            Ok(c) => c,
            Err(e) => {
                return Ok(ToolResult::error(format!("Failed to read file: {e}")));
            }
        };

        let match_count = content.matches(&old_string).count();

        if match_count == 0 {
            return Ok(ToolResult::error("old_string not found in file"));
        }

        if match_count > 1 {
            return Ok(ToolResult::error(format!(
                "old_string matches {match_count} times; must match exactly once"
            )));
        }

        let new_content = content.replacen(&old_string, new_string, 1);

        match tokio::fs::write(&resolved_target, &new_content).await {
            Ok(()) => Ok(ToolResult::success(format!(
                "Edited {path}: replaced 1 occurrence ({} bytes)",
                new_content.len()
            ))),
            Err(e) => Ok(ToolResult::error(format!("Failed to write file: {e}"))),
        }
    }
}
