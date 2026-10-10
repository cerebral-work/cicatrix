//! Settle audit ledger persistence and deduplication (CER-2764, Phase 4.3).
//!
//! Stores settled worker proposals in `cortex_settle_events` SQLite table.
//! Enforces:
//! - Append-only immutability (triggers reject UPDATE and DELETE).
//! - `:db/neverZeroValue` schema integrity.
//! - Deduplication on `(job_id, settle_action)` or `event_id`.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::cortex::error::CortexError;

/// Settle event record retrieved from or stored to `cortex_settle_events`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordedSettleEvent {
    /// Internal primary key ID.
    pub id: String,
    /// Upstream event ID or dedup key.
    pub event_id: String,
    /// Job identifier.
    pub job_id: String,
    /// Origin source or channel (e.g. `slack`, `email`, `github`).
    pub source: String,
    /// Action type proposed (e.g. `draft_reply`, `triage_issue`).
    pub action_type: String,
    /// Guard tier (`tier1`, `tier2`, `tier3`).
    pub guard_tier: String,
    /// Guard decision (`allow`, `deny`, `escalate`).
    pub guard_decision: Option<String>,
    /// Operator decision (`apply`, `discard`, `defer`).
    pub operator_decision: String,
    /// Settle outcome action (`apply`, `discard`, `select`, `defer`).
    pub settle_action: String,
    /// Proposal summary.
    pub proposal_summary: String,
    /// Rejection reason if discarded.
    pub rejection_reason: Option<String>,
    /// Edit delta if modified.
    pub edit_delta: Option<String>,
    /// Files or code surfaces touched.
    pub files: Vec<String>,
    /// Roll-up meta pattern classification.
    pub meta_pattern: Option<String>,
    /// Whether this is a negative verdict.
    pub is_negative: bool,
    /// Linked observed defect fact ID (e.g. `fact:cortex-settle-...`).
    pub fact_id: Option<String>,
    /// Path to the generated markdown file in `docs/sessions/observed/`.
    pub fact_path: Option<String>,
    /// ISO-8601 creation timestamp.
    pub created_at: String,
}

impl RecordedSettleEvent {
    /// Validates `:db/neverZeroValue` schema constraints.
    pub fn validate_never_zero_value(&self) -> Result<(), CortexError> {
        if self.id.trim().is_empty() {
            return Err(CortexError::Validation(
                ":db/neverZeroValue violation: field `id` is empty".to_string(),
            ));
        }
        if self.event_id.trim().is_empty() {
            return Err(CortexError::Validation(
                ":db/neverZeroValue violation: field `event_id` is empty".to_string(),
            ));
        }
        if self.job_id.trim().is_empty() {
            return Err(CortexError::Validation(
                ":db/neverZeroValue violation: field `job_id` is empty".to_string(),
            ));
        }
        if self.source.trim().is_empty() {
            return Err(CortexError::Validation(
                ":db/neverZeroValue violation: field `source` is empty".to_string(),
            ));
        }
        if self.action_type.trim().is_empty() {
            return Err(CortexError::Validation(
                ":db/neverZeroValue violation: field `action_type` is empty".to_string(),
            ));
        }
        if self.operator_decision.trim().is_empty() {
            return Err(CortexError::Validation(
                ":db/neverZeroValue violation: field `operator_decision` is empty".to_string(),
            ));
        }
        if self.settle_action.trim().is_empty() {
            return Err(CortexError::Validation(
                ":db/neverZeroValue violation: field `settle_action` is empty".to_string(),
            ));
        }
        if self.proposal_summary.trim().is_empty() {
            return Err(CortexError::Validation(
                ":db/neverZeroValue violation: field `proposal_summary` is empty".to_string(),
            ));
        }
        Ok(())
    }
}

/// Query filter for querying settle events.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettleQueryFilter {
    /// Filter by source channel.
    pub source: Option<String>,
    /// Filter by negative verdict flag.
    pub is_negative: Option<bool>,
    /// Filter by job ID.
    pub job_id: Option<String>,
    /// Maximum number of records to return.
    pub limit: Option<usize>,
}

