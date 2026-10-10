//! Workflow engine managing embedded `autumn-harvest-sqlite` runtime (CER-2756, Phase 2.1).
//!
//! Enforces safety bounds on event log length (50,000 events) and serialized size (50 MiB),
//! exposes execution APIs, and provides lock-free read-only database inspection.

use std::path::{Path, PathBuf};

use autumn_harvest_sqlite::{RunState, SqliteError, SqliteRuntime};
use rusqlite::Connection;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::workflow::audit::{
    aggregate_drift_report_info, audit_workflow_info, run_aggregate_drift_report,
    run_scan_repo_markers, scan_repo_markers_info,
};
use crate::workflow::signal::{DurableSignal, SignalDeliveryStatus, StoredSignalRecord};
use crate::workflow::triage::{
    bisect_commits_info, ingest_test_failure_info, isolate_minimal_reproducer_info,
    review_gate_workflow_info, run_bisect_commits, run_ingest_test_failure,
    run_isolate_minimal_reproducer, triage_workflow_info, BisectCommitsInput, ReproducerInput,
    TriageInput,
};
use crate::workflow::{MAX_WORKFLOW_BYTES, MAX_WORKFLOW_EVENTS};

/// Errors encountered during workflow engine lifecycle or execution.
#[derive(Debug)]
pub enum WorkflowEngineError {
    /// Failure originating within the embedded `autumn-harvest-sqlite` runtime.
    Sqlite(SqliteError),
    /// Failure originating from read-only SQLite inspection queries.
    Rusqlite(rusqlite::Error),
    /// JSON serialization or deserialization failure.
    Serialization(serde_json::Error),
    /// Standard I/O failure.
    Io(std::io::Error),
    /// Workflow returned a terminal failure state.
    ExecutionFailed(String),
    /// Workflow exceeded the maximum permitted event count safety bound.
    EventLimitExceeded {
        /// Number of events recorded.
        count: usize,
        /// Maximum permitted event count.
        max: usize,
    },
    /// Workflow exceeded the maximum permitted serialized history size.
    ByteLimitExceeded {
        /// Serialized size in bytes.
        bytes: usize,
        /// Maximum permitted size in bytes.
        max: usize,
    },
    /// Target execution not found in database.
    NotFound(String),
}

impl std::fmt::Display for WorkflowEngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sqlite(e) => write!(f, "workflow SQLite error: {e}"),
            Self::Rusqlite(e) => write!(f, "database query error: {e}"),
            Self::Serialization(e) => write!(f, "serialization error: {e}"),
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::ExecutionFailed(e) => write!(f, "workflow execution failed: {e}"),
            Self::EventLimitExceeded { count, max } => {
                write!(
                    f,
                    "workflow event limit exceeded: {count} events exceeds maximum {max}"
                )
            }
            Self::ByteLimitExceeded { bytes, max } => {
                write!(
                    f,
                    "workflow history byte limit exceeded: {bytes} bytes exceeds maximum {max} bytes"
                )
            }
            Self::NotFound(id) => write!(f, "workflow execution `{id}` not found"),
        }
    }
}

impl std::error::Error for WorkflowEngineError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Sqlite(e) => Some(e),
            Self::Rusqlite(e) => Some(e),
            Self::Serialization(e) => Some(e),
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<SqliteError> for WorkflowEngineError {
    fn from(e: SqliteError) -> Self {
        Self::Sqlite(e)
    }
}

impl From<rusqlite::Error> for WorkflowEngineError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Rusqlite(e)
    }
}

impl From<serde_json::Error> for WorkflowEngineError {
    fn from(e: serde_json::Error) -> Self {
        Self::Serialization(e)
    }
}

