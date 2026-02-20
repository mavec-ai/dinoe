use crate::tools::{extract_string_arg, extract_string_arg_opt};
use crate::traits::{MemoryCategory, Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;

pub struct MemoryWriteTool {
    memory: std::sync::Arc<dyn crate::traits::Memory>,
}

impl MemoryWriteTool {
    pub fn new(memory: std::sync::Arc<dyn crate::traits::Memory>) -> Self {
        Self { memory }
    }
}

#[async_trait]
impl Tool for MemoryWriteTool {
    fn name(&self) -> &str {
        "memory_write"
    }

    fn description(&self) -> &str {
        "Store information in memory for future reference. Use this for important facts, user preferences, decisions, or context that should persist."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "key": {
                    "type": "string",
                    "description": "A unique key/identifier for this memory"
                },
                "content": {
                    "type": "string",
                    "description": "The content to store in memory"
                },
                "category": {
                    "type": "string",
                    "description": "Category: 'core' for long-term facts, 'daily' for logs (default: 'core')"
                }
            },
            "required": ["key", "content"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let key = extract_string_arg(&args, "key")?;
        let content = extract_string_arg(&args, "content")?;
        let category_str = extract_string_arg_opt(&args, "category", "core");

        if category_str.is_empty() {
            return Ok(ToolResult::error("Category cannot be empty"));
        }

        let category = match category_str.as_str() {
            "core" => MemoryCategory::Core,
            "daily" => MemoryCategory::Daily,
            _ => MemoryCategory::Custom(category_str),
        };

        match self.memory.store(&key, &content, category, None).await {
            Ok(()) => Ok(ToolResult::success(format!(
                "Stored memory with key: {}",
                key
            ))),
            Err(e) => Ok(ToolResult::error(format!("Failed to store memory: {}", e))),
        }
    }
}
