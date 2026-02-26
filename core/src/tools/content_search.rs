use crate::tools::extract_string_arg;
use crate::tools::security::RateLimiter;
use crate::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, OnceLock};

const MAX_RESULTS: usize = 1000;
const MAX_OUTPUT_BYTES: usize = 1_048_576;
const TIMEOUT_SECS: u64 = 30;
const RATE_LIMIT_MAX: u64 = 100;
const RATE_LIMIT_WINDOW_SECS: u64 = 3600;

static GLOBAL_RATE_LIMITER: OnceLock<Arc<RateLimiter>> = OnceLock::new();

pub struct ContentSearchTool {
    workspace: std::path::PathBuf,
    rate_limiter: Arc<RateLimiter>,
}

impl ContentSearchTool {
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
impl Tool for ContentSearchTool {
    fn name(&self) -> &str {
        "content_search"
    }

    fn description(&self) -> &str {
        "Search file contents by regex pattern within the workspace using grep. \
         Output modes: 'content' (matching lines), 'files_with_matches' (paths only), 'count' (match counts)."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Regular expression pattern to search for"
                },
                "path": {
                    "type": "string",
                    "description": "Directory to search in, relative to workspace root. Defaults to '.'",
                    "default": "."
                },
                "output_mode": {
                    "type": "string",
                    "description": "Output format: 'content', 'files_with_matches', 'count'",
                    "enum": ["content", "files_with_matches", "count"],
                    "default": "content"
                },
                "include": {
                    "type": "string",
                    "description": "File glob filter, e.g. '*.rs', '*.txt'"
                },
                "case_sensitive": {
                    "type": "boolean",
                    "description": "Case-sensitive matching. Defaults to true",
                    "default": true
                },
                "context_before": {
                    "type": "integer",
                    "description": "Lines of context before each match (content mode only)",
                    "default": 0
                },
                "context_after": {
                    "type": "integer",
                    "description": "Lines of context after each match (content mode only)",
                    "default": 0
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
        if pattern.is_empty() {
            return Ok(ToolResult::error("Empty pattern is not allowed."));
        }

        let search_path = args
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or(".")
            .to_string();

        let output_mode = args
            .get("output_mode")
            .and_then(|v| v.as_str())
            .unwrap_or("content")
            .to_string();

        if !matches!(output_mode.as_str(), "content" | "files_with_matches" | "count") {
            return Ok(ToolResult::error(format!(
                "Invalid output_mode '{output_mode}'. Allowed: content, files_with_matches, count."
            )));
        }

        let include = args.get("include").and_then(|v| v.as_str()).map(|s| s.to_string());
        let case_sensitive = args
            .get("case_sensitive")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        #[allow(clippy::cast_possible_truncation)]
        let context_before = args
            .get("context_before")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;

        #[allow(clippy::cast_possible_truncation)]
        let context_after = args
            .get("context_after")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;

        if std::path::Path::new(&search_path).is_absolute() {
            return Ok(ToolResult::error(
                "Absolute paths are not allowed. Use a relative path.",
            ));
        }

        if search_path.contains("../") || search_path.contains("..\\") || search_path == ".." {
            return Ok(ToolResult::error(
                "Path traversal ('..') is not allowed in search path.",
            ));
        }

        let resolved_path = self.workspace.join(&search_path);

        let resolved_canon = match std::fs::canonicalize(&resolved_path) {
            Ok(p) => p,
            Err(e) => {
                return Ok(ToolResult::error(format!(
                    "Cannot resolve path '{search_path}': {e}"
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

        if !resolved_canon.starts_with(&workspace_canon) {
            return Ok(ToolResult::error(
                "Resolved path is outside the allowed workspace.",
            ));
        }

        let output = tokio::task::spawn_blocking(move || {
            run_grep_search(
                &pattern,
                &resolved_canon,
                &workspace_canon,
                &output_mode,
                include.as_deref(),
                case_sensitive,
                context_before,
                context_after,
            )
        });

        let result = match tokio::time::timeout(
            std::time::Duration::from_secs(TIMEOUT_SECS),
            output,
        )
        .await
        {
            Ok(Ok(output)) => output,
            Ok(Err(e)) => {
                return Ok(ToolResult::error(format!("Search failed: {e}")));
            }
            Err(_) => {
                return Ok(ToolResult::error(format!(
                    "Search timed out after {TIMEOUT_SECS} seconds."
                )));
            }
        };

        if result.len() > MAX_OUTPUT_BYTES {
            let truncated = truncate_utf8(&result, MAX_OUTPUT_BYTES);
            return Ok(ToolResult::success(format!(
                "{truncated}\n\n[Output truncated: exceeded 1 MB limit]"
            )));
        }

        Ok(ToolResult::success(result))
    }
}

#[allow(clippy::too_many_arguments)]
fn run_grep_search(
    pattern: &str,
    search_path: &std::path::Path,
    workspace_canon: &std::path::Path,
    output_mode: &str,
    include: Option<&str>,
    case_sensitive: bool,
    context_before: usize,
    context_after: usize,
) -> String {
    let mut cmd = Command::new("grep");

    cmd.arg("-r")
        .arg("-n")
        .arg("-E")
        .arg("--binary-files=without-match");

    match output_mode {
        "files_with_matches" => {
            cmd.arg("-l");
        }
        "count" => {
            cmd.arg("-c");
        }
        _ => {
            if context_before > 0 {
                cmd.arg("-B").arg(context_before.to_string());
            }
            if context_after > 0 {
                cmd.arg("-A").arg(context_after.to_string());
            }
        }
    }

    if !case_sensitive {
        cmd.arg("-i");
    }

    if let Some(glob) = include {
        cmd.arg("--include").arg(glob);
    }

    cmd.arg("--").arg(pattern).arg(search_path);

    let output = match cmd.output() {
        Ok(o) => o,
        Err(e) => return format!("Failed to execute grep: {e}"),
    };

    let exit_code = output.status.code().unwrap_or(-1);
    if exit_code >= 2 {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return format!("Search error: {}", stderr.trim());
    }

    let raw = String::from_utf8_lossy(&output.stdout);
    format_grep_output(&raw, workspace_canon, output_mode)
}

fn format_grep_output(raw: &str, workspace_canon: &std::path::Path, output_mode: &str) -> String {
    if raw.trim().is_empty() {
        return "No matches found.".to_string();
    }

    let workspace_prefix = workspace_canon.to_string_lossy();
    let mut lines: Vec<String> = Vec::new();
    let mut truncated = false;
    let mut file_set = std::collections::HashSet::new();
    let mut total_matches: usize = 0;

    for line in raw.lines() {
        if line.is_empty() {
            continue;
        }

        let relativized = relativize_path(line, &workspace_prefix);

        match output_mode {
            "files_with_matches" => {
                let path = relativized.trim();
                if !path.is_empty() && file_set.insert(path.to_string()) {
                    lines.push(path.to_string());
                    if lines.len() >= MAX_RESULTS {
                        truncated = true;
                        break;
                    }
                }
            }
            "count" => {
                if let Some((path, count)) = parse_count_line(&relativized)
                    && count > 0
                {
                    file_set.insert(path.to_string());
                    total_matches += count;
                    lines.push(format!("{path}:{count}"));
                    if lines.len() >= MAX_RESULTS {
                        truncated = true;
                        break;
                    }
                }
            }
            _ => {
                if let Some((path, _)) = parse_content_line(&relativized) {
                    file_set.insert(path.to_string());
                    total_matches += 1;
                }
                lines.push(relativized);
                if lines.len() >= MAX_RESULTS {
                    truncated = true;
                    break;
                }
            }
        }
    }

    if lines.is_empty() {
        return "No matches found.".to_string();
    }

    let mut buf = lines.join("\n");

    if truncated {
        buf.push_str(&format!("\n\n[Results truncated: showing first {MAX_RESULTS} results]"));
    }

    match output_mode {
        "files_with_matches" => {
            buf.push_str(&format!("\n\nTotal: {} files", file_set.len()));
        }
        "count" => {
            buf.push_str(&format!(
                "\n\nTotal: {} matches in {} files",
                total_matches,
                file_set.len()
            ));
        }
        _ => {
            buf.push_str(&format!(
                "\n\nTotal: {} matching lines in {} files",
                total_matches,
                file_set.len()
            ));
        }
    }

    buf
}

fn relativize_path(line: &str, workspace_prefix: &str) -> String {
    if let Some(rest) = line.strip_prefix(workspace_prefix) {
        let trimmed = rest
            .strip_prefix('/')
            .or_else(|| rest.strip_prefix('\\'))
            .unwrap_or(rest);
        return trimmed.to_string();
    }
    line.to_string()
}

fn parse_count_line(line: &str) -> Option<(String, usize)> {
    let parts: Vec<&str> = line.rsplitn(2, ':').collect();
    if parts.len() == 2
        && let Ok(count) = parts[0].parse::<usize>()
    {
        return Some((parts[1].to_string(), count));
    }
    None
}

fn parse_content_line(line: &str) -> Option<(String, bool)> {
    if line.starts_with('-') || line == "--" {
        return None;
    }

    let parts: Vec<&str> = line.splitn(2, ':').collect();
    if parts.len() == 2 {
        Some((parts[0].to_string(), true))
    } else {
        None
    }
}

fn truncate_utf8(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }

    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}