impl From<std::io::Error> for WorkflowEngineError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// Lightweight summary of a stored workflow execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkflowExecutionSummary {
    /// Unique execution identifier.
    pub exec_id: String,
    /// Canonical workflow name.
    pub workflow_name: String,
    /// Business workflow identifier.
    pub workflow_id: String,
    /// Execution status state (`RUNNING`, `COMPLETED`, `FAILED`).
    pub state: String,
    /// Number of durable events in history.
    pub event_count: usize,
    /// Total serialized size of history in bytes.
    pub byte_size: usize,
}

/// Outcome of delivering a durable signal to an execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SignalDeliveryReport {
    /// Workflow execution identifier.
    pub exec_id: String,
    /// Canonical signal name.
    pub signal_name: String,
    /// Unique idempotency key.
    pub idempotency_key: String,
    /// Status: delivered or duplicate.
    pub status: SignalDeliveryStatus,
    /// Current workflow execution state following signal delivery attempt.
    pub run_state: String,
}

/// Initialize the durable signal ledger table in SQLite.
pub fn init_signal_ledger_table(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS workflow_signal_ledger (
            exec_id TEXT NOT NULL,
            signal_name TEXT NOT NULL,
            idempotency_key TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            received_at INTEGER NOT NULL,
            PRIMARY KEY (exec_id, idempotency_key)
        );
        CREATE INDEX IF NOT EXISTS idx_workflow_signal_ledger_exec ON workflow_signal_ledger (exec_id);",
    )?;
    Ok(())
}

/// Detailed inspection view of a workflow execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkflowExecutionDetail {
    /// Unique execution identifier.
    pub exec_id: String,
    /// Canonical workflow name.
    pub workflow_name: String,
    /// Business workflow identifier.
    pub workflow_id: String,
    /// Execution status state.
    pub state: String,
    /// Serialized input JSON.
    pub input_json: String,
    /// Serialized output JSON if completed.
    pub output_json: Option<String>,
    /// Terminal error message if failed.
    pub error: Option<String>,
    /// Number of durable events in history.
    pub event_count: usize,
    /// Total serialized size of history in bytes.
    pub byte_size: usize,
    /// List of raw event JSON payloads.
    pub events: Vec<serde_json::Value>,
    /// List of durable signals received by this execution.
    pub signals: Vec<StoredSignalRecord>,
}

/// Execution outcome report returned by `WorkflowEngine::run_workflow`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowExecutionReport<T> {
    /// Unique execution identifier.
    pub exec_id: String,
    /// Executed workflow name.
    pub workflow_name: String,
    /// Terminal execution state (`COMPLETED`, `FAILED`, etc.).
    pub state: String,
    /// Total number of durable events recorded.
    pub event_count: usize,
    /// Total serialized size of history in bytes.
    pub byte_size: usize,
    /// Parsed output payload upon successful completion.
    pub output: Option<T>,
    /// Error message upon failure.
    pub error: Option<String>,
}

/// Durable workflow engine wrapping embedded `autumn-harvest-sqlite`.
pub struct WorkflowEngine {
    runtime: SqliteRuntime,
    ledger_conn: Connection,
    db_path: Option<PathBuf>,
}

