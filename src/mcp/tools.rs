//! Core MCP regression and workflow tool implementations.
//!
//! Exposes:
//! - `cicatrix_query_known_bugs`
//! - `cicatrix_verify_diff`
//! - `cicatrix_record_defect`
//! - `cicatrix_start_workflow`
//! - `cicatrix_workflow_status`
//! - `cicatrix_submit_signal`
//! - `cicatrix_verify_reversibility`
//! - `cicatrix_check_tripwire`

use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::branch::{validate_snapshot_id, BranchManager};
use crate::gitf::{filter_as_of, filter_by_frontier};
use crate::mcp::protocol::McpToolDefinition;
use crate::reversibility::{AutonomyTier, ReversibilityPipeline};
use crate::store::sqlite::validate_never_zero_value;
use crate::store::{BugFact, Frontier, SqliteStore, StochasticSpec};
use crate::workflow::audit::{AuditInput, AuditReport};
use crate::workflow::default_workflow_db_path;
use crate::workflow::engine::{
    get_execution_detail_from_db, SignalDeliveryReport, WorkflowEngine, WorkflowExecutionDetail,
    WorkflowExecutionReport,
};
use crate::workflow::signal::{DurableSignal, OperatorDecision, OperatorVerdict};
use crate::workflow::triage::{ReviewGateInput, ReviewGateReport, TriageInput, TriageReport};

/// Error classification for tool execution: client-side validation vs internal error.
#[derive(Debug, Clone, PartialEq)]
pub enum ToolExecutionError {
    /// Client-side invalid argument, schema violation, or bad request.
    Client(String),
    /// Internal runtime or database failure (to be masked with a correlation id).
    Internal(String),
}

impl std::fmt::Display for ToolExecutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Client(msg) => write!(f, "client error: {msg}"),
            Self::Internal(msg) => write!(f, "internal error: {msg}"),
        }
    }
}

impl std::error::Error for ToolExecutionError {}

