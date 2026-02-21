use crate::ChatRequest;
use crate::ProviderEvent;
use crate::agent::{ContextBuilder, ToolRegistry};
use crate::skills::Skill;
use crate::traits::{ChatMessage, MemoryCategory, Provider, ToolCall};
use anyhow::Result;
use futures_util::StreamExt;
use std::collections::VecDeque;
use std::io::Write;
use std::sync::Arc;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;
use tracing::error;

const DEFAULT_MAX_HISTORY: usize = 50;
const COMPACT_KEEP_RECENT: usize = 20;
const COMPACTION_MAX_SOURCE_CHARS: usize = 12_000;
const COMPACTION_MAX_SUMMARY_CHARS: usize = 2_000;

const TOOL_CALL_OPEN_TAGS: &[&str] = &["<function=", "<tool_call", "<invoke"];
const TOOL_CALL_CLOSE_TAGS: &[&str] = &["</function>", "</tool_call", "</invoke>"];

const LOOP_DETECTION_WINDOW: usize = 5;
const LOOP_DETECTION_THRESHOLD: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ToolCallSignature {
    name: String,
    args_hash: String,
}

impl ToolCallSignature {
    fn from_tool_call(tool_call: &ToolCall) -> Self {
        let args_hash = format!("{:x}", md5::compute(tool_call.arguments.as_bytes()));
        Self {
            name: tool_call.name.clone(),
            args_hash,
        }
    }
}

