use crate::skills::Skill;
use crate::traits::{ChatMessage, Memory, ToolSpec};
use std::fmt::Write;
use std::path::Path;
use std::sync::Arc;

const BOOTSTRAP_MAX_CHARS: usize = 20_000;
const MEMORY_MIN_RELEVANCE_SCORE: f64 = 0.4;

const BOOTSTRAP_FILES: &[(&str, &str)] = &[
    ("SOUL.md", "## Agent Identity (SOUL.md)"),
    ("TOOLS.md", "## Local Tool Notes (TOOLS.md)"),
    ("USER.md", "## User Context (USER.md)"),
];

pub struct ContextBuilder {
    pub workspace: std::path::PathBuf,
    pub memory: Option<Arc<dyn Memory>>,
    pub skills: Vec<Skill>,
    pub tool_specs: Vec<ToolSpec>,
}

impl ContextBuilder {
    pub fn new(workspace: impl AsRef<Path>) -> Self {
        Self {
            workspace: workspace.as_ref().to_path_buf(),
            memory: None,
            skills: vec![],
            tool_specs: vec![],
        }
    }

    pub fn with_memory(mut self, memory: Arc<dyn Memory>) -> Self {
        self.memory = Some(memory);
        self
    }

    pub fn with_skills(mut self, skills: Vec<Skill>) -> Self {
        self.skills = skills;
        self
    }

    pub fn with_tool_specs(mut self, tool_specs: Vec<ToolSpec>) -> Self {
        self.tool_specs = tool_specs;
        self
    }

    pub async fn build_system_prompt(&self, user_message: &str) -> String {
        let mut parts = vec![];

        if let Some(bootstrap) = self.load_bootstrap_files() {
            parts.push(bootstrap);
        }

        parts.push(self.get_tool_instructions());
        parts.push(self.get_runtime_context());

        if let Some(skills_context) = self.get_skills_context() {
            parts.push(skills_context);
        }

        if let Some(memory_context) = self.get_memory_context(user_message).await {
            parts.push(memory_context);
        }

        parts.join("\n\n---\n\n")
    }

    fn get_tool_instructions(&self) -> String {
        if self.tool_specs.is_empty() {
            return String::new();
        }

        let mut instructions = String::new();
        instructions.push_str("## Tool Use Protocol\n\n");
        instructions.push_str("To use a tool, wrap a JSON object in <tool_call> tags:\n\n");
        instructions.push_str("```\n<tool_call>\n{\"name\": \"tool_name\", \"arguments\": {\"param\": \"value\"}}\n</tool_call>\n```\n\n");
        instructions.push_str(
            "CRITICAL: Output actual <tool_call> tags—never describe steps or give examples.\n\n",
        );
        instructions.push_str("Example: User says \"what's the date?\". You MUST respond with:\n<tool_call>\n{\"name\":\"shell\",\"arguments\":{\"command\":\"date\"}}\n</tool_call>\n\n");
        instructions.push_str("You may use multiple tool calls in a single response. ");
        instructions.push_str("After tool execution, results appear in <tool_result> tags. ");
        instructions
            .push_str("Continue reasoning with the results until you can give a final answer.\n\n");
        instructions.push_str("### Available Tools\n\n");

        for tool in &self.tool_specs {
            let _ = writeln!(
                instructions,
                "**{}**: {}\nParameters: `{}`\n",
                tool.name, tool.description, tool.parameters_schema
            );
        }

        instructions
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

    fn get_skills_context(&self) -> Option<String> {
        if self.skills.is_empty() {
            return None;
        }

        let mut parts = vec!["## Available Skills\n\n<available_skills>".to_string()];

        for skill in &self.skills {
            let location = skill.location.clone().unwrap_or_else(|| {
                self.workspace
                    .join("skills")
                    .join(&skill.name)
                    .join("SKILL.md")
            });

            parts.push(format!(
                "  <skill>\n    <name>{}</name>\n    <description>{}</description>\n    <location>{}</location>\n  </skill>",
                skill.name,
                skill.description,
                location.display()
            ));
        }

        parts.push("</available_skills>".to_string());

        Some(parts.join("\n"))
    }

    async fn get_memory_context(&self, user_message: &str) -> Option<String> {
        if let Some(ref memory) = self.memory {
            let mut context_parts = vec![];

            if let Ok(entries) = memory.recall(user_message, 5, None).await
                && !entries.is_empty()
            {
                let relevant: Vec<_> = entries
                    .iter()
                    .filter(|e| match e.score {
                        Some(score) => score >= MEMORY_MIN_RELEVANCE_SCORE,
                        None => true,
                    })
                    .collect();

                if !relevant.is_empty() {
                    context_parts.push("## Relevant Memory".to_string());
                    for entry in relevant {
                        if !entry.content.is_empty() {
                            context_parts.push(format!("- {}", entry.content));
                        }
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
        let mut parts = vec![];

        for (filename, section_header) in BOOTSTRAP_FILES {
            if let Ok(content) = std::fs::read_to_string(self.workspace.join(filename)) {
                let trimmed = content.trim();

                if !trimmed.is_empty() {
                    let content = if trimmed.chars().count() > BOOTSTRAP_MAX_CHARS {
                        let truncated: String = trimmed.chars().take(BOOTSTRAP_MAX_CHARS).collect();
                        format!(
                            "{}\n\n[... truncated at {} chars — use file_read for full content]\n",
                            truncated, BOOTSTRAP_MAX_CHARS
                        )
                    } else {
                        trimmed.to_string()
                    };

                    parts.push(format!("{}\n\n{}", section_header, content));
                }
            }
        }

        if parts.is_empty() {
            None
        } else {
            Some(parts.join("\n\n---\n\n"))
        }
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
