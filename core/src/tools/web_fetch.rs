use crate::tools::extract_string_arg;
use crate::tools::security::RateLimiter;
use crate::traits::{Tool, ToolResult};
use async_trait::async_trait;
use reqwest::redirect::Policy;
use serde_json::json;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

const MAX_RESPONSE_SIZE: usize = 500_000;
const TIMEOUT_SECS: u64 = 30;
const MAX_REDIRECTS: usize = 10;
const RATE_LIMIT_MAX: u64 = 60;
const RATE_LIMIT_WINDOW_SECS: u64 = 3600;

static GLOBAL_RATE_LIMITER: OnceLock<Arc<RateLimiter>> = OnceLock::new();

pub struct WebFetchTool {
    client: reqwest::Client,
    max_size: usize,
    rate_limiter: Arc<RateLimiter>,
}

impl WebFetchTool {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(TIMEOUT_SECS))
            .connect_timeout(Duration::from_secs(10))
            .redirect(Policy::limited(MAX_REDIRECTS))
            .user_agent("Dinoe/0.2 (web_fetch)")
            .build()
            .expect("Failed to build HTTP client");

        let rate_limiter = GLOBAL_RATE_LIMITER
            .get_or_init(|| Arc::new(RateLimiter::new(RATE_LIMIT_MAX, RATE_LIMIT_WINDOW_SECS)))
            .clone();

        Self {
            client,
            max_size: MAX_RESPONSE_SIZE,
            rate_limiter,
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

    fn truncate(&self, text: &str) -> String {
        if text.len() > self.max_size {
            let mut truncated: String = text.chars().take(self.max_size).collect();
            truncated.push_str("\n\n... [truncated]");
            truncated
        } else {
            text.to_string()
        }
    }
}

impl Default for WebFetchTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        "web_fetch"
    }

    fn description(&self) -> &str {
        "Fetch a web page and convert HTML to clean plain text. \
         Supports text/html, text/plain, text/markdown, application/json. \
         Blocks local/private hosts for security."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The HTTP or HTTPS URL to fetch"
                }
            },
            "required": ["url"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        if !self.rate_limiter.check_and_record() {
            return Ok(ToolResult::error(
                "Rate limit exceeded: too many web fetches. Please wait a moment.",
            ));
        }

        let url = extract_string_arg(&args, "url")?;

        let url = match self.validate_url(&url) {
            Ok(u) => u,
            Err(e) => return Ok(ToolResult::error(e)),
        };

        let response = match self.client.get(&url).send().await {
            Ok(r) => r,
            Err(e) => return Ok(ToolResult::error(format!("HTTP request failed: {}", e))),
        };

        let status = response.status();
        if !status.is_success() {
            return Ok(ToolResult::error(format!(
                "HTTP {} {}",
                status.as_u16(),
                status.canonical_reason().unwrap_or("Unknown")
            )));
        }

        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_lowercase();

        let is_html =
            content_type.contains("text/html") || content_type.is_empty() || content_type.contains("application/xhtml");

        if !is_html
            && !content_type.contains("text/plain")
            && !content_type.contains("text/markdown")
            && !content_type.contains("application/json")
        {
            return Ok(ToolResult::error(format!(
                "Unsupported content type: {}",
                content_type
            )));
        }

        let body = match response.text().await {
            Ok(t) => t,
            Err(e) => return Ok(ToolResult::error(format!("Failed to read response: {}", e))),
        };

        let text = if is_html {
            html_to_text(&body)
        } else {
            body
        };

        Ok(ToolResult::success(self.truncate(&text)))
    }
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

fn html_to_text(html: &str) -> String {
    let mut text = String::new();
    let mut in_tag = false;
    let mut in_script = false;
    let mut in_style = false;
    let mut tag_name = String::new();
    let mut prev_char = ' ';

    for c in html.chars() {
        if c == '<' {
            in_tag = true;
            tag_name.clear();
            continue;
        }

        if c == '>' && in_tag {
            in_tag = false;
            let tag = tag_name.to_lowercase();

            if tag == "script" || tag.starts_with("script ") {
                in_script = true;
            } else if tag == "/script" {
                in_script = false;
            } else if tag == "style" || tag.starts_with("style ") {
                in_style = true;
            } else if tag == "/style" {
                in_style = false;
            } else if tag == "br" || tag == "br/" || tag == "/p" || tag == "/div" || tag == "/li" {
                if !text.ends_with('\n') && !text.is_empty() {
                    text.push('\n');
                }
            } else if tag == "li" || tag.starts_with("li ") {
                if !text.ends_with('\n') && !text.is_empty() {
                    text.push('\n');
                }
                text.push_str("- ");
            }
            continue;
        }

        if in_tag {
            tag_name.push(c);
            continue;
        }

        if in_script || in_style {
            continue;
        }

        if c == '&' {
            continue;
        }

        if c == ';' && prev_char == '&' {
            continue;
        }

        if c.is_whitespace() {
            if !text.is_empty() && !text.ends_with(' ') && !text.ends_with('\n') {
                text.push(' ');
            }
        } else {
            text.push(c);
        }

        prev_char = c;
    }

    text.lines()
        .map(|line| line.trim_end())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}