/// Aggregate summary of settle learning loop metrics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettleStatusSummary {
    /// Total settle events recorded.
    pub total_events: usize,
    /// Count of negative settle verdicts (discarded / rejected).
    pub negative_verdicts: usize,
    /// Count of positive or accepted settle verdicts.
    pub positive_verdicts: usize,
    /// Count of events linked to observed defect facts.
    pub observed_facts_linked: usize,
    /// Breakdown of event counts by source channel.
    pub sources: Vec<SourceCount>,
    /// Timestamp of most recent event recorded.
    pub latest_event_at: Option<String>,
}

/// Breakdown count per source channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceCount {
    pub source: String,
    pub count: usize,
}

/// Check if a settle event has already been recorded.
/// Matches on `event_id` or the `(job_id, settle_action)` tuple.
pub fn is_settle_event_recorded(
    conn: &Connection,
    event_id: &str,
    job_id: &str,
    settle_action: &str,
) -> Result<bool, CortexError> {
    let mut stmt = conn.prepare(
        "SELECT 1 FROM cortex_settle_events 
         WHERE id = ?1 OR event_id = ?1 OR (job_id = ?2 AND settle_action = ?3) 
         LIMIT 1;",
    )?;
    let mut rows = stmt.query(params![event_id, job_id, settle_action])?;
    Ok(rows.next()?.is_some())
}

/// Record a settle event into the immutable audit ledger.
pub fn record_settle_event(
    conn: &Connection,
    event: &RecordedSettleEvent,
) -> Result<(), CortexError> {
    event.validate_never_zero_value()?;

    let files_json = serde_json::to_string(&event.files).map_err(CortexError::Json)?;

    conn.execute(
        "INSERT INTO cortex_settle_events (
            id, event_id, job_id, source, action_type, guard_tier, guard_decision,
            operator_decision, settle_action, proposal_summary, rejection_reason,
            edit_delta, files_json, meta_pattern, is_negative, fact_id, fact_path, created_at
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18
        );",
        params![
            &event.id,
            &event.event_id,
            &event.job_id,
            &event.source,
            &event.action_type,
            &event.guard_tier,
            &event.guard_decision,
            &event.operator_decision,
            &event.settle_action,
            &event.proposal_summary,
            &event.rejection_reason,
            &event.edit_delta,
            &files_json,
            &event.meta_pattern,
            if event.is_negative { 1 } else { 0 },
            &event.fact_id,
            &event.fact_path,
            &event.created_at,
        ],
    )?;

    Ok(())
}

/// Retrieve a single settle event by ID or event_id.
pub fn get_settle_event(
    conn: &Connection,
    id: &str,
) -> Result<Option<RecordedSettleEvent>, CortexError> {
    let mut stmt = conn.prepare(
        "SELECT id, event_id, job_id, source, action_type, guard_tier, guard_decision,
                operator_decision, settle_action, proposal_summary, rejection_reason,
                edit_delta, files_json, meta_pattern, is_negative, fact_id, fact_path, created_at
         FROM cortex_settle_events
         WHERE id = ?1 OR event_id = ?1
         LIMIT 1;",
    )?;

    let mut rows = stmt.query(params![id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(row_to_recorded_event(row)?))
    } else {
        Ok(None)
    }
}

/// Query recorded settle events matching optional criteria.
pub fn list_settle_events(
    conn: &Connection,
    filter: &SettleQueryFilter,
) -> Result<Vec<RecordedSettleEvent>, CortexError> {
    let mut sql = "SELECT id, event_id, job_id, source, action_type, guard_tier, guard_decision,
                          operator_decision, settle_action, proposal_summary, rejection_reason,
                          edit_delta, files_json, meta_pattern, is_negative, fact_id, fact_path, created_at
                   FROM cortex_settle_events WHERE 1=1".to_string();

    let mut params_vec: Vec<rusqlite::types::Value> = Vec::new();

    if let Some(source) = &filter.source {
        params_vec.push(rusqlite::types::Value::Text(source.clone()));
        sql.push_str(&format!(" AND source = ?{}", params_vec.len()));
    }

    if let Some(is_neg) = filter.is_negative {
        params_vec.push(rusqlite::types::Value::Integer(if is_neg { 1 } else { 0 }));
        sql.push_str(&format!(" AND is_negative = ?{}", params_vec.len()));
    }

    if let Some(job_id) = &filter.job_id {
        params_vec.push(rusqlite::types::Value::Text(job_id.clone()));
        sql.push_str(&format!(" AND job_id = ?{}", params_vec.len()));
    }

    sql.push_str(" ORDER BY created_at DESC");

    let limit = filter.limit.unwrap_or(50);
    params_vec.push(rusqlite::types::Value::Integer(limit as i64));
    sql.push_str(&format!(" LIMIT ?{}", params_vec.len()));

    let mut stmt = conn.prepare(&sql)?;
    let rusqlite_params: Vec<&dyn rusqlite::ToSql> = params_vec
        .iter()
        .map(|v| v as &dyn rusqlite::ToSql)
        .collect();

    let rows = stmt.query_map(rusqlite_params.as_slice(), |row| {
        row_to_recorded_event(row).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })
    })?;

    let mut list = Vec::new();
    for item in rows {
        list.push(item?);
    }
    Ok(list)
}

