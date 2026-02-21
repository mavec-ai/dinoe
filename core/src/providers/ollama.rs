use crate::traits::{ChatMessage, ChatResponse, Provider, ToolCall, ToolSpec};
use crate::{ChatRequest, ProviderEvent};
use async_trait::async_trait;
use futures_util::stream::BoxStream;
use serde::{Deserialize, Serialize};
use tokio_stream::wrappers::ReceiverStream;

#[derive(Debug, Serialize)]
struct OllamaRequest {
    model: String,
    messages: Vec<OllamaMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OllamaTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<OllamaOptions>,
    stream: bool,
}

#[derive(Debug, Serialize)]
struct OllamaMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OllamaToolCallRequest>>,
}

#[derive(Debug, Serialize)]
struct OllamaToolCallRequest {
    function: OllamaFunctionRequest,
}

#[derive(Debug, Serialize)]
struct OllamaFunctionRequest {
    name: String,
    arguments: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct OllamaTool {
    r#type: String,
    function: OllamaToolFunction,
}

#[derive(Debug, Serialize)]
struct OllamaToolFunction {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct OllamaOptions {
    temperature: f64,
}

#[derive(Debug, Deserialize)]
struct OllamaResponse {
    message: OllamaResponseMessage,
}

#[derive(Debug, Deserialize)]
struct OllamaResponseMessage {
    content: Option<String>,
    tool_calls: Option<Vec<OllamaToolCallResponse>>,
    #[serde(default)]
    thinking: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OllamaToolCallResponse {
    function: OllamaFunctionResponse,
}

#[derive(Debug, Deserialize)]
struct OllamaFunctionResponse {
    name: String,
    arguments: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct StreamResponse {
    message: Option<StreamMessage>,
    #[allow(dead_code)]
    done: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct StreamMessage {
    content: Option<String>,
    tool_calls: Option<Vec<OllamaToolCallResponse>>,
    #[serde(default)]
    thinking: Option<String>,
}

pub struct OllamaProvider {
    client: reqwest::Client,
    base_url: String,
    model: String,
}

impl OllamaProvider {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .connect_timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_default();

        Self {
            client,
            base_url: "http://localhost:11434".to_string(),
            model: "llama3.2".to_string(),
        }
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        let url = base_url.into();
        self.base_url = url.trim_end_matches('/').to_string();
        self
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    fn convert_messages(&self, messages: &[ChatMessage]) -> Vec<OllamaMessage> {
        let mut result = Vec::new();
        let mut tool_results_buffer: Vec<String> = Vec::new();

        for m in messages {
            if m.role == "tool" {
                let tool_call_id = m.tool_call_id.as_deref().unwrap_or("unknown");
                tool_results_buffer.push(format!(
                    "<tool_result id=\"{}\">\n{}\n</tool_result>",
                    tool_call_id, m.content
                ));
            } else {
                if !tool_results_buffer.is_empty() {
                    let combined_content = tool_results_buffer.join("\n");
                    let content = format!("[Tool results]\n{}", combined_content);
                    result.push(OllamaMessage {
                        role: "user".to_string(),
                        content: Some(content),
                        tool_calls: None,
                    });
                    tool_results_buffer.clear();
                }

                let tool_calls = m.tool_calls.as_ref().map(|tcs| {
                    tcs.iter()
                        .map(|tc| {
                            let args: serde_json::Value =
                                serde_json::from_str(&tc.arguments).unwrap_or(serde_json::Value::Null);
                            OllamaToolCallRequest {
                                function: OllamaFunctionRequest {
                                    name: tc.name.clone(),
                                    arguments: args,
                                },
                            }
                        })
                        .collect()
                });

                result.push(OllamaMessage {
                    role: m.role.clone(),
                    content: if m.content.is_empty() { None } else { Some(m.content.clone()) },
                    tool_calls,
                });
            }
        }

        if !tool_results_buffer.is_empty() {
            let combined_content = tool_results_buffer.join("\n");
            let content = format!("[Tool results]\n{}", combined_content);
            result.push(OllamaMessage {
                role: "user".to_string(),
                content: Some(content),
                tool_calls: None,
            });
        }

        result
    }

    fn convert_tools(tools: &[ToolSpec]) -> Vec<OllamaTool> {
        tools
            .iter()
            .map(|t| OllamaTool {
                r#type: "function".to_string(),
                function: OllamaToolFunction {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    parameters: t.parameters_schema.clone(),
                },
            })
            .collect()
    }

    fn parse_stream_line(line: &str) -> Option<ProviderEvent> {
        let line = line.trim();

        if line.is_empty() {
            return None;
        }

        if let Ok(response) = serde_json::from_str::<StreamResponse>(line)
            && let Some(message) = response.message {
                if let Some(content) = &message.content
                    && !content.is_empty() {
                        return Some(ProviderEvent::Token(content.clone()));
                    }

                if let Some(thinking) = &message.thinking
                    && !thinking.is_empty() {
                        return Some(ProviderEvent::Thinking(thinking.clone()));
                    }

                if let Some(tool_calls) = &message.tool_calls
                    && let Some(tc) = tool_calls.first() {
                        let args_str = serde_json::to_string(&tc.function.arguments)
                            .unwrap_or_default();
                        return Some(ProviderEvent::ToolCall(ToolCall {
                            id: format!("ollama_{}", uuid::Uuid::new_v4()),
                            name: tc.function.name.clone(),
                            arguments: args_str,
                        }));
                    }
            }

        None
    }
}

impl Default for OllamaProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for OllamaProvider {
    async fn chat(
        &self,
        request: ChatRequest<'_>,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ChatResponse> {
        let tools = request.tools.map(Self::convert_tools);
        let ollama_request = OllamaRequest {
            model: model.to_string(),
            messages: self.convert_messages(request.messages),
            tools,
            options: Some(OllamaOptions { temperature }),
            stream: false,
        };

        let response = self
            .client
            .post(format!("{}/api/chat", self.base_url))
            .json(&ollama_request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Ollama API error ({}): {}",
                status,
                error_text
            ));
        }

        let ollama_response: OllamaResponse = response.json().await?;

        let tool_calls: Vec<ToolCall> = ollama_response
            .message
            .tool_calls
            .map(|tcs| {
                tcs.into_iter()
                    .map(|tc| {
                        let args_str =
                            serde_json::to_string(&tc.function.arguments).unwrap_or_default();
                        ToolCall {
                            id: format!("ollama_{}", uuid::Uuid::new_v4()),
                            name: tc.function.name,
                            arguments: args_str,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        let content = ollama_response.message.content;

        let text = if content.as_ref().is_none_or(|c| c.is_empty()) {
            if tool_calls.is_empty() {
                if let Some(thinking) = &ollama_response.message.thinking {
                    let preview = if thinking.len() > 200 { &thinking[..200] } else { thinking };
                    Some(format!(
                        "I was thinking about this: {}... but I didn't complete my response. Could you try asking again?",
                        preview
                    ))
                } else {
                    content
                }
            } else {
                content
            }
        } else {
            content
        };

        Ok(ChatResponse { text, tool_calls })
    }

    async fn chat_stream(
        &self,
        request: ChatRequest<'_>,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<BoxStream<'static, ProviderEvent>> {
        let tools = request.tools.map(Self::convert_tools);
        let ollama_request = OllamaRequest {
            model: model.to_string(),
            messages: self.convert_messages(request.messages),
            tools,
            options: Some(OllamaOptions { temperature }),
            stream: true,
        };

        let response = self
            .client
            .post(format!("{}/api/chat", self.base_url))
            .json(&ollama_request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Ollama API error ({}): {}",
                status,
                error_text
            ));
        }

        let (tx, rx) = tokio::sync::mpsc::channel::<ProviderEvent>(256);

        tokio::spawn(async move {
            use futures_util::StreamExt as _;
            let mut stream = response.bytes_stream();
            let mut buffer = String::new();

            while let Some(chunk_result) = stream.next().await {
                match chunk_result {
                    Ok(chunk) => {
                        if let Ok(text) = std::str::from_utf8(&chunk) {
                            buffer.push_str(text);

                            while let Some(pos) = buffer.find('\n') {
                                let line = buffer[..pos].to_string();
                                buffer = buffer[pos + 1..].to_string();

                                if let Some(event) = Self::parse_stream_line(&line)
                                    && tx.send(event).await.is_err()
                                {
                                    return;
                                }
                            }
                        }
                    }
                    Err(_) => break,
                }
            }

            let _ = tx.send(ProviderEvent::Done).await;
        });

        Ok(Box::pin(ReceiverStream::new(rx)))
    }
}
