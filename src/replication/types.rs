//! Data transfer models and contracts for log-segment shipping replication (CER-2765, Phase 4.4).

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::replication::error::ReplicationError;

/// A single change entry in the replication changelog ledger.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplicationRecord {
    /// Monotonically increasing transaction ID within this node's changelog.
    pub tx_id: u64,
    /// Originating node identifier (e.g. `ceres`, `cygnus`).
    pub node_id: String,
    /// Type of entity changed (e.g. `bug_fact`, `cortex_settle`, `autonomy_event`).
    pub entity_type: String,
    /// Identifier of the entity changed.
    pub entity_id: String,
    /// Mutation action (e.g. `insert`, `update`, `delete`, `settle`).
    pub action: String,
    /// Dialect-neutral canonical JSON representation of the entity payload.
    pub payload_json: String,
    /// Multi-replica vector clock / frontier stamp at time of write.
    pub frontier: String,
    /// ISO 8601 UTC timestamp of creation.
    pub created_at: String,
}

impl ReplicationRecord {
    /// Enforces `:db/neverZeroValue` schema integrity.
    pub fn validate_never_zero_value(&self) -> Result<(), ReplicationError> {
        if self.tx_id == 0 {
            return Err(ReplicationError::Validation(
                ":db/neverZeroValue violation: tx_id must be greater than zero".to_string(),
            ));
        }
        if self.node_id.trim().is_empty() {
            return Err(ReplicationError::Validation(
                ":db/neverZeroValue violation: node_id is empty or whitespace".to_string(),
            ));
        }
        if self.entity_type.trim().is_empty() {
            return Err(ReplicationError::Validation(
                ":db/neverZeroValue violation: entity_type is empty or whitespace".to_string(),
            ));
        }
        if self.entity_id.trim().is_empty() {
            return Err(ReplicationError::Validation(
                ":db/neverZeroValue violation: entity_id is empty or whitespace".to_string(),
            ));
        }
        if self.action.trim().is_empty() {
            return Err(ReplicationError::Validation(
                ":db/neverZeroValue violation: action is empty or whitespace".to_string(),
            ));
        }
        if self.payload_json.trim().is_empty() {
            return Err(ReplicationError::Validation(
                ":db/neverZeroValue violation: payload_json is empty or whitespace".to_string(),
            ));
        }
        if self.frontier.trim().is_empty() {
            return Err(ReplicationError::Validation(
                ":db/neverZeroValue violation: frontier is empty or whitespace".to_string(),
            ));
        }
        if self.created_at.trim().is_empty() {
            return Err(ReplicationError::Validation(
                ":db/neverZeroValue violation: created_at is empty or whitespace".to_string(),
            ));
        }
        Ok(())
    }
}

/// A windowed log segment shipped across the network mesh.
/// Window interval semantics: `(from_tx, to_tx]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogSegment {
    /// Lower bound (exclusive): start transaction ID.
    pub from_tx: u64,
    /// Upper bound (inclusive): ending transaction ID.
    pub to_tx: u64,
    /// Originating node identifier.
    pub node_id: String,
    /// Ordered list of changelog entries in the window `(from_tx, to_tx]`.
    pub records: Vec<ReplicationRecord>,
    /// Latest local head transaction ID at time of export.
    pub head_tx: u64,
    /// Causality frontier vector clock string.
    pub frontier: String,
    /// Cryptographic SHA-256 hex checksum over canonical records sequence.
    pub checksum: String,
}

impl LogSegment {
    /// Validates `:db/neverZeroValue` schema integrity and window invariants.
    pub fn validate_never_zero_value(&self) -> Result<(), ReplicationError> {
        if self.node_id.trim().is_empty() {
            return Err(ReplicationError::Validation(
                ":db/neverZeroValue violation: node_id is empty or whitespace".to_string(),
            ));
        }
        if self.frontier.trim().is_empty() {
            return Err(ReplicationError::Validation(
                ":db/neverZeroValue violation: frontier is empty or whitespace".to_string(),
            ));
        }
        if self.checksum.trim().is_empty() {
            return Err(ReplicationError::Validation(
                ":db/neverZeroValue violation: checksum is empty or whitespace".to_string(),
            ));
        }
        if self.to_tx < self.from_tx {
            return Err(ReplicationError::InvalidWindow {
                from_tx: self.from_tx,
                to_tx: self.to_tx,
            });
        }
        for record in &self.records {
            record.validate_never_zero_value()?;
        }
        Ok(())
    }
}