impl WorkflowEngine {
    /// Open the workflow database at the given path and register default workflows.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, WorkflowEngineError> {
        let runtime = SqliteRuntime::open(path.as_ref())?;
        let ledger_conn = Connection::open(path.as_ref())?;
        ledger_conn.busy_timeout(std::time::Duration::from_secs(5))?;
        init_signal_ledger_table(&ledger_conn)?;
        let mut engine = Self {
            runtime,
            ledger_conn,
            db_path: Some(path.as_ref().to_path_buf()),
        };
        engine.register_defaults();
        Ok(engine)
    }

    /// Open an isolated in-memory workflow database (primarily for testing).
    pub fn open_in_memory() -> Result<Self, WorkflowEngineError> {
        let runtime = SqliteRuntime::open_in_memory()?;
        let ledger_conn = Connection::open_in_memory()?;
        init_signal_ledger_table(&ledger_conn)?;
        let mut engine = Self {
            runtime,
            ledger_conn,
            db_path: None,
        };
        engine.register_defaults();
        Ok(engine)
    }

    /// Register canonical workflows and activities with the underlying runtime.
    fn register_defaults(&mut self) {
        self.runtime.register_workflow(&triage_workflow_info());
        self.runtime.register_workflow(&audit_workflow_info());
        self.runtime.register_workflow(&review_gate_workflow_info());

        self.runtime
            .register_activity(&ingest_test_failure_info(), |val| {
                let input: TriageInput = serde_json::from_value(val).map_err(|e| e.to_string())?;
                let output = run_ingest_test_failure(input)?;
                serde_json::to_value(output).map_err(|e| e.to_string())
            });

        self.runtime
            .register_activity(&bisect_commits_info(), |val| {
                let input: BisectCommitsInput =
                    serde_json::from_value(val).map_err(|e| e.to_string())?;
                let output = run_bisect_commits(input)?;
                serde_json::to_value(output).map_err(|e| e.to_string())
            });

        self.runtime
            .register_activity(&isolate_minimal_reproducer_info(), |val| {
                let input: ReproducerInput =
                    serde_json::from_value(val).map_err(|e| e.to_string())?;
                let output = run_isolate_minimal_reproducer(input)?;
                serde_json::to_value(output).map_err(|e| e.to_string())
            });

        self.runtime
            .register_activity(&scan_repo_markers_info(), |val| {
                let input: crate::workflow::audit::ScanTargetInput =
                    serde_json::from_value(val).map_err(|e| e.to_string())?;
                let output = run_scan_repo_markers(input)?;
                serde_json::to_value(output).map_err(|e| e.to_string())
            });

        self.runtime
            .register_activity(&aggregate_drift_report_info(), |val| {
                let input: crate::workflow::audit::AggregateInput =
                    serde_json::from_value(val).map_err(|e| e.to_string())?;
                let output = run_aggregate_drift_report(input)?;
                serde_json::to_value(output).map_err(|e| e.to_string())
            });
    }

    /// Check history metrics against safety bounds (50,000 events and 50 MiB).
    pub fn check_history_safety_bounds(
        event_count: usize,
        byte_size: usize,
    ) -> Result<(), WorkflowEngineError> {
        if event_count > MAX_WORKFLOW_EVENTS {
            return Err(WorkflowEngineError::EventLimitExceeded {
                count: event_count,
                max: MAX_WORKFLOW_EVENTS,
            });
        }

        if byte_size > MAX_WORKFLOW_BYTES {
            return Err(WorkflowEngineError::ByteLimitExceeded {
                bytes: byte_size,
                max: MAX_WORKFLOW_BYTES,
            });
        }

        Ok(())
    }

    /// Execute a registered workflow by name to completion, enforcing safety bounds.
    pub async fn run_workflow<I: Serialize, O: DeserializeOwned>(
        &mut self,
        workflow_name: &str,
        input: I,
    ) -> Result<WorkflowExecutionReport<O>, WorkflowEngineError> {
        let input_val = serde_json::to_value(input)?;
        let exec_id = self.runtime.start_workflow(workflow_name, input_val)?;

        let run_state = self.runtime.run_until_blocked(exec_id).await?;

        let history = self.runtime.load_history(exec_id)?;
        let event_count = history.len();

        let serialized_history = serde_json::to_vec(&history)?;
        let byte_size = serialized_history.len();

        // Enforce safety limits
        Self::check_history_safety_bounds(event_count, byte_size)?;

        match run_state {
            RunState::Completed(val) => {
                let parsed_output: O = serde_json::from_value(val)?;
                Ok(WorkflowExecutionReport {
                    exec_id: exec_id.to_string(),
                    workflow_name: workflow_name.to_string(),
                    state: "COMPLETED".to_string(),
                    event_count,
                    byte_size,
                    output: Some(parsed_output),
                    error: None,
                })
            }
            RunState::Failed(err) => Ok(WorkflowExecutionReport {
                exec_id: exec_id.to_string(),
                workflow_name: workflow_name.to_string(),
                state: "FAILED".to_string(),
                event_count,
                byte_size,
                output: None,
                error: Some(err),
            }),
            RunState::WaitingSignal(sig) => {
                let state_str = format!("WAITING_SIGNAL({sig})");
                let _ = self.ledger_conn.execute(
                    "UPDATE harvest_executions SET state = ? WHERE exec_id = ?",
                    [&state_str, &exec_id.to_string()],
                );
                Ok(WorkflowExecutionReport {
                    exec_id: exec_id.to_string(),
                    workflow_name: workflow_name.to_string(),
                    state: state_str,
                    event_count,
                    byte_size,
                    output: None,
                    error: None,
                })
            }
            other => Ok(WorkflowExecutionReport {
                exec_id: exec_id.to_string(),
                workflow_name: workflow_name.to_string(),
                state: format!("{other:?}"),
                event_count,
                byte_size,
                output: None,
                error: None,
            }),
        }
    }

    /// Deliver a durable signal to a running or parked workflow execution.
    ///
    /// Deduplicates deliveries on `(exec_id, idempotency_key)` using the SQLite signal ledger.
    /// If duplicate, returns `SignalDeliveryStatus::Duplicate` with current run state without advancing.
    /// If fresh, persists to ledger, delivers to `autumn-harvest-sqlite` runtime, and advances execution.
    pub async fn deliver_signal<T: Serialize + Sync>(
        &mut self,
        exec_id: &str,
        signal: &DurableSignal<T>,
    ) -> Result<SignalDeliveryReport, WorkflowEngineError> {
        signal
            .validate()
            .map_err(WorkflowEngineError::ExecutionFailed)?;

        let exec_uuid = exec_id
            .parse::<autumn_harvest::ExecutionId>()
            .map_err(|e| {
                WorkflowEngineError::ExecutionFailed(format!("invalid execution ID: {e}"))
            })?;

        // Check if (exec_id, idempotency_key) already recorded in ledger
        let is_duplicate: bool = self
            .ledger_conn
            .query_row(
                "SELECT COUNT(*) FROM workflow_signal_ledger WHERE exec_id = ? AND idempotency_key = ?",
                [exec_id, &signal.idempotency_key],
                |r| r.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .map_err(WorkflowEngineError::Rusqlite)?;

        if is_duplicate {
            let current_state = self.get_current_execution_state(exec_id)?;
            return Ok(SignalDeliveryReport {
                exec_id: exec_id.to_string(),
                signal_name: signal.signal_name.clone(),
                idempotency_key: signal.idempotency_key.clone(),
                status: SignalDeliveryStatus::Duplicate,
                run_state: current_state,
            });
        }

        let payload_json =
            serde_json::to_string(&signal.payload).map_err(WorkflowEngineError::Serialization)?;
        let now_millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);

        self.ledger_conn
            .execute(
                "INSERT INTO workflow_signal_ledger (exec_id, signal_name, idempotency_key, payload_json, received_at) \
                 VALUES (?, ?, ?, ?, ?)",
                rusqlite::params![exec_id, &signal.signal_name, &signal.idempotency_key, &payload_json, now_millis],
            )
            .map_err(WorkflowEngineError::Rusqlite)?;

        let payload_value =
            serde_json::to_value(&signal.payload).map_err(WorkflowEngineError::Serialization)?;

        self.runtime
            .send_signal(exec_uuid, &signal.signal_name, payload_value)
            .map_err(WorkflowEngineError::Sqlite)?;

        let new_run_state = self.runtime.run_until_blocked(exec_uuid).await?;
        let state_str = match new_run_state {
            RunState::Completed(_) => "COMPLETED".to_string(),
            RunState::Failed(e) => format!("FAILED({e})"),
            RunState::WaitingSignal(s) => {
                let sig_state = format!("WAITING_SIGNAL({s})");
                let _ = self.ledger_conn.execute(
                    "UPDATE harvest_executions SET state = ? WHERE exec_id = ?",
                    [&sig_state, exec_id],
                );
                sig_state
            }
            RunState::InProgress => "IN_PROGRESS".to_string(),
            RunState::WaitingTimer => "WAITING_TIMER".to_string(),
        };

        Ok(SignalDeliveryReport {
            exec_id: exec_id.to_string(),
            signal_name: signal.signal_name.clone(),
            idempotency_key: signal.idempotency_key.clone(),
            status: SignalDeliveryStatus::Delivered,
            run_state: state_str,
        })
    }

    /// Determine the current state of an execution.
    fn get_current_execution_state(&self, exec_id: &str) -> Result<String, WorkflowEngineError> {
        if self.db_path.is_some() {
            let state: Result<String, rusqlite::Error> = self.ledger_conn.query_row(
                "SELECT state FROM harvest_executions WHERE exec_id = ?",
                [exec_id],
                |r| r.get(0),
            );
            if let Ok(st) = state {
                return Ok(st);
            }
        }
        let exec_uuid = exec_id
            .parse::<autumn_harvest::ExecutionId>()
            .map_err(|e| {
                WorkflowEngineError::ExecutionFailed(format!("invalid execution ID: {e}"))
            })?;
        match self.runtime.outcome(exec_uuid) {
            Ok(autumn_harvest_sqlite::ExecutionOutcome::Completed(_)) => {
                Ok("COMPLETED".to_string())
            }
            Ok(autumn_harvest_sqlite::ExecutionOutcome::Failed(e)) => Ok(format!("FAILED({e})")),
            Ok(autumn_harvest_sqlite::ExecutionOutcome::Terminated(s)) => Ok(s),
            Ok(autumn_harvest_sqlite::ExecutionOutcome::Running) => Ok("RUNNING".to_string()),
            Err(e) => Err(WorkflowEngineError::Sqlite(e)),
        }
    }

    /// List all durable signals received for a given execution from the signal ledger.
    pub fn list_signals_for_execution(
        &self,
        exec_id: &str,
    ) -> Result<Vec<StoredSignalRecord>, WorkflowEngineError> {
        let mut sig_stmt = self.ledger_conn.prepare(
            "SELECT exec_id, signal_name, idempotency_key, payload_json, received_at \
             FROM workflow_signal_ledger WHERE exec_id = ? ORDER BY rowid ASC",
        )?;
        let sig_rows = sig_stmt.query_map([exec_id], |r| {
            Ok(StoredSignalRecord {
                exec_id: r.get(0)?,
                signal_name: r.get(1)?,
                idempotency_key: r.get(2)?,
                payload_json: r.get(3)?,
                received_at: r.get(4)?,
            })
        })?;
        let mut signals = Vec::new();
        for sig in sig_rows {
            signals.push(sig?);
        }
        Ok(signals)
    }
}