/// Return the complete list of MCP tool definitions provided by Cicatrix.
pub fn mcp_tools() -> Vec<McpToolDefinition> {
    vec![
        McpToolDefinition {
            name: "cicatrix_query_known_bugs".to_string(),
            description: "Query regression database for known bug facts touching specific file paths, optionally filtered by temporal frontier or commit ancestry.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "paths": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "List of changed or queried file paths"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of bug facts to return"
                    },
                    "frontier": {
                        "type": "string",
                        "description": "Version-vector frontier string (e.g. 'replicaA:42,replicaB:10')"
                    },
                    "as_of": {
                        "type": "string",
                        "description": "Commit SHA or comma-separated commit heads for historical filtering"
                    },
                    "branch": {
                        "type": "string",
                        "description": "Snapshot branch identifier to query an isolated branch database"
                    }
                },
                "required": ["paths"]
            }),
        },
        McpToolDefinition {
            name: "cicatrix_verify_diff".to_string(),
            description: "Analyze a unified git diff, extract touched files, match against known regression surfaces, and verify regression test coverage.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "diff": {
                        "type": "string",
                        "description": "Unified git diff patch string"
                    },
                    "branch": {
                        "type": "string",
                        "description": "Snapshot branch identifier"
                    },
                    "as_of": {
                        "type": "string",
                        "description": "Commit SHA for historical filtering"
                    },
                    "frontier": {
                        "type": "string",
                        "description": "Version-vector frontier string"
                    }
                },
                "required": ["diff"]
            }),
        },
        McpToolDefinition {
            name: "cicatrix_record_defect".to_string(),
            description: "Record a verified regression fact into the database, enforcing :db/neverZeroValue schema integrity constraints.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Canonical defect slug" },
                    "files": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "List of touched file paths"
                    },
                    "symptom": { "type": "string", "description": "Observable failure symptom" },
                    "fix_commit": { "type": "string", "description": "Commit SHA introducing the verified fix" },
                    "regression_test": { "type": "string", "description": "Path to executable regression test" },
                    "meta_pattern": { "type": "string", "description": "Roll-up architectural or rule pattern" },
                    "scope": { "type": "string", "description": "Optional blast-radius directory or crate prefix" },
                    "do_not_generalize": { "type": "boolean", "description": "Whether to exclude from generalized meta-pattern prompts" },
                    "reproducer": { "type": "string", "description": "Turnkey CLI command or flag to reproduce failure" },
                    "stochastic": {
                        "type": "object",
                        "description": "Structured stochastic failure telemetry and rerun policy"
                    },
                    "frontier": { "type": "string", "description": "Version-vector frontier timestamp" },
                    "branch": { "type": "string", "description": "Target snapshot branch identifier" }
                },
                "required": ["id", "files", "symptom", "fix_commit", "regression_test", "meta_pattern"]
            }),
        },
        McpToolDefinition {
            name: "cicatrix_start_workflow".to_string(),
            description: "Execute a durable Autumn Harvest workflow (triage, audit, or review-gate) backed by SQLite event log.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "workflow_name": {
                        "type": "string",
                        "enum": ["triage", "audit", "review-gate"],
                        "description": "Name of the durable workflow to execute"
                    },
                    "input": {
                        "type": "object",
                        "description": "Input payload for the specified workflow"
                    },
                    "db_path": {
                        "type": "string",
                        "description": "Optional custom path to the workflow SQLite database"
                    }
                },
                "required": ["workflow_name", "input"]
            }),
        },
        McpToolDefinition {
            name: "cicatrix_workflow_status".to_string(),
            description: "Inspect detailed execution status, history events, and signals of a durable workflow execution.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "exec_id": {
                        "type": "string",
                        "description": "Workflow execution UUID"
                    },
                    "db_path": {
                        "type": "string",
                        "description": "Optional custom path to the workflow SQLite database"
                    }
                },
                "required": ["exec_id"]
            }),
        },
        McpToolDefinition {
            name: "cicatrix_submit_signal".to_string(),
            description: "Deliver a durable signal to a parked or running workflow execution with deduplication on idempotency_key.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "exec_id": {
                        "type": "string",
                        "description": "Workflow execution UUID to receive the signal"
                    },
                    "verdict": {
                        "type": "string",
                        "enum": ["approve", "reject", "request-re-review"],
                        "description": "Operator verdict outcome"
                    },
                    "idempotency_key": {
                        "type": "string",
                        "description": "Unique key to deduplicate signal deliveries"
                    },
                    "operator": {
                        "type": "string",
                        "description": "Identity of operator delivering the signal"
                    },
                    "comments": {
                        "type": "string",
                        "description": "Optional notes accompanying the verdict"
                    },
                    "db_path": {
                        "type": "string",
                        "description": "Optional custom path to the workflow SQLite database"
                    }
                },
                "required": ["exec_id", "verdict", "idempotency_key"]
            }),
        },
        McpToolDefinition {
            name: "cicatrix_verify_reversibility".to_string(),
            description: "Evaluate reversibility of a unified git diff through the 4-stage Wheelhorse pipeline (classify, plan, validate, decide).".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "diff": {
                        "type": "string",
                        "description": "Unified git diff patch string to evaluate"
                    },
                    "tier": {
                        "type": "string",
                        "enum": ["shadow", "supervised", "autonomous"],
                        "description": "Autonomy tier for decision policy (default: supervised)"
                    },
                    "stage": {
                        "type": "string",
                        "enum": ["eval", "classify", "plan", "validate"],
                        "description": "Pipeline evaluation stage to return (default: eval)"
                    }
                },
                "required": ["diff"]
            }),
        },
        McpToolDefinition {
            name: "cicatrix_check_tripwire".to_string(),
            description: "Check if a target path or content touches any synthetic canary tripwires, verifying actor authorization and failing closed upon unauthorized intrusion.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "target": {
                        "type": "string",
                        "description": "Target file path or pattern to evaluate against tripwire registry"
                    },
                    "content": {
                        "type": "string",
                        "description": "Optional diff or file content to scan for canary sentinel markers"
                    },
                    "actor": {
                        "type": "string",
                        "description": "Identity or role of the agent or operator initiating the check (default: CICATRIX_ACTOR or 'agent')"
                    },
                    "action": {
                        "type": "string",
                        "description": "Action type being performed (default: 'check')"
                    },
                    "branch": {
                        "type": "string",
                        "description": "Optional snapshot branch identifier"
                    }
                },
                "required": ["target"]
            }),
        },
    ]
}

/// Helper to open the appropriate `SqliteStore` targeting either trunk or an isolated branch.
pub fn open_store_target(branch: Option<&str>) -> Result<SqliteStore, ToolExecutionError> {
    if let Some(branch_id) = branch {
        validate_snapshot_id(branch_id).map_err(|e| {
            ToolExecutionError::Client(format!("invalid branch id `{branch_id}`: {e}"))
        })?;
        let trunk_store = SqliteStore::open_default().map_err(|e| {
            ToolExecutionError::Internal(format!("failed to open trunk database: {e}"))
        })?;
        let manager = BranchManager::new(trunk_store).map_err(|e| {
            ToolExecutionError::Internal(format!("failed to init branch manager: {e}"))
        })?;
        let path = manager.branch_path(branch_id).map_err(|e| {
            ToolExecutionError::Client(format!(
                "failed to resolve branch path for `{branch_id}`: {e}"
            ))
        })?;
        SqliteStore::open(path).map_err(|e| {
            ToolExecutionError::Internal(format!("failed to open branch database: {e}"))
        })
    } else {
        SqliteStore::from_env().map_err(|e| {
            ToolExecutionError::Internal(format!("failed to open sqlite database: {e}"))
        })
    }
}