/// Compute aggregate metrics over all recorded settle events.
pub fn get_settle_status_summary(conn: &Connection) -> Result<SettleStatusSummary, CortexError> {
    let total_events: i64 =
        conn.query_row("SELECT COUNT(*) FROM cortex_settle_events;", [], |r| {
            r.get(0)
        })?;

    let negative_verdicts: i64 = conn.query_row(
        "SELECT COUNT(*) FROM cortex_settle_events WHERE is_negative = 1;",
        [],
        |r| r.get(0),
    )?;

    let total_events = total_events as usize;
    let negative_verdicts = negative_verdicts as usize;
    let positive_verdicts = total_events.saturating_sub(negative_verdicts);

    let observed_facts_linked: i64 = conn.query_row(
        "SELECT COUNT(*) FROM cortex_settle_events WHERE fact_id IS NOT NULL AND trim(fact_id) != '';",
        [],
        |r| r.get(0),
    )?;
    let observed_facts_linked = observed_facts_linked as usize;

    let latest_event_at: Option<String> = conn
        .query_row(
            "SELECT created_at FROM cortex_settle_events ORDER BY created_at DESC LIMIT 1;",
            [],
            |r| r.get(0),
        )
        .ok();

    let mut stmt = conn.prepare(
        "SELECT source, COUNT(*) FROM cortex_settle_events GROUP BY source ORDER BY COUNT(*) DESC;",
    )?;
    let rows = stmt.query_map([], |row| {
        let source: String = row.get(0)?;
        let count: i64 = row.get(1)?;
        Ok(SourceCount {
            source,
            count: count as usize,
        })
    })?;

    let mut sources = Vec::new();
    for row in rows {
        sources.push(row?);
    }

    Ok(SettleStatusSummary {
        total_events,
        negative_verdicts,
        positive_verdicts,
        observed_facts_linked,
        sources,
        latest_event_at,
    })
}

fn row_to_recorded_event(row: &rusqlite::Row) -> Result<RecordedSettleEvent, rusqlite::Error> {
    let id: String = row.get(0)?;
    let event_id: String = row.get(1)?;
    let job_id: String = row.get(2)?;
    let source: String = row.get(3)?;
    let action_type: String = row.get(4)?;
    let guard_tier: String = row.get(5)?;
    let guard_decision: Option<String> = row.get(6)?;
    let operator_decision: String = row.get(7)?;
    let settle_action: String = row.get(8)?;
    let proposal_summary: String = row.get(9)?;
    let rejection_reason: Option<String> = row.get(10)?;
    let edit_delta: Option<String> = row.get(11)?;
    let files_json: Option<String> = row.get(12)?;
    let meta_pattern: Option<String> = row.get(13)?;
    let is_negative_int: i32 = row.get(14)?;
    let fact_id: Option<String> = row.get(15)?;
    let fact_path: Option<String> = row.get(16)?;
    let created_at: String = row.get(17)?;

    let files: Vec<String> = if let Some(fj) = files_json {
        serde_json::from_str(&fj).unwrap_or_default()
    } else {
        Vec::new()
    };

    Ok(RecordedSettleEvent {
        id,
        event_id,
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
        is_negative: is_negative_int == 1,
        fact_id,
        fact_path,
        created_at,
    })
}
