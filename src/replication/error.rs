//! Error types for cluster log-segment shipping replication (CER-2765, Phase 4.4).

use std::fmt;
use std::io;

/// Errors arising during replication export, ingest, checksum verification, or peer sync.
#[derive(Debug)]
pub enum ReplicationError {
    /// Database-related error.
    Database(rusqlite::Error),
    /// Filesystem or I/O error.
    Io(io::Error),
    /// JSON serialization or deserialization failure.
    Json(serde_json::Error),
    /// Schema or `:db/neverZeroValue` validation error.
    Validation(String),
    /// Fail-closed security tripwire canary violation.
    Tripwire(crate::tripwire::TripwireIntrusionError),
    /// Checksum verification failure.
    ChecksumMismatch { expected: String, actual: String },
    /// Peer not found in registry.
    PeerNotFound(String),
    /// Network or HTTP transport error.
    Network(String),
    /// Cortex settle error.
    Cortex(crate::cortex::CortexError),
    /// Invalid window range `(since_tx, until_tx]`.
    InvalidWindow { from_tx: u64, to_tx: u64 },
}

impl fmt::Display for ReplicationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(e) => write!(f, "database error: {e}"),
            Self::Io(e) => write!(f, "io error: {e}"),
            Self::Json(e) => write!(f, "json error: {e}"),
            Self::Validation(msg) => write!(f, "validation error: {msg}"),
            Self::Tripwire(e) => write!(f, "tripwire violation: {e}"),
            Self::ChecksumMismatch { expected, actual } => {
                write!(
                    f,
                    "checksum mismatch: expected `{expected}`, calculated `{actual}`"
                )
            }
            Self::PeerNotFound(id) => write!(f, "replication peer not found: `{id}`"),
            Self::Network(msg) => write!(f, "network error: {msg}"),
            Self::Cortex(e) => write!(f, "cortex error: {e}"),
            Self::InvalidWindow { from_tx, to_tx } => {
                write!(
                    f,
                    "invalid replication window: since_tx ({from_tx}) cannot exceed until_tx ({to_tx})"
                )
            }
        }
    }
}

impl std::error::Error for ReplicationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(e) => Some(e),
            Self::Io(e) => Some(e),
            Self::Json(e) => Some(e),
            Self::Tripwire(e) => Some(e),
            Self::Cortex(e) => Some(e),
            Self::Validation(_)
            | Self::ChecksumMismatch { .. }
            | Self::PeerNotFound(_)
            | Self::Network(_)
            | Self::InvalidWindow { .. } => None,
        }
    }
}

impl From<rusqlite::Error> for ReplicationError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Database(e)
    }
}

impl From<io::Error> for ReplicationError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<serde_json::Error> for ReplicationError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}

impl From<crate::tripwire::TripwireIntrusionError> for ReplicationError {
    fn from(e: crate::tripwire::TripwireIntrusionError) -> Self {
        Self::Tripwire(e)
    }
}

impl From<crate::cortex::CortexError> for ReplicationError {
    fn from(e: crate::cortex::CortexError) -> Self {
        Self::Cortex(e)
    }
}
