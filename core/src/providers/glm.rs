use crate::traits::{ChatMessage, ChatResponse, Provider, ToolCall, ToolSpec};
use crate::{ChatRequest, ProviderEvent};
use async_trait::async_trait;
use futures_util::{StreamExt, stream::BoxStream};
use ring::hmac;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio_stream::wrappers::ReceiverStream;

#[derive(Debug, Serialize)]
struct GlmRequest<'a> {
    model: String,
    messages: Vec<GlmMessage<'a>>,
    tools: Option<Vec<GlmTool>>,
    temperature: f64,
    stream: bool,
}

#[derive(Debug, Serialize)]
struct GlmMessage<'a> {
    role: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<GlmToolCallRequest<'a>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<&'a str>,
}

#[derive(Debug, Serialize)]
struct GlmToolCallRequest<'a> {
    id: &'a str,
    r#type: &'a str,
    function: GlmFunctionRequest<'a>,
}

#[derive(Debug, Serialize)]
struct GlmFunctionRequest<'a> {
    name: &'a str,
    arguments: &'a str,
}

#[derive(Debug, Serialize)]
struct GlmTool {
    r#type: String,
    function: GlmToolFunction,
}

#[derive(Debug, Serialize)]
struct GlmToolFunction {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct GlmResponse {
    choices: Vec<GlmChoice>,
}

#[derive(Debug, Deserialize)]
struct GlmChoice {
    message: GlmResponseMessage,
}

#[derive(Debug, Deserialize)]
struct GlmResponseMessage {
    content: Option<String>,
    #[serde(default)]
    reasoning_content: Option<String>,
    tool_calls: Option<Vec<GlmToolCall>>,
}

#[derive(Debug, Deserialize)]
struct GlmToolCall {
    id: String,
    function: GlmFunction,
}

#[derive(Debug, Deserialize)]
struct GlmFunction {
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

pub struct GlmProvider {
    client: reqwest::Client,
    api_key_id: String,
    api_key_secret: String,
    model: String,
    base_url: String,
    token_cache: Mutex<Option<(String, u64)>>,
}

impl GlmProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .connect_timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_default();

        let api_key = api_key.into();
        let (id, secret) = api_key
            .split_once('.')
            .map(|(id, secret)| (id.to_string(), secret.to_string()))
            .unwrap_or_default();

        Self {
            client,
            api_key_id: id,
            api_key_secret: secret,
            model: "glm-4.7".to_string(),
            base_url: "https://api.z.ai/api/paas/v4".to_string(),
            token_cache: Mutex::new(None),
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

    fn base64url_encode_bytes(data: &[u8]) -> String {
        const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut result = String::new();
        let mut i = 0;
        while i < data.len() {
            let b0 = data[i] as u32;
            let b1 = if i + 1 < data.len() { data[i + 1] as u32 } else { 0 };
            let b2 = if i + 2 < data.len() { data[i + 2] as u32 } else { 0 };
            let triple = (b0 << 16) | (b1 << 8) | b2;

            result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
            result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);

            if i + 1 < data.len() {
                result.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
            }
            if i + 2 < data.len() {
                result.push(CHARS[(triple & 0x3F) as usize] as char);
            }

            i += 3;
        }

        result.replace('+', "-").replace('/', "_")
    }

    fn base64url_encode_str(s: &str) -> String {
        Self::base64url_encode_bytes(s.as_bytes())
    }

    fn generate_token(&self) -> anyhow::Result<String> {
        if self.api_key_id.is_empty() || self.api_key_secret.is_empty() {
            anyhow::bail!(
                "GLM API key not set or invalid format. Expected 'id.secret'. \
                 Run `dinoe onboard` or set GLM_API_KEY env var."
            );
        }

        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_millis() as u64;

        if let Ok(cache) = self.token_cache.lock()
            && let Some((ref token, expiry)) = *cache
            && now_ms < expiry
        {
            return Ok(token.clone());
        }

        let exp_ms = now_ms + 210_000;

        let header_json = r#"{"alg":"HS256","typ":"JWT","sign_type":"SIGN"}"#;
        let header_b64 = Self::base64url_encode_str(header_json);

        let payload_json = format!(
            r#"{{"api_key":"{}","exp":{},"timestamp":{}}}"#,
            self.api_key_id, exp_ms, now_ms
        );
        let payload_b64 = Self::base64url_encode_str(&payload_json);

        let signing_input = format!("{header_b64}.{payload_b64}");
        let key = hmac::Key::new(hmac::HMAC_SHA256, self.api_key_secret.as_bytes());
        let signature = hmac::sign(&key, signing_input.as_bytes());
        let sig_b64 = Self::base64url_encode_bytes(signature.as_ref());

        let token = format!("{signing_input}.{sig_b64}");

        if let Ok(mut cache) = self.token_cache.lock() {
            *cache = Some((token.clone(), now_ms + 180_000));
        }

        Ok(token)
    }

