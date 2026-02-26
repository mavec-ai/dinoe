use crate::tools::{extract_string_arg, security::RateLimiter};
use crate::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;
use std::path::Path;
use std::sync::{Arc, OnceLock};
use walkdir::WalkDir;

const MAX_RESULTS: usize = 1000;
const RATE_LIMIT_MAX: u64 = 100;
const RATE_LIMIT_WINDOW_SECS: u64 = 3600;

static GLOBAL_RATE_LIMITER: OnceLock<Arc<RateLimiter>> = OnceLock::new();

pub struct GlobSearchTool {
    workspace_dir: std::path::PathBuf,
    rate_limiter: Arc<RateLimiter>,
}

impl GlobSearchTool {
    pub fn new(workspace_dir: impl AsRef<Path>) -> Self {
        let rate_limiter = GLOBAL_RATE_LIMITER
            .get_or_init(|| Arc::new(RateLimiter::new(RATE_LIMIT_MAX, RATE_LIMIT_WINDOW_SECS)))
            .clone();
        Self {
            workspace_dir: workspace_dir.as_ref().to_path_buf(),
            rate_limiter,
        }
    }

    fn match_pattern(path: &str, pattern: &str) -> bool {
        let pattern_lower = pattern.to_lowercase();
        let path_lower = path.to_lowercase();

        if pattern.contains("**") {
            let parts: Vec<&str> = pattern.split("**").collect();
            if parts.len() == 2 {
                let prefix = parts[0].trim_end_matches('/');
                let suffix = parts[1].trim_start_matches('/');

                if !prefix.is_empty() && !path_lower.starts_with(&prefix.to_lowercase()) {
                    return false;
                }
                if !suffix.is_empty() {
                    let suffix_lower = suffix.to_lowercase();
                    let suffix_clean = suffix_lower.trim_start_matches('*');
                    if !path_lower.ends_with(suffix_clean) && !path_lower.contains(suffix_clean) {
                        return false;
                    }
                }
                return true;
            }
        }

        if pattern.contains('*') {
            let parts: Vec<&str> = pattern.split('*').collect();
            if parts.len() == 2 {
                let prefix = parts[0].to_lowercase();
                let suffix = parts[1].to_lowercase();
                return path_lower.starts_with(&prefix) && path_lower.ends_with(&suffix);
            }
        }

        path_lower == pattern_lower
    }
}

#[async_trait]
impl Tool for GlobSearchTool {
    fn name(&self) -> &str {
        "glob_search"
    }

    fn description(&self) -> &str {
        "Search for files matching a glob pattern within the workspace"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern to match files, e.g. '**/*.rs', 'src/**/mod.rs', '*.txt'"
                }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        if !self.rate_limiter.check_and_record() {
            return Ok(ToolResult::error(
                "Rate limit exceeded: too many searches. Please wait a moment.",
            ));
        }

        let pattern = extract_string_arg(&args, "pattern")?;

        if pattern.starts_with('/') || pattern.starts_with('\\') {
            return Ok(ToolResult::error(
                "Absolute paths are not allowed. Use a relative glob pattern.",
            ));
        }

        if pattern.contains("../") || pattern.contains("..\\") || pattern == ".." {
            return Ok(ToolResult::error(
                "Path traversal ('..') is not allowed in glob patterns.",
            ));
        }

        let workspace_canon = match std::fs::canonicalize(&self.workspace_dir) {
            Ok(p) => p,
            Err(e) => {
                return Ok(ToolResult::error(format!(
                    "Cannot resolve workspace directory: {e}"
                )));
            }
        };

        let mut results = Vec::new();
        let mut truncated = false;

        for entry in WalkDir::new(&workspace_canon)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();

            if !path.is_file() {
                continue;
            }

            let resolved = match std::fs::canonicalize(path) {
                Ok(p) => p,
                Err(_) => continue,
            };

            if !resolved.starts_with(&workspace_canon) {
                continue;
            }

            if let Ok(rel) = resolved.strip_prefix(&workspace_canon) {
                let rel_str = rel.to_string_lossy();
                if Self::match_pattern(&rel_str, &pattern) {
                    results.push(rel_str.to_string());
                    if results.len() >= MAX_RESULTS {
                        truncated = true;
                        break;
                    }
                }
            }
        }

        results.sort();

        let output = if results.is_empty() {
            format!("No files matching pattern '{pattern}' found in workspace.")
        } else {
            let mut buf = results.join("\n");
            if truncated {
                buf.push_str(&format!(
                    "\n\n[Results truncated: showing first {MAX_RESULTS} of more matches]"
                ));
            }
            buf.push_str(&format!("\n\nTotal: {} files", results.len()));
            buf
        };

        Ok(ToolResult::success(output))
    }
}
