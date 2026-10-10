//! Durable signal architecture and review gates (CER-2757, Phase 2.2).
//!
//! Provides `Signal<OperatorVerdict>` and `DurableSignal<T>` primitives with
//! persistent deduplication on `idempotency_key` and schema integrity enforcement
//! under the `:db/neverZeroValue` invariant.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Decision recorded by an operator during review gating.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OperatorDecision {
    /// The change or regression reproducer is approved.
    Approved,
    /// The change or regression reproducer is rejected.
    Rejected,
    /// Changes or additional evidence are requested before approval.
    ChangesRequested,
}

impl FromStr for OperatorDecision {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "approved" | "approve" => Ok(Self::Approved),
            "rejected" | "reject" => Ok(Self::Rejected),
            "changes_requested" | "changes-requested" | "request_changes" | "changes" => {
                Ok(Self::ChangesRequested)
            }
            other => Err(format!(
                "invalid operator decision `{other}`, expected: approved, rejected, or changes_requested"
            )),
        }
    }
}

impl fmt::Display for OperatorDecision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Approved => write!(f, "approved"),
            Self::Rejected => write!(f, "rejected"),
            Self::ChangesRequested => write!(f, "changes_requested"),
        }
    }
}

/// Human operator verdict payload for review gates.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OperatorVerdict {
    /// Decision reached by operator.
    pub decision: OperatorDecision,
    /// Operator identifier or handle (must not be empty or whitespace).
    pub operator: String,
    /// Optional operator rationale or comments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comments: Option<String>,
    /// ISO-8601 UTC timestamp of verdict.
    pub timestamp: String,
}

/// Compute current UTC RFC-3339 timestamp from system time without third-party crates.
fn current_utc_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (secs / 86_400) as i64;
    let day_secs = secs % 86_400;
    let h = day_secs / 3600;
    let m = (day_secs % 3600) / 60;
    let s = day_secs % 60;

    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    format!("{year:04}-{month:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

impl OperatorVerdict {
    /// Construct a new `OperatorVerdict` with the current UTC timestamp.
    pub fn new(
        decision: OperatorDecision,
        operator: impl Into<String>,
        comments: Option<String>,
    ) -> Result<Self, String> {
        let op = operator.into();
        let ts = current_utc_rfc3339();
        let verdict = Self {
            decision,
            operator: op,
            comments,
            timestamp: ts,
        };
        verdict.validate()?;
        Ok(verdict)
    }

    /// Validate against the `:db/neverZeroValue` schema integrity invariant.
    pub fn validate(&self) -> Result<(), String> {
        if self.operator.trim().is_empty() {
            return Err(
                ":db/neverZeroValue violation: operator cannot be empty or whitespace".to_string(),
            );
        }
        if self.timestamp.trim().is_empty() {
            return Err(
                ":db/neverZeroValue violation: timestamp cannot be empty or whitespace".to_string(),
            );
        }
        if let Some(ref c) = self.comments {
            if c.trim().is_empty() {
                return Err(
                    ":db/neverZeroValue violation: comments cannot be empty or whitespace if present"
                        .to_string(),
                );
            }
        }
        Ok(())
    }
}

/// Durable signal carrying typed payload and a unique deduplication key.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DurableSignal<T> {
    /// Canonical signal name matching workflow `wait_for_signal(name)`.
    pub signal_name: String,
    /// Unique idempotency key for deduplicating deliveries across retries.
    pub idempotency_key: String,
    /// Typed payload.
    pub payload: T,
}

/// Type alias aligning with `Signal<OperatorVerdict>` specification.
pub type Signal<T> = DurableSignal<T>;

impl<T> DurableSignal<T> {
    /// Construct a new `DurableSignal` validating non-empty names and keys.
    pub fn new(
        signal_name: impl Into<String>,
        idempotency_key: impl Into<String>,
        payload: T,
    ) -> Result<Self, String> {
        let name = signal_name.into();
        let key = idempotency_key.into();
        if name.trim().is_empty() {
            return Err(
                ":db/neverZeroValue violation: signal_name cannot be empty or whitespace"
                    .to_string(),
            );
        }
        if key.trim().is_empty() {
            return Err(
                ":db/neverZeroValue violation: idempotency_key cannot be empty or whitespace"
                    .to_string(),
            );
        }
        Ok(Self {
            signal_name: name,
            idempotency_key: key,
            payload,
        })
    }

    /// Validate signal metadata against `:db/neverZeroValue` constraints.
    pub fn validate(&self) -> Result<(), String> {
        if self.signal_name.trim().is_empty() {
            return Err(
                ":db/neverZeroValue violation: signal_name cannot be empty or whitespace"
                    .to_string(),
            );
        }
        if self.idempotency_key.trim().is_empty() {
            return Err(
                ":db/neverZeroValue violation: idempotency_key cannot be empty or whitespace"
                    .to_string(),
            );
        }
        Ok(())
    }
}

