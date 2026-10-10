-- Schema migration 0005: Cluster log-segment shipping replication (CER-2765, Phase 4.4).
-- Lineage: wbrown/janus-datalog & autumn-harvest-sqlite.
-- Enforces :db/neverZeroValue schema integrity, append-only changelog ledger,
-- immutability triggers, and peer replication watermark tracking.

CREATE TABLE IF NOT EXISTS replication_log (
    tx_id INTEGER PRIMARY KEY AUTOINCREMENT,
    node_id TEXT NOT NULL,
    entity_type TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    action TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    frontier TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK (length(trim(node_id)) > 0),
    CHECK (length(trim(entity_type)) > 0),
    CHECK (length(trim(entity_id)) > 0),
    CHECK (length(trim(action)) > 0),
    CHECK (length(trim(payload_json)) > 0),
    CHECK (length(trim(frontier)) > 0)
);

CREATE INDEX IF NOT EXISTS idx_replication_log_tx ON replication_log(tx_id);
CREATE INDEX IF NOT EXISTS idx_replication_log_entity ON replication_log(entity_type, entity_id);
CREATE INDEX IF NOT EXISTS idx_replication_log_node_tx ON replication_log(node_id, tx_id);

-- Immutability enforcement: prevent updates to replication changelog
CREATE TRIGGER IF NOT EXISTS prevent_replication_log_update
BEFORE UPDATE ON replication_log
BEGIN
    SELECT RAISE(ABORT, 'replication_log is append-only: updates are prohibited');
END;

-- Immutability enforcement: prevent deletes from replication changelog
CREATE TRIGGER IF NOT EXISTS prevent_replication_log_delete
BEFORE DELETE ON replication_log
BEGIN
    SELECT RAISE(ABORT, 'replication_log is append-only: deletions are prohibited');
END;

CREATE TABLE IF NOT EXISTS replication_peers (
    peer_id TEXT PRIMARY KEY NOT NULL,
    peer_url TEXT NOT NULL,
    last_shipped_tx INTEGER NOT NULL DEFAULT 0,
    last_applied_tx INTEGER NOT NULL DEFAULT 0,
    last_sync_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK (length(trim(peer_id)) > 0),
    CHECK (length(trim(peer_url)) > 0),
    CHECK (last_shipped_tx >= 0),
    CHECK (last_applied_tx >= 0)
);

CREATE INDEX IF NOT EXISTS idx_replication_peers_last_sync ON replication_peers(last_sync_at);