/// Parse touched file paths from a unified diff patch string.
pub fn parse_diff_touched_files(diff: &str) -> Vec<String> {
    let mut files = BTreeSet::new();
    for line in diff.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("diff --git ") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() >= 2 {
                let p1 = parts[0].strip_prefix("a/").unwrap_or(parts[0]);
                let p2 = parts[1].strip_prefix("b/").unwrap_or(parts[1]);
                if p1 != "/dev/null" && !p1.is_empty() {
                    files.insert(p1.to_string());
                }
                if p2 != "/dev/null" && !p2.is_empty() {
                    files.insert(p2.to_string());
                }
            }
        } else if let Some(rest) = trimmed.strip_prefix("--- ") {
            let path = rest.split_whitespace().next().unwrap_or(rest);
            let clean = path.strip_prefix("a/").unwrap_or(path);
            if clean != "/dev/null" && !clean.is_empty() {
                files.insert(clean.to_string());
            }
        } else if let Some(rest) = trimmed.strip_prefix("+++ ") {
            let path = rest.split_whitespace().next().unwrap_or(rest);
            let clean = path.strip_prefix("b/").unwrap_or(path);
            if clean != "/dev/null" && !clean.is_empty() {
                files.insert(clean.to_string());
            }
        }
    }
    files.into_iter().collect()
}

/// Check whether a file path appears to be an automated test.
pub fn is_test_file(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.contains("/tests/")
        || lower.starts_with("tests/")
        || lower.contains("/test/")
        || lower.starts_with("test/")
        || lower.ends_with("_test.rs")
        || lower.ends_with("tests.rs")
        || lower.ends_with("_spec.rs")
        || lower.ends_with(".test.ts")
        || lower.ends_with(".spec.ts")
        || lower.ends_with(".test.js")
        || lower.ends_with(".spec.js")
        || lower.contains("test_")
        || lower.ends_with("_test.py")
}

/// Dispatch and execute an MCP tool call by name.
pub async fn execute_tool(name: &str, args: &Value) -> Result<Value, ToolExecutionError> {
    match name {
        "cicatrix_query_known_bugs" => handle_query_known_bugs(args),
        "cicatrix_verify_diff" => handle_verify_diff(args),
        "cicatrix_record_defect" => handle_record_defect(args),
        "cicatrix_start_workflow" => handle_start_workflow(args).await,
        "cicatrix_workflow_status" => handle_workflow_status(args),
        "cicatrix_submit_signal" => handle_submit_signal(args).await,
        "cicatrix_verify_reversibility" => handle_verify_reversibility(args),
        "cicatrix_check_tripwire" => handle_check_tripwire(args),
        other => Err(ToolExecutionError::Client(format!(
            "unknown tool `{other}`"
        ))),
    }
}

fn handle_query_known_bugs(args: &Value) -> Result<Value, ToolExecutionError> {
    let paths_val = args.get("paths").ok_or_else(|| {
        ToolExecutionError::Client("missing required parameter `paths`".to_string())
    })?;
    let paths: Vec<String> = serde_json::from_value(paths_val.clone()).map_err(|e| {
        ToolExecutionError::Client(format!("parameter `paths` must be array of strings: {e}"))
    })?;

    if paths.is_empty() {
        return Err(ToolExecutionError::Client(
            "parameter `paths` cannot be empty".to_string(),
        ));
    }

    let branch = args.get("branch").and_then(Value::as_str);
    let as_of = args.get("as_of").and_then(Value::as_str);
    let frontier_str = args.get("frontier").and_then(Value::as_str);
    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .map(|l| l as usize);
    let actor = args
        .get("actor")
        .and_then(Value::as_str)
        .map(|s| s.to_string())
        .or_else(|| {
            std::env::var("CICATRIX_ACTOR")
                .ok()
                .map(|a| a.trim().to_string())
                .filter(|a| !a.is_empty())
        })
        .unwrap_or_else(|| "agent".to_string());

    let store = open_store_target(branch)?;

    // Fail-closed synthetic canary tripwire check (CER-2760, Phase 3.2)
    store
        .guard_paths(&paths, &actor, "mcp_query")
        .map_err(|e| ToolExecutionError::Client(format!("tripwire intrusion detected: {e}")))?;

    let mut hits = store.touches_known_bug(&paths).map_err(|e| {
        ToolExecutionError::Internal(format!("query touches_known_bug failed: {e}"))
    })?;

    let mut skipped = Vec::new();
    if let Some(f_str) = frontier_str {
        let frontier = Frontier::parse(f_str).map_err(|e| {
            ToolExecutionError::Client(format!("invalid `frontier` parameter: {e}"))
        })?;
        let (kept, sk) = filter_by_frontier(hits, &frontier);
        hits = kept;
        skipped.extend(sk);
    }

    if let Some(commit) = as_of {
        let (kept, sk) = filter_as_of(hits, commit);
        hits = kept;
        skipped.extend(sk);
    }

    if let Some(lim) = limit {
        hits.truncate(lim);
    }

    let count = hits.len();
    Ok(json!({
        "count": count,
        "facts": hits,
        "skipped": skipped,
        "queried_paths": paths,
    }))
}

