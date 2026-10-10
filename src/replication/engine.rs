//! Core replication engine: windowed export, checksum verification, idempotent apply, and peer sync (CER-2765, Phase 4.4).

use rusqlite::{params, Connection};
use std::time::Duration;

use crate::cortex::store::{is_settle_event_recorded, record_settle_event, RecordedSettleEvent};
use crate::replication::error::ReplicationError;
use crate::replication::store::{
    get_head_tx, get_peer, local_node_id, query_window, update_peer_applied, update_peer_shipped,
    upsert_peer,
};
use crate::replication::types::{
    compute_records_checksum, ApplyResult, LogSegment, ReplicationPeer, SyncResult,
};
use crate::store::sqlite::validate_never_zero_value;
use crate::store::BugFact;
use crate::tripwire::guard_check;

/// Export a windowed log segment `(since_tx, until_tx]` from the local changelog ledger.
pub fn export_segment(
    conn: &Connection,
    since_tx: u64,
    until_tx: Option<u64>,
    limit: Option<u64>,
    actor: Option<&str>,
) -> Result<LogSegment, ReplicationError> {
    let head = get_head_tx(conn)?;
    let until = until_tx.unwrap_or(head);

    if since_tx > until {
        return Err(ReplicationError::InvalidWindow {
            from_tx: since_tx,
            to_tx: until,
        });
    }

    let records = query_window(conn, since_tx, until, limit)?;
    let actor_id = actor.unwrap_or("cicatrix-replication-exporter");

    // Fail-closed security tripwire canary validation on every exported record
    for record in &records {
        guard_check(
            conn,
            &record.entity_id,
            Some(&record.payload_json),
            actor_id,
            "export_replication_segment",
        )?;
    }

    let node_id = local_node_id();
    let effective_head = head.max(1);
    let frontier = format!("{node_id}:{effective_head}");
    let checksum = compute_records_checksum(&records);

    let segment = LogSegment {
        from_tx: since_tx,
        to_tx: until,
        node_id,
        records,
        head_tx: head,
        frontier,
        checksum,
    };

    segment.validate_never_zero_value()?;
    Ok(segment)
}

/// Apply an incoming replication log segment into the local database idempotently.
pub fn apply_segment(
    conn: &Connection,
    segment: &LogSegment,
    actor: Option<&str>,
) -> Result<ApplyResult, ReplicationError> {
    segment.validate_never_zero_value()?;

    // 1. Verify cryptographic integrity checksum
    let calculated_checksum = compute_records_checksum(&segment.records);
    if calculated_checksum != segment.checksum {
        return Err(ReplicationError::ChecksumMismatch {
            expected: segment.checksum.clone(),
            actual: calculated_checksum,
        });
    }

    let actor_id = actor.unwrap_or("cicatrix-replication-ingester");
    let mut applied_count = 0;
    let mut skipped_count = 0;

    // 2. Process records sequentially within a transaction
    let tx = conn.unchecked_transaction()?;

    for record in &segment.records {
        record.validate_never_zero_value()?;

        // Fail-closed security tripwire check on incoming record
        guard_check(
            &tx,
            &record.entity_id,
            Some(&record.payload_json),
            actor_id,
            "apply_replication_segment",
        )?;

        match record.entity_type.as_str() {
            "bug_fact" => {
                let fact: BugFact = serde_json::from_str(&record.payload_json)?;
                validate_never_zero_value(&fact)
                    .map_err(|e| ReplicationError::Validation(e.to_string()))?;
                apply_bug_fact_record(&tx, &fact)?;
                applied_count += 1;
            }
            "cortex_settle" => {
                let event: RecordedSettleEvent = serde_json::from_str(&record.payload_json)?;
                event.validate_never_zero_value()?;
                if is_settle_event_recorded(
                    &tx,
                    &event.event_id,
                    &event.job_id,
                    &event.settle_action,
                )? {
                    skipped_count += 1;
                } else {
                    record_settle_event(&tx, &event)?;
                    applied_count += 1;
                }
            }
            _ => {
                // Unknown entity type is skipped to ensure forward compatibility
                skipped_count += 1;
            }
        }
    }

    tx.commit()?;

    // 3. Update peer watermark if this peer exists or has records
    if !segment.node_id.is_empty() {
        let _ = update_peer_applied(conn, &segment.node_id, segment.to_tx);
    }

    let local_head = get_head_tx(conn)?;

    Ok(ApplyResult {
        peer_id: segment.node_id.clone(),
        from_tx: segment.from_tx,
        to_tx: segment.to_tx,
        records_received: segment.records.len(),
        records_applied: applied_count,
        duplicates_skipped: skipped_count,
        local_head_tx: local_head,
        status: "ok".to_string(),
    })
}

