//! Cluster log-segment shipping replication (CER-2765, Phase 4.4).
//!
//! Provides multi-node regression database synchronization across the Tailscale mesh.
//! Features:
//! - Windowed `(SinceTx, UntilTx]` log segment export and ingest.
//! - Cryptographic SHA-256 integrity checksum verification.
//! - Idempotent, deduplicated replay into SQLite storage.
//! - Automatic changelog tracking in `replication_log` with `:db/neverZeroValue` validation.
//! - Peer replication watermark tracking in `replication_peers`.
//! - Immutability triggers prohibiting UPDATE and DELETE mutations on log entries.
//! - Fail-closed synthetic canary tripwire circuit-breaking.

pub mod engine;
pub mod error;
pub mod store;
pub mod types;

pub use engine::{apply_segment, export_segment, sync_peer};
pub use error::ReplicationError;
pub use store::{
    append_log_entry, get_head_tx, get_peer, get_replication_status, get_total_records, list_peers,
    local_node_id, query_window, update_peer_applied, update_peer_shipped, upsert_peer,
};
pub use types::{
    compute_records_checksum, ApplyResult, LogSegment, ReplicationPeer, ReplicationRecord,
    ReplicationStatus, SyncResult,
};