/// List all workflow executions directly from the database file without acquiring a runtime lock.
pub fn list_executions_from_db(
    db_path: &Path,
) -> Result<Vec<WorkflowExecutionSummary>, WorkflowEngineError> {
    if !db_path.exists() {
        return Ok(Vec::new());
    }

    let conn = Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;

    let table_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='harvest_executions'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .map(|c| c > 0)
        .unwrap_or(false);

    if !table_exists {
        return Ok(Vec::new());
    }

    let mut stmt = conn.prepare(
        "SELECT e.exec_id, e.workflow_name, e.workflow_id, e.state, \
         (SELECT COUNT(*) FROM harvest_events ev WHERE ev.exec_id = e.exec_id) AS event_count, \
         (SELECT COALESCE(SUM(LENGTH(ev.event_json)), 0) FROM harvest_events ev WHERE ev.exec_id = e.exec_id) AS byte_size \
         FROM harvest_executions e ORDER BY e.rowid DESC",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(WorkflowExecutionSummary {
            exec_id: row.get(0)?,
            workflow_name: row.get(1)?,
            workflow_id: row.get(2)?,
            state: row.get(3)?,
            event_count: row.get::<_, i64>(4).unwrap_or(0).max(0) as usize,
            byte_size: row.get::<_, i64>(5).unwrap_or(0).max(0) as usize,
        })
    })?;

    let mut list = Vec::new();
    for row in rows {
        list.push(row?);
    }
    Ok(list)
}

