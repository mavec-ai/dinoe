use crate::tools::extract_string_arg;
use crate::traits::{Tool, ToolResult};
use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MAX_RESPONSE_SIZE: usize = 500_000;
const TIMEOUT_SECS: u64 = 30;
const RATE_LIMIT_MAX: u64 = 60;
const RATE_LIMIT_WINDOW_SECS: u64 = 3600;

static RATE_LIMITER: HttpRateLimiter = HttpRateLimiter::new(RATE_LIMIT_MAX, RATE_LIMIT_WINDOW_SECS);

pub struct HttpRequestTool {
    client: reqwest::Client,
    max_size: usize,
}

impl HttpRequestTool {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(TIMEOUT_SECS))
            .connect_timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::limited(10))
            .user_agent("Dinoe/0.2 (http_request)")
            .build()
            .expect("Failed to build HTTP client");

        Self {
            client,
            max_size: MAX_RESPONSE_SIZE,
        }
    }

    fn validate_url(&self, url: &str) -> Result<String, String> {
        let url = url.trim();

        if url.is_empty() {
            return Err("URL cannot be empty".into());
        }

        if url.chars().any(char::is_whitespace) {
            return Err("URL cannot contain whitespace".into());
        }

        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err("Only http:// and https:// URLs are allowed".into());
        }

        let host = extract_host(url)?;
        if is_private_host(&host) {
            return Err(format!("Blocked local/private host: {}", host));
        }

        Ok(url.to_string())
    }

    fn parse_method(&self, method: &str) -> Result<reqwest::Method, String> {
        match method.to_uppercase().as_str() {
            "GET" => Ok(reqwest::Method::GET),
            "POST" => Ok(reqwest::Method::POST),
            "PUT" => Ok(reqwest::Method::PUT),
            "DELETE" => Ok(reqwest::Method::DELETE),
            _ => Err(format!(
                "Unsupported method: {}. Supported: GET, POST, PUT, DELETE",
                method
            )),
        }
    }

    fn parse_headers(&self, headers_val: &serde_json::Value) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Some(obj) = headers_val.as_object() {
            for (key, value) in obj {
                if let Some(str_val) = value.as_str()
                    && let Ok(name) = HeaderName::try_from(key.clone())
                    && let Ok(val) = HeaderValue::try_from(str_val)
                {
                    headers.insert(name, val);
                }
            }
        }
        headers
    }

    fn truncate(&self, text: &str) -> String {
        if text.len() > self.max_size {
            let mut truncated: String = text.chars().take(self.max_size).collect();
            truncated.push_str("\n\n... [truncated]");
            truncated
        } else {
            text.to_string()
        }
    }

    fn redact_sensitive_headers(headers: &HeaderMap) -> String {
        headers
            .iter()
            .map(|(name, value)| {
                let lower = name.as_str().to_lowercase();
                let is_sensitive = lower.contains("authorization")
                    || lower.contains("api-key")
                    || lower.contains("apikey")
                    || lower.contains("token")
                    || lower.contains("secret")
                    || lower.contains("cookie");

                if is_sensitive {
                    format!("{}: ***REDACTED***", name)
                } else {
                    format!("{}: {}", name, value.to_str().unwrap_or("[binary]"))
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl Default for HttpRequestTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for HttpRequestTool {
    fn name(&self) -> &str {
        "http_request"
    }

    fn description(&self) -> &str {
        "Make HTTP requests to external APIs. Supports GET, POST, PUT, DELETE methods with custom headers and body. \
         Blocks local/private hosts for security."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "HTTP or HTTPS URL to request"
                },
                "method": {
                    "type": "string",
                    "description": "HTTP method (GET, POST, PUT, DELETE)",
                    "default": "GET"
                },
                "headers": {
                    "type": "object",
                    "description": "Optional HTTP headers as key-value pairs",
                    "default": {}
                },
                "body": {
                    "type": "string",
                    "description": "Optional request body (for POST, PUT, DELETE)"
                }
            },
            "required": ["url"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        if !RATE_LIMITER.check_and_record() {
            return Ok(ToolResult::error(
                "Rate limit exceeded: too many HTTP requests. Please wait a moment.",
            ));
        }

        let url = extract_string_arg(&args, "url")?;

        let url = match self.validate_url(&url) {
            Ok(u) => u,
            Err(e) => return Ok(ToolResult::error(e)),
        };

        let method_str = args
            .get("method")
            .and_then(|v| v.as_str())
            .unwrap_or("GET");

        let method = match self.parse_method(method_str) {
            Ok(m) => m,
            Err(e) => return Ok(ToolResult::error(e)),
        };

        let headers_val = args.get("headers").cloned().unwrap_or(json!({}));
        let headers = self.parse_headers(&headers_val);
        let body = args.get("body").and_then(|v| v.as_str());

        let mut request = self.client.request(method, &url);

        if !headers.is_empty() {
            request = request.headers(headers);
        }

        if let Some(body_str) = body {
            request = request.body(body_str.to_string());
        }

        let response = match request.send().await {
            Ok(r) => r,
            Err(e) => return Ok(ToolResult::error(format!("HTTP request failed: {}", e))),
        };

        let status = response.status();
        let status_code = status.as_u16();

        let response_headers = Self::redact_sensitive_headers(response.headers());

        let response_text = match response.text().await {
            Ok(text) => self.truncate(&text),
            Err(e) => format!("[Failed to read response body: {}]", e),
        };

        let output = format!(
            "Status: {} {}\nResponse Headers:\n{}\n\nResponse Body:\n{}",
            status_code,
            status.canonical_reason().unwrap_or("Unknown"),
            response_headers,
            response_text
        );

        if status.is_success() {
            Ok(ToolResult::success(output))
        } else {
            Ok(ToolResult {
                success: false,
                output,
                error: Some(format!("HTTP {}", status_code)),
            })
        }
    }
}

struct HttpRateLimiter {
    window_start: AtomicU64,
    count: AtomicU64,
    max_actions: u64,
    window_secs: u64,
}

impl HttpRateLimiter {
    const fn new(max_actions: u64, window_secs: u64) -> Self {
        Self {
            window_start: AtomicU64::new(0),
            count: AtomicU64::new(0),
            max_actions,
            window_secs,
        }
    }

    fn check_and_record(&self) -> bool {
        let now = current_timestamp();
        let start = self.window_start.load(Ordering::Relaxed);

        if now < start || now - start >= self.window_secs {
            self.window_start.store(now, Ordering::Relaxed);
            self.count.store(1, Ordering::Relaxed);
            return true;
        }

        let count = self.count.fetch_add(1, Ordering::Relaxed);
        count < self.max_actions
    }
}

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn extract_host(url: &str) -> Result<String, String> {
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .ok_or_else(|| "Invalid URL scheme".to_string())?;

    let authority = rest
        .split(['/', '?', '#'])
        .next()
        .ok_or_else(|| "Invalid URL".to_string())?;

    if authority.is_empty() {
        return Err("URL must include a host".into());
    }

    if authority.contains('@') {
        return Err("URL userinfo is not allowed".into());
    }

    let host = authority
        .split(':')
        .next()
        .unwrap_or_default()
        .trim()
        .trim_end_matches('.')
        .to_lowercase();

    if host.is_empty() {
        return Err("URL must include a valid host".into());
    }

    Ok(host)
}

fn is_private_host(host: &str) -> bool {
    if host == "localhost" || host.ends_with(".localhost") {
        return true;
    }

    if host.ends_with(".local") || host == "local" {
        return true;
    }

    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return match ip {
            std::net::IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
            std::net::IpAddr::V6(v6) => v6.is_loopback(),
        };
    }

    false
}
