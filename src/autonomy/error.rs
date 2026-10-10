//! Autonomy ladder and promotion ledger error definitions.

use std::fmt;

/// Errors arising from autonomy ladder transitions, validation, and storage.
#[derive(Debug)]
pub enum AutonomyError {
    /// Low-level database error.
    Database(String),
    /// Violation of the Soma invariant (Soma production gates strictly require human operator verdicts).
    SomaInvariantViolation(String),
    /// Invalid trust ladder state transition (e.g. promoting downwards or demoting upwards).
    InvalidTransition {
        from: String,
        to: String,
        reason: String,
    },
    /// Input validation failure or :db/neverZeroValue violation.
    Validation(String),
    /// Violation of append-only ledger immutability.
    AppendOnlyViolation(String),
}

impl fmt::Display for AutonomyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(msg) => write!(f, "database error: {msg}"),
            Self::SomaInvariantViolation(msg) => {
                write!(f, "soma invariant violation: {msg}")
            }
            Self::InvalidTransition { from, to, reason } => {
                write!(
                    f,
                    "invalid autonomy transition from `{from}` to `{to}`: {reason}"
                )
            }
            Self::Validation(msg) => write!(f, "validation error: {msg}"),
            Self::AppendOnlyViolation(msg) => {
                write!(f, "append-only audit ledger violation: {msg}")
            }
        }
    }
}

impl std::error::Error for AutonomyError {}

impl From<rusqlite::Error> for AutonomyError {
    fn from(err: rusqlite::Error) -> Self {
        let msg = err.to_string();
        if msg.contains("autonomy_ledger is append-only") {
            Self::AppendOnlyViolation(msg)
        } else if msg.contains("CHECK constraint failed") && msg.contains("soma") {
            Self::SomaInvariantViolation(
                "Soma production gates strictly require human operator verdicts: autonomous tier is forbidden".to_string()
            )
        } else {
            Self::Database(msg)
        }
    }
}