/// Helper to apply a `BugFact` entity into SQLite tables without appending a local replication log.
fn apply_bug_fact_record(conn: &Connection, fact: &BugFact) -> Result<(), ReplicationError> {
    let (reproducer_command, rerun_policy, closing_invariant, stochastic_json) =
        if let Some(st) = &fact.stochastic {
            (
                st.reproducer_command.as_deref(),
                st.rerun_policy.as_deref(),
                st.closing_invariant.as_deref(),
                serde_json::to_string(st).ok(),
            )
        } else {
            (None, None, None, None)
        };

    let frontier_str = fact.frontier.as_ref().map(|f| f.to_string());

    conn.execute(
        "INSERT INTO bug_facts (
            id, symptom, fix_commit, regression_test, meta_pattern, scope,
            do_not_generalize, reproducer, reproducer_command, rerun_policy,
            closing_invariant, stochastic_json, frontier
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
         ON CONFLICT(id) DO UPDATE SET
            symptom = excluded.symptom,
            fix_commit = excluded.fix_commit,
            regression_test = excluded.regression_test,
            meta_pattern = excluded.meta_pattern,
            scope = excluded.scope,
            do_not_generalize = excluded.do_not_generalize,
            reproducer = excluded.reproducer,
            reproducer_command = excluded.reproducer_command,
            rerun_policy = excluded.rerun_policy,
            closing_invariant = excluded.closing_invariant,
            stochastic_json = excluded.stochastic_json,
            frontier = excluded.frontier;",
        params![
            fact.id,
            fact.symptom,
            fact.fix_commit,
            fact.regression_test,
            fact.meta_pattern,
            fact.scope,
            if fact.do_not_generalize { 1 } else { 0 },
            fact.reproducer,
            reproducer_command,
            rerun_policy,
            closing_invariant,
            stochastic_json,
            frontier_str,
        ],
    )?;

    conn.execute(
        "DELETE FROM bug_fact_files WHERE bug_id = ?1;",
        params![fact.id],
    )?;
    let mut stmt = conn.prepare("INSERT INTO bug_fact_files (bug_id, path) VALUES (?1, ?2);")?;
    for file in &fact.files {
        stmt.execute(params![fact.id, file])?;
    }

    conn.execute(
        "DELETE FROM bug_occurrences WHERE bug_id = ?1;",
        params![fact.id],
    )?;
    if let Some(st) = &fact.stochastic {
        let mut occ_stmt = conn.prepare(
            "INSERT INTO bug_occurrences (bug_id, n, date, config, result) VALUES (?1, ?2, ?3, ?4, ?5);",
        )?;
        for occ in &st.occurrences {
            occ_stmt.execute(params![fact.id, occ.n, occ.date, occ.config, occ.result])?;
        }
    }

    Ok(())
}

