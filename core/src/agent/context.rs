use crate::traits::{ChatMessage, Memory};
use std::path::Path;
use std::sync::Arc;

pub struct ContextBuilder {
    pub workspace: std::path::PathBuf,
    pub memory: Option<Arc<dyn Memory>>,
}

impl ContextBuilder {
    pub fn new(workspace: impl AsRef<Path>) -> Self {
        Self {
            workspace: workspace.as_ref().to_path_buf(),
            memory: None,
        }
    }

    pub fn with_memory(mut self, memory: Arc<dyn Memory>) -> Self {
        self.memory = Some(memory);
        self
    }

    pub async fn build_system_prompt(&self, user_message: &str) -> String {
        let mut parts = vec![];

        if let Some(bootstrap) = self.load_bootstrap_files() {
            parts.push(bootstrap);
        }

        parts.push(self.get_runtime_context());

        if let Some(memory_context) = self.get_memory_context(user_message).await {
            parts.push(memory_context);
        }

        parts.join("\n\n---\n\n")
    }

    fn get_runtime_context(&self) -> String {
        let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M (%A)");

        format!(
            "## Runtime Context

### Current Time
{}

### Workspace
{}",
            timestamp,
            self.workspace.display()
        )
    }

    async fn get_memory_context(&self, user_message: &str) -> Option<String> {
        if let Some(ref memory) = self.memory {
            let mut context_parts = vec![];

            if let Ok(entries) = memory.recall(user_message, 5, None).await
                && !entries.is_empty()
            {
                context_parts.push("## Relevant Memory".to_string());
                for entry in entries {
                    if !entry.content.is_empty() {
                        context_parts.push(format!("- {}", entry.content));
                    }
                }
            }

            if !context_parts.is_empty() {
                return Some(context_parts.join("\n\n"));
            }
        }
        None
    }

    fn load_bootstrap_files(&self) -> Option<String> {
        std::fs::read_to_string(self.workspace.join("SOUL.md")).ok()
    }

    pub async fn build_messages(
        &self,
        history: Vec<ChatMessage>,
        current_message: &str,
    ) -> Vec<ChatMessage> {
        let mut messages = vec![ChatMessage::system(
            self.build_system_prompt(current_message).await,
        )];
        messages.extend(history);
        messages.push(ChatMessage::user(current_message));
        messages
    }
}