fn handle_verify_diff(args: &Value) -> Result<Value, ToolExecutionError> {
    let diff = args.get("diff").and_then(Value::as_str).ok_or_else(|| {
        ToolExecutionError::Client("missing required parameter `diff`".to_string())
    })?;

    let branch = args.get("branch").and_then(Value::as_str);
    let as_of = args.get("as_of").and_then(Value::as_str);
    let frontier_str = args.get("frontier").and_then(Value::as_str);
    let actor = args
        .get("actor")
        .and_then(Value::as_str)
        .map(|s| s.to_string())
        .or_else(|| {
            std::env::var("CICATRIX_ACTOR")
                .ok()
                .map(|a| a.trim().to_string())
                .filter(|a| !a.is_empty())
        })
        .unwrap_or_else(|| "agent".to_string());

    let touched_files = parse_diff_touched_files(diff);
    if touched_files.is_empty() {
        return Ok(json!({
            "touched_files": [],
            "matched_bugs": [],
            "has_test_coverage": false,
            "risk_level": "low",
            "recommendations": ["No touched files parsed from unified diff."],
        }));
    }

    let has_test_coverage = touched_files.iter().any(|f| is_test_file(f));

    let store = open_store_target(branch)?;

    // Fail-closed synthetic canary tripwire check (CER-2760, Phase 3.2)
    store
        .guard_diff(diff, &actor)
        .map_err(|e| ToolExecutionError::Client(format!("tripwire intrusion detected: {e}")))?;

    let mut hits = store
        .touches_known_bug(&touched_files)
        .map_err(|e| ToolExecutionError::Internal(format!("touches_known_bug failed: {e}")))?;

    if let Some(f_str) = frontier_str {
        let frontier = Frontier::parse(f_str).map_err(|e| {
            ToolExecutionError::Client(format!("invalid `frontier` parameter: {e}"))
        })?;
        let (kept, _) = filter_by_frontier(hits, &frontier);
        hits = kept;
    }

    if let Some(commit) = as_of {
        let (kept, _) = filter_as_of(hits, commit);
        hits = kept;
    }

    let mut recommendations = Vec::new();
    let risk_level = if !hits.is_empty() {
        for bug in &hits {
            recommendations.push(format!(
                "Regression risk in bug `{}`: run test `{}` and verify symptom `{}`",
                bug.id, bug.regression_test, bug.symptom
            ));
        }
        if has_test_coverage {
            "medium"
        } else {
            recommendations.push(
                "Touched regression-sensitive files without new or updated test coverage"
                    .to_string(),
            );
            "high"
        }
    } else if has_test_coverage {
        recommendations.push(
            "Diff includes automated test coverage and touches no known bug surfaces.".to_string(),
        );
        "low"
    } else {
        recommendations.push(
            "No known regression surface touched, but diff lacks automated tests.".to_string(),
        );
        "medium"
    };

    Ok(json!({
        "touched_files": touched_files,
        "matched_bugs": hits,
        "has_test_coverage": has_test_coverage,
        "risk_level": risk_level,
        "recommendations": recommendations,
    }))
}

