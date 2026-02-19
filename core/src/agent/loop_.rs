use crate::ChatRequest;
use crate::agent::{ContextBuilder, ToolRegistry};
use crate::traits::{ChatMessage, MemoryCategory, Provider};
use anyhow::Result;
use std::sync::Arc;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;
use tracing::error;

const DEFAULT_MAX_HISTORY: usize = 50;
const COMPACT_KEEP_RECENT: usize = 20;
const COMPACTION_MAX_SOURCE_CHARS: usize = 12_000;
const COMPACTION_MAX_SUMMARY_CHARS: usize = 2_000;

pub struct AgentLoop {
    provider: Arc<dyn Provider>,
    context_builder: ContextBuilder,
    tool_registry: Arc<ToolRegistry>,
    max_iterations: usize,
    max_history: usize,
}

impl AgentLoop {
    pub fn new(
        provider: Arc<dyn Provider>,
        context_builder: ContextBuilder,
        tool_registry: Arc<ToolRegistry>,
    ) -> Self {
        Self {
            provider,
            context_builder,
            tool_registry,
            max_iterations: 20,
            max_history: DEFAULT_MAX_HISTORY,
        }
    }

    pub fn with_max_iterations(mut self, max: usize) -> Self {
        self.max_iterations = max;
        self
    }

    pub fn with_max_history(mut self, max: usize) -> Self {
        self.max_history = max;
        self
    }

    async fn store_message(&self, role: &str, content: &str) {
        if let Some(ref memory) = self.context_builder.memory {
            if content.trim().is_empty() {
                return;
            }
            let memory = memory.clone();
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
                .to_string();
            let content = content.to_string();
            let role = role.to_string();

            drop(tokio::spawn(async move {
                if let Err(e) = memory
                    .store(
                        &format!("msg_{}_{:x}", role, md5::compute(content.as_bytes())),
                        &content,
                        MemoryCategory::Daily,
                        Some(&timestamp),
                    )
                    .await
                {
                    error!("Failed to store message in memory: {}", e);
                }
            }));
        }
    }

    pub async fn process(&self, message: &str) -> Result<String> {
        let history = vec![];
        self.process_with_history(message, history).await
    }

    pub async fn process_with_history(
        &self,
        message: &str,
        history: Vec<ChatMessage>,
    ) -> Result<String> {
        self.store_message("user", message).await;

        let mut messages = self.context_builder.build_messages(history, message).await;
        let mut iterations = 0;

        while iterations < self.max_iterations {
            iterations += 1;

            let tools = self.tool_registry.get_specs();
            let request = ChatRequest {
                messages: &messages,
                tools: if tools.is_empty() { None } else { Some(&tools) },
            };

            let response = self.provider.chat(request).await?;

            if !response.has_tool_calls() {
                if let Some(text) = &response.text {
                    messages.push(ChatMessage::assistant(text.clone()));
                    self.store_message("assistant", text).await;
                }
                if let Some(text) = response.text {
                    return Ok(text);
                }
                return Ok("No response from provider".to_string());
            }

            let assistant_text = response.text.unwrap_or_default();
            messages.push(ChatMessage::assistant_with_tool_calls(
                assistant_text.clone(),
                response.tool_calls.clone(),
            ));

            if !assistant_text.trim().is_empty() {
                self.store_message("assistant", &assistant_text).await;
            }

            for tool_call in response.tool_calls {
                let args: serde_json::Value =
                    serde_json::from_str(&tool_call.arguments).map_err(|e| {
                        anyhow::anyhow!(
                            "Failed to parse tool arguments for {}: {}",
                            tool_call.name,
                            e
                        )
                    })?;

                let result = self.tool_registry.execute(&tool_call.name, args).await;

                messages.push(ChatMessage::tool_result(
                    tool_call.id,
                    serde_json::to_string(&result).unwrap_or_default(),
                ));
            }

            if self.should_compact_history(&messages) {
                self.compact_history(&mut messages).await;
            }
        }

        Ok("Max iterations reached".to_string())
    }

    fn should_compact_history(&self, messages: &[ChatMessage]) -> bool {
        let has_system = messages.first().is_some_and(|m| m.role == "system");
        let non_system_count = if has_system {
            messages.len().saturating_sub(1)
        } else {
            messages.len()
        };
        non_system_count > self.max_history
    }

    async fn compact_history(&self, messages: &mut Vec<ChatMessage>) {
        let has_system = messages.first().is_some_and(|m| m.role == "system");
        let start = if has_system { 1 } else { 0 };
        let non_system_count = if has_system {
            messages.len().saturating_sub(1)
        } else {
            messages.len()
        };

        let keep_recent = COMPACT_KEEP_RECENT.min(non_system_count);
        let compact_count = non_system_count.saturating_sub(keep_recent);
        if compact_count == 0 {
            return;
        }

        let compact_end = start + compact_count;
        let to_compact: Vec<ChatMessage> = messages[start..compact_end].to_vec();
        let transcript = self.build_transcript(&to_compact);

        let summary = match self.summarize(&transcript).await {
            Ok(s) => s,
            Err(_) => self.truncate_transcript(&transcript),
        };

        let summary_msg =
            ChatMessage::assistant(format!("[Conversation summary]\n{}", summary.trim()));
        messages.splice(start..compact_end, std::iter::once(summary_msg));
    }

    fn build_transcript(&self, messages: &[ChatMessage]) -> String {
        let mut transcript = String::new();
        for msg in messages {
            let role = msg.role.to_uppercase();
            let _ = std::fmt::write(
                &mut transcript,
                format_args!("{}: {}\n", role, msg.content.trim()),
            );
        }

        if transcript.chars().count() > COMPACTION_MAX_SOURCE_CHARS {
            self.truncate_transcript(&transcript)
        } else {
            transcript
        }
    }

    fn truncate_transcript(&self, text: &str) -> String {
        let chars: Vec<char> = text.chars().collect();
        if chars.len() <= COMPACTION_MAX_SUMMARY_CHARS {
            return text.to_string();
        }

        let truncated: String = chars[..COMPACTION_MAX_SUMMARY_CHARS].iter().collect();
        format!("{}...", truncated)
    }

    async fn summarize(&self, transcript: &str) -> Result<String> {
        let system_prompt = "You are a conversation summarizer. Summarize the following conversation into a concise context that preserves: user preferences, decisions, unresolved tasks, and key facts. Keep it under 2000 characters.";

        let user_prompt = format!("Summarize this conversation:\n\n{}", transcript);

        let request = ChatRequest {
            messages: &[
                ChatMessage {
                    role: "system".to_string(),
                    content: system_prompt.to_string(),
                    tool_calls: None,
                    tool_call_id: None,
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: user_prompt,
                    tool_calls: None,
                    tool_call_id: None,
                },
            ],
            tools: None,
        };

        let response = self.provider.chat(request).await?;
        let summary = response
            .text
            .unwrap_or_else(|| self.truncate_transcript(transcript));
        Ok(summary)
    }
}