/// Compute a cryptographic SHA-256 checksum over a sequence of replication records.
pub fn compute_records_checksum(records: &[ReplicationRecord]) -> String {
    let mut hasher = Sha256::new();
    for r in records {
        hasher.update(r.tx_id.to_be_bytes());
        hasher.update(r.node_id.as_bytes());
        hasher.update(r.entity_type.as_bytes());
        hasher.update(r.entity_id.as_bytes());
        hasher.update(r.action.as_bytes());
        hasher.update(r.payload_json.as_bytes());
        hasher.update(r.frontier.as_bytes());
    }
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()
}

/// Known replication peer node in the network mesh.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplicationPeer {
    /// Peer unique identifier (e.g. `cygnus`).
    pub peer_id: String,
    /// Base URL for HTTP/REST communication (e.g. `http://cygnus:8080`).
    pub peer_url: String,
    /// Highest transaction ID successfully shipped to this peer.
    pub last_shipped_tx: u64,
    /// Highest transaction ID successfully received and applied from this peer.
    pub last_applied_tx: u64,
    /// Timestamp of most recent synchronization pass.
    pub last_sync_at: String,
}

impl ReplicationPeer {
    /// Enforces `:db/neverZeroValue` schema integrity.
    pub fn validate_never_zero_value(&self) -> Result<(), ReplicationError> {
        if self.peer_id.trim().is_empty() {
            return Err(ReplicationError::Validation(
                ":db/neverZeroValue violation: peer_id is empty or whitespace".to_string(),
            ));
        }
        if self.peer_url.trim().is_empty() {
            return Err(ReplicationError::Validation(
                ":db/neverZeroValue violation: peer_url is empty or whitespace".to_string(),
            ));
        }
        if self.last_sync_at.trim().is_empty() {
            return Err(ReplicationError::Validation(
                ":db/neverZeroValue violation: last_sync_at is empty or whitespace".to_string(),
            ));
        }
        Ok(())
    }
}

/// Outcome of applying a replication log segment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyResult {
    /// Node identifier from which the segment was received.
    pub peer_id: String,
    /// Beginning window transaction ID.
    pub from_tx: u64,
    /// Ending window transaction ID.
    pub to_tx: u64,
    /// Total records in segment.
    pub records_received: usize,
    /// Count of records newly applied to local storage.
    pub records_applied: usize,
    /// Count of records skipped due to idempotency.
    pub duplicates_skipped: usize,
    /// Local head transaction ID after application.
    pub local_head_tx: u64,
    /// Status description.
    pub status: String,
}

/// Overall outcome of a bidirectional sync round.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncResult {
    /// Peer node identifier.
    pub peer_id: String,
    /// Target peer base URL.
    pub peer_url: String,
    /// Number of local records shipped to peer.
    pub shipped_records: usize,
    /// Number of remote records applied locally.
    pub applied_records: usize,
    /// Local head transaction ID.
    pub local_head_tx: u64,
    /// Remote head transaction ID reported by peer.
    pub remote_head_tx: u64,
    /// Sync status (`ok`, `up_to_date`, `partial`).
    pub status: String,
    /// Informational message.
    pub message: String,
}

/// Status report of local replication state and registered mesh peers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplicationStatus {
    /// This node's identifier.
    pub node_id: String,
    /// Current head transaction ID in `replication_log`.
    pub local_head_tx: u64,
    /// Total count of records in `replication_log`.
    pub total_log_records: u64,
    /// Registered peer watermarks.
    pub peers: Vec<ReplicationPeer>,
    /// Current local causality frontier.
    pub frontier: String,
}
