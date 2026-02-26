use std::collections::{HashMap, HashSet, VecDeque};

use crate::traits::ToolCall;
use super::parsing::map_tool_name_alias;

const LOOP_DETECTION_WINDOW: usize = 10;
const LOOP_DETECTION_THRESHOLD: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ToolCallSignature {
    name: String,
    args_hash: String,
}

impl ToolCallSignature {
    pub fn from_tool_call(tool_call: &ToolCall) -> Self {
        let (name, args_json) = tool_call_signature(&tool_call.name, &tool_call.arguments);
        let args_hash = format!("{:x}", md5::compute(args_json.as_bytes()));
        Self { name, args_hash }
    }
}

pub fn canonicalize_json_for_tool_signature(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut keys: Vec<String> = map.keys().cloned().collect();
            keys.sort_unstable();
            let mut ordered = serde_json::Map::new();
            for key in keys {
                if let Some(child) = map.get(&key) {
                    ordered.insert(key, canonicalize_json_for_tool_signature(child));
                }
            }
            serde_json::Value::Object(ordered)
        }
        serde_json::Value::Array(items) => serde_json::Value::Array(
            items
                .iter()
                .map(canonicalize_json_for_tool_signature)
                .collect(),
        ),
        _ => value.clone(),
    }
}

pub fn tool_call_signature(name: &str, arguments_json: &str) -> (String, String) {
    let parsed: serde_json::Value = serde_json::from_str(arguments_json).unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
    let canonical_args = canonicalize_json_for_tool_signature(&parsed);
    let args_json = serde_json::to_string(&canonical_args).unwrap_or_else(|_| "{}".to_string());
    let lower_name = name.trim().to_ascii_lowercase();
    (map_tool_name_alias(&lower_name).to_string(), args_json)
}

pub fn deduplicate_tool_calls(tool_calls: &[ToolCall]) -> (Vec<ToolCall>, Vec<(String, String)>) {
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let mut unique: Vec<ToolCall> = Vec::new();
    let mut duplicates: Vec<(String, String)> = Vec::new();

    for tc in tool_calls {
        let sig = tool_call_signature(&tc.name, &tc.arguments);
        if seen.insert(sig.clone()) {
            unique.push(tc.clone());
        } else {
            duplicates.push((tc.name.clone(), tc.id.clone()));
        }
    }

    (unique, duplicates)
}

