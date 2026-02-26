mod helpers;
mod normalize;
mod formats;

use crate::traits::ToolCall;
pub use helpers::map_tool_name_alias;
use helpers::{extract_json_values, find_first_tag, matching_tool_call_close_tag};
use formats::{
    parse_tool_call_from_json, parse_xml_tool_calls, parse_glm_shortened_body,
    parse_function_call_style, try_parse_openai_json_response, try_parse_minimax_invoke,
};

pub fn parse_tool_calls_fallback(response: &str) -> (String, Vec<ToolCall>) {
    let mut text_parts = Vec::new();
    let mut calls = Vec::new();
    let mut remaining = response;

    if let Some((text, json_calls)) = try_parse_openai_json_response(remaining)
        && !json_calls.is_empty()
    {
        return (text, json_calls);
    }

    if let Some((text, minimax_calls)) = try_parse_minimax_invoke(remaining)
        && !minimax_calls.is_empty()
    {
        return (text, minimax_calls);
    }

    while let Some((start, open_tag)) = find_first_tag(remaining, helpers::TOOL_CALL_OPEN_TAGS) {
        let before = &remaining[..start];
        if !before.trim().is_empty() {
            text_parts.push(before.trim().to_string());
        }

        let close_tag = match matching_tool_call_close_tag(open_tag) {
            Some(tag) => tag,
            None => break,
        };

        let after_open = &remaining[start + open_tag.len()..];
        if let Some(close_idx) = after_open.find(close_tag) {
            let inner = &after_open[..close_idx];
            let parsed_any = parse_inner_content(inner, &mut calls);

            if !parsed_any {
                tracing::warn!("Malformed tool call tag: {}", open_tag);
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

fn parse_inner_content(inner: &str, calls: &mut Vec<ToolCall>) -> bool {
    let json_values = extract_json_values(inner);
    if !json_values.is_empty() {
        let mut parsed_any = false;
        for value in json_values {
            if let Some(call) = parse_tool_call_from_json(&value) {
                calls.push(call);
                parsed_any = true;
            } else if let Some(nested) = value.get("tool_calls").and_then(|v| v.as_array()) {
                for tc in nested {
                    if let Some(call) = parse_tool_call_from_json(tc) {
                        calls.push(call);
                        parsed_any = true;
                    }
                }
            }
        }
        if parsed_any {
            return true;
        }
    }

    if let Some(xml_calls) = parse_xml_tool_calls(inner)
        && !xml_calls.is_empty()
    {
        calls.extend(xml_calls);
        return true;
    }

    if let Some(glm_call) = parse_glm_shortened_body(inner) {
        calls.push(glm_call);
        return true;
    }

    if let Some(func_calls) = parse_function_call_style(inner)
        && !func_calls.is_empty()
    {
        calls.extend(func_calls);
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_tool_calls_fallback_json() {
        let input = r#"{"tool_calls": [{"function": {"name": "shell", "arguments": {"command": "pwd"}}}]}"#;
        let (_text, calls) = parse_tool_calls_fallback(input);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "shell");
    }

    #[test]
    fn test_parse_tool_calls_fallback_xml() {
        let input = r#"<tool_call<shell><command>ls</command></shell></tool_call"#;
        let (_text, calls) = parse_tool_calls_fallback(input);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "shell");
    }

    #[test]
    fn test_parse_tool_calls_fallback_text() {
        let input = "Hello world";
        let (text, calls) = parse_tool_calls_fallback(input);
        assert!(calls.is_empty());
        assert_eq!(text, "Hello world");
    }
}