fn handle_record_defect(args: &Value) -> Result<Value, ToolExecutionError> {
    let id = args
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolExecutionError::Client("missing required parameter `id`".to_string()))?
        .to_string();

    let files_val = args.get("files").ok_or_else(|| {
        ToolExecutionError::Client("missing required parameter `files`".to_string())
    })?;
    let files: Vec<String> = serde_json::from_value(files_val.clone()).map_err(|e| {
        ToolExecutionError::Client(format!("parameter `files` must be array of strings: {e}"))
    })?;

    let symptom = args
        .get("symptom")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ToolExecutionError::Client("missing required parameter `symptom`".to_string())
        })?
        .to_string();

    let fix_commit = args
        .get("fix_commit")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ToolExecutionError::Client("missing required parameter `fix_commit`".to_string())
        })?
        .to_string();

    let regression_test = args
        .get("regression_test")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ToolExecutionError::Client("missing required parameter `regression_test`".to_string())
        })?
        .to_string();

    let meta_pattern = args
        .get("meta_pattern")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ToolExecutionError::Client("missing required parameter `meta_pattern`".to_string())
        })?
        .to_string();

    let scope = args
        .get("scope")
        .and_then(Value::as_str)
        .map(str::to_string);
    let do_not_generalize = args
        .get("do_not_generalize")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let reproducer = args
        .get("reproducer")
        .and_then(Value::as_str)
        .map(str::to_string);

    let stochastic: Option<StochasticSpec> = if let Some(st_val) = args.get("stochastic") {
        if st_val.is_null() {
            None
        } else {
            Some(serde_json::from_value(st_val.clone()).map_err(|e| {
                ToolExecutionError::Client(format!("invalid `stochastic` specification: {e}"))
            })?)
        }
    } else {
        None
    };

    let frontier = if let Some(f_str) = args.get("frontier").and_then(Value::as_str) {
        Some(Frontier::parse(f_str).map_err(|e| {
            ToolExecutionError::Client(format!("invalid `frontier` specification: {e}"))
        })?)
    } else {
        None
    };

    let branch = args.get("branch").and_then(Value::as_str);

    let fact = BugFact {
        id: id.clone(),
        files,
        symptom,
        fix_commit,
        regression_test,
        meta_pattern,
        scope,
        do_not_generalize,
        reproducer,
        stochastic,
        frontier,
    };

    validate_never_zero_value(&fact).map_err(|e| {
        ToolExecutionError::Client(format!(":db/neverZeroValue validation failed: {e}"))
    })?;

    let mut store = open_store_target(branch)?;
    store
        .record(&fact)
        .map_err(|e| ToolExecutionError::Internal(format!("failed to record bug fact: {e}")))?;

    Ok(json!({
        "status": "recorded",
        "id": id,
        "files_count": fact.files.len(),
        "branch": branch,
    }))
}

async fn handle_start_workflow(args: &Value) -> Result<Value, ToolExecutionError> {
    let workflow_name = args
        .get("workflow_name")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ToolExecutionError::Client("missing required parameter `workflow_name`".to_string())
        })?;

    let input_val = args.get("input").ok_or_else(|| {
        ToolExecutionError::Client("missing required parameter `input`".to_string())
    })?;

    let db_path_buf = if let Some(p) = args.get("db_path").and_then(Value::as_str) {
        PathBuf::from(p)
    } else {
        default_workflow_db_path().map_err(|e| {
            ToolExecutionError::Internal(format!("failed to resolve workflow db path: {e}"))
        })?
    };

    let mut engine = WorkflowEngine::open(&db_path_buf).map_err(|e| {
        ToolExecutionError::Internal(format!("failed to open workflow engine: {e}"))
    })?;

    match workflow_name {
        "triage" | "triage_workflow" => {
            let input: TriageInput = serde_json::from_value(input_val.clone()).map_err(|e| {
                ToolExecutionError::Client(format!("invalid input for triage workflow: {e}"))
            })?;
            let report: WorkflowExecutionReport<TriageReport> = engine
                .run_workflow("triage_workflow", input)
                .await
                .map_err(|e| {
                    ToolExecutionError::Internal(format!("triage workflow execution failed: {e}"))
                })?;
            serde_json::to_value(report).map_err(|e| {
                ToolExecutionError::Internal(format!("failed to serialize report: {e}"))
            })
        }
        "audit" | "audit_workflow" => {
            let input: AuditInput = serde_json::from_value(input_val.clone()).map_err(|e| {
                ToolExecutionError::Client(format!("invalid input for audit workflow: {e}"))
            })?;
            let report: WorkflowExecutionReport<AuditReport> = engine
                .run_workflow("audit_workflow", input)
                .await
                .map_err(|e| {
                    ToolExecutionError::Internal(format!("audit workflow execution failed: {e}"))
                })?;
            serde_json::to_value(report).map_err(|e| {
                ToolExecutionError::Internal(format!("failed to serialize report: {e}"))
            })
        }
        "review-gate" | "review_gate" | "review_gate_workflow" => {
            let input: ReviewGateInput =
                serde_json::from_value(input_val.clone()).map_err(|e| {
                    ToolExecutionError::Client(format!(
                        "invalid input for review-gate workflow: {e}"
                    ))
                })?;
            let report: WorkflowExecutionReport<ReviewGateReport> = engine
                .run_workflow("review_gate_workflow", input)
                .await
                .map_err(|e| {
                    ToolExecutionError::Internal(format!(
                        "review-gate workflow execution failed: {e}"
                    ))
                })?;
            serde_json::to_value(report).map_err(|e| {
                ToolExecutionError::Internal(format!("failed to serialize report: {e}"))
            })
        }
        other => Err(ToolExecutionError::Client(format!(
            "unsupported workflow `{other}`; expected triage, audit, or review-gate"
        ))),
    }
}