    fn convert_messages<'a>(&self, messages: &'a [ChatMessage]) -> Vec<GlmMessage<'a>> {
        messages
            .iter()
            .map(|m| {
                let tool_calls = m.tool_calls.as_ref().map(|tool_calls| {
                    tool_calls
                        .iter()
                        .map(|tc| GlmToolCallRequest {
                            id: &tc.id,
                            r#type: "function",
                            function: GlmFunctionRequest {
                                name: &tc.name,
                                arguments: &tc.arguments,
                            },
                        })
                        .collect()
                });

                let content = Some(m.content.as_str());

                GlmMessage {
                    role: &m.role,
                    content,
                    tool_calls,
                    tool_call_id: m.tool_call_id.as_deref(),
                }
            })
            .collect()
    }

    fn convert_tools(&self, tools: &[ToolSpec]) -> Vec<GlmTool> {
        tools
            .iter()
            .map(|t| GlmTool {
                r#type: "function".to_string(),
                function: GlmToolFunction {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    parameters: t.parameters_schema.clone(),
                },
            })
            .collect()
    }
}

#[async_trait]
impl Provider for GlmProvider {
    async fn chat(
        &self,
        request: ChatRequest<'_>,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ChatResponse> {
        let token = self.generate_token()?;

        let glm_request = GlmRequest {
            model: model.to_string(),
            messages: self.convert_messages(request.messages),
            tools: request.tools.map(|t| self.convert_tools(t)),
            temperature,
            stream: false,
        };

        let response = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json")
            .json(&glm_request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "GLM API error {}: {}",
                status,
                error_text
            ));
        }

        let glm_response: GlmResponse = response.json().await?;

        let choice = glm_response
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
        let has_reasoning = choice
            .message
            .reasoning_content
            .as_ref()
            .is_some_and(|c| !c.trim().is_empty());

        if !has_content && !has_reasoning && tool_calls.is_empty() {
            return Err(anyhow::anyhow!(
                "Empty response from API: no content or tool calls"
            ));
        }

        let text = match &choice.message.content {
            Some(c) if !c.trim().is_empty() => Some(c.clone()),
            _ => choice.message.reasoning_content.clone(),
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
        let token = self.generate_token()?;

        let glm_request = GlmRequest {
            model: model.to_string(),
            messages: self.convert_messages(request.messages),
            tools: request.tools.map(|t| self.convert_tools(t)),
            temperature,
            stream: true,
        };

        let response = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json")
            .json(&glm_request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "GLM API error {}: {}",
                status,
                error_text
            ));
        }

        let (tx, rx) = tokio::sync::mpsc::channel::<ProviderEvent>(100);

        tokio::spawn(async move {
            let mut buffer = String::new();
            let mut bytes_stream = response.bytes_stream();
            let mut pending_tool_calls: std::collections::HashMap<usize, (String, String, String)> =
                std::collections::HashMap::new();

            while let Some(item) = bytes_stream.next().await {
                match item {
                    Ok(bytes) => {
                        if let Ok(text) = String::from_utf8(bytes.to_vec()) {
                            buffer.push_str(&text);

                            while let Some(pos) = buffer.find('\n') {
                                let line: String = buffer.drain(..=pos).collect();

                                if let Some(event) = parse_sse_line(&line, &mut pending_tool_calls)
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

        Ok(ReceiverStream::new(rx).boxed())
    }
}

fn parse_sse_line(
    line: &str,
    pending_tool_calls: &mut std::collections::HashMap<usize, (String, String, String)>,
) -> Option<ProviderEvent> {
    let line = line.trim();

    if line.is_empty() || line.starts_with(':') {
        return None;
    }

    if let Some(data) = line.strip_prefix("data:") {
        let data = data.trim();

        if data == "[DONE]" {
            return None;
        }

        if let Ok(chunk) = serde_json::from_str::<StreamResponse>(data)
            && let Some(choice) = chunk.choices.first()
        {
            if let Some(content) = &choice.delta.content
                && !content.is_empty()
            {
                return Some(ProviderEvent::Token(content.clone()));
            }

            if let Some(reasoning) = &choice.delta.reasoning_content
                && !reasoning.is_empty()
            {
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
    }

    None
}
