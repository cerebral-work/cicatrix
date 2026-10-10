//! SQLite storage integration for replication log and peer watermark management (CER-2765, Phase 4.4).

use rusqlite::{params, Connection};

use crate::replication::error::ReplicationError;
use crate::replication::types::{ReplicationPeer, ReplicationRecord, ReplicationStatus};

/// Retrieve local node identity from environment variable `CICATRIX_NODE_ID` or default to `ceres`.
pub fn local_node_id() -> String {
    if let Ok(id) = std::env::var("CICATRIX_NODE_ID") {
        let trimmed = id.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    "ceres".to_string()
}

/// Append a new mutation record into the immutable `replication_log` changelog table.
/// Returns the allocated auto-incrementing `tx_id`.
pub fn append_log_entry(
    conn: &Connection,
    node_id: &str,
    entity_type: &str,
    entity_id: &str,
    action: &str,
    payload_json: &str,
    frontier: &str,
) -> Result<u64, ReplicationError> {
    if node_id.trim().is_empty() {
        return Err(ReplicationError::Validation(
            ":db/neverZeroValue violation: node_id is empty".to_string(),
        ));
    }
    if entity_type.trim().is_empty() {
        return Err(ReplicationError::Validation(
            ":db/neverZeroValue violation: entity_type is empty".to_string(),
        ));
    }
    if entity_id.trim().is_empty() {
        return Err(ReplicationError::Validation(
            ":db/neverZeroValue violation: entity_id is empty".to_string(),
        ));
    }
    if action.trim().is_empty() {
        return Err(ReplicationError::Validation(
            ":db/neverZeroValue violation: action is empty".to_string(),
        ));
    }
    if payload_json.trim().is_empty() {
        return Err(ReplicationError::Validation(
            ":db/neverZeroValue violation: payload_json is empty".to_string(),
        ));
    }
    if frontier.trim().is_empty() {
        return Err(ReplicationError::Validation(
            ":db/neverZeroValue violation: frontier is empty".to_string(),
        ));
    }

    conn.execute(
        "INSERT INTO replication_log (node_id, entity_type, entity_id, action, payload_json, frontier)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6);",
        params![node_id, entity_type, entity_id, action, payload_json, frontier],
    )?;

    let tx_id = conn.last_insert_rowid() as u64;
    Ok(tx_id)
}

/// Query entries from `replication_log` within the window `(since_tx, until_tx]`.
pub fn query_window(
    conn: &Connection,
    since_tx: u64,
    until_tx: u64,
    limit: Option<u64>,
) -> Result<Vec<ReplicationRecord>, ReplicationError> {
    let limit_val = limit.unwrap_or(1000) as i64;
    let mut stmt = conn.prepare(
        "SELECT tx_id, node_id, entity_type, entity_id, action, payload_json, frontier, created_at
         FROM replication_log
         WHERE tx_id > ?1 AND tx_id <= ?2
         ORDER BY tx_id ASC
         LIMIT ?3;",
    )?;

    let rows = stmt.query_map(
        params![since_tx as i64, until_tx as i64, limit_val],
        |row| {
            let tx_i64: i64 = row.get(0)?;
            Ok(ReplicationRecord {
                tx_id: tx_i64 as u64,
                node_id: row.get(1)?,
                entity_type: row.get(2)?,
                entity_id: row.get(3)?,
                action: row.get(4)?,
                payload_json: row.get(5)?,
                frontier: row.get(6)?,
                created_at: row.get(7)?,
            })
        },
    )?;

    let mut records = Vec::new();
    for r in rows {
        records.push(r?);
    }
    Ok(records)
}

/// Retrieve the highest transaction ID currently stored in `replication_log`.
pub fn get_head_tx(conn: &Connection) -> Result<u64, ReplicationError> {
    let mut stmt = conn.prepare("SELECT COALESCE(MAX(tx_id), 0) FROM replication_log;")?;
    let mut rows = stmt.query([])?;
    if let Some(row) = rows.next()? {
        let val: i64 = row.get(0)?;
        Ok(val.max(0) as u64)
    } else {
        Ok(0)
    }
}

/// Retrieve the total number of records in `replication_log`.
pub fn get_total_records(conn: &Connection) -> Result<u64, ReplicationError> {
    let mut stmt = conn.prepare("SELECT COUNT(*) FROM replication_log;")?;
    let mut rows = stmt.query([])?;
    if let Some(row) = rows.next()? {
        let val: i64 = row.get(0)?;
        Ok(val.max(0) as u64)
    } else {
        Ok(0)
    }
}

/// Retrieve a specific peer record by its ID.
pub fn get_peer(
    conn: &Connection,
    peer_id: &str,
) -> Result<Option<ReplicationPeer>, ReplicationError> {
    let mut stmt = conn.prepare(
        "SELECT peer_id, peer_url, last_shipped_tx, last_applied_tx, last_sync_at
         FROM replication_peers
         WHERE peer_id = ?1;",
    )?;

    let mut rows = stmt.query(params![peer_id])?;
    if let Some(row) = rows.next()? {
        let shipped: i64 = row.get(2)?;
        let applied: i64 = row.get(3)?;
        Ok(Some(ReplicationPeer {
            peer_id: row.get(0)?,
            peer_url: row.get(1)?,
            last_shipped_tx: shipped.max(0) as u64,
            last_applied_tx: applied.max(0) as u64,
            last_sync_at: row.get(4)?,
        }))
    } else {
        Ok(None)
    }
}

/// List all registered replication peers.
pub fn list_peers(conn: &Connection) -> Result<Vec<ReplicationPeer>, ReplicationError> {
    let mut stmt = conn.prepare(
        "SELECT peer_id, peer_url, last_shipped_tx, last_applied_tx, last_sync_at
         FROM replication_peers
         ORDER BY peer_id ASC;",
    )?;

    let rows = stmt.query_map([], |row| {
        let shipped: i64 = row.get(2)?;
        let applied: i64 = row.get(3)?;
        Ok(ReplicationPeer {
            peer_id: row.get(0)?,
            peer_url: row.get(1)?,
            last_shipped_tx: shipped.max(0) as u64,
            last_applied_tx: applied.max(0) as u64,
            last_sync_at: row.get(4)?,
        })
    })?;

    let mut peers = Vec::new();
    for r in rows {
        peers.push(r?);
    }
    Ok(peers)
}

/// Register or update a replication peer entry.
pub fn upsert_peer(conn: &Connection, peer: &ReplicationPeer) -> Result<(), ReplicationError> {
    peer.validate_never_zero_value()?;
    conn.execute(
        "INSERT INTO replication_peers (peer_id, peer_url, last_shipped_tx, last_applied_tx, last_sync_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(peer_id) DO UPDATE SET
            peer_url = excluded.peer_url,
            last_shipped_tx = MAX(replication_peers.last_shipped_tx, excluded.last_shipped_tx),
            last_applied_tx = MAX(replication_peers.last_applied_tx, excluded.last_applied_tx),
            last_sync_at = excluded.last_sync_at;",
        params![
            &peer.peer_id,
            &peer.peer_url,
            peer.last_shipped_tx as i64,
            peer.last_applied_tx as i64,
            &peer.last_sync_at,
        ],
    )?;
    Ok(())
}

/// Update peer last shipped transaction watermark.
pub fn update_peer_shipped(
    conn: &Connection,
    peer_id: &str,
    last_shipped_tx: u64,
) -> Result<(), ReplicationError> {
    conn.execute(
        "UPDATE replication_peers
         SET last_shipped_tx = MAX(last_shipped_tx, ?2),
             last_sync_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE peer_id = ?1;",
        params![peer_id, last_shipped_tx as i64],
    )?;
    Ok(())
}

/// Update peer last applied transaction watermark.
pub fn update_peer_applied(
    conn: &Connection,
    peer_id: &str,
    last_applied_tx: u64,
) -> Result<(), ReplicationError> {
    conn.execute(
        "UPDATE replication_peers
         SET last_applied_tx = MAX(last_applied_tx, ?2),
             last_sync_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE peer_id = ?1;",
        params![peer_id, last_applied_tx as i64],
    )?;
    Ok(())
}

/// Aggregate local replication status and registered peers.
pub fn get_replication_status(
    conn: &Connection,
    peer_filter: Option<&str>,
) -> Result<ReplicationStatus, ReplicationError> {
    let node_id = local_node_id();
    let local_head_tx = get_head_tx(conn)?;
    let total_log_records = get_total_records(conn)?;

    let peers = if let Some(pid) = peer_filter {
        match get_peer(conn, pid)? {
            Some(p) => vec![p],
            None => Vec::new(),
        }
    } else {
        list_peers(conn)?
    };

    let effective_head = local_head_tx.max(1);
    let frontier = format!("{node_id}:{effective_head}");

    Ok(ReplicationStatus {
        node_id,
        local_head_tx,
        total_log_records,
        peers,
        frontier,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::SqliteStore;

    #[test]
    fn test_replication_store_append_and_query_window() {
        let store = SqliteStore::open_in_memory().expect("in-memory db should open");
        let conn = store.conn();

        let tx1 = append_log_entry(
            conn,
            "ceres",
            "bug_fact",
            "fact-1",
            "record",
            r#"{"id":"fact-1"}"#,
            "ceres:1",
        )
        .expect("append tx1");
        assert_eq!(tx1, 1);

        let tx2 = append_log_entry(
            conn,
            "ceres",
            "bug_fact",
            "fact-2",
            "record",
            r#"{"id":"fact-2"}"#,
            "ceres:2",
        )
        .expect("append tx2");
        assert_eq!(tx2, 2);

        let tx3 = append_log_entry(
            conn,
            "ceres",
            "cortex_settle",
            "settle-1",
            "record",
            r#"{"id":"settle-1"}"#,
            "ceres:3",
        )
        .expect("append tx3");
        assert_eq!(tx3, 3);

        assert_eq!(get_head_tx(conn).expect("head"), 3);
        assert_eq!(get_total_records(conn).expect("total"), 3);

        let w1 = query_window(conn, 0, 2, None).expect("window 0..2");
        assert_eq!(w1.len(), 2);
        assert_eq!(w1[0].tx_id, 1);
        assert_eq!(w1[0].entity_id, "fact-1");
        assert_eq!(w1[1].tx_id, 2);
        assert_eq!(w1[1].entity_id, "fact-2");

        let w2 = query_window(conn, 2, 3, None).expect("window 2..3");
        assert_eq!(w2.len(), 1);
        assert_eq!(w2[0].tx_id, 3);
        assert_eq!(w2[0].entity_id, "settle-1");

        let w3 = query_window(conn, 3, 3, None).expect("empty window");
        assert!(w3.is_empty());
    }

    #[test]
    fn test_replication_log_immutability_triggers() {
        let store = SqliteStore::open_in_memory().expect("in-memory db should open");
        let conn = store.conn();

        let tx = append_log_entry(
            conn,
            "ceres",
            "bug_fact",
            "fact-1",
            "record",
            r#"{"id":"fact-1"}"#,
            "ceres:1",
        )
        .expect("append tx");
        assert_eq!(tx, 1);

        // Attempting UPDATE must fail due to trigger
        let update_res = conn.execute(
            "UPDATE replication_log SET action = 'tampered' WHERE tx_id = ?1;",
            params![tx as i64],
        );
        assert!(update_res.is_err(), "UPDATE on replication_log must fail");
        let err_str = update_res.unwrap_err().to_string();
        assert!(
            err_str.contains("replication_log is append-only"),
            "error: {err_str}"
        );

        // Attempting DELETE must fail due to trigger
        let delete_res = conn.execute(
            "DELETE FROM replication_log WHERE tx_id = ?1;",
            params![tx as i64],
        );
        assert!(delete_res.is_err(), "DELETE on replication_log must fail");
        let err_str = delete_res.unwrap_err().to_string();
        assert!(
            err_str.contains("replication_log is append-only"),
            "error: {err_str}"
        );
    }

    #[test]
    fn test_replication_log_never_zero_value_constraints() {
        let store = SqliteStore::open_in_memory().expect("in-memory db should open");
        let conn = store.conn();

        // Empty entity_id must fail check constraint
        let res = conn.execute(
            "INSERT INTO replication_log (node_id, entity_type, entity_id, action, payload_json, frontier)
             VALUES ('ceres', 'bug_fact', '   ', 'record', '{}', 'ceres:1');",
            [],
        );
        assert!(res.is_err(), "empty entity_id must be rejected");

        // Empty payload_json must fail check constraint
        let res = conn.execute(
            "INSERT INTO replication_log (node_id, entity_type, entity_id, action, payload_json, frontier)
             VALUES ('ceres', 'bug_fact', 'f1', 'record', '', 'ceres:1');",
            [],
        );
        assert!(res.is_err(), "empty payload_json must be rejected");
    }

    #[test]
    fn test_replication_peer_lifecycle() {
        let store = SqliteStore::open_in_memory().expect("in-memory db should open");
        let conn = store.conn();

        let peer = ReplicationPeer {
            peer_id: "cygnus".to_string(),
            peer_url: "http://cygnus:8080".to_string(),
            last_shipped_tx: 0,
            last_applied_tx: 0,
            last_sync_at: "2026-10-10T12:00:00Z".to_string(),
        };
        upsert_peer(conn, &peer).expect("upsert peer");

        let fetched = get_peer(conn, "cygnus")
            .expect("get peer")
            .expect("peer exists");
        assert_eq!(fetched.peer_id, "cygnus");
        assert_eq!(fetched.peer_url, "http://cygnus:8080");

        update_peer_shipped(conn, "cygnus", 42).expect("update shipped");
        let fetched2 = get_peer(conn, "cygnus")
            .expect("get peer")
            .expect("peer exists");
        assert_eq!(fetched2.last_shipped_tx, 42);

        update_peer_applied(conn, "cygnus", 84).expect("update applied");
        let fetched3 = get_peer(conn, "cygnus")
            .expect("get peer")
            .expect("peer exists");
        assert_eq!(fetched3.last_applied_tx, 84);

        let status = get_replication_status(conn, None).expect("status");
        assert_eq!(status.peers.len(), 1);
        assert_eq!(status.peers[0].peer_id, "cygnus");
        assert_eq!(status.peers[0].last_shipped_tx, 42);
        assert_eq!(status.peers[0].last_applied_tx, 84);
    }
}
