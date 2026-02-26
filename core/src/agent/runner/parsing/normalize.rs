use super::helpers::{map_tool_name_alias, SHELL_COMMAND_ALIASES};
use crate::traits::ToolCall;

pub fn build_tool_call(
    name: &str,
    args: serde_json::Map<String, serde_json::Value>,
) -> Option<ToolCall> {
    if name.is_empty() {
        return None;
    }

    let tool_name = map_tool_name_alias(name);
    let normalized_args = normalize_tool_arguments(tool_name, serde_json::Value::Object(args));
    let arguments_str = serde_json::to_string(&normalized_args).ok()?;
    let digest = md5::compute(arguments_str.as_bytes());
    let id = format!("call_{:x}", digest);

    Some(ToolCall {
        id,
        name: tool_name.to_string(),
        arguments: arguments_str,
    })
}

pub fn normalize_tool_arguments(tool_name: &str, arguments: serde_json::Value) -> serde_json::Value {
    match map_tool_name_alias(tool_name) {
        "shell" => normalize_shell_arguments(arguments),
        _ => arguments,
    }
}

pub fn normalize_shell_arguments(arguments: serde_json::Value) -> serde_json::Value {
    match arguments {
        serde_json::Value::Object(mut map) => {
            if map
                .get("command")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .is_some_and(|cmd| !cmd.is_empty())
            {
                return serde_json::Value::Object(map);
            }

            for alias in SHELL_COMMAND_ALIASES {
                if let Some(value) = map.get(*alias).and_then(|v| v.as_str())
                    && let Some(command) = normalize_shell_command(value)
                {
                    map.insert("command".to_string(), serde_json::Value::String(command));
                    return serde_json::Value::Object(map);
                }
            }

            if let Some(url) = map
                .get("url")
                .or_else(|| map.get("http_url"))
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|url| !url.is_empty())
                && let Some(command) = build_curl_command(url)
            {
                map.insert("command".to_string(), serde_json::Value::String(command));
                return serde_json::Value::Object(map);
            }

            serde_json::Value::Object(map)
        }
        serde_json::Value::String(raw) => normalize_shell_command(&raw)
            .map(|command| serde_json::json!({ "command": command }))
            .unwrap_or_else(|| serde_json::Value::Object(serde_json::Map::new())),
        _ => serde_json::Value::Object(serde_json::Map::new()),
    }
}

fn normalize_shell_command(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let unwrapped = trimmed
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .or_else(|| {
            trimmed
                .strip_prefix('\'')
                .and_then(|s| s.strip_suffix('\''))
        })
        .unwrap_or(trimmed)
        .trim();

    if unwrapped.is_empty() {
        return None;
    }

    if (unwrapped.starts_with('{') && unwrapped.ends_with('}'))
        || (unwrapped.starts_with('[') && unwrapped.ends_with(']'))
    {
        return None;
    }

    if unwrapped.starts_with("http://") || unwrapped.starts_with("https://") {
        return build_curl_command(unwrapped).or_else(|| Some(unwrapped.to_string()));
    }

    Some(unwrapped.to_string())
}

pub fn build_curl_command(url: &str) -> Option<String> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return None;
    }

    if url.chars().any(char::is_whitespace) {
        return None;
    }

    let escaped = url.replace('\'', r#"'\\''"#);
    Some(format!("curl -s '{}'", escaped))
}
