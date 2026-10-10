//! Cortex settle outcome event definitions (CER-1827 / CER-2764, Phase 4.3).
//!
//! Models settle outcome events emitted from the Cortex outbox and pending_settles stream.
//! Distinguishes negative verdicts (proposals discarded or rejected by operator/evaluator)
//! to ingest them automatically as observed defect facts in `docs/sessions/observed/`.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

/// Settle action outcome classification according to Cortex domain (Spec 0024 & CER-1827).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SettleAction {
    /// Proposal accepted and applied without modification.
    Apply,
    /// Proposal rejected/discarded by operator (negative sample).
    Discard,
    /// Option selected from candidate set.
    Select,
    /// Settlement deferred or pending escalation.
    Defer,
}

impl SettleAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Apply => "apply",
            Self::Discard => "discard",
            Self::Select => "select",
            Self::Defer => "defer",
        }
    }
}

impl fmt::Display for SettleAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for SettleAction {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "apply" => Ok(Self::Apply),
            "discard" => Ok(Self::Discard),
            "select" => Ok(Self::Select),
            "defer" => Ok(Self::Defer),
            other => Err(format!(
                "invalid settle action `{other}`; expected apply, discard, select, or defer"
            )),
        }
    }
}

fn current_timestamp() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{now}")
}

fn generate_event_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("cortex-settle-{nanos:x}")
}

fn default_guard_tier() -> String {
    "tier1".to_string()
}

fn default_settle_action() -> String {
    "apply".to_string()
}

/// Settle outcome event emitted from Cortex outbox stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettleOutcomeEvent {
    /// Event identifier or unique UUID.
    #[serde(default = "generate_event_id")]
    pub id: String,
    /// Identifier of the underlying job or task.
    pub job_id: String,
    /// Originating channel or source (e.g. `slack`, `email`, `github`, `blackwall`).
    pub source: String,
    /// Proposed action type (e.g. `draft_reply`, `triage_issue`, `propose_patch`).
    pub action_type: String,
    /// Guard tier under which the proposal was generated (`tier1`, `tier2`, `tier3`).
    #[serde(default = "default_guard_tier")]
    pub guard_tier: String,
    /// Guard decision (`allow`, `deny`, `escalate`).
    #[serde(default)]
    pub guard_decision: Option<String>,
    /// Operator decision (`apply`, `discard`, `defer`, etc.).
    pub operator_decision: String,
    /// Settle action classification (`apply`, `discard`, `select`, `defer`).
    #[serde(default = "default_settle_action")]
    pub settle_action: String,
    /// Summary of the worker proposal.
    pub proposal_summary: String,
    /// Rejection reason provided by the operator or evaluator.
    #[serde(default)]
    pub rejection_reason: Option<String>,
    /// Operator edit delta if modified before applying.
    #[serde(default)]
    pub edit_delta: Option<String>,
    /// Files or code surfaces touched by the proposed action.
    #[serde(default)]
    pub files: Vec<String>,
    /// Optional roll-up meta-pattern classification.
    #[serde(default)]
    pub meta_pattern: Option<String>,
    /// ISO-8601 or unix creation timestamp.
    #[serde(default = "current_timestamp")]
    pub created_at: String,
    /// Explicit idempotency key; if omitted, defaults to `{job_id}:{settle_action}`.
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

impl SettleOutcomeEvent {
    /// Determines whether this settle outcome represents a negative verdict
    /// (e.g. operator discard or guard deny upheld).
    pub fn is_negative_verdict(&self) -> bool {
        let action = self.settle_action.trim().to_lowercase();
        let op_dec = self.operator_decision.trim().to_lowercase();
        let guard_dec = self
            .guard_decision
            .as_deref()
            .unwrap_or("")
            .trim()
            .to_lowercase();

        action == "discard"
            || op_dec == "discard"
            || (guard_dec == "deny" && (action == "discard" || op_dec == "discard"))
    }

