use serde::{Deserialize, Serialize};

const STATUS_MAX: usize = 200;
const TOOL_RESULT_MAX: usize = 200;

fn truncate_preview(input: &str, max: usize) -> String {
    let input = input.trim();
    if input.chars().count() <= max {
        input.to_string()
    } else {
        let truncated: String = input.chars().take(max - 3).collect();
        format!("{}...", truncated)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StatusUpdate {
    Thinking(String),
    ToolStarted { name: String },
    ToolCompleted { name: String, success: bool },
    ToolResult { name: String, preview: String },
    Status(String),
}

impl StatusUpdate {
    pub fn thinking(msg: impl Into<String>) -> Self {
        StatusUpdate::Thinking(msg.into())
    }

    pub fn tool_started(name: impl Into<String>) -> Self {
        StatusUpdate::ToolStarted { name: name.into() }
    }

    pub fn tool_completed(name: impl Into<String>, success: bool) -> Self {
        StatusUpdate::ToolCompleted {
            name: name.into(),
            success,
        }
    }

    pub fn tool_result(name: impl Into<String>, result: &str) -> Self {
        let preview = if result.len() > 10 {
            truncate_preview(result, TOOL_RESULT_MAX)
        } else {
            result.to_string()
        };
        StatusUpdate::ToolResult {
            name: name.into(),
            preview,
        }
    }

    pub fn status(msg: impl Into<String>) -> Self {
        StatusUpdate::Status(msg.into())
    }
}

pub struct StatusPrinter;

impl StatusPrinter {
    pub fn new() -> Self {
        Self
    }

    pub fn print(&self, status: &StatusUpdate) {
        match status {
            StatusUpdate::Thinking(msg) => {
                let display = truncate_preview(msg, 60);
                if display.is_empty() || display == "." {
                    eprintln!("  \x1b[90m\u{25CB} Thinking...\x1b[0m");
                } else {
                    eprintln!("  \x1b[90m\u{25CB} {}\x1b[0m", display);
                }
            }
            StatusUpdate::ToolStarted { name } => {
                eprintln!("  \x1b[33m\u{25CB} {}\x1b[0m", name);
            }
            StatusUpdate::ToolCompleted { name, success } => {
                if *success {
                    eprintln!("  \x1b[32m\u{25CF} {}\x1b[0m", name);
                } else {
                    eprintln!("  \x1b[31m\u{2717} {} (failed)\x1b[0m", name);
                }
            }
            StatusUpdate::ToolResult { name: _, preview } => {
                let display = truncate_preview(preview, TOOL_RESULT_MAX);
                eprintln!("    \x1b[90m{}\x1b[0m", display);
            }
            StatusUpdate::Status(msg) => {
                let display = truncate_preview(msg, STATUS_MAX);
                eprintln!("  \x1b[90m{}\x1b[0m", display);
            }
        }
    }
}

impl Default for StatusPrinter {
    fn default() -> Self {
        Self::new()
    }
}