/// Execute bidirectional replication sync with a remote cluster peer node over HTTP.
pub fn sync_peer(
    conn: &Connection,
    peer_url: &str,
    peer_id_hint: Option<&str>,
    actor: Option<&str>,
) -> Result<SyncResult, ReplicationError> {
    let clean_url = peer_url.trim_end_matches('/');
    if clean_url.is_empty() {
        return Err(ReplicationError::Validation(
            "peer_url must not be empty".to_string(),
        ));
    }

    // 1. Fetch remote replication status
    let status_endpoint = format!("{clean_url}/api/v1/replication/status");
    let resp: serde_json::Value = ureq::get(&status_endpoint)
        .timeout(Duration::from_secs(5))
        .call()
        .map_err(|e| ReplicationError::Network(format!("failed to connect to peer status: {e}")))?
        .into_json()
        .map_err(|e| ReplicationError::Network(format!("invalid status response: {e}")))?;

    let remote_node_id = resp["node_id"]
        .as_str()
        .unwrap_or(peer_id_hint.unwrap_or("remote_peer"))
        .to_string();
    let remote_head_tx = resp["local_head_tx"].as_u64().unwrap_or(0);

    let effective_peer_id = peer_id_hint.unwrap_or(&remote_node_id);

    // Ensure peer entry exists locally
    let existing_peer = get_peer(conn, effective_peer_id)?;
    let last_applied = existing_peer
        .as_ref()
        .map(|p| p.last_applied_tx)
        .unwrap_or(0);
    let last_shipped = existing_peer
        .as_ref()
        .map(|p| p.last_shipped_tx)
        .unwrap_or(0);

    let mut applied_records = 0;

    // 2. PULL Phase: Fetch and apply missing segments from remote peer
    if remote_head_tx > last_applied {
        let segment_endpoint = format!(
            "{clean_url}/api/v1/replication/segment?since_tx={last_applied}&until_tx={remote_head_tx}"
        );
        let segment: LogSegment = ureq::get(&segment_endpoint)
            .timeout(Duration::from_secs(10))
            .call()
            .map_err(|e| {
                ReplicationError::Network(format!("failed to fetch segment from peer: {e}"))
            })?
            .into_json()
            .map_err(|e| {
                ReplicationError::Network(format!("invalid segment JSON from peer: {e}"))
            })?;

        let apply_result = apply_segment(conn, &segment, actor)?;
        applied_records = apply_result.records_applied;
    }

    // 3. PUSH Phase: Ship local updates to remote peer
    let local_head = get_head_tx(conn)?;
    let mut shipped_records = 0;

    if local_head > last_shipped {
        let local_segment = export_segment(conn, last_shipped, Some(local_head), Some(500), actor)?;
        if !local_segment.records.is_empty() {
            let post_endpoint = format!("{clean_url}/api/v1/replication/segment");
            let resp = ureq::post(&post_endpoint)
                .timeout(Duration::from_secs(10))
                .send_json(&local_segment)
                .map_err(|e| {
                    ReplicationError::Network(format!("failed to ship segment to peer: {e}"))
                })?;

            if resp.status() == 200 {
                shipped_records = local_segment.records.len();
                update_peer_shipped(conn, effective_peer_id, local_segment.to_tx)?;
            }
        }
    }

    // 4. Update peer record in database
    let now = sqlite_timestamp_iso(conn)?;
    let updated_peer = ReplicationPeer {
        peer_id: effective_peer_id.to_string(),
        peer_url: clean_url.to_string(),
        last_shipped_tx: if shipped_records > 0 {
            local_head
        } else {
            last_shipped
        },
        last_applied_tx: remote_head_tx.max(last_applied),
        last_sync_at: now,
    };
    upsert_peer(conn, &updated_peer)?;

    Ok(SyncResult {
        peer_id: effective_peer_id.to_string(),
        peer_url: clean_url.to_string(),
        shipped_records,
        applied_records,
        local_head_tx: local_head,
        remote_head_tx,
        status: "ok".to_string(),
        message: format!(
            "replication sync complete: shipped {shipped_records} records, applied {applied_records} records"
        ),
    })
}