fn handle_workflow_status(args: &Value) -> Result<Value, ToolExecutionError> {
    let exec_id = args.get("exec_id").and_then(Value::as_str).ok_or_else(|| {
        ToolExecutionError::Client("missing required parameter `exec_id`".to_string())
    })?;

    let db_path_buf = if let Some(p) = args.get("db_path").and_then(Value::as_str) {
        PathBuf::from(p)
    } else {
        default_workflow_db_path().map_err(|e| {
            ToolExecutionError::Internal(format!("failed to resolve workflow db path: {e}"))
        })?
    };

    let detail: WorkflowExecutionDetail = get_execution_detail_from_db(&db_path_buf, exec_id)
        .map_err(|e| {
            ToolExecutionError::Internal(format!("failed to inspect workflow execution: {e}"))
        })?;

    serde_json::to_value(detail)
        .map_err(|e| ToolExecutionError::Internal(format!("failed to serialize detail: {e}")))
}

async fn handle_submit_signal(args: &Value) -> Result<Value, ToolExecutionError> {
    let exec_id = args.get("exec_id").and_then(Value::as_str).ok_or_else(|| {
        ToolExecutionError::Client("missing required parameter `exec_id`".to_string())
    })?;

    let verdict_str = args.get("verdict").and_then(Value::as_str).ok_or_else(|| {
        ToolExecutionError::Client("missing required parameter `verdict`".to_string())
    })?;

    let idempotency_key = args
        .get("idempotency_key")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ToolExecutionError::Client("missing required parameter `idempotency_key`".to_string())
        })?
        .to_string();

    let operator = args
        .get("operator")
        .and_then(Value::as_str)
        .unwrap_or("operator")
        .to_string();

    let comments = args
        .get("comments")
        .and_then(Value::as_str)
        .map(|s| s.to_string())
        .unwrap_or_else(|| "Operator verdict submitted via MCP".to_string());

    let decision = match verdict_str.to_lowercase().as_str() {
        "approve" | "approved" => OperatorDecision::Approved,
        "reject" | "rejected" => OperatorDecision::Rejected,
        "changes_requested" | "changes-requested" | "request_changes" | "changes"
        | "request-re-review" | "rereview" | "re-review" => OperatorDecision::ChangesRequested,
        other => {
            return Err(ToolExecutionError::Client(format!(
                "invalid verdict `{other}`; expected approved, rejected, or changes_requested"
            )))
        }
    };

    let verdict = OperatorVerdict::new(decision, operator, Some(comments))
        .map_err(|e| ToolExecutionError::Client(format!("invalid operator verdict: {e}")))?;

    let signal = DurableSignal::new("operator_verdict", idempotency_key, verdict)
        .map_err(|e| ToolExecutionError::Client(format!("invalid durable signal: {e}")))?;

    let db_path_buf = if let Some(p) = args.get("db_path").and_then(Value::as_str) {
        PathBuf::from(p)
    } else {
        default_workflow_db_path().map_err(|e| {
            ToolExecutionError::Internal(format!("failed to resolve workflow db path: {e}"))
        })?
    };

    let mut engine = WorkflowEngine::open(&db_path_buf).map_err(|e| {
        ToolExecutionError::Internal(format!("failed to open workflow engine: {e}"))
    })?;

    let report: SignalDeliveryReport = engine
        .deliver_signal(exec_id, &signal)
        .await
        .map_err(|e| ToolExecutionError::Internal(format!("signal delivery failed: {e}")))?;

    serde_json::to_value(report).map_err(|e| {
        ToolExecutionError::Internal(format!("failed to serialize signal report: {e}"))
    })
}

