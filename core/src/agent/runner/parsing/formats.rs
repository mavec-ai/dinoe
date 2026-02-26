use crate::traits::ToolCall;
use super::helpers::{
    extract_attribute, extract_json_values, extract_xml_pairs, is_xml_meta_tag, map_tool_name_alias,
    parse_arguments_value, default_param_for_tool,
};
use super::normalize::{build_tool_call, build_curl_command, normalize_tool_arguments};

pub fn try_parse_openai_json_response(response: &str) -> Option<(String, Vec<ToolCall>)> {
    let trimmed = response.trim();
    if !trimmed.starts_with('{') {
        return None;
    }

    let json_value = serde_json::from_str::<serde_json::Value>(trimmed).ok()?;
    let tool_calls = json_value.get("tool_calls")?.as_array()?;

    let mut calls = Vec::new();
    for tc in tool_calls {
        if let Some(call) = parse_tool_call_from_json(tc) {
            calls.push(call);
        }
    }

    if calls.is_empty() {
        return None;
    }

    let text = json_value
        .get("content")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    Some((text, calls))
}

pub fn try_parse_minimax_invoke(response: &str) -> Option<(String, Vec<ToolCall>)> {
    let mut calls = Vec::new();
    let mut text_parts = Vec::new();
    let mut last_end = 0;

    let invoke_start = "<invoke";
    let invoke_end = "</invoke>";

    let mut pos = 0;
    while let Some(start) = response[pos..].find(invoke_start) {
        let abs_start = pos + start;
        let after_start = &response[abs_start..];

        let name = extract_attribute(after_start, "name");
        let name = name.filter(|n| !n.is_empty());

        if let Some(end_pos) = after_start.find(invoke_end) {
            let inner = &after_start[..end_pos];
            let full_end = abs_start + end_pos + invoke_end.len();

            if last_end < abs_start {
                let before = response[last_end..abs_start].trim();
                if !before.is_empty() {
                    text_parts.push(before.to_string());
                }
            }

            if let Some(tool_name) = name {
                let args = parse_minimax_parameters(inner);
                if let Some(call) = build_tool_call(&tool_name, args) {
                    calls.push(call);
                }
            }

            last_end = full_end;
            pos = full_end;
        } else {
            pos = abs_start + 1;
        }
    }

    if calls.is_empty() {
        return None;
    }

    if last_end < response.len() {
        let after = response[last_end..].trim();
        if !after.is_empty() {
            text_parts.push(after.to_string());
        }
    }

    let text = text_parts
        .join("\n")
        .replace("<minimax:tool_call>", "")
        .replace("</minimax:tool_call>", "")
        .replace("<minimax:toolcall>", "")
        .replace("</minimax:toolcall>", "")
        .trim()
        .to_string();

    Some((text, calls))
}

fn parse_minimax_parameters(inner: &str) -> serde_json::Map<String, serde_json::Value> {
    let mut args = serde_json::Map::new();
    let param_start = "<parameter";
    let param_end = "</parameter>";

    let mut pos = 0;
    while let Some(start) = inner[pos..].find(param_start) {
        let after_start = &inner[pos + start..];

        let name = extract_attribute(after_start, "name");
        if let Some(end_pos) = after_start.find(param_end) {
            if let Some(gte_pos) = after_start.find('>')
                && gte_pos < end_pos
            {
                let value = &after_start[gte_pos + 1..end_pos];
                if let Some(key) = name {
                    let parsed = extract_json_values(value)
                        .into_iter()
                        .next()
                        .unwrap_or_else(|| serde_json::Value::String(value.trim().to_string()));
                    args.insert(key, parsed);
                }
            }
            pos = pos + start + end_pos + param_end.len();
        } else {
            break;
        }
    }

    args
}