impl DurableSignal<OperatorVerdict> {
    /// Validate both signal metadata and operator verdict payload.
    pub fn validate_verdict(&self) -> Result<(), String> {
        self.validate()?;
        self.payload.validate()?;
        Ok(())
    }
}

/// Result status of delivering a durable signal.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum SignalDeliveryStatus {
    /// Signal was delivered and accepted by the workflow runtime.
    Delivered,
    /// Signal was previously delivered with the same idempotency key and deduplicated.
    Duplicate,
}

impl fmt::Display for SignalDeliveryStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Delivered => write!(f, "DELIVERED"),
            Self::Duplicate => write!(f, "DUPLICATE"),
        }
    }
}

/// Persistent record of a signal stored in the SQLite deduplication ledger.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredSignalRecord {
    /// Workflow execution identifier.
    pub exec_id: String,
    /// Canonical signal name.
    pub signal_name: String,
    /// Idempotency deduplication key.
    pub idempotency_key: String,
    /// Serialized JSON payload.
    pub payload_json: String,
    /// Unix epoch millisecond arrival timestamp.
    pub received_at: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_operator_decision_parsing() {
        assert_eq!(
            "approved".parse::<OperatorDecision>().unwrap(),
            OperatorDecision::Approved
        );
        assert_eq!(
            "approve".parse::<OperatorDecision>().unwrap(),
            OperatorDecision::Approved
        );
        assert_eq!(
            "rejected".parse::<OperatorDecision>().unwrap(),
            OperatorDecision::Rejected
        );
        assert_eq!(
            "reject".parse::<OperatorDecision>().unwrap(),
            OperatorDecision::Rejected
        );
        assert_eq!(
            "changes_requested".parse::<OperatorDecision>().unwrap(),
            OperatorDecision::ChangesRequested
        );
        assert_eq!(
            "changes".parse::<OperatorDecision>().unwrap(),
            OperatorDecision::ChangesRequested
        );
        assert!("invalid".parse::<OperatorDecision>().is_err());
    }

    #[test]
    fn test_operator_verdict_never_zero_value_validation() {
        // Valid verdict
        let valid = OperatorVerdict::new(
            OperatorDecision::Approved,
            "ctodie",
            Some("LGTM verified fix".to_string()),
        )
        .expect("valid verdict");
        assert!(valid.validate().is_ok());

        // Empty operator rejected
        let empty_op = OperatorVerdict {
            decision: OperatorDecision::Approved,
            operator: "   ".to_string(),
            comments: None,
            timestamp: "2026-10-10T00:00:00Z".to_string(),
        };
        assert!(empty_op.validate().is_err());

        // Empty timestamp rejected
        let empty_ts = OperatorVerdict {
            decision: OperatorDecision::Approved,
            operator: "ctodie".to_string(),
            comments: None,
            timestamp: "".to_string(),
        };
        assert!(empty_ts.validate().is_err());

        // Empty comment string rejected
        let empty_comm = OperatorVerdict {
            decision: OperatorDecision::Approved,
            operator: "ctodie".to_string(),
            comments: Some("  \t".to_string()),
            timestamp: "2026-10-10T00:00:00Z".to_string(),
        };
        assert!(empty_comm.validate().is_err());
    }

    #[test]
    fn test_durable_signal_validation() {
        let verdict = OperatorVerdict::new(OperatorDecision::Approved, "ctodie", None).unwrap();

        // Valid signal
        let sig = DurableSignal::new("operator_verdict", "idem-123", verdict.clone()).unwrap();
        assert!(sig.validate_verdict().is_ok());

        // Empty signal name rejected
        assert!(DurableSignal::new("  ", "idem-123", verdict.clone()).is_err());

        // Empty idempotency key rejected
        assert!(DurableSignal::new("operator_verdict", "\t", verdict).is_err());
    }

    #[test]
    fn test_signal_serialization_roundtrip() {
        let verdict = OperatorVerdict::new(
            OperatorDecision::ChangesRequested,
            "ctodie",
            Some("need reproducer test".to_string()),
        )
        .unwrap();

        let sig = DurableSignal::new("operator_verdict", "key_abc_456", verdict).unwrap();
        let json = serde_json::to_string(&sig).expect("serialize");
        let parsed: DurableSignal<OperatorVerdict> =
            serde_json::from_str(&json).expect("deserialize");
        assert_eq!(sig, parsed);
    }
}
