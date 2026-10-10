//! Data structures, error types, and payloads for cicatrix diagnose auto-authoring.
//!
//! Clean-room Rust port of the hypothesis-fork and fork-and-judge contract
//! from agent-afk (Griffin Long, Apache-2.0).

use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::store::BugFact;

/// Category of root-cause hypothesis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HypothesisCategory {
    /// Invariant broken in core implementation logic.
    ImplementationDefect,
    /// Assertion or test harness expectation is brittle, outdated, or erroneous.
    AssertionDefect,
    /// Concurrency race, deadlock, timing, or asynchronous task ordering defect.
    ConcurrencyDefect,
    /// Schema, interface, or RPC protocol contract violation.
    ContractDefect,
    /// Off-by-one, underflow, overflow, or boundary condition defect.
    BoundaryDefect,
    /// Uncontrolled or unexpected mutation of shared or cached state.
    StateMutationDefect,
}

impl fmt::Display for HypothesisCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HypothesisCategory::ImplementationDefect => write!(f, "Implementation Defect"),
            HypothesisCategory::AssertionDefect => write!(f, "Assertion/Test Defect"),
            HypothesisCategory::ConcurrencyDefect => write!(f, "Concurrency/Race Defect"),
            HypothesisCategory::ContractDefect => write!(f, "Contract/Schema Defect"),
            HypothesisCategory::BoundaryDefect => write!(f, "Boundary/Condition Defect"),
            HypothesisCategory::StateMutationDefect => write!(f, "State Mutation Defect"),
        }
    }
}

/// A single root-cause hypothesis produced by the hypothesis forker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RootCauseHypothesis {
    /// Unique hypothesis identifier (e.g. `hyp-1`, `hyp-2`).
    pub id: String,
    /// Classification category.
    pub category: HypothesisCategory,
    /// Concise title of the proposed failure hypothesis.
    pub title: String,
    /// Detailed physical mechanism of failure.
    pub mechanism: String,
    /// Erroneous mental model or assumption that produced the bug.
    pub mental_model_error: String,
    /// Proposed regression test assertion or scenario.
    pub suggested_regression_test: String,
    /// Direction or plan for resolving the defect.
    pub suggested_fix: String,
    /// Meta-pattern classification (aligns with CLAUDE.md named classes).
    pub meta_pattern: String,
    /// Diagnostic confidence score in range [0.0, 1.0].
    pub confidence: f64,
    /// Concrete evidence points supporting this hypothesis.
    pub evidence: Vec<String>,
}

/// Target subject for diagnosis.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnoseTarget {
    /// Target name, test signature, or defect slug.
    pub name: String,
    /// Raw compiler, panic, assertion, or runtime failure log.
    pub failure_log: String,
    /// Turnkey reproduction command if known (e.g. `cargo test -j 2 test_foo`).
    pub repro_command: Option<String>,
    /// Suspected or touched source file paths.
    pub candidate_files: Vec<String>,
}

/// Configuration options for hypothesis forking and diagnosis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForkOptions {
    /// Concurrency bound for parallel hypothesis branches (strictly 1..=5, default 3).
    pub num_forks: usize,
    /// Optional model tier designation (e.g. `capable`, `default`).
    pub model_tier: Option<String>,
    /// Target directory for output markdown (defaults to `docs/bugs/observed/`).
    pub target_dir: Option<PathBuf>,
    /// Explicit target output file path.
    pub output_file: Option<PathBuf>,
    /// Whether to persist the generated BugFact to disk.
    pub write_file: bool,
    /// Deterministic mock hypotheses for testing or offline evaluation.
    pub mock_hypotheses: Option<Vec<RootCauseHypothesis>>,
}

impl Default for ForkOptions {
    fn default() -> Self {
        Self {
            num_forks: 3,
            model_tier: None,
            target_dir: None,
            output_file: None,
            write_file: true,
            mock_hypotheses: None,
        }
    }
}

/// Final convergence and diagnosis report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConvergenceReport {
    /// Target subject diagnosed.
    pub target: String,
    /// All evaluated hypotheses.
    pub hypotheses: Vec<RootCauseHypothesis>,
    /// ID of the winning hypothesis.
    pub winning_hypothesis_id: String,
    /// Synthesis rationale explaining why the winning hypothesis was selected.
    pub convergence_rationale: String,
    /// Fully populated BugFact data structure.
    pub bug_fact: BugFact,
    /// Formatted markdown document ready for docs/bugs/observed/BUG_<SLUG>.md.
    pub rendered_markdown: String,
    /// Path where the file was written, if write_file was true.
    pub output_file: Option<String>,
}

/// Errors occurring during diagnosis and auto-authoring.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum DiagnoseError {
    /// Target name or signature is empty.
    EmptyTarget,
    /// An input or generated attribute violates Datomic `:db/neverZeroValue`.
    NeverZeroValue { field: String, detail: String },
    /// Fork count exceeds operational bounds [1, 5].
    InvalidForks(String),
    /// Markdown parse or schema validation failed.
    SchemaValidation(String),
    /// File system or IO failure.
    Io(String),
    /// Convergence failed to isolate a viable hypothesis.
    ConvergenceFailure(String),
}

impl fmt::Display for DiagnoseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DiagnoseError::EmptyTarget => {
                write!(f, "diagnose target cannot be empty or whitespace")
            }
            DiagnoseError::NeverZeroValue { field, detail } => {
                write!(f, ":db/neverZeroValue violation on `{field}`: {detail}")
            }
            DiagnoseError::InvalidForks(detail) => {
                write!(f, "invalid fork count: {detail}")
            }
            DiagnoseError::SchemaValidation(err) => {
                write!(f, "bug fact schema validation failed: {err}")
            }
            DiagnoseError::Io(err) => write!(f, "IO failure: {err}"),
            DiagnoseError::ConvergenceFailure(err) => {
                write!(f, "diagnosis convergence failure: {err}")
            }
        }
    }
}

impl std::error::Error for DiagnoseError {}

impl From<std::io::Error> for DiagnoseError {
    fn from(err: std::io::Error) -> Self {
        DiagnoseError::Io(err.to_string())
    }
}
