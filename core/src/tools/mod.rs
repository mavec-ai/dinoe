use serde_json::Value;
use std::sync::{Arc, OnceLock};

pub mod content_search;
pub mod file_edit;
pub mod file_read;
pub mod file_write;
pub mod git_operations;
pub mod glob_search;
pub mod http_request;
pub mod memory_read;
pub mod memory_write;
pub mod security;
pub mod shell;
pub mod web_fetch;

use security::RateLimiter;

static GLOBAL_RATE_LIMITER: OnceLock<Arc<RateLimiter>> = OnceLock::new();

pub fn get_global_rate_limiter() -> Arc<RateLimiter> {
    GLOBAL_RATE_LIMITER
        .get_or_init(|| Arc::new(RateLimiter::default()))
        .clone()
}

pub use content_search::ContentSearchTool;
pub use file_edit::FileEditTool;
pub use file_read::FileReadTool;
pub use file_write::FileWriteTool;
pub use git_operations::GitOperationsTool;
pub use glob_search::GlobSearchTool;
pub use http_request::HttpRequestTool;
pub use memory_read::MemoryReadTool;
pub use memory_write::MemoryWriteTool;
pub use shell::ShellTool;
pub use web_fetch::WebFetchTool;

pub fn extract_string_arg(args: &Value, key: &str) -> anyhow::Result<String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing '{}' parameter", key))
        .map(|s| s.to_string())
}

pub fn extract_string_arg_opt(args: &Value, key: &str, default: &str) -> String {
    args.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or(default)
        .to_string()
}

pub fn extract_usize_arg_opt(args: &Value, key: &str, default: usize) -> usize {
    args.get(key)
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(default)
}
