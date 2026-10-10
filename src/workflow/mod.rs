//! Autumn Harvest durable workflow engine integration (CER-2756, Phase 2.1).
//!
//! Provides deterministic event-sourced workflow execution backed by embedded
//! `autumn-harvest-sqlite`. Implements regression triage and convention audit workflows,
//! enforcing safety bounds on event history size, activity timeouts, and retry policies.

pub mod audit;
pub mod engine;
pub mod signal;
pub mod triage;

pub use audit::{audit_workflow_info, AuditInput, AuditReport, TargetScanResult};
pub use engine::{
    get_execution_detail_from_db, init_signal_ledger_table, list_executions_from_db,
    SignalDeliveryReport, WorkflowEngine, WorkflowEngineError, WorkflowExecutionDetail,
    WorkflowExecutionReport, WorkflowExecutionSummary,
};
pub use signal::{
    DurableSignal, OperatorDecision, OperatorVerdict, Signal, SignalDeliveryStatus,
    StoredSignalRecord,
};
pub use triage::{
    review_gate_workflow, review_gate_workflow_info, triage_workflow_info, BisectionResult,
    ReviewGateInput, ReviewGateReport, TriageInput, TriageNormalizedFailure, TriageReport,
};

pub use crate::mcp::{mcp_tools, McpToolDefinition};

use std::path::PathBuf;

/// Maximum number of events allowed in a single workflow execution history.
pub const MAX_WORKFLOW_EVENTS: usize = 50_000;

/// Maximum serialized event log size in bytes (50 MiB).
pub const MAX_WORKFLOW_BYTES: usize = 50 * 1024 * 1024;

/// Maximum activity execution timeout in seconds (10 minutes).
pub const ACTIVITY_TIMEOUT_SECS: u64 = 600;

/// Maximum number of bisection evaluation steps allowed per triage execution.
pub const MAX_BISECTION_STEPS: usize = 3;

/// Default workflow persistence database filename under `~/.cicatrix/`.
pub const DEFAULT_WORKFLOW_DB_NAME: &str = "cicatrix_workflows.db";

/// Resolve the default path to the workflow SQLite database.
///
/// Respects the `CICATRIX_WORKFLOW_DB` environment variable if set.
/// Otherwise defaults to `~/.cicatrix/cicatrix_workflows.db`.
pub fn default_workflow_db_path() -> std::io::Result<PathBuf> {
    if let Ok(env_path) = std::env::var("CICATRIX_WORKFLOW_DB") {
        if !env_path.trim().is_empty() {
            return Ok(PathBuf::from(env_path.trim()));
        }
    }

    let home = std::env::var("HOME").map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "cannot resolve HOME environment variable for workflow database path",
        )
    })?;

    let dir = PathBuf::from(home).join(".cicatrix");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join(DEFAULT_WORKFLOW_DB_NAME))
}
