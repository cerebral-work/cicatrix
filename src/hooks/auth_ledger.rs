//! Authorization ledger extraction ported from upstream Janus (`lib/auth_ledger.jq`).
//!
//! Extracts a complete, ground-truth record of explicit user decisions from a session
//! transcript, independent of any recency window.

use serde_json::Value;
use std::collections::HashSet;

/// Maximum character length for a user message to be treated as user intent.
/// Larger messages are treated as synthetic context injections and excluded.
pub const MAX_USER_TEXT_LEN: usize = 3000;

/// Prefix for synthetic task notifications from subagent runners.
pub const TASK_NOTIFICATION_PREFIX: &str = "<task-notification>";

/// An entry in the authorization ledger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthEntry {
    /// A direct user text instruction.
    UserText(String),
    /// An explicit user decision captured via `ExitPlanMode` or `AskUserQuestion`.
    UserDecision(String),
}

impl AuthEntry {
    /// Format the entry with standard prefix (`user/text: ` or `user/decision: `).
    pub fn format_line(&self) -> String {
        match self {
            AuthEntry::UserText(text) => format!("user/text: {text}"),
            AuthEntry::UserDecision(dec) => format!("user/decision: {dec}"),
        }
    }
}

/// Extract all authorization ledger entries from a session transcript array.
///
/// Accepts a JSON array (or a slice of message objects) conforming to the
/// Claude conversation transcript schema.
pub fn extract_auth_ledger(transcript: &[Value]) -> Vec<AuthEntry> {
    // 1. Collect all decision tool_use IDs from assistant messages
    let mut decision_ids = HashSet::new();
    for item in transcript {
        if item.get("type").and_then(Value::as_str) == Some("assistant") {
            if let Some(content_arr) = item
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(Value::as_array)
            {
                for block in content_arr {
                    if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                        if let Some(name) = block.get("name").and_then(Value::as_str) {
                            if name == "ExitPlanMode" || name == "AskUserQuestion" {
                                if let Some(id) = block.get("id").and_then(Value::as_str) {
                                    decision_ids.insert(id.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // 2. Scan user messages for text and decision tool results
    let mut entries = Vec::new();
    for item in transcript {
        if item.get("type").and_then(Value::as_str) != Some("user") {
            continue;
        }

        let Some(content) = item.get("message").and_then(|m| m.get("content")) else {
            continue;
        };

        match content {
            Value::String(s) => {
                if !s.starts_with(TASK_NOTIFICATION_PREFIX) && s.len() <= MAX_USER_TEXT_LEN {
                    entries.push(AuthEntry::UserText(s.clone()));
                }
            }
            Value::Array(blocks) => {
                for block in blocks {
                    let block_type = block.get("type").and_then(Value::as_str);
                    match block_type {
                        Some("text") => {
                            if let Some(text) = block.get("text").and_then(Value::as_str) {
                                if !text.starts_with(TASK_NOTIFICATION_PREFIX)
                                    && text.len() <= MAX_USER_TEXT_LEN
                                {
                                    entries.push(AuthEntry::UserText(text.to_string()));
                                }
                            }
                        }
                        Some("tool_result") => {
                            let tool_use_id = block.get("tool_use_id").and_then(Value::as_str);
                            if let Some(id) = tool_use_id {
                                if decision_ids.contains(id) {
                                    if let Some(decision_text) = extract_tool_result_content(block)
                                    {
                                        entries.push(AuthEntry::UserDecision(decision_text));
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    entries
}

/// Extract text representation from a `tool_result` content field.
fn extract_tool_result_content(block: &Value) -> Option<String> {
    let content = block.get("content")?;
    match content {
        Value::String(s) => Some(s.clone()),
        Value::Array(blocks) => {
            let mut parts = Vec::new();
            for b in blocks {
                if let Some(t) = b.get("text").and_then(Value::as_str) {
                    parts.push(t);
                }
            }
            if parts.is_empty() {
                None
            } else {
                Some(parts.join("\n"))
            }
        }
        _ => None,
    }
}

/// Parse transcript JSON string (either an array or JSON lines) and extract formatted lines.
pub fn parse_and_extract_auth_ledger(json_str: &str) -> Result<Vec<String>, serde_json::Error> {
    let trimmed = json_str.trim();
    let values: Vec<Value> = if trimmed.starts_with('[') {
        serde_json::from_str(trimmed)?
    } else {
        // Handle JSON lines
        let mut list = Vec::new();
        for line in trimmed.lines() {
            let line_trimmed = line.trim();
            if !line_trimmed.is_empty() {
                list.push(serde_json::from_str(line_trimmed)?);
            }
        }
        list
    };

    let entries = extract_auth_ledger(&values);
    Ok(entries.iter().map(AuthEntry::format_line).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_user_text_string() {
        let transcript = vec![json!({
            "type": "user",
            "message": {
                "content": "Please implement the new feature"
            }
        })];

        let ledger = extract_auth_ledger(&transcript);
        assert_eq!(
            ledger,
            vec![AuthEntry::UserText(
                "Please implement the new feature".into()
            )]
        );
        assert_eq!(
            ledger[0].format_line(),
            "user/text: Please implement the new feature"
        );
    }

    #[test]
    fn extracts_user_text_blocks() {
        let transcript = vec![json!({
            "type": "user",
            "message": {
                "content": [
                    { "type": "text", "text": "Approve the plan" }
                ]
            }
        })];

        let ledger = extract_auth_ledger(&transcript);
        assert_eq!(ledger, vec![AuthEntry::UserText("Approve the plan".into())]);
    }

    #[test]
    fn filters_task_notifications_and_oversized_text() {
        let oversized = "a".repeat(3001);
        let transcript = vec![
            json!({
                "type": "user",
                "message": {
                    "content": "<task-notification> subagent done"
                }
            }),
            json!({
                "type": "user",
                "message": {
                    "content": oversized
                }
            }),
            json!({
                "type": "user",
                "message": {
                    "content": "Real directive"
                }
            }),
        ];

        let ledger = extract_auth_ledger(&transcript);
        assert_eq!(ledger, vec![AuthEntry::UserText("Real directive".into())]);
    }

    #[test]
    fn captures_explicit_tool_decisions() {
        let transcript = vec![
            json!({
                "type": "assistant",
                "message": {
                    "content": [
                        {
                            "type": "tool_use",
                            "id": "toolu_01_ask",
                            "name": "AskUserQuestion",
                            "input": { "question": "Should we proceed?" }
                        },
                        {
                            "type": "tool_use",
                            "id": "toolu_02_bash",
                            "name": "Bash",
                            "input": { "command": "ls" }
                        }
                    ]
                }
            }),
            json!({
                "type": "user",
                "message": {
                    "content": [
                        {
                            "type": "tool_result",
                            "tool_use_id": "toolu_01_ask",
                            "content": "Yes, proceed with option A"
                        },
                        {
                            "type": "tool_result",
                            "tool_use_id": "toolu_02_bash",
                            "content": "file1.rs\nfile2.rs"
                        }
                    ]
                }
            }),
        ];

        let ledger = extract_auth_ledger(&transcript);
        assert_eq!(
            ledger,
            vec![AuthEntry::UserDecision("Yes, proceed with option A".into())]
        );
        assert_eq!(
            ledger[0].format_line(),
            "user/decision: Yes, proceed with option A"
        );
    }

    #[test]
    fn parse_and_extract_json_lines_and_array() {
        let array_str = r#"[
            {"type": "user", "message": {"content": "First instruction"}}
        ]"#;
        let lines = parse_and_extract_auth_ledger(array_str).unwrap();
        assert_eq!(lines, vec!["user/text: First instruction"]);

        let jsonl_str = r#"{"type": "user", "message": {"content": "Line instruction"}}"#;
        let lines = parse_and_extract_auth_ledger(jsonl_str).unwrap();
        assert_eq!(lines, vec!["user/text: Line instruction"]);
    }
}