fn handle_verify_reversibility(args: &Value) -> Result<Value, ToolExecutionError> {
    let diff = args.get("diff").and_then(Value::as_str).ok_or_else(|| {
        ToolExecutionError::Client("missing required parameter `diff`".to_string())
    })?;

    if diff.trim().is_empty() {
        return Err(ToolExecutionError::Client(
            "parameter `diff` cannot be empty".to_string(),
        ));
    }

    let branch = args.get("branch").and_then(Value::as_str);
    let actor = args
        .get("actor")
        .and_then(Value::as_str)
        .map(|s| s.to_string())
        .or_else(|| {
            std::env::var("CICATRIX_ACTOR")
                .ok()
                .map(|a| a.trim().to_string())
                .filter(|a| !a.is_empty())
        })
        .unwrap_or_else(|| "agent".to_string());

    let store = open_store_target(branch)?;
    store
        .guard_diff(diff, &actor)
        .map_err(|e| ToolExecutionError::Client(format!("tripwire intrusion detected: {e}")))?;

    let tier_str = args
        .get("tier")
        .and_then(Value::as_str)
        .unwrap_or("supervised");
    let tier: AutonomyTier = tier_str
        .parse()
        .map_err(|e| ToolExecutionError::Client(format!("invalid `tier` parameter: {e}")))?;

    let stage = args.get("stage").and_then(Value::as_str).unwrap_or("eval");
    let pipeline = ReversibilityPipeline::new(tier);

    match stage {
        "classify" => {
            let res = pipeline.classify(diff).map_err(|e| {
                ToolExecutionError::Internal(format!("reversibility classification failed: {e}"))
            })?;
            serde_json::to_value(res).map_err(|e| {
                ToolExecutionError::Internal(format!("failed to serialize classification: {e}"))
            })
        }
        "plan" => {
            let res = pipeline.plan(diff).map_err(|e| {
                ToolExecutionError::Internal(format!("reversibility plan generation failed: {e}"))
            })?;
            serde_json::to_value(res)
                .map_err(|e| ToolExecutionError::Internal(format!("failed to serialize plan: {e}")))
        }
        "validate" => {
            let res = pipeline.validate(diff).map_err(|e| {
                ToolExecutionError::Internal(format!("reversibility validation failed: {e}"))
            })?;
            serde_json::to_value(res).map_err(|e| {
                ToolExecutionError::Internal(format!("failed to serialize validation: {e}"))
            })
        }
        "eval" | "evaluate" => {
            let res = pipeline.evaluate(diff).map_err(|e| {
                ToolExecutionError::Internal(format!("reversibility evaluation failed: {e}"))
            })?;
            serde_json::to_value(res).map_err(|e| {
                ToolExecutionError::Internal(format!("failed to serialize evaluation report: {e}"))
            })
        }
        other => Err(ToolExecutionError::Client(format!(
            "invalid `stage` `{other}`; expected: eval, classify, plan, or validate"
        ))),
    }
}