    /// Computes the unique deduplication key for this event.
    /// Uses `idempotency_key` if non-empty; otherwise `{job_id}:{settle_action}`.
    pub fn dedup_key(&self) -> String {
        if let Some(key) = &self.idempotency_key {
            let trimmed = key.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
        format!(
            "{}:{}",
            self.job_id.trim(),
            self.settle_action.trim().to_lowercase()
        )
    }

    /// Parse a `SettleOutcomeEvent` from a JSON `serde_json::Value`, supporting both direct
    /// serialization, outbox envelope payloads (`{"payload": {...}}`), and reverie body payloads.
    pub fn parse_from_value(val: &serde_json::Value) -> Result<Self, String> {
        let obj = if let Some(payload) = val.get("payload").and_then(|p| p.as_object()) {
            serde_json::Value::Object(payload.clone())
        } else if let Some(body) = val.get("body") {
            if let Some(body_str) = body.as_str() {
                serde_json::from_str(body_str)
                    .map_err(|e| format!("failed to parse body JSON string: {e}"))?
            } else if body.is_object() {
                body.clone()
            } else {
                val.clone()
            }
        } else {
            val.clone()
        };

        // If direct deserialization works, use it
        if let Ok(mut event) = serde_json::from_value::<SettleOutcomeEvent>(obj.clone()) {
            event.normalize_aliases(&obj);
            event.validate_integrity()?;
            return Ok(event);
        }

        // Otherwise extract and normalize flexible fields
        let job_id = obj
            .get("job_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing required field `job_id`".to_string())?
            .to_string();

        let source = obj
            .get("source")
            .and_then(|v| v.as_str())
            .unwrap_or("cortex")
            .to_string();

        let action_type = obj
            .get("action_type")
            .and_then(|v| v.as_str())
            .unwrap_or("settle")
            .to_string();

        let guard_tier = obj
            .get("guard_tier")
            .and_then(|v| v.as_str())
            .unwrap_or("tier1")
            .to_string();

        let guard_decision = obj
            .get("guard_decision")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let operator_decision = obj
            .get("operator_decision")
            .or_else(|| obj.get("decision"))
            .or_else(|| obj.get("outcome"))
            .and_then(|v| v.as_str())
            .unwrap_or("discard")
            .to_string();

        let settle_action = obj
            .get("settle_action")
            .or_else(|| obj.get("outcome"))
            .or_else(|| obj.get("action"))
            .and_then(|v| v.as_str())
            .unwrap_or(&operator_decision)
            .to_string();

        let proposal_summary = obj
            .get("proposal_summary")
            .or_else(|| obj.get("summary"))
            .or_else(|| obj.get("proposal"))
            .and_then(|v| v.as_str())
            .unwrap_or("No proposal summary provided")
            .to_string();

        let rejection_reason = obj
            .get("rejection_reason")
            .or_else(|| obj.get("reason"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let edit_delta = obj
            .get("edit_delta")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let mut files = Vec::new();
        if let Some(arr) = obj.get("files").and_then(|v| v.as_array()) {
            for item in arr {
                if let Some(s) = item.as_str() {
                    let trimmed = s.trim();
                    if !trimmed.is_empty() {
                        files.push(trimmed.to_string());
                    }
                }
            }
        }

        let meta_pattern = obj
            .get("meta_pattern")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let id = obj
            .get("id")
            .or_else(|| obj.get("event_id"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(generate_event_id);

        let created_at = obj
            .get("created_at")
            .or_else(|| obj.get("timestamp"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(current_timestamp);

        let idempotency_key = obj
            .get("idempotency_key")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let event = SettleOutcomeEvent {
            id,
            job_id,
            source,
            action_type,
            guard_tier,
            guard_decision,
            operator_decision,
            settle_action,
            proposal_summary,
            rejection_reason,
            edit_delta,
            files,
            meta_pattern,
            created_at,
            idempotency_key,
        };

        event.validate_integrity()?;
        Ok(event)
    }

    fn normalize_aliases(&mut self, obj: &serde_json::Value) {
        if self.rejection_reason.is_none() {
            if let Some(r) = obj.get("reason").and_then(|v| v.as_str()) {
                self.rejection_reason = Some(r.to_string());
            }
        }
        if self.settle_action.is_empty() || self.settle_action == "apply" {
            if let Some(o) = obj.get("outcome").and_then(|v| v.as_str()) {
                self.settle_action = o.to_string();
            }
        }
    }

    /// Enforces `:db/neverZeroValue` schema integrity on mandatory fields.
    pub fn validate_integrity(&self) -> Result<(), String> {
        if self.id.trim().is_empty() {
            return Err(":db/neverZeroValue violation: field `id` is empty".to_string());
        }
        if self.job_id.trim().is_empty() {
            return Err(":db/neverZeroValue violation: field `job_id` is empty".to_string());
        }
        if self.source.trim().is_empty() {
            return Err(":db/neverZeroValue violation: field `source` is empty".to_string());
        }
        if self.action_type.trim().is_empty() {
            return Err(":db/neverZeroValue violation: field `action_type` is empty".to_string());
        }
        if self.operator_decision.trim().is_empty() {
            return Err(
                ":db/neverZeroValue violation: field `operator_decision` is empty".to_string(),
            );
        }
        if self.settle_action.trim().is_empty() {
            return Err(":db/neverZeroValue violation: field `settle_action` is empty".to_string());
        }
        if self.proposal_summary.trim().is_empty() {
            return Err(
                ":db/neverZeroValue violation: field `proposal_summary` is empty".to_string(),
            );
        }
        Ok(())
    }

    /// Generates canonical session defect fact markdown conforming to `docs/sessions/_SCHEMA.md`.
    /// Returns `(slug, markdown_text)`.
    pub fn to_session_fact_markdown(&self) -> (String, String) {
        let clean_source = sanitize_slug(&self.source);
        let clean_job = sanitize_slug(&self.job_id);
        let slug = format!("cortex-settle-{}-{}", clean_source, clean_job);
        let h1_title = format!("FACT_{}", slug.to_uppercase().replace('-', "_"));

        let mut lines = Vec::new();
        lines.push(format!("# {h1_title}"));
        lines.push(String::new());
        lines.push(format!("- **id:** fact:{slug}"));
        if !self.files.is_empty() {
            lines.push(format!("- **files:** {}", self.files.join(", ")));
        }
        let pattern = self
            .meta_pattern
            .as_deref()
            .unwrap_or("Negative worker settlement: operator discarded proposal");
        lines.push(format!("- **meta-pattern:** {pattern}"));
        lines.push("- **status:** observed".to_string());
        if !self.files.is_empty() {
            let scope = derive_scope(&self.files);
            lines.push(format!("- **scope:** {scope}"));
        } else {
            lines.push(format!("- **scope:** cortex/{}", clean_source));
        }
        lines.push(String::new());
        lines.push("## Symptom".to_string());
        lines.push(format!(
            "Operator discarded worker proposal for job `{}` from source `{}` (action type: `{}`).",
            self.job_id, self.source, self.action_type
        ));
        lines.push(format!("Proposal summary: {}", self.proposal_summary));
        if let Some(reason) = &self.rejection_reason {
            if !reason.trim().is_empty() {
                lines.push(format!("Rejection reason: {reason}"));
            }
        }
        lines.push(String::new());
        lines.push("## Root cause".to_string());
        lines.push(format!(
            "Worker generated an invalid proposal under guard tier `{}` (guard decision: {}). \
             Operator settlement rejected the proposed action.",
            self.guard_tier,
            self.guard_decision.as_deref().unwrap_or("none")
        ));
        lines.push(
            "Mental-model error: Worker emitted an ungrounded or non-compliant proposal without validating domain invariants."
                .to_string(),
        );
        lines.push(String::new());
        lines.push("## Reproduction".to_string());
        lines.push(format!(
            "Cortex settle outcome event `{}` for job `{}`.\nChannel/Source: `{}`.\nAction type: `{}`.",
            self.id, self.job_id, self.source, self.action_type
        ));
        lines.push(String::new());
        lines.push("## Resolution".to_string());
        lines.push(
            "Draft observed defect fact recorded. Pending root cause analysis, prompt/policy hardening, or regression test addition."
                .to_string(),
        );
        lines.push(String::new());
        lines.push("## Lesson".to_string());
        lines.push(format!(
            "Validate domain invariants for `{}` proposals prior to operator settlement to prevent repeated rejections.",
            self.action_type
        ));
        lines.push(String::new());

        (slug, lines.join("\n"))
    }
}

/// Sanitize text for a filesystem and markdown friendly slug.
pub fn sanitize_slug(s: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for c in s.chars() {
        if c.is_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

/// Derive directory blast-radius scope from a list of files.
pub fn derive_scope(files: &[String]) -> String {
    if files.is_empty() {
        return "cortex".to_string();
    }
    let first = &files[0];
    let path = first.split(':').next().unwrap_or(first);
    match path.rfind('/') {
        Some(i) => path[..i].to_string(),
        None => path.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_negative_verdict_detection() {
        let mut evt = SettleOutcomeEvent {
            id: "evt-1".into(),
            job_id: "job-101".into(),
            source: "slack".into(),
            action_type: "draft_reply".into(),
            guard_tier: "tier1".into(),
            guard_decision: Some("allow".into()),
            operator_decision: "discard".into(),
            settle_action: "discard".into(),
            proposal_summary: "Bad proposal".into(),
            rejection_reason: Some("Factually wrong".into()),
            edit_delta: None,
            files: vec!["crates/worker/reply.rs".into()],
            meta_pattern: None,
            created_at: "1000".into(),
            idempotency_key: None,
        };
        assert!(evt.is_negative_verdict());
        assert_eq!(evt.dedup_key(), "job-101:discard");

        evt.operator_decision = "apply".into();
        evt.settle_action = "apply".into();
        assert!(!evt.is_negative_verdict());
    }

    #[test]
    fn test_to_session_fact_markdown_adheres_to_schema() {
        let evt = SettleOutcomeEvent {
            id: "evt-99".into(),
            job_id: "cfx-42".into(),
            source: "github".into(),
            action_type: "triage_issue".into(),
            guard_tier: "tier2".into(),
            guard_decision: Some("deny".into()),
            operator_decision: "discard".into(),
            settle_action: "discard".into(),
            proposal_summary: "Misrouted ticket to wrong team".into(),
            rejection_reason: Some("Team CER does not own docs".into()),
            edit_delta: None,
            files: vec!["src/triage.rs".into()],
            meta_pattern: Some("Misrouting issue".into()),
            created_at: "1000".into(),
            idempotency_key: None,
        };

        let (slug, md) = evt.to_session_fact_markdown();
        assert_eq!(slug, "cortex-settle-github-cfx-42");
        assert!(md.contains("# FACT_CORTEX_SETTLE_GITHUB_CFX_42"));
        assert!(md.contains("- **id:** fact:cortex-settle-github-cfx-42"));
        assert!(md.contains("- **status:** observed"));
        assert!(md.contains("## Symptom"));
        assert!(md.contains("## Root cause"));
        assert!(md.contains("## Reproduction"));
        assert!(md.contains("## Resolution"));
        assert!(md.contains("## Lesson"));
    }

    #[test]
    fn test_parse_from_outbox_payload() {
        let json = serde_json::json!({
            "to_role": "worker",
            "payload": {
                "job_id": "job-55",
                "source": "email",
                "action_type": "send_email",
                "outcome": "discard",
                "operator_decision": "discard",
                "summary": "Phishing proposal",
                "reason": "Violated security gate"
            }
        });

        let event = SettleOutcomeEvent::parse_from_value(&json).expect("parse");
        assert_eq!(event.job_id, "job-55");
        assert_eq!(event.source, "email");
        assert_eq!(event.settle_action, "discard");
        assert_eq!(
            event.rejection_reason,
            Some("Violated security gate".into())
        );
        assert!(event.is_negative_verdict());
    }
}
