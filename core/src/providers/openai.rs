use crate::traits::{ChatMessage, ChatResponse, Provider, ToolCall, ToolSpec};
use crate::{ChatRequest, ProviderEvent};
use async_trait::async_trait;
use futures_util::{Stream, stream};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
struct OpenAIRequest<'a> {
    model: String,
    messages: Vec<OpenAIMessage<'a>>,
    tools: Option<Vec<OpenAITool>>,
}

#[derive(Debug, Serialize)]
struct OpenAIMessage<'a> {
    role: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAIToolCallRequest<'a>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<&'a str>,
}

#[derive(Debug, Serialize)]
struct OpenAIToolCallRequest<'a> {
    id: &'a str,
    r#type: &'a str,
    function: OpenAIFunctionRequest<'a>,
}

#[derive(Debug, Serialize)]
struct OpenAIFunctionRequest<'a> {
    name: &'a str,
    arguments: &'a str,
}

#[derive(Debug, Serialize)]
struct OpenAITool {
    r#type: String,
    function: OpenAIToolFunction,
}

#[derive(Debug, Serialize)]
struct OpenAIToolFunction {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct OpenAIResponse {
    choices: Vec<OpenAIChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenAIChoice {
    message: OpenAIResponseMessage,
}

#[derive(Debug, Deserialize)]
struct OpenAIResponseMessage {
    content: Option<String>,
    tool_calls: Option<Vec<OpenAIToolCall>>,
}

#[derive(Debug, Deserialize)]
struct OpenAIToolCall {
    id: String,
    function: OpenAIFunction,
}

#[derive(Debug, Deserialize)]
struct OpenAIFunction {
    name: String,
    arguments: String,
}

pub struct OpenAIProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
    base_url: String,
}

impl OpenAIProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .connect_timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_default();

        Self {
            client,
            api_key: api_key.into(),
            model: "gpt-4o".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
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

    fn convert_messages<'a>(&self, messages: &'a [ChatMessage]) -> Vec<OpenAIMessage<'a>> {
        messages
            .iter()
            .map(|m| {
                let tool_calls = m.tool_calls.as_ref().map(|tool_calls| {
                    tool_calls
                        .iter()
                        .map(|tc| OpenAIToolCallRequest {
                            id: &tc.id,
                            r#type: "function",
                            function: OpenAIFunctionRequest {
                                name: &tc.name,
                                arguments: &tc.arguments,
                            },
                        })
                        .collect()
                });

                let content = Some(m.content.as_str());

                OpenAIMessage {
                    role: &m.role,
                    content,
                    tool_calls,
                    tool_call_id: m.tool_call_id.as_deref(),
                }
            })
            .collect()
    }

    fn convert_tools(&self, tools: &[ToolSpec]) -> Vec<OpenAITool> {
        tools
            .iter()
            .map(|t| OpenAITool {
                r#type: "function".to_string(),
                function: OpenAIToolFunction {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    parameters: t.parameters.clone(),
                },
            })
            .collect()
    }
}

#[async_trait]
impl Provider for OpenAIProvider {
    async fn chat(&self, request: ChatRequest<'_>) -> anyhow::Result<ChatResponse> {
        let openai_request = OpenAIRequest {
            model: self.model.clone(),
            messages: self.convert_messages(request.messages),
            tools: request.tools.map(|t| self.convert_tools(t)),
        };

        let response = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&openai_request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "OpenAI API error {}: {}",
                status,
                error_text
            ));
        }

        let openai_response: OpenAIResponse = response.json().await?;

        let choice = openai_response
            .choices
            .first()
            .ok_or_else(|| anyhow::anyhow!("No choices in response"))?;

        let tool_calls: Vec<ToolCall> = choice
            .message
            .tool_calls
            .as_ref()
            .map(|calls| {
                calls
                    .iter()
                    .map(|c| ToolCall {
                        id: c.id.clone(),
                        name: c.function.name.clone(),
                        arguments: c.function.arguments.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default();

        let has_content = choice
            .message
            .content
            .as_ref()
            .is_some_and(|c| !c.trim().is_empty());
        if !has_content && tool_calls.is_empty() {
            return Err(anyhow::anyhow!(
                "Empty response from API: no content or tool calls"
            ));
        }

        Ok(ChatResponse {
            text: choice.message.content.clone(),
            tool_calls,
        })
    }

    async fn chat_stream(
        &self,
        _request: ChatRequest<'_>,
    ) -> anyhow::Result<Box<dyn Stream<Item = ProviderEvent> + Send>> {
        let events = vec![ProviderEvent::Done];
        Ok(Box::new(stream::iter(events)) as Box<dyn Stream<Item = ProviderEvent> + Send>)
    }
}
