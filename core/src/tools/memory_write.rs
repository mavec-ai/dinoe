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
        let key = args
            .get("key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'key' parameter"))?;

        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'content' parameter"))?;

        let category_str = args
            .get("category")
            .and_then(|v| v.as_str())
            .unwrap_or("core");

        let category = match category_str {
            "core" => MemoryCategory::Core,
            "daily" => MemoryCategory::Daily,
            _ => MemoryCategory::Custom(category_str.to_string()),
        };

        match self.memory.store(key, content, category, None).await {
            Ok(()) => Ok(ToolResult::success(format!(
                "Stored memory with key: {}",
                key
            ))),
            Err(e) => Ok(ToolResult::error(format!("Failed to store memory: {}", e))),
        }
    }
}