fn handle_check_tripwire(args: &Value) -> Result<Value, ToolExecutionError> {
    let target = args.get("target").and_then(Value::as_str).ok_or_else(|| {
        ToolExecutionError::Client("missing required parameter `target`".to_string())
    })?;
    let content = args.get("content").and_then(Value::as_str);
    let branch = args.get("branch").and_then(Value::as_str);
    let actor = args
        .get("actor")
        .and_then(Value::as_str)
        .map(|s| s.to_string())
        .or_else(|| {
            std::env::var("CICATRIX_ACTOR")
                .ok()
                .map(|a| a.trim().to_string())
                .filter(|a| !a.is_empty())
        })
        .unwrap_or_else(|| "agent".to_string());
    let action = args
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("check");

    let store = open_store_target(branch)?;
    match store.guard_check(target, content, &actor, action) {
        Ok(()) => Ok(json!({
            "status": "clear",
            "verdict": "permitted",
            "target": target,
            "actor": actor,
            "action": action,
        })),
        Err(e) => Err(ToolExecutionError::Client(format!(
            "tripwire intrusion detected: {e}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_tools_list_completeness() {
        let tools = mcp_tools();
        assert_eq!(tools.len(), 8);
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"cicatrix_query_known_bugs"));
        assert!(names.contains(&"cicatrix_verify_diff"));
        assert!(names.contains(&"cicatrix_record_defect"));
        assert!(names.contains(&"cicatrix_verify_reversibility"));
        assert!(names.contains(&"cicatrix_check_tripwire"));
        assert!(names.contains(&"cicatrix_start_workflow"));
        assert!(names.contains(&"cicatrix_workflow_status"));
        assert!(names.contains(&"cicatrix_submit_signal"));
        assert!(names.contains(&"cicatrix_verify_reversibility"));
    }

    #[test]
    fn test_handle_verify_reversibility() {
        let diff = r#"
diff --git a/tests/test_foo.rs b/tests/test_foo.rs
new file mode 100644
index 0000000..1111111
--- /dev/null
+++ b/tests/test_foo.rs
@@ -0,0 +1,3 @@
+#[test]
+fn test_smoke() {}
"#;
        let args = json!({
            "diff": diff,
            "tier": "autonomous",
            "stage": "eval"
        });
        let res = handle_verify_reversibility(&args).expect("tool execution should succeed");
        assert_eq!(res["verdict"]["verdict"], "auto_commit");
        assert_eq!(res["action_class"]["type"], "reversible_additive");
        assert_eq!(res["touched_files"], json!(["tests/test_foo.rs"]));
    }

    #[test]
    fn test_parse_diff_touched_files() {
        let diff = r#"
diff --git a/src/main.rs b/src/main.rs
index 1234567..89abcdef 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,4 @@
+// new line
diff --git a/tests/cli.rs b/tests/cli.rs
new file mode 100644
--- /dev/null
+++ b/tests/cli.rs
@@ -0,0 +1,5 @@
+// test
"#;
        let files = parse_diff_touched_files(diff);
        assert_eq!(files.len(), 2);
        assert!(files.contains(&"src/main.rs".to_string()));
        assert!(files.contains(&"tests/cli.rs".to_string()));
    }

    #[test]
    fn test_is_test_file_detection() {
        assert!(is_test_file("tests/cli.rs"));
        assert!(is_test_file("src/tests.rs"));
        assert!(is_test_file("src/mcp_test.rs"));
        assert!(is_test_file("tests/integration/test_suite.rs"));
        assert!(is_test_file("frontend/src/app.test.ts"));
        assert!(!is_test_file("src/main.rs"));
        assert!(!is_test_file("src/mcp/protocol.rs"));
    }

    #[test]
    fn test_handle_record_defect_rejects_empty_fields() {
        let invalid_args = json!({
            "id": "BUG_TEST",
            "files": [""],
            "symptom": "symptom",
            "fix_commit": "abc1234",
            "regression_test": "test",
            "meta_pattern": "pattern"
        });
        let res = handle_record_defect(&invalid_args);
        assert!(res.is_err());
        match res.unwrap_err() {
            ToolExecutionError::Client(msg) => {
                assert!(msg.contains(":db/neverZeroValue violation"));
            }
            ToolExecutionError::Internal(_) => panic!("expected client error"),
        }
    }

    #[tokio::test]
    async fn test_verify_diff_clean_report() {
        let diff = r#"
diff --git a/tests/new_test.rs b/tests/new_test.rs
--- /dev/null
+++ b/tests/new_test.rs
@@ -0,0 +1,1 @@
+// test
"#;
        let args = json!({ "diff": diff });
        let res = handle_verify_diff(&args).expect("verify diff succeeded");
        assert_eq!(res["has_test_coverage"], true);
        assert_eq!(res["risk_level"], "low");
        let touched = res["touched_files"].as_array().unwrap();
        assert_eq!(touched.len(), 1);
        assert_eq!(touched[0], "tests/new_test.rs");
    }

    #[tokio::test]
    async fn test_tripwire_check_authorized_and_unauthorized() {
        // Authorized operator check passes
        let op_args = json!({
            "target": ".cicatrix/sentinel/canary_alpha.rs",
            "actor": "operator"
        });
        let res = handle_check_tripwire(&op_args).expect("operator check should be permitted");
        assert_eq!(res["status"], "clear");
        assert_eq!(res["verdict"], "permitted");

        // Unauthorized agent check trips circuit breaker
        let agent_args = json!({
            "target": ".cicatrix/sentinel/canary_alpha.rs",
            "actor": "unauthorized-agent"
        });
        let err = handle_check_tripwire(&agent_args).expect_err("agent check should tripwire");
        match err {
            ToolExecutionError::Client(msg) => {
                assert!(msg.contains("tripwire intrusion detected"));
                assert!(msg.contains("TRIPWIRE_CANARY_SENTINEL_ALPHA"));
            }
            ToolExecutionError::Internal(_) => panic!("expected client error on intrusion"),
        }
    }

    #[tokio::test]
    async fn test_query_known_bugs_blocks_tripwire_path() {
        let args = json!({
            "paths": [".cicatrix/sentinel/canary_alpha.rs"],
            "actor": "suspicious-crawler"
        });
        let err = handle_query_known_bugs(&args).expect_err("querying tripwire path should fail");
        match err {
            ToolExecutionError::Client(msg) => {
                assert!(msg.contains("tripwire intrusion detected"));
            }
            ToolExecutionError::Internal(_) => panic!("expected client error on intrusion"),
        }
    }

    #[tokio::test]
    async fn test_verify_diff_blocks_tripwire_sentinel() {
        let diff = r#"
diff --git a/src/secrets.rs b/src/secrets.rs
--- a/src/secrets.rs
+++ b/src/secrets.rs
@@ -1,1 +1,2 @@
+// TRIPWIRE_MARKER_CREDENTIAL_CANARY_DO_NOT_READ
"#;
        let args = json!({
            "diff": diff,
            "actor": "rogue-agent"
        });
        let err = handle_verify_diff(&args).expect_err("diff with sentinel must be blocked");
        match err {
            ToolExecutionError::Client(msg) => {
                assert!(msg.contains("tripwire intrusion detected"));
            }
            ToolExecutionError::Internal(_) => panic!("expected client error on intrusion"),
        }
    }
}
