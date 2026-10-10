//! Error types for Cortex settle learning loop integration (CER-2764, Phase 4.3).

use std::fmt;
use std::io;

/// Errors arising during Cortex settle outbox ingestion and deduplication.
#[derive(Debug)]
pub enum CortexError {
    /// SQLite or database-related error.
    Database(rusqlite::Error),
    /// Filesystem or I/O error when reading streams or writing defect facts.
    Io(io::Error),
    /// JSON serialization or deserialization failure.
    Json(serde_json::Error),
    /// Schema or `:db/neverZeroValue` validation error.
    Validation(String),
    /// Fail-closed security tripwire canary violation.
    Tripwire(crate::tripwire::TripwireIntrusionError),
    /// Settle record or fact not found.
    NotFound(String),
}

impl fmt::Display for CortexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(e) => write!(f, "database error: {e}"),
            Self::Io(e) => write!(f, "io error: {e}"),
            Self::Json(e) => write!(f, "json error: {e}"),
            Self::Validation(msg) => write!(f, "validation error: {msg}"),
            Self::Tripwire(e) => write!(f, "tripwire violation: {e}"),
            Self::NotFound(msg) => write!(f, "not found: {msg}"),
        }
    }
}

impl std::error::Error for CortexError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(e) => Some(e),
            Self::Io(e) => Some(e),
            Self::Json(e) => Some(e),
            Self::Tripwire(e) => Some(e),
            Self::Validation(_) | Self::NotFound(_) => None,
        }
    }
}

impl From<rusqlite::Error> for CortexError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Database(e)
    }
}

impl From<io::Error> for CortexError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<serde_json::Error> for CortexError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}

impl From<crate::tripwire::TripwireIntrusionError> for CortexError {
    fn from(e: crate::tripwire::TripwireIntrusionError) -> Self {
        Self::Tripwire(e)
    }
}
