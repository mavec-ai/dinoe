use crate::traits::{ChatMessage, ChatResponse, Provider, ToolCall, ToolSpec};
use crate::{ChatRequest, ProviderEvent};
use async_trait::async_trait;
use futures_util::stream::BoxStream;
use serde::{Deserialize, Serialize};
use tokio_stream::wrappers::ReceiverStream;

#[derive(Debug, Serialize)]
struct OpenRouterRequest<'a> {
    model: String,
    messages: Vec<OpenRouterMessage<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OpenRouterTool>>,
    temperature: f64,
    stream: bool,
}

#[derive(Debug, Serialize)]
struct OpenRouterMessage<'a> {
    role: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenRouterToolCallRequest<'a>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<&'a str>,
}

#[derive(Debug, Serialize)]
struct OpenRouterToolCallRequest<'a> {
    id: &'a str,
    r#type: &'a str,
    function: OpenRouterFunctionRequest<'a>,
}

#[derive(Debug, Serialize)]
struct OpenRouterFunctionRequest<'a> {
    name: &'a str,
    arguments: &'a str,
}

#[derive(Debug, Serialize)]
struct OpenRouterTool {
    r#type: String,
    function: OpenRouterToolFunction,
}

#[derive(Debug, Serialize)]
struct OpenRouterToolFunction {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct OpenRouterResponse {
    choices: Vec<OpenRouterChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterChoice {
    message: OpenRouterResponseMessage,
}

#[derive(Debug, Deserialize)]
struct OpenRouterResponseMessage {
    content: Option<String>,
    #[serde(default)]
    reasoning_content: Option<String>,
    tool_calls: Option<Vec<OpenRouterToolCall>>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterToolCall {
    id: String,
    function: OpenRouterFunction,
}

#[derive(Debug, Deserialize)]
struct OpenRouterFunction {
    name: String,
    arguments: String,
}

#[derive(Debug, Deserialize)]
struct StreamResponse {
    choices: Vec<StreamChoice>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    delta: StreamDelta,
    #[allow(dead_code)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StreamDelta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    reasoning_content: Option<String>,
    tool_calls: Option<Vec<StreamToolCall>>,
}

#[derive(Debug, Deserialize)]
struct StreamToolCall {
    #[serde(default)]
    index: usize,
    id: Option<String>,
    function: Option<StreamFunction>,
}

#[derive(Debug, Deserialize)]
struct StreamFunction {
    name: Option<String>,
    arguments: Option<String>,
}

pub struct OpenRouterProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
    base_url: String,
}

impl OpenRouterProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .connect_timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_default();

        Self {
            client,
            api_key: api_key.into(),
            model: "anthropic/claude-sonnet-4".to_string(),
            base_url: "https://openrouter.ai/api/v1".to_string(),
        }
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    fn convert_messages<'a>(&self, messages: &'a [ChatMessage]) -> Vec<OpenRouterMessage<'a>> {
        messages
            .iter()
            .map(|m| {
                let tool_calls = m.tool_calls.as_ref().map(|tcs| {
                    tcs.iter()
                        .map(|tc| OpenRouterToolCallRequest {
                            id: &tc.id,
                            r#type: "function",
                            function: OpenRouterFunctionRequest {
                                name: &tc.name,
                                arguments: &tc.arguments,
                            },
                        })
                        .collect()
                });

                OpenRouterMessage {
                    role: &m.role,
                    content: if m.content.is_empty() { None } else { Some(&m.content) },
                    tool_calls,
                    tool_call_id: m.tool_call_id.as_deref(),
                }
            })
            .collect()
    }

    fn convert_tools(tools: &[ToolSpec]) -> Vec<OpenRouterTool> {
        tools
            .iter()
            .map(|t| OpenRouterTool {
                r#type: "function".to_string(),
                function: OpenRouterToolFunction {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    parameters: t.parameters_schema.clone(),
                },
            })
            .collect()
    }

    fn parse_sse_line(
        line: &str,
        pending_tool_calls: &mut std::collections::HashMap<usize, (String, String, String)>,
    ) -> Option<ProviderEvent> {
        let line = line.trim();

        if line.is_empty() || line == "data: [DONE]" {
            return None;
        }

        if let Some(data) = line.strip_prefix("data: ")
            && let Ok(response) = serde_json::from_str::<StreamResponse>(data)
                && let Some(choice) = response.choices.first() {
                    if let Some(content) = &choice.delta.content
                        && !content.is_empty() {
                            return Some(ProviderEvent::Token(content.clone()));
                        }

                    if let Some(reasoning) = &choice.delta.reasoning_content
                        && !reasoning.is_empty() {
                            return Some(ProviderEvent::Thinking(reasoning.clone()));
                        }

                    if let Some(tool_calls) = &choice.delta.tool_calls {
                        for stream_tc in tool_calls {
                            let idx = stream_tc.index;
                            let id = stream_tc.id.clone().unwrap_or_default();
                            let func = &stream_tc.function;

                            if let Some(func) = func {
                                let name = func.name.clone().unwrap_or_default();
                                let args = func.arguments.clone().unwrap_or_default();

                                let entry = pending_tool_calls
                                    .entry(idx)
                                    .or_insert_with(|| (String::new(), String::new(), String::new()));

                                if !id.is_empty() {
                                    entry.0 = id;
                                }
                                if !name.is_empty() {
                                    entry.1 = name;
                                }
                                entry.2.push_str(&args);
                            }
                        }
                    }

                    if choice.finish_reason.as_deref() == Some("tool_calls") {
                        let mut result = None;
                        let keys: Vec<usize> = pending_tool_calls.keys().cloned().collect();
                        for key in keys {
                            if let Some((id, name, args)) = pending_tool_calls.remove(&key) {
                                result = Some(ProviderEvent::ToolCall(ToolCall {
                                    id,
                                    name,
                                    arguments: args,
                                }));
                            }
                        }
                        return result;
                    }
                }

        None
    }
}

