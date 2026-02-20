use crate::traits::{Tool, ToolResult, ToolSpec};
use std::sync::{Arc, Mutex};

pub struct ToolRegistry {
    tools: Mutex<Vec<Arc<dyn Tool>>>,
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: Mutex::new(Vec::new()),
        }
    }

    pub fn register(&self, tool: Box<dyn Tool>) {
        let mut tools = self.tools.lock().unwrap();
        tools.push(Arc::from(tool));
    }

    pub fn get_specs(&self) -> Vec<ToolSpec> {
        let tools = self.tools.lock().unwrap();
        tools.iter().map(|t| t.spec()).collect()
    }

    pub async fn execute(&self, name: &str, args: serde_json::Value) -> ToolResult {
        let tool = {
            let tools = self.tools.lock().unwrap();
            tools.iter().find(|t| t.name() == name).cloned()
        };

        match tool {
            Some(tool) => {
                let result = tool.execute(args).await;
                match result {
                    Ok(result) => result,
                    Err(e) => ToolResult::error(format!("Execution failed: {}", e)),
                }
            }
            None => ToolResult::error(format!("Tool '{}' not found", name)),
        }
    }
}
