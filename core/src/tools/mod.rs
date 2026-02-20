use serde_json::Value;

pub mod file_read;
pub mod file_write;
pub mod memory_read;
pub mod memory_write;
pub mod shell;

pub use file_read::FileReadTool;
pub use file_write::FileWriteTool;
pub use memory_read::MemoryReadTool;
pub use memory_write::MemoryWriteTool;
pub use shell::ShellTool;

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