#[async_trait]
impl Provider for OpenRouterProvider {
    async fn chat(
        &self,
        request: ChatRequest<'_>,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ChatResponse> {
        let tools = request.tools.map(Self::convert_tools);
        let openrouter_request = OpenRouterRequest {
            model: model.to_string(),
            messages: self.convert_messages(request.messages),
            tools,
            temperature,
            stream: false,
        };

        let response = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("HTTP-Referer", "https://github.com/mavec-ai/dinoe")
            .header("X-Title", "Dinoe")
            .json(&openrouter_request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "OpenRouter API error ({}): {}",
                status,
                error_text
            ));
        }

        let openrouter_response: OpenRouterResponse = response.json().await?;

        let message = openrouter_response
            .choices
            .into_iter()
            .next()
            .map(|c| c.message)
            .ok_or_else(|| anyhow::anyhow!("No response from OpenRouter"))?;

        let tool_calls: Vec<ToolCall> = message
            .tool_calls
            .map(|tcs| {
                tcs.into_iter()
                    .map(|tc| ToolCall {
                        id: tc.id,
                        name: tc.function.name,
                        arguments: tc.function.arguments,
                    })
                    .collect()
            })
            .unwrap_or_default();

        let text = match &message.content {
            Some(c) if !c.is_empty() => message.content,
            _ => message.reasoning_content,
        };

        Ok(ChatResponse {
            text,
            tool_calls,
        })
    }

    async fn chat_stream(
        &self,
        request: ChatRequest<'_>,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<BoxStream<'static, ProviderEvent>> {
        let tools = request.tools.map(Self::convert_tools);
        let openrouter_request = OpenRouterRequest {
            model: model.to_string(),
            messages: self.convert_messages(request.messages),
            tools,
            temperature,
            stream: true,
        };

        let response = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("HTTP-Referer", "https://github.com/mavec-ai/dinoe")
            .header("X-Title", "Dinoe")
            .json(&openrouter_request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "OpenRouter API error ({}): {}",
                status,
                error_text
            ));
        }

        let (tx, rx) = tokio::sync::mpsc::channel::<ProviderEvent>(256);

        tokio::spawn(async move {
            use futures_util::StreamExt as _;
            let mut stream = response.bytes_stream();
            let mut buffer = String::new();
            let mut pending_tool_calls: std::collections::HashMap<usize, (String, String, String)> =
                std::collections::HashMap::new();

            while let Some(chunk_result) = stream.next().await {
                match chunk_result {
                    Ok(chunk) => {
                        if let Ok(text) = std::str::from_utf8(&chunk) {
                            buffer.push_str(text);

                            while let Some(pos) = buffer.find('\n') {
                                let line = buffer[..pos].to_string();
                                buffer = buffer[pos + 1..].to_string();

                                if let Some(event) = Self::parse_sse_line(&line, &mut pending_tool_calls)
                                    && tx.send(event).await.is_err() {
                                        return;
                                    }
                            }
                        }
                    }
                    Err(_) => break,
                }
            }

            for (_, (id, name, args)) in pending_tool_calls {
                if !args.is_empty() {
                    let _ = tx
                        .send(ProviderEvent::ToolCall(ToolCall { id, name, arguments: args }))
                        .await;
                }
            }

            let _ = tx.send(ProviderEvent::Done).await;
        });

        Ok(Box::pin(ReceiverStream::new(rx)))
    }
}
