use std::sync::Arc;

use crate::agent::ToolRegistry;
use crate::traits::{ToolCall, ToolResult};

pub struct ToolExecutor {
    tool_registry: Arc<ToolRegistry>,
}

impl ToolExecutor {
    pub fn new(tool_registry: Arc<ToolRegistry>) -> Self {
        Self { tool_registry }
    }

    pub async fn execute(&self, tool_call: &ToolCall) -> ToolResult {
        let args: serde_json::Value = match serde_json::from_str(&tool_call.arguments) {
            Ok(a) => a,
            Err(e) => {
                return ToolResult::error(format!(
                    "Failed to parse tool arguments for {}: {}",
                    tool_call.name, e
                ));
            }
        };

        self.tool_registry.execute(&tool_call.name, args).await
    }

    pub async fn execute_batch(&self, tool_calls: &[ToolCall]) -> Vec<ToolResult> {
        if tool_calls.len() <= 1 {
            let mut results = Vec::with_capacity(tool_calls.len());
            for tool_call in tool_calls {
                results.push(self.execute(tool_call).await);
            }
            return results;
        }

        let futures: Vec<_> = tool_calls
            .iter()
            .map(|tool_call| {
                let registry = self.tool_registry.clone();
                let tool_call = tool_call.clone();
                async move {
                    let args: serde_json::Value =
                        match serde_json::from_str(&tool_call.arguments) {
                            Ok(a) => a,
                            Err(e) => {
                                return ToolResult::error(format!(
                                    "Failed to parse tool arguments for {}: {}",
                                    tool_call.name, e
                                ));
                            }
                        };
                    registry.execute(&tool_call.name, args).await
                }
            })
            .collect();

        futures_util::future::join_all(futures).await
    }
}
