use std::sync::Arc;

use anyhow::Result;

use crate::ChatRequest;
use crate::traits::{ChatMessage, Provider};

const COMPACT_KEEP_RECENT: usize = 20;
const COMPACTION_MAX_SOURCE_CHARS: usize = 12_000;
const COMPACTION_MAX_SUMMARY_CHARS: usize = 2_000;
const SUMMARIZER_TEMPERATURE: f64 = 0.2;

pub struct HistoryManager {
    provider: Arc<dyn Provider>,
    model_name: String,
    max_history: usize,
}

impl HistoryManager {
    pub fn new(provider: Arc<dyn Provider>, model_name: String, max_history: usize) -> Self {
        Self {
            provider,
            model_name,
            max_history,
        }
    }

    pub fn should_compact(&self, messages: &[ChatMessage]) -> bool {
        count_non_system(messages) > self.max_history
    }

    pub async fn compact(&self, messages: &mut Vec<ChatMessage>) -> Result<bool> {
        let has_system = messages.first().is_some_and(|m| m.role == "system");
        let non_system_count = count_non_system(messages);

        if non_system_count <= self.max_history {
            return Ok(false);
        }

        let start = if has_system { 1 } else { 0 };
        let keep_recent = COMPACT_KEEP_RECENT.min(non_system_count);
        let compact_count = non_system_count.saturating_sub(keep_recent);
        if compact_count == 0 {
            return Ok(false);
        }

        let compact_end = start + compact_count;
        let to_compact: Vec<ChatMessage> = messages[start..compact_end].to_vec();
        let transcript = build_transcript(&to_compact);

        let summary = match self.summarize(&transcript).await {
            Ok(s) => truncate_with_ellipsis(&s, COMPACTION_MAX_SUMMARY_CHARS),
            Err(_) => truncate_with_ellipsis(&transcript, COMPACTION_MAX_SUMMARY_CHARS),
        };

        let summary_msg =
            ChatMessage::assistant(format!("[Compaction summary]\n{}", summary.trim()));
        messages.splice(start..compact_end, std::iter::once(summary_msg));
        Ok(true)
    }

    pub fn trim(&self, messages: &mut Vec<ChatMessage>) -> bool {
        let non_system_count = count_non_system(messages);

        if non_system_count <= self.max_history {
            return false;
        }

        let has_system = messages.first().is_some_and(|m| m.role == "system");
        let start = if has_system { 1 } else { 0 };
        let to_remove = non_system_count.saturating_sub(self.max_history);
        messages.drain(start..start + to_remove);
        true
    }

    async fn summarize(&self, transcript: &str) -> Result<String> {
        let system_prompt = "You are a conversation compaction engine. Summarize older chat history into concise context for future turns. Preserve: user preferences, commitments, decisions, unresolved tasks, key facts. Omit: filler, repeated chit-chat, verbose tool logs. Output plain text bullet points only.";

        let user_prompt = format!(
            "Summarize the following conversation history for context preservation. Keep it short (max 12 bullet points).\n\n{}",
            transcript
        );

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

        let response = self
            .provider
            .chat(request, &self.model_name, SUMMARIZER_TEMPERATURE)
            .await?;
        let summary = response.text.unwrap_or_default();
        Ok(summary)
    }
}

fn build_transcript(messages: &[ChatMessage]) -> String {
    let mut transcript = String::new();
    for msg in messages {
        let role = msg.role.to_uppercase();
        let _ = std::fmt::write(
            &mut transcript,
            format_args!("{}: {}\n", role, msg.content.trim()),
        );
    }

    if transcript.chars().count() > COMPACTION_MAX_SOURCE_CHARS {
        truncate_with_ellipsis(&transcript, COMPACTION_MAX_SOURCE_CHARS)
    } else {
        transcript
    }
}

fn truncate_with_ellipsis(text: &str, max_chars: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_chars {
        return text.to_string();
    }

    let truncated: String = chars[..max_chars.saturating_sub(3)].iter().collect();
    format!("{}...", truncated)
}

fn count_non_system(messages: &[ChatMessage]) -> usize {
    let has_system = messages.first().is_some_and(|m| m.role == "system");
    if has_system {
        messages.len().saturating_sub(1)
    } else {
        messages.len()
    }
}
