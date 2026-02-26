pub const TOOL_CALL_OPEN_TAGS: &[&str] = &[
    "âćŖ",
    "<function=",
    "<toolcall>",
    "<tool-call>",
    "<tool_call",
    "<invoke>",
    "<invoke ",
    "<minimax:tool_call>",
    "<minimax:toolcall>",
];

pub const TOOL_CALL_CLOSE_TAGS: &[&str] = &[
    "âćŖ",
    "</function>",
    "</toolcall>",
    "</tool-call>",
    "</tool_call",
    "</invoke>",
    "</invoke>",
    "</minimax:tool_call>",
    "</minimax:toolcall>",
];

pub const XML_META_TAGS: &[&str] = &[
    "tool_call", "toolcall", "tool-call", "invoke", "thinking", "thought", "analysis",
    "reasoning", "reflection", "function",
];

pub const SHELL_COMMAND_ALIASES: &[&str] = &[
    "cmd", "script", "shell_command", "command_line", "bash", "sh", "input", "code", "exec",
];

pub fn map_tool_name_alias(tool_name: &str) -> &str {
    match tool_name {
        "shell" | "bash" | "sh" | "exec" | "command" | "cmd" => "shell",
        "fileread" | "file_read" | "readfile" | "read_file" | "file" => "file_read",
        "filewrite" | "file_write" | "writefile" | "write_file" => "file_write",
        "fileedit" | "file_edit" | "editfile" | "edit_file" => "file_edit",
        "memoryrecall" | "memory_recall" | "recall" | "memrecall" => "memory_read",
        "memorystore" | "memory_store" | "store" | "memstore" => "memory_write",
        "globsearch" | "glob_search" | "glob" | "findfiles" | "find_files" => "glob_search",
        "contentsearch" | "content_search" | "grep" | "search" => "content_search",
        "httprequest" | "http_request" | "http" | "fetch" | "curl" | "wget" => "http_request",
        "webfetch" | "web_fetch" | "fetchurl" | "fetch_url" => "web_fetch",
        "gitoperations" | "git_operations" | "git" => "git_operations",
        _ => tool_name,
    }
}

pub fn default_param_for_tool(tool: &str) -> &'static str {
    match map_tool_name_alias(tool) {
        "shell" => "command",
        "file_read" | "file_write" => "path",
        "memory_read" => "query",
        "memory_write" => "content",
        _ => "input",
    }
}

pub fn find_first_tag<'a>(text: &'a str, tags: &'a [&'a str]) -> Option<(usize, &'a str)> {
    tags.iter()
        .filter_map(|tag| text.find(tag).map(|idx| (idx, *tag)))
        .min_by_key(|(idx, _)| *idx)
}

pub fn matching_tool_call_close_tag(open_tag: &str) -> Option<&'static str> {
    let idx = TOOL_CALL_OPEN_TAGS.iter().position(|&t| t == open_tag)?;
    TOOL_CALL_CLOSE_TAGS.get(idx).copied()
}

pub fn is_xml_meta_tag(tag: &str) -> bool {
    let normalized = tag.to_ascii_lowercase();
    XML_META_TAGS.contains(&normalized.as_str())
}

pub fn extract_attribute(text: &str, attr_name: &str) -> Option<String> {
    let pattern = format!("{}=\"", attr_name);
    let start = text.find(&pattern)?;
    let after = &text[start + pattern.len()..];
    let end = after.find('"')?;
    Some(after[..end].trim().to_string())
}

pub fn extract_xml_pairs(input: &str) -> Vec<(&str, &str)> {
    let mut results = Vec::new();
    let mut pos = 0;

    while pos < input.len() {
        if let Some(open_start) = input[pos..].find('<') {
            let abs_open_start = pos + open_start;

            if abs_open_start + 1 >= input.len() {
                break;
            }

            let after_lt = &input[abs_open_start + 1..];
            if after_lt.starts_with('/') || after_lt.starts_with('!') {
                pos = abs_open_start + 1;
                continue;
            }

            if let Some(gt_pos) = after_lt.find('>') {
                let tag_name = after_lt[..gt_pos].trim();
                let tag_name = tag_name.split_whitespace().next().unwrap_or(tag_name);

                if tag_name.is_empty() {
                    pos = abs_open_start + 1;
                    continue;
                }

                let inner_start = abs_open_start + 1 + gt_pos + 1;
                let close_tag = format!("</{}>", tag_name);

                if let Some(close_pos) = input[inner_start..].find(&close_tag) {
                    let inner = &input[inner_start..inner_start + close_pos];
                    results.push((tag_name, inner.trim()));
                    pos = inner_start + close_pos + close_tag.len();
                } else {
                    pos = abs_open_start + 1;
                }
            } else {
                pos = abs_open_start + 1;
            }
        } else {
            break;
        }
    }

    results
}

pub fn extract_json_values(input: &str) -> Vec<serde_json::Value> {
    let mut values = Vec::new();
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return values;
    }

    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
        values.push(value);
        return values;
    }

    let char_positions: Vec<(usize, char)> = trimmed.char_indices().collect();
    let mut idx = 0;

    while idx < char_positions.len() {
        let (byte_idx, ch) = char_positions[idx];
        if ch == '{' || ch == '[' {
            let slice = &trimmed[byte_idx..];
            let mut stream =
                serde_json::Deserializer::from_str(slice).into_iter::<serde_json::Value>();
            if let Some(Ok(value)) = stream.next() {
                let consumed = stream.byte_offset();
                if consumed > 0 {
                    values.push(value);
                    let next_byte = byte_idx + consumed;
                    while idx < char_positions.len() && char_positions[idx].0 < next_byte {
                        idx += 1;
                    }
                    continue;
                }
            }
        }
        idx += 1;
    }

    values
}

pub fn parse_arguments_value(raw: Option<&serde_json::Value>) -> serde_json::Value {
    match raw {
        Some(serde_json::Value::String(s)) => serde_json::from_str::<serde_json::Value>(s)
            .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new())),
        Some(value) => value.clone(),
        None => serde_json::Value::Object(serde_json::Map::new()),
    }
}