pub fn parse_xml_tool_calls(xml_content: &str) -> Option<Vec<ToolCall>> {
    let trimmed = xml_content.trim();
    if !trimmed.starts_with('<') {
        return None;
    }

    let mut calls = Vec::new();

    for (tool_name, inner_content) in extract_xml_pairs(trimmed) {
        if is_xml_meta_tag(tool_name) {
            continue;
        }

        if inner_content.is_empty() {
            continue;
        }

        let args = parse_xml_arguments(inner_content);
        if let Some(call) = build_tool_call(tool_name, args) {
            calls.push(call);
        }
    }

    if calls.is_empty() {
        None
    } else {
        Some(calls)
    }
}

fn parse_xml_arguments(inner: &str) -> serde_json::Map<String, serde_json::Value> {
    let mut args = serde_json::Map::new();

    if let Some(first_json) = extract_json_values(inner).into_iter().next() {
        match first_json {
            serde_json::Value::Object(obj) => return obj,
            other => {
                args.insert("value".to_string(), other);
                return args;
            }
        }
    }

    for (key, value) in extract_xml_pairs(inner) {
        if is_xml_meta_tag(key) {
            continue;
        }
        if !value.is_empty() {
            args.insert(key.to_string(), serde_json::Value::String(value.to_string()));
        }
    }

    if args.is_empty() && !inner.trim().is_empty() {
        args.insert(
            "content".to_string(),
            serde_json::Value::String(inner.trim().to_string()),
        );
    }

    args
}

pub fn parse_glm_shortened_body(body: &str) -> Option<ToolCall> {
    let body = body.trim();
    if body.is_empty() {
        return None;
    }

    let (tool_raw, value_part) = if body.contains("=\"") {
        let split_pos = body.find(|c: char| c.is_whitespace()).unwrap_or(body.len());
        let tool = body[..split_pos].trim();
        let attrs = body[split_pos..]
            .trim()
            .trim_end_matches("/>")
            .trim_end_matches('>')
            .trim_end_matches('/')
            .trim();
        (tool, attrs)
    } else if let Some(gt_pos) = body.find('>') {
        let tool = body[..gt_pos].trim();
        let value = body[gt_pos + 1..]
            .trim()
            .trim_end_matches("/>")
            .trim_end_matches('/')
            .trim();
        (tool, value)
    } else {
        return None;
    };

    let tool_raw = tool_raw.trim_end_matches(|c: char| c.is_whitespace());
    if tool_raw.is_empty() || !tool_raw.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return None;
    }

    let tool_name = map_tool_name_alias(tool_raw);

    if value_part.contains("=\"")
        && let Some(args) = parse_attribute_style(value_part)
    {
        return build_tool_call(tool_name, args);
    }

    if value_part.contains('\n')
        && let Some(args) = parse_yaml_style(value_part)
        && !args.is_empty()
    {
        return build_tool_call(tool_name, args);
    }

    if !value_part.is_empty() {
        let param = default_param_for_tool(tool_raw);
        let mut args = serde_json::Map::new();

        match tool_name {
            "shell" => {
                let command = if value_part.starts_with("http://") || value_part.starts_with("https://") {
                    build_curl_command(value_part).unwrap_or_else(|| value_part.to_string())
                } else {
                    value_part.to_string()
                };
                args.insert("command".to_string(), serde_json::Value::String(command));
            }
            _ => {
                args.insert(param.to_string(), serde_json::Value::String(value_part.to_string()));
            }
        }

        return build_tool_call(tool_name, args);
    }

    None
}

fn parse_attribute_style(attrs: &str) -> Option<serde_json::Map<String, serde_json::Value>> {
    let mut args = serde_json::Map::new();
    let mut rest = attrs;

    while let Some(eq_pos) = rest.find("=\"") {
        let key_start = rest[..eq_pos]
            .rfind(|c: char| c.is_whitespace())
            .map(|p| p + 1)
            .unwrap_or(0);
        let key = rest[key_start..eq_pos]
            .trim()
            .trim_matches(|c: char| c == ',' || c == ';');

        let after_quote = &rest[eq_pos + 2..];
        if let Some(end_quote) = after_quote.find('"') {
            let value = &after_quote[..end_quote];
            if !key.is_empty() {
                args.insert(key.to_string(), serde_json::Value::String(value.to_string()));
            }
            rest = &after_quote[end_quote + 1..];
        } else {
            break;
        }
    }

    if args.is_empty() {
        None
    } else {
        Some(args)
    }
}