pub struct AgentLoop {
    provider: Arc<dyn Provider>,
    context_builder: ContextBuilder,
    tool_registry: Arc<ToolRegistry>,
    max_iterations: usize,
    max_history: usize,
    model_name: String,
    temperature: f64,
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
            model_name: "openai/gpt-5-mini".to_string(),
            temperature: 1.0,
        }
    }

    pub fn with_skills(mut self, skills: Vec<Skill>) -> Self {
        self.context_builder = self.context_builder.with_skills(skills);
        self
    }

    pub fn with_max_iterations(mut self, max: usize) -> Self {
        self.max_iterations = max;
        self
    }

    pub fn with_max_history(mut self, max: usize) -> Self {
        self.max_history = max;
        self
    }

    pub fn with_model_name(mut self, model_name: String) -> Self {
        self.model_name = model_name;
        self
    }

    pub fn with_temperature(mut self, temperature: f64) -> Self {
        self.temperature = temperature;
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

    fn detect_tool_loop(
        recent_calls: &mut VecDeque<ToolCallSignature>,
        tool_calls: &[ToolCall],
    ) -> Option<String> {
        for tc in tool_calls {
            let sig = ToolCallSignature::from_tool_call(tc);
            recent_calls.push_back(sig);

            if recent_calls.len() > LOOP_DETECTION_WINDOW {
                recent_calls.pop_front();
            }
        }

        let mut consecutive_count = 1;
        let mut last_sig: Option<&ToolCallSignature> = None;

        for sig in recent_calls.iter() {
            if let Some(prev) = last_sig {
                if sig == prev {
                    consecutive_count += 1;
                    if consecutive_count >= LOOP_DETECTION_THRESHOLD {
                        return Some(format!(
                            "Tool loop detected: '{}' called {} times with same arguments. \
                             The model may be stuck. Try rephrasing your request or using a larger model.",
                            sig.name,
                            consecutive_count
                        ));
                    }
                } else {
                    consecutive_count = 1;
                }
            }
            last_sig = Some(sig);
        }

        None
    }

    pub async fn process(&self, message: &str) -> Result<String> {
        let history = vec![];
        self.process_with_history(message, history).await
    }

    pub async fn process_stream(&self, message: &str) -> Result<String> {
        let history = vec![];
        self.process_stream_with_history(message, history).await
    }

    pub async fn process_stream_with_history(
        &self,
        message: &str,
        history: Vec<ChatMessage>,
    ) -> Result<String> {
        self.store_message("user", message).await;

        let mut messages = self.context_builder.build_messages(history, message).await;
        let mut iterations = 0;
        let mut recent_tool_calls: VecDeque<ToolCallSignature> = VecDeque::new();

        while iterations < self.max_iterations {
            iterations += 1;

            let tools = self.tool_registry.get_specs();
            let request = ChatRequest {
                messages: &messages,
                tools: if tools.is_empty() { None } else { Some(&tools) },
            };

            let mut stream = self
                .provider
                .chat_stream(request, &self.model_name, self.temperature)
                .await?;

            let mut full_response = String::new();
            let mut thinking_content = String::new();
            let mut tool_calls: Vec<ToolCall> = Vec::new();

            while let Some(event) = stream.next().await {
                match event {
                    ProviderEvent::Token(token) => {
                        if !thinking_content.is_empty() {
                            println!("\n");
                            thinking_content.clear();
                        }
                        print!("{}", token);
                        let _ = std::io::stdout().flush();
                        full_response.push_str(&token);
                    }
                    ProviderEvent::Thinking(thinking) => {
                        print!("\x1b[90m{}\x1b[0m", thinking);
                        let _ = std::io::stdout().flush();
                        thinking_content.push_str(&thinking);
                    }
                    ProviderEvent::ToolCall(tool_call) => {
                        tool_calls.push(tool_call);
                    }
                    ProviderEvent::Done => break,
                }
            }

            println!();

            if tool_calls.is_empty() {
                if !full_response.is_empty() {
                    messages.push(ChatMessage::assistant(full_response.clone()));
                    self.store_message("assistant", &full_response).await;
                    return Ok(full_response);
                } else if !thinking_content.is_empty() {
                    let response = format!(
                        "I was thinking: {}... but didn't complete my response. Please try again.",
                        if thinking_content.len() > 200 { &thinking_content[..200] } else { &thinking_content }
                    );
                    println!("\x1b[33m{}\x1b[0m", response);
                    return Ok(response);
                } else {
                    anyhow::bail!("Empty response from model. Please try again.");
                }
            }

            if let Some(loop_msg) = Self::detect_tool_loop(&mut recent_tool_calls, &tool_calls) {
                println!("\x1b[33m⚠ {}\x1b[0m", loop_msg);
                anyhow::bail!("{}", loop_msg);
            }

            messages.push(ChatMessage::assistant_with_tool_calls(
                full_response.clone(),
                tool_calls.clone(),
            ));

            if !full_response.trim().is_empty() {
                self.store_message("assistant", &full_response).await;
            }

            for tool_call in tool_calls {
                let args: serde_json::Value =
                    serde_json::from_str(&tool_call.arguments).map_err(|e| {
                        anyhow::anyhow!(
                            "Failed to parse tool arguments for {}: {}",
                            tool_call.name,
                            e
                        )
                    })?;

                println!("\x1b[36m⚙ Executing: {}\x1b[0m", tool_call.name);
                let result = self.tool_registry.execute(&tool_call.name, args).await;
                println!("\x1b[36m✓ Result: {}\x1b[0m\n", serde_json::to_string(&result).unwrap_or_default());

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

    pub async fn process_with_history(
        &self,
        message: &str,
        history: Vec<ChatMessage>,
    ) -> Result<String> {
        self.store_message("user", message).await;

        let mut messages = self.context_builder.build_messages(history, message).await;
        let mut iterations = 0;
        let mut recent_tool_calls: VecDeque<ToolCallSignature> = VecDeque::new();

        while iterations < self.max_iterations {
            iterations += 1;

            let tools = self.tool_registry.get_specs();
            let request = ChatRequest {
                messages: &messages,
                tools: if tools.is_empty() { None } else { Some(&tools) },
            };

            let response = self.provider.chat(request, &self.model_name, self.temperature).await?;

            let (assistant_text, tool_calls) = if response.has_tool_calls() {
                (
                    response.text.clone().unwrap_or_default(),
                    response.tool_calls.clone(),
                )
            } else if let Some(text) = &response.text {
                let (parsed_text, parsed_calls) = self.parse_tool_calls_fallback(text);
                (parsed_text, parsed_calls)
            } else {
                return Ok("No response from provider".to_string());
            };

            if tool_calls.is_empty() {
                if !assistant_text.is_empty() {
                    messages.push(ChatMessage::assistant(assistant_text.clone()));
                    self.store_message("assistant", &assistant_text).await;
                    return Ok(assistant_text);
                } else {
                    anyhow::bail!("Empty response from model. Please try again.");
                }
            }

            if let Some(loop_msg) = Self::detect_tool_loop(&mut recent_tool_calls, &tool_calls) {
                println!("\x1b[33m⚠ {}\x1b[0m", loop_msg);
                anyhow::bail!("{}", loop_msg);
            }

            messages.push(ChatMessage::assistant_with_tool_calls(
                assistant_text.clone(),
                tool_calls.clone(),
            ));

            if !assistant_text.trim().is_empty() {
                self.store_message("assistant", &assistant_text).await;
            }

            for tool_call in tool_calls.clone() {
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

        let response = self.provider.chat(request, &self.model_name, self.temperature).await?;
        let summary = response
            .text
            .unwrap_or_else(|| self.truncate_transcript(transcript));
        Ok(summary)
    }

    fn parse_tool_calls_fallback(&self, response: &str) -> (String, Vec<ToolCall>) {
        let mut text_parts = Vec::new();
        let mut calls = Vec::new();
        let mut remaining = response;

        while let Some((start, open_tag)) = self.find_first_tag(remaining, TOOL_CALL_OPEN_TAGS) {
            let before = &remaining[..start];
            if !before.trim().is_empty() {
                text_parts.push(before.trim().to_string());
            }

            let close_tag = self.matching_tool_call_close_tag(open_tag);
            let close_tag = match close_tag {
                Some(tag) => tag,
                None => break,
            };

            let after_open = &remaining[start + open_tag.len()..];
            if let Some(close_idx) = after_open.find(close_tag) {
                let inner = &after_open[..close_idx];
                let json_values = self.extract_json_values(inner);
                for value in json_values {
                    if let Some(call) = self.parse_tool_call_value(&value) {
                        calls.push(call);
                    }
                }

                remaining = &after_open[close_idx + close_tag.len()..];
            } else {
                break;
            }
        }

        if !remaining.trim().is_empty() {
            text_parts.push(remaining.trim().to_string());
        }

        let text = text_parts.join("\n");
        (text, calls)
    }

    fn find_first_tag<'a>(&self, text: &'a str, tags: &'a [&'a str]) -> Option<(usize, &'a str)> {
        tags.iter()
            .filter_map(|tag| text.find(tag).map(|idx| (idx, *tag)))
            .min_by_key(|(idx, _)| *idx)
    }

    fn matching_tool_call_close_tag(&self, open_tag: &str) -> Option<&'static str> {
        let idx = TOOL_CALL_OPEN_TAGS.iter().position(|&t| t == open_tag)?;
        TOOL_CALL_CLOSE_TAGS.get(idx).copied()
    }

    fn extract_json_values(&self, text: &str) -> Vec<serde_json::Value> {
        let mut values = Vec::new();
        let mut depth = 0;
        let mut start = None;
        let mut in_string = false;
        let mut escape_next = false;

        for (i, ch) in text.char_indices() {
            match ch {
                '{' if !in_string => {
                    if depth == 0 {
                        start = Some(i);
                    }
                    depth += 1;
                }
                '}' if !in_string => {
                    depth -= 1;
                    if depth == 0 {
                        if let Some(s) = start
                            && let Ok(value) =
                                serde_json::from_str::<serde_json::Value>(&text[s..=i])
                        {
                            values.push(value);
                        }
                        start = None;
                    }
                }
                '"' if !escape_next => {
                    in_string = !in_string;
                }
                '\\' if in_string => {
                    escape_next = true;
                }
                _ => {
                    escape_next = false;
                }
            }
        }

        values
    }

    fn parse_tool_call_value(&self, value: &serde_json::Value) -> Option<ToolCall> {
        let name = value.get("name")?.as_str()?.to_string();
        let arguments = value.get("arguments")?;
        let arguments_str = serde_json::to_string(arguments).ok()?;
        let digest = md5::compute(arguments_str.as_bytes());
        let id = format!("call_{:x}", digest);

        Some(ToolCall {
            id,
            name,
            arguments: arguments_str,
        })
    }
}
