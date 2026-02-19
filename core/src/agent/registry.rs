use crate::traits::{Tool, ToolResult, ToolSpec};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub struct ToolRegistry {
    tools: Mutex<HashMap<String, Arc<dyn Tool>>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: Mutex::new(HashMap::new()),
        }
    }

    pub fn register(&self, tool: Arc<dyn Tool>) {
        let name = tool.name().to_string();
        let mut tools = self.tools.lock().unwrap();
        tools.insert(name, tool);
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        let tools = self.tools.lock().unwrap();
        tools.get(name).cloned()
    }

    pub fn get_specs(&self) -> Vec<ToolSpec> {
        let tools = self.tools.lock().unwrap();
        tools.values().map(|t| t.spec()).collect()
    }

    pub async fn execute(&self, name: &str, args: serde_json::Value) -> ToolResult {
        match self.get(name) {
            Some(tool) => match tool.execute(args).await {
                Ok(result) => result,
                Err(e) => ToolResult::error(format!("Execution failed: {}", e)),
            },
            None => ToolResult::error(format!("Tool '{}' not found", name)),
        }
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