pub fn detect_tool_loop(
    recent_calls: &mut VecDeque<ToolCallSignature>,
    tool_calls: &[ToolCall],
) -> Option<String> {
    for tc in tool_calls {
        let sig = ToolCallSignature::from_tool_call(tc);
        recent_calls.push_back(sig);
    }

    while recent_calls.len() > LOOP_DETECTION_WINDOW {
        recent_calls.pop_front();
    }

    let mut freq: HashMap<&ToolCallSignature, usize> = HashMap::new();
    for sig in recent_calls.iter() {
        *freq.entry(sig).or_insert(0) += 1;
    }

    for (sig, &count) in freq.iter() {
        if count >= LOOP_DETECTION_THRESHOLD {
            return Some(format!(
                "Tool loop detected: '{}' called {} times with same arguments within {} turns. \
                 The model may be stuck. Try rephrasing your request or using a larger model.",
                sig.name,
                count,
                LOOP_DETECTION_WINDOW
            ));
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tool_call(name: &str, args: &str) -> ToolCall {
        ToolCall {
            id: "test".to_string(),
            name: name.to_string(),
            arguments: args.to_string(),
        }
    }

    #[test]
    fn test_consecutive_loop_detected() {
        let mut recent = VecDeque::new();
        let tc = make_tool_call("file_read", r#"{"path":"test.txt"}"#);

        let result = detect_tool_loop(&mut recent, &[tc.clone(), tc.clone(), tc.clone()]);
        assert!(result.is_some());
    }

    #[test]
    fn test_alternating_loop_detected() {
        let mut recent = VecDeque::new();
        let tc_a = make_tool_call("file_read", r#"{"path":"a.txt"}"#);
        let tc_b = make_tool_call("file_read", r#"{"path":"b.txt"}"#);

        let _ = detect_tool_loop(&mut recent, std::slice::from_ref(&tc_a));
        let _ = detect_tool_loop(&mut recent, std::slice::from_ref(&tc_b));
        let _ = detect_tool_loop(&mut recent, std::slice::from_ref(&tc_a));
        let _ = detect_tool_loop(&mut recent, std::slice::from_ref(&tc_b));
        let result = detect_tool_loop(&mut recent, std::slice::from_ref(&tc_a));

        assert!(result.is_some(), "Should detect alternating loop A->B->A->B->A");
    }

    #[test]
    fn test_no_loop_different_args() {
        let mut recent = VecDeque::new();
        let tc1 = make_tool_call("file_read", r#"{"path":"a.txt"}"#);
        let tc2 = make_tool_call("file_read", r#"{"path":"b.txt"}"#);
        let tc3 = make_tool_call("file_read", r#"{"path":"c.txt"}"#);

        let result = detect_tool_loop(&mut recent, &[tc1, tc2, tc3]);
        assert!(result.is_none());
    }

    #[test]
    fn test_canonicalize_json_sorts_keys() {
        let json_a = serde_json::json!({"z": 1, "a": 2, "m": 3});
        let json_b = serde_json::json!({"a": 2, "m": 3, "z": 1});

        let canon_a = canonicalize_json_for_tool_signature(&json_a);
        let canon_b = canonicalize_json_for_tool_signature(&json_b);

        assert_eq!(
            serde_json::to_string(&canon_a).unwrap(),
            serde_json::to_string(&canon_b).unwrap()
        );
    }

    #[test]
    fn test_canonicalize_json_nested() {
        let json_a = serde_json::json!({"outer": {"z": 1, "a": 2}});
        let json_b = serde_json::json!({"outer": {"a": 2, "z": 1}});

        let canon_a = canonicalize_json_for_tool_signature(&json_a);
        let canon_b = canonicalize_json_for_tool_signature(&json_b);

        assert_eq!(
            serde_json::to_string(&canon_a).unwrap(),
            serde_json::to_string(&canon_b).unwrap()
        );
    }

    #[test]
    fn test_canonicalize_json_arrays() {
        let json = serde_json::json!({"items": [{"b": 1, "a": 2}, {"c": 3}]});
        let canonical = canonicalize_json_for_tool_signature(&json);

        let expected = serde_json::json!({"items": [{"a": 2, "b": 1}, {"c": 3}]});
        assert_eq!(canonical, expected);
    }

    #[test]
    fn test_tool_call_signature_normalizes_name() {
        let (name_a, args_a) = tool_call_signature("FileRead", r#"{"path":"test.txt"}"#);
        let (name_b, args_b) = tool_call_signature("fileread", r#"{"path":"test.txt"}"#);

        assert_eq!(name_a, name_b);
        assert_eq!(args_a, args_b);
    }

    #[test]
    fn test_tool_call_signature_different_key_order_same() {
        let (_, args_a) = tool_call_signature("test", r#"{"z":1,"a":2}"#);
        let (_, args_b) = tool_call_signature("test", r#"{"a":2,"z":1}"#);

        assert_eq!(args_a, args_b);
    }

    #[test]
    fn test_deduplicate_tool_calls_removes_duplicates() {
        let tc1 = make_tool_call("file_read", r#"{"path":"test.txt"}"#);
        let tc2 = make_tool_call("file_read", r#"{"path":"test.txt"}"#);
        let tc3 = make_tool_call("file_read", r#"{"path":"other.txt"}"#);

        let (unique, duplicates) = deduplicate_tool_calls(&[tc1, tc2, tc3]);

        assert_eq!(unique.len(), 2);
        assert_eq!(duplicates.len(), 1);
        assert_eq!(duplicates[0].0, "file_read");
    }

    #[test]
    fn test_deduplicate_preserves_all_unique() {
        let tc1 = make_tool_call("file_read", r#"{"path":"a.txt"}"#);
        let tc2 = make_tool_call("file_read", r#"{"path":"b.txt"}"#);
        let tc3 = make_tool_call("shell", r#"{"command":"ls"}"#);

        let (unique, duplicates) = deduplicate_tool_calls(&[tc1, tc2, tc3]);

        assert_eq!(unique.len(), 3);
        assert!(duplicates.is_empty());
    }

    #[test]
    fn test_deduplicate_with_different_key_order() {
        let tc1 = make_tool_call("test", r#"{"a":1,"b":2}"#);
        let tc2 = make_tool_call("test", r#"{"b":2,"a":1}"#);

        let (unique, duplicates) = deduplicate_tool_calls(&[tc1, tc2]);

        assert_eq!(unique.len(), 1);
        assert_eq!(duplicates.len(), 1);
    }

    #[test]
    fn test_tool_call_signature_uses_alias_mapping() {
        let (name_bash, args_bash) = tool_call_signature("bash", r#"{"command":"ls -la"}"#);
        let (name_shell, args_shell) = tool_call_signature("shell", r#"{"command":"ls -la"}"#);

        assert_eq!(name_bash, "shell");
        assert_eq!(name_bash, name_shell);
        assert_eq!(args_bash, args_shell);
    }

    #[test]
    fn test_deduplicate_with_tool_name_aliases() {
        let tc1 = make_tool_call("bash", r#"{"command":"ls"}"#);
        let tc2 = make_tool_call("shell", r#"{"command":"ls"}"#);
        let tc3 = make_tool_call("sh", r#"{"command":"ls"}"#);

        let (unique, duplicates) = deduplicate_tool_calls(&[tc1, tc2, tc3]);

        assert_eq!(unique.len(), 1, "All aliases should be treated as same tool");
        assert_eq!(duplicates.len(), 2);
    }

    #[test]
    fn test_file_read_alias_mapping() {
        let (name_fileread, _) = tool_call_signature("fileread", r#"{"path":"test.txt"}"#);
        let (name_file_read, _) = tool_call_signature("file_read", r#"{"path":"test.txt"}"#);
        let (name_readfile, _) = tool_call_signature("readfile", r#"{"path":"test.txt"}"#);

        assert_eq!(name_fileread, "file_read");
        assert_eq!(name_file_read, "file_read");
        assert_eq!(name_readfile, "file_read");
    }

    #[test]
    fn test_search_tools_alias_mapping() {
        let (name_glob, _) = tool_call_signature("glob", r#"{"pattern":"*.rs"}"#);
        let (name_globsearch, _) = tool_call_signature("globsearch", r#"{"pattern":"*.rs"}"#);
        let (name_grep, _) = tool_call_signature("grep", r#"{"pattern":"fn main"}"#);

        assert_eq!(name_glob, "glob_search");
        assert_eq!(name_globsearch, "glob_search");
        assert_eq!(name_grep, "content_search");
    }

    #[test]
    fn test_http_tools_alias_mapping() {
        let (name_curl, _) = tool_call_signature("curl", r#"{"url":"https://example.com"}"#);
        let (name_fetch, _) = tool_call_signature("fetch", r#"{"url":"https://example.com"}"#);
        let (name_webfetch, _) = tool_call_signature("webfetch", r#"{"url":"https://example.com"}"#);

        assert_eq!(name_curl, "http_request");
        assert_eq!(name_fetch, "http_request");
        assert_eq!(name_webfetch, "web_fetch");
    }

    #[test]
    fn test_git_alias_mapping() {
        let (name_git, _) = tool_call_signature("git", r#"{"command":"status"}"#);
        let (name_gitops, _) = tool_call_signature("gitoperations", r#"{"command":"status"}"#);

        assert_eq!(name_git, "git_operations");
        assert_eq!(name_gitops, "git_operations");
    }
}