/// Retrieve detailed execution status and history from the database file without acquiring a runtime lock.
pub fn get_execution_detail_from_db(
    db_path: &Path,
    exec_id: &str,
) -> Result<WorkflowExecutionDetail, WorkflowEngineError> {
    if !db_path.exists() {
        return Err(WorkflowEngineError::NotFound(format!(
            "database file does not exist: {}",
            db_path.display()
        )));
    }

    let conn = Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;

    let mut stmt = conn.prepare(
        "SELECT exec_id, workflow_name, workflow_id, state, input_json, output_json, error \
         FROM harvest_executions WHERE exec_id = ?",
    )?;

    let mut rows = stmt.query([exec_id])?;
    let row = match rows.next()? {
        Some(r) => r,
        None => return Err(WorkflowEngineError::NotFound(exec_id.to_string())),
    };

    let exec_id_str: String = row.get(0)?;
    let workflow_name: String = row.get(1)?;
    let workflow_id: String = row.get(2)?;
    let state: String = row.get(3)?;
    let input_json: String = row.get(4)?;
    let output_json: Option<String> = row.get(5)?;
    let error: Option<String> = row.get(6)?;

    let mut ev_stmt =
        conn.prepare("SELECT event_json FROM harvest_events WHERE exec_id = ? ORDER BY seq ASC")?;
    let ev_rows = ev_stmt.query_map([exec_id], |r| r.get::<_, String>(0))?;

    let mut events = Vec::new();
    let mut byte_size = 0;
    for ev in ev_rows {
        let json_str = ev?;
        byte_size += json_str.len();
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&json_str) {
            events.push(parsed);
        }
    }
    let event_count = events.len();

    let signal_table_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='workflow_signal_ledger'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .map(|c| c > 0)
        .unwrap_or(false);

    let mut signals = Vec::new();
    if signal_table_exists {
        let mut sig_stmt = conn.prepare(
            "SELECT exec_id, signal_name, idempotency_key, payload_json, received_at \
             FROM workflow_signal_ledger WHERE exec_id = ? ORDER BY rowid ASC",
        )?;
        let sig_rows = sig_stmt.query_map([exec_id], |r| {
            Ok(StoredSignalRecord {
                exec_id: r.get(0)?,
                signal_name: r.get(1)?,
                idempotency_key: r.get(2)?,
                payload_json: r.get(3)?,
                received_at: r.get(4)?,
            })
        })?;
        for sig in sig_rows {
            signals.push(sig?);
        }
    }

    Ok(WorkflowExecutionDetail {
        exec_id: exec_id_str,
        workflow_name,
        workflow_id,
        state,
        input_json,
        output_json,
        error,
        event_count,
        byte_size,
        events,
        signals,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::audit::AuditReport;
    use crate::workflow::triage::TriageReport;
    use crate::workflow::{OperatorDecision, OperatorVerdict, ReviewGateInput, ReviewGateReport};

    #[tokio::test]
    async fn test_engine_run_triage_workflow_in_memory() {
        let mut engine = WorkflowEngine::open_in_memory().expect("open in-memory engine");
        let input = TriageInput {
            test_signature: "tests::test_memory_triage".to_string(),
            target_repo: Some("cicatrix".to_string()),
            candidate_commits: vec!["sha_a".to_string(), "sha_b".to_string()],
            failure_log: "assertion failed: `(left == right)`\n --> src/store/sqlite.rs:100:1"
                .to_string(),
            reproducer_file: None,
            require_operator_review: false,
        };

        let report: WorkflowExecutionReport<TriageReport> = engine
            .run_workflow("triage_workflow", input)
            .await
            .expect("workflow execution should succeed");

        assert_eq!(report.state, "COMPLETED");
        assert!(report.event_count > 0);
        assert!(report.byte_size > 0);
        let output = report.output.expect("triage output present");
        assert_eq!(output.test_signature, "tests::test_memory_triage");
        assert_eq!(output.bisection.culprit_commit, Some("sha_b".to_string()));
        let bug_fact = output
            .candidate_bug_fact
            .expect("candidate bug fact present");
        assert_eq!(bug_fact.id, "BUG_TESTS_TEST_MEMORY_TRIAGE");
    }

    #[tokio::test]
    async fn test_engine_run_audit_workflow_in_memory() {
        let mut engine = WorkflowEngine::open_in_memory().expect("open in-memory engine");
        let input = crate::workflow::audit::AuditInput {
            repo_paths: vec![".".to_string()],
            convention_marker: Some("DRIFT".to_string()),
            scan_labels: vec![],
        };

        let report: WorkflowExecutionReport<AuditReport> = engine
            .run_workflow("audit_workflow", input)
            .await
            .expect("audit workflow should succeed");

        assert_eq!(report.state, "COMPLETED");
        assert!(report.event_count > 0);
        let output = report.output.expect("audit output present");
        assert_eq!(output.scanned_targets, 1);
    }

    #[test]
    fn test_safety_bounds_limits() {
        assert!(WorkflowEngine::check_history_safety_bounds(100, 1024).is_ok());
        assert!(WorkflowEngine::check_history_safety_bounds(
            MAX_WORKFLOW_EVENTS,
            MAX_WORKFLOW_BYTES
        )
        .is_ok());

        let event_err = WorkflowEngine::check_history_safety_bounds(MAX_WORKFLOW_EVENTS + 1, 100);
        match event_err {
            Err(WorkflowEngineError::EventLimitExceeded { count, max }) => {
                assert_eq!(count, MAX_WORKFLOW_EVENTS + 1);
                assert_eq!(max, MAX_WORKFLOW_EVENTS);
            }
            other => panic!("expected EventLimitExceeded, got {other:?}"),
        }

        let byte_err = WorkflowEngine::check_history_safety_bounds(100, MAX_WORKFLOW_BYTES + 1);
        match byte_err {
            Err(WorkflowEngineError::ByteLimitExceeded { bytes, max }) => {
                assert_eq!(bytes, MAX_WORKFLOW_BYTES + 1);
                assert_eq!(max, MAX_WORKFLOW_BYTES);
            }
            other => panic!("expected ByteLimitExceeded, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_on_disk_engine_and_read_only_queries() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let db_path = temp_dir.path().join("test_workflows.db");

        let exec_id = {
            let mut engine = WorkflowEngine::open(&db_path).expect("open on-disk engine");
            let input = TriageInput {
                test_signature: "tests::test_disk_triage".to_string(),
                target_repo: None,
                candidate_commits: vec!["commit_x".to_string()],
                failure_log: "panicked at 'boom'\n --> src/main.rs:10:1".to_string(),
                reproducer_file: None,
                require_operator_review: false,
            };

            let report: WorkflowExecutionReport<TriageReport> = engine
                .run_workflow("triage_workflow", input)
                .await
                .expect("workflow run should succeed");

            assert_eq!(report.state, "COMPLETED");
            report.exec_id
        };

        // Test lock-free read-only queries
        let list = list_executions_from_db(&db_path).expect("list executions from db");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].exec_id, exec_id);
        assert_eq!(list[0].workflow_name, "triage_workflow");
        assert_eq!(list[0].state, "COMPLETED");
        assert!(list[0].event_count > 0);

        let detail = get_execution_detail_from_db(&db_path, &exec_id).expect("get detail");
        assert_eq!(detail.exec_id, exec_id);
        assert_eq!(detail.state, "COMPLETED");
        assert!(detail.input_json.contains("tests::test_disk_triage"));
        assert!(detail.output_json.is_some());
        assert!(!detail.events.is_empty());

        let not_found = get_execution_detail_from_db(&db_path, "missing_exec_id");
        match not_found {
            Err(WorkflowEngineError::NotFound(id)) => assert_eq!(id, "missing_exec_id"),
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_review_gate_parking_and_signal_delivery() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let db_path = temp_dir.path().join("review_gate.db");

        let mut engine = WorkflowEngine::open(&db_path).expect("open on-disk engine");

        let input = ReviewGateInput {
            target_ref: "refs/heads/main".to_string(),
            description: Some("Deploy approval gate".to_string()),
            requested_by: Some("alice".to_string()),
        };

        let report: WorkflowExecutionReport<ReviewGateReport> = engine
            .run_workflow("review_gate_workflow", input)
            .await
            .expect("run review gate workflow");

        assert_eq!(report.state, "WAITING_SIGNAL(operator_verdict)");
        assert!(report.output.is_none());
        let exec_id = report.exec_id;

        // Verify read-only status shows WAITING_SIGNAL
        let detail = get_execution_detail_from_db(&db_path, &exec_id).expect("get detail");
        assert_eq!(detail.state, "WAITING_SIGNAL(operator_verdict)");
        assert!(detail.signals.is_empty());

        // Deliver approved verdict
        let verdict = OperatorVerdict::new(
            OperatorDecision::Approved,
            "ctodie".to_string(),
            Some("Production deploy ratified".to_string()),
        )
        .expect("valid verdict");

        let signal = DurableSignal::new(
            "operator_verdict",
            "idem-key-prod-001".to_string(),
            verdict.clone(),
        )
        .expect("valid signal");

        let delivery = engine
            .deliver_signal(&exec_id, &signal)
            .await
            .expect("deliver signal");

        assert_eq!(delivery.status, SignalDeliveryStatus::Delivered);
        assert_eq!(delivery.run_state, "COMPLETED");

        // Verify history and detail updated
        let updated_detail =
            get_execution_detail_from_db(&db_path, &exec_id).expect("get updated detail");
        assert_eq!(updated_detail.state, "COMPLETED");
        assert_eq!(updated_detail.signals.len(), 1);
        assert_eq!(
            updated_detail.signals[0].idempotency_key,
            "idem-key-prod-001"
        );

        // Attempt duplicate delivery with identical idempotency key
        let dup_delivery = engine
            .deliver_signal(&exec_id, &signal)
            .await
            .expect("duplicate delivery");

        assert_eq!(dup_delivery.status, SignalDeliveryStatus::Duplicate);
        assert_eq!(dup_delivery.run_state, "COMPLETED");

        // Verify signal count did not increase
        let ledger_signals = engine
            .list_signals_for_execution(&exec_id)
            .expect("list signals");
        assert_eq!(ledger_signals.len(), 1);
    }

    #[tokio::test]
    async fn test_signal_validation_failure() {
        let mut engine = WorkflowEngine::open_in_memory().expect("in-memory engine");

        // Empty operator fails schema validation (:db/neverZeroValue)
        let invalid_verdict = OperatorVerdict {
            decision: OperatorDecision::Approved,
            operator: "  ".to_string(),
            comments: Some("approved".to_string()),
            timestamp: "2026-10-10T00:00:00Z".to_string(),
        };
        assert!(invalid_verdict.validate().is_err());

        // Empty signal name fails schema validation (:db/neverZeroValue)
        let signal = DurableSignal {
            signal_name: "   ".to_string(),
            idempotency_key: "idem-err-1".to_string(),
            payload: invalid_verdict,
        };

        let err = engine.deliver_signal("nonexistent-exec-id", &signal).await;
        assert!(err.is_err());
        let err_msg = err.err().unwrap().to_string();
        assert!(err_msg.contains(":db/neverZeroValue"));
    }
}
