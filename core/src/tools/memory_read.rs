use crate::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;

pub struct MemoryReadTool {
    memory: std::sync::Arc<dyn crate::traits::Memory>,
}

impl MemoryReadTool {
    pub fn new(memory: std::sync::Arc<dyn crate::traits::Memory>) -> Self {
        Self { memory }
    }
}

#[async_trait]
impl Tool for MemoryReadTool {
    fn name(&self) -> &str {
        "memory_read"
    }

    fn description(&self) -> &str {
        "Retrieve memories from the memory store using a search query"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Keywords or phrase to search for in memory"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of results to return (default: 10)"
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");

        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;

        match self.memory.recall(query, limit, None).await {
            Ok(entries) => {
                if entries.is_empty() {
                    Ok(ToolResult::success(
                        "No memories found matching the query.".to_string(),
                    ))
                } else {
                    let formatted: Vec<String> = entries
                        .iter()
                        .map(|e| {
                            let score = e
                                .score
                                .map(|s| format!(" (score: {:.2})", s))
                                .unwrap_or_default();
                            format!("- {}{}", e.content, score)
                        })
                        .collect();
                    Ok(ToolResult::success(format!(
                        "Found {} memories:\n{}",
                        entries.len(),
                        formatted.join("\n")
                    )))
                }
            }
            Err(e) => Ok(ToolResult::error(format!("Failed to read memory: {}", e))),
        }
    }
}