fn sqlite_timestamp_iso(conn: &Connection) -> Result<String, rusqlite::Error> {
    conn.query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ', 'now');", [], |r| {
        r.get(0)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replication::store::append_log_entry;
    use crate::replication::types::ReplicationRecord;
    use crate::store::{BugFact, SqliteStore};

    #[test]
    fn test_export_segment_and_checksum() {
        let store = SqliteStore::open_in_memory().expect("in-memory db");
        let conn = store.conn();

        append_log_entry(
            conn,
            "ceres",
            "bug_fact",
            "fact-1",
            "record",
            r#"{"id":"fact-1"}"#,
            "ceres:1",
        )
        .expect("append 1");
        append_log_entry(
            conn,
            "ceres",
            "bug_fact",
            "fact-2",
            "record",
            r#"{"id":"fact-2"}"#,
            "ceres:2",
        )
        .expect("append 2");

        let segment =
            export_segment(conn, 0, Some(2), None, Some("test-exporter")).expect("export segment");
        assert_eq!(segment.from_tx, 0);
        assert_eq!(segment.to_tx, 2);
        assert_eq!(segment.records.len(), 2);
        assert_eq!(segment.head_tx, 2);
        assert!(!segment.checksum.is_empty());
        assert_eq!(segment.checksum, compute_records_checksum(&segment.records));

        // Invalid window (since > until)
        let err = export_segment(conn, 5, Some(2), None, None);
        assert!(matches!(err, Err(ReplicationError::InvalidWindow { .. })));
    }

    #[test]
    fn test_apply_segment_idempotent_and_zero_echo() {
        let store = SqliteStore::open_in_memory().expect("in-memory db");
        let conn = store.conn();

        let fact = BugFact {
            id: "replicated-bug-1".to_string(),
            symptom: "crash on startup".to_string(),
            fix_commit: "abc1234".to_string(),
            regression_test: "cargo test -j 2".to_string(),
            meta_pattern: "nil-pointer-deref".to_string(),
            scope: Some("backend".to_string()),
            files: vec!["src/main.rs".to_string()],
            do_not_generalize: false,
            reproducer: Some("cargo run".to_string()),
            stochastic: None,
            frontier: None,
        };
        let payload_json = serde_json::to_string(&fact).expect("json");

        let record = ReplicationRecord {
            tx_id: 1,
            node_id: "cygnus".to_string(),
            entity_type: "bug_fact".to_string(),
            entity_id: "replicated-bug-1".to_string(),
            action: "record".to_string(),
            payload_json,
            frontier: "cygnus:1".to_string(),
            created_at: "2026-10-10T12:00:00Z".to_string(),
        };

        let checksum = compute_records_checksum(std::slice::from_ref(&record));
        let segment = LogSegment {
            from_tx: 0,
            to_tx: 1,
            node_id: "cygnus".to_string(),
            records: vec![record],
            head_tx: 1,
            frontier: "cygnus:1".to_string(),
            checksum,
        };

        // First apply
        let result1 = apply_segment(conn, &segment, Some("test-ingester")).expect("apply segment");
        assert_eq!(result1.records_applied, 1);
        assert_eq!(result1.records_received, 1);
        assert_eq!(result1.peer_id, "cygnus");

        // Verify fact is recorded in SQLite
        let hits = store
            .touches_known_bug(&["src/main.rs".to_string()])
            .expect("query");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "replicated-bug-1");

        // Verify ZERO ECHO CYCLES: local replication log must not contain incoming replicated entries
        let local_records = crate::replication::store::get_total_records(conn).expect("total");
        assert_eq!(
            local_records, 0,
            "incoming replication facts must not generate echo log entries"
        );

        // Idempotent second apply: re-applying the same segment succeeds cleanly
        let result2 = apply_segment(conn, &segment, Some("test-ingester"))
            .expect("second apply should succeed");
        assert_eq!(result2.records_applied, 1);
    }

    #[test]
    fn test_apply_segment_checksum_mismatch_rejected() {
        let store = SqliteStore::open_in_memory().expect("in-memory db");
        let conn = store.conn();

        let record = ReplicationRecord {
            tx_id: 1,
            node_id: "cygnus".to_string(),
            entity_type: "bug_fact".to_string(),
            entity_id: "fact-1".to_string(),
            action: "record".to_string(),
            payload_json: r#"{"id":"fact-1"}"#.to_string(),
            frontier: "cygnus:1".to_string(),
            created_at: "2026-10-10T12:00:00Z".to_string(),
        };

        let segment = LogSegment {
            from_tx: 0,
            to_tx: 1,
            node_id: "cygnus".to_string(),
            records: vec![record],
            head_tx: 1,
            frontier: "cygnus:1".to_string(),
            checksum: "tampered_checksum_0000000000000000000000000000000000000000".to_string(),
        };

        let err = apply_segment(conn, &segment, None);
        assert!(matches!(
            err,
            Err(ReplicationError::ChecksumMismatch { .. })
        ));
    }

    #[test]
    fn test_tripwire_canary_fail_closed_on_export_and_apply() {
        let store = SqliteStore::open_in_memory().expect("in-memory db");
        let conn = store.conn();

        // Seed a known tripwire canary fact
        append_log_entry(
            conn,
            "ceres",
            "bug_fact",
            "TRIPWIRE_CANARY_SENTINEL_ALPHA",
            "record",
            r#"{"id":"TRIPWIRE_CANARY_SENTINEL_ALPHA","symptom":"leak"}"#,
            "ceres:1",
        )
        .expect("append canary");

        // Export should fail closed due to tripwire guard_check
        let export_err = export_segment(conn, 0, Some(1), None, Some("unauthorized-agent"));
        assert!(
            matches!(export_err, Err(ReplicationError::Tripwire(_))),
            "export must trip"
        );

        // Apply with canary entity_id should also fail closed
        let record = ReplicationRecord {
            tx_id: 1,
            node_id: "cygnus".to_string(),
            entity_type: "bug_fact".to_string(),
            entity_id: "TRIPWIRE_CANARY_AUTH_TOKEN".to_string(),
            action: "record".to_string(),
            payload_json: r#"{"id":"TRIPWIRE_CANARY_AUTH_TOKEN"}"#.to_string(),
            frontier: "cygnus:1".to_string(),
            created_at: "2026-10-10T12:00:00Z".to_string(),
        };
        let checksum = compute_records_checksum(std::slice::from_ref(&record));
        let segment = LogSegment {
            from_tx: 0,
            to_tx: 1,
            node_id: "cygnus".to_string(),
            records: vec![record],
            head_tx: 1,
            frontier: "cygnus:1".to_string(),
            checksum,
        };

        let apply_err = apply_segment(conn, &segment, Some("unauthorized-agent"));
        assert!(
            matches!(apply_err, Err(ReplicationError::Tripwire(_))),
            "apply must trip"
        );
    }
}
