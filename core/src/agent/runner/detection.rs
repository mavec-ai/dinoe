use std::collections::{HashMap, VecDeque};

use crate::traits::ToolCall;

const LOOP_DETECTION_WINDOW: usize = 10;
const LOOP_DETECTION_THRESHOLD: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ToolCallSignature {
    name: String,
    args_hash: String,
}

impl ToolCallSignature {
    pub fn from_tool_call(tool_call: &ToolCall) -> Self {
        let args_hash = format!("{:x}", md5::compute(tool_call.arguments.as_bytes()));
        Self {
            name: tool_call.name.clone(),
            args_hash,
        }
    }
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

        let _ = detect_tool_loop(&mut recent, &[tc_a.clone()]);
        let _ = detect_tool_loop(&mut recent, &[tc_b.clone()]);
        let _ = detect_tool_loop(&mut recent, &[tc_a.clone()]);
        let _ = detect_tool_loop(&mut recent, &[tc_b.clone()]);
        let result = detect_tool_loop(&mut recent, &[tc_a.clone()]);

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
}