fn parse_yaml_style(content: &str) -> Option<serde_json::Map<String, serde_json::Value>> {
    let mut args = serde_json::Map::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(colon_pos) = line.find(':') {
            let key = line[..colon_pos].trim();
            let value = line[colon_pos + 1..].trim();
            if !key.is_empty() && !value.is_empty() {
                let json_value = match value {
                    "true" | "yes" => serde_json::Value::Bool(true),
                    "false" | "no" => serde_json::Value::Bool(false),
                    _ => serde_json::Value::String(value.to_string()),
                };
                args.insert(key.to_string(), json_value);
            }
        }
    }

    if args.is_empty() {
        None
    } else {
        Some(args)
    }
}

pub fn parse_function_call_style(content: &str) -> Option<Vec<ToolCall>> {
    let mut calls = Vec::new();
    let start_tag = "<FunctionCall>";
    let end_tag = "</FunctionCall>";

    let mut pos = 0;
    while let Some(start) = content[pos..].find(start_tag) {
        let after_start = &content[pos + start + start_tag.len()..];
        if let Some(end) = after_start.find(end_tag) {
            let inner = after_start[..end].trim();

            let lines: Vec<&str> = inner.lines().collect();
            if lines.is_empty() {
                pos = pos + start + start_tag.len() + end + end_tag.len();
                continue;
            }

            let tool_name = lines[0].trim();
            if tool_name.is_empty() || !tool_name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                pos = pos + start + start_tag.len() + end + end_tag.len();
                continue;
            }

            let mut args = serde_json::Map::new();
            for line in &lines[1..] {
                let line = line.trim();
                if let Some(gt_pos) = line.find('>') {
                    let key = line[..gt_pos].trim();
                    let value = line[gt_pos + 1..].trim();
                    if !key.is_empty() && !value.is_empty() {
                        args.insert(key.to_string(), serde_json::Value::String(value.to_string()));
                    }
                }
            }

            if let Some(call) = build_tool_call(tool_name, args) {
                calls.push(call);
            }

            pos = pos + start + start_tag.len() + end + end_tag.len();
        } else {
            break;
        }
    }

    if calls.is_empty() {
        None
    } else {
        Some(calls)
    }
}

pub fn parse_tool_call_from_json(value: &serde_json::Value) -> Option<ToolCall> {
    let function = value.get("function");
    let source = function.unwrap_or(value);

    let name = source.get("name")?.as_str()?.trim().to_string();
    if name.is_empty() {
        return None;
    }

    let raw_args = source.get("arguments").or_else(|| source.get("parameters"));
    let args = parse_arguments_value(raw_args);
    let normalized = normalize_tool_arguments(&name, args);
    let args_str = serde_json::to_string(&normalized).ok()?;

    let id = value
        .get("id")
        .or_else(|| value.get("tool_call_id"))
        .or_else(|| value.get("call_id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            let digest = md5::compute(args_str.as_bytes());
            format!("call_{:x}", digest)
        });

    Some(ToolCall {
        id,
        name,
        arguments: args_str,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_xml_tool_calls() {
        let input = r#"<shell><command>ls -la</command></shell>"#;
        let result = parse_xml_tool_calls(input);
        assert!(result.is_some());
        let calls = result.unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "shell");
    }

    #[test]
    fn test_parse_json_tool_call() {
        let input = r#"{"name": "shell", "arguments": {"command": "pwd"}}"#;
        let value: serde_json::Value = serde_json::from_str(input).unwrap();
        let result = parse_tool_call_from_json(&value);
        assert!(result.is_some());
        let call = result.unwrap();
        assert_eq!(call.name, "shell");
    }

    #[test]
    fn test_parse_glm_shortened() {
        let input = "shell>ls -la";
        let result = parse_glm_shortened_body(input);
        assert!(result.is_some());
        let call = result.unwrap();
        assert_eq!(call.name, "shell");
    }
}
