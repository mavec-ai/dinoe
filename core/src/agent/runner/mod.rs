mod detection;
mod execution;
mod history;
mod parsing;

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use tokio::sync::mpsc::Sender;
use tracing::error;

use crate::ChatRequest;
use crate::agent::status::{StatusPrinter, StatusUpdate};
use crate::agent::{ContextBuilder, ToolRegistry};
use crate::skills::Skill;
use crate::traits::{ChatMessage, MemoryCategory, Provider};

use detection::{detect_tool_loop, deduplicate_tool_calls};
use execution::ToolExecutor;
use history::HistoryManager;
use parsing::parse_tool_calls_fallback;

const DEFAULT_MAX_HISTORY: usize = 50;

pub struct AgentLoop {
    provider: Arc<dyn Provider>,
    context_builder: ContextBuilder,
    tool_registry: Arc<ToolRegistry>,
    max_iterations: usize,
    max_history: usize,
    model_name: String,
    temperature: f64,
    parallel_tools: bool,
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
            parallel_tools: true,
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

    pub fn with_parallel_tools(mut self, parallel: bool) -> Self {
        self.parallel_tools = parallel;
        self
    }

    fn emit_status(status_tx: Option<&Sender<StatusUpdate>>, status: StatusUpdate) {
        if let Some(tx) = status_tx {
            let _ = tx.try_send(status);
        } else {
            StatusPrinter::new().print(&status);
        }
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
        self.process_with_status(message, None).await
    }

    pub async fn process_with_status(
        &self,
        message: &str,
        status_tx: Option<Sender<StatusUpdate>>,
    ) -> Result<String> {
        let history = vec![];
        self.process_with_history_and_status(message, history, status_tx).await
    }

    pub async fn process_with_history(
        &self,
        message: &str,
        history: Vec<ChatMessage>,
    ) -> Result<String> {
        self.process_with_history_and_status(message, history, None).await
    }

    pub async fn process_with_history_and_status(
        &self,
        message: &str,
        history: Vec<ChatMessage>,
        status_tx: Option<Sender<StatusUpdate>>,
    ) -> Result<String> {
        self.store_message("user", message).await;

        let mut messages = self.context_builder.build_messages(history, message).await;
        let mut iterations = 0;
        let mut recent_tool_calls: VecDeque<detection::ToolCallSignature> = VecDeque::new();
        let executor = ToolExecutor::new(self.tool_registry.clone());
        let history_manager = HistoryManager::new(
            self.provider.clone(),
            self.model_name.clone(),
            self.max_history,
        );

        Self::emit_status(status_tx.as_ref(), StatusUpdate::thinking("Processing..."));

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
                parse_tool_calls_fallback(text)
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

            if let Some(loop_msg) = detect_tool_loop(&mut recent_tool_calls, &tool_calls) {
                Self::emit_status(status_tx.as_ref(), StatusUpdate::status(format!("⚠ {}", loop_msg)));
                anyhow::bail!("{}", loop_msg);
            }

            let (tool_calls, duplicates) = deduplicate_tool_calls(&tool_calls);
            for (name, _id) in &duplicates {
                Self::emit_status(
                    status_tx.as_ref(),
                    StatusUpdate::status(format!(
                        "⚠ Skipped duplicate tool call '{}' with identical arguments",
                        name
                    )),
                );
            }

            messages.push(ChatMessage::assistant_with_tool_calls(
                assistant_text.clone(),
                tool_calls.clone(),
            ));

            if !assistant_text.trim().is_empty() {
                self.store_message("assistant", &assistant_text).await;
            }

            if self.parallel_tools && tool_calls.len() > 1 {
                let results = executor.execute_batch(&tool_calls).await;
                for (tool_call, result) in tool_calls.iter().zip(results.iter()) {
                    Self::emit_status(status_tx.as_ref(), StatusUpdate::tool_started(&tool_call.name));
                    let result_json = serde_json::to_string(&result).unwrap_or_default();
                    Self::emit_status(status_tx.as_ref(), StatusUpdate::tool_result(&tool_call.name, &result_json));
                    Self::emit_status(status_tx.as_ref(), StatusUpdate::tool_completed(&tool_call.name, result.success));
                    messages.push(ChatMessage::tool_result(
                        tool_call.id.clone(),
                        result_json,
                    ));
                }
            } else {
                for tool_call in tool_calls.clone() {
                    Self::emit_status(status_tx.as_ref(), StatusUpdate::tool_started(&tool_call.name));
                    let result = executor.execute(&tool_call).await;
                    let result_json = serde_json::to_string(&result).unwrap_or_default();
                    Self::emit_status(status_tx.as_ref(), StatusUpdate::tool_result(&tool_call.name, &result_json));
                    Self::emit_status(status_tx.as_ref(), StatusUpdate::tool_completed(&tool_call.name, result.success));
                    messages.push(ChatMessage::tool_result(
                        tool_call.id,
                        result_json,
                    ));
                }
            }

            if history_manager.should_compact(&messages) {
                let _ = history_manager.compact(&mut messages).await;
                history_manager.trim(&mut messages);
            }
        }

        Ok("Max iterations reached".to_string())
    }
}
