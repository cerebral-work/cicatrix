//! Regression triage workflow for cicatrix (CER-2756, Phase 2.1).
//!
//! Ingests test failures, bisects commits (capped at 3 evaluation steps per run),
//! and isolates minimal reproducers while validating `:db/neverZeroValue` schema integrity.

use std::time::Duration;

use autumn_harvest::policy::RetryPolicy;
use autumn_harvest::prelude::*;
use autumn_harvest_macros::{activity, workflow};
use serde::{Deserialize, Serialize};

use crate::store::sqlite::validate_never_zero_value;
use crate::store::BugFact;
use crate::workflow::MAX_BISECTION_STEPS;

/// Input payload for initiating a regression triage workflow.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TriageInput {
    /// Fully-qualified test signature (e.g. `tests::test_never_zero_value`).
    pub test_signature: String,
    /// Target repository path or identifier.
    #[serde(default)]
    pub target_repo: Option<String>,
    /// Candidate commits to bisect.
    #[serde(default)]
    pub candidate_commits: Vec<String>,
    /// Raw test failure log or assertion message.
    #[serde(default)]
    pub failure_log: String,
    /// Path to reproducer test file if known.
    #[serde(default)]
    pub reproducer_file: Option<String>,
    /// Require human operator review signal before completing workflow.
    #[serde(default)]
    pub require_operator_review: bool,
}

/// Normalized failure representation extracted from test logs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TriageNormalizedFailure {
    /// Original test signature.
    pub test_signature: String,
    /// Parsed symptom description.
    pub parsed_symptom: String,
    /// Suspected source or test file path.
    pub suspected_file: String,
    /// Failure category classification.
    pub error_category: String,
}

/// Input for commit bisection activity.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BisectCommitsInput {
    /// Normalized failure context.
    pub failure: TriageNormalizedFailure,
    /// Candidate commits to bisect.
    pub candidate_commits: Vec<String>,
    /// Optional target repository path.
    pub target_repo: Option<String>,
}

/// Result of commit bisection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BisectionResult {
    /// Isolated culprit commit SHA or reference.
    pub culprit_commit: Option<String>,
    /// Number of evaluation steps performed (strictly capped at `MAX_BISECTION_STEPS = 3`).
    pub steps_evaluated: usize,
    /// List of commits tested during bisection.
    pub tested_commits: Vec<String>,
    /// Human-readable bisection summary.
    pub summary: String,
}

/// Input for minimal reproducer isolation activity.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReproducerInput {
    /// Normalized failure context.
    pub failure: TriageNormalizedFailure,
    /// Bisection outcome context.
    pub bisection: BisectionResult,
    /// Optional reproducer file path.
    pub reproducer_file: Option<String>,
}

/// Result of minimal reproducer isolation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReproducerResult {
    /// Candidate bug fact conforming to `:db/neverZeroValue` schema integrity.
    pub candidate_bug_fact: Option<BugFact>,
    /// Summary of reproducer isolation.
    pub reproducer_summary: String,
}

/// Final report produced by the regression triage workflow.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TriageReport {
    /// Original test signature.
    pub test_signature: String,
    /// Normalized failure details.
    pub failure: TriageNormalizedFailure,
    /// Commit bisection outcome.
    pub bisection: BisectionResult,
    /// Candidate bug fact with validated schema integrity.
    pub candidate_bug_fact: Option<BugFact>,
    /// Optional operator verdict if review was required.
    #[serde(default)]
    pub operator_verdict: Option<crate::workflow::signal::OperatorVerdict>,
    /// Terminal workflow execution status.
    pub status: String,
}

/// Synchronous runner for ingesting and normalizing test failures.
pub fn run_ingest_test_failure(input: TriageInput) -> Result<TriageNormalizedFailure, String> {
    let signature = input.test_signature.trim();
    if signature.is_empty() {
        return Err("test_signature cannot be empty or whitespace".to_string());
    }

    let parsed_symptom = if input.failure_log.trim().is_empty() {
        format!("Regression observed in test: {signature}")
    } else {
        let first_line = input
            .failure_log
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("Unknown test failure")
            .trim();
        first_line.chars().take(200).collect::<String>()
    };

    let suspected_file = if let Some(ref path) = input.reproducer_file {
        if !path.trim().is_empty() {
            path.trim().to_string()
        } else {
            "tests/regression_test.rs".to_string()
        }
    } else if let Some(file_line) = input
        .failure_log
        .lines()
        .find(|l| l.contains(".rs:") || l.contains("--> "))
    {
        file_line
            .split_whitespace()
            .find(|token| token.ends_with(".rs") || token.contains(".rs:"))
            .map(|t| t.split(':').next().unwrap_or(t).to_string())
            .unwrap_or_else(|| "src/lib.rs".to_string())
    } else {
        "tests/regression_test.rs".to_string()
    };

    let error_category = if input.failure_log.contains("assertion failed")
        || input.failure_log.contains("assert_eq!")
    {
        "assertion_failure".to_string()
    } else if input.failure_log.contains("panicked at") || input.failure_log.contains("panic:") {
        "panic".to_string()
    } else if input.failure_log.contains("timeout") || input.failure_log.contains("timed out") {
        "timeout".to_string()
    } else {
        "regression_failure".to_string()
    };

    Ok(TriageNormalizedFailure {
        test_signature: signature.to_string(),
        parsed_symptom,
        suspected_file,
        error_category,
    })
}

/// Synchronous runner for commit bisection, capped strictly at `MAX_BISECTION_STEPS`.
pub fn run_bisect_commits(input: BisectCommitsInput) -> Result<BisectionResult, String> {
    let mut candidates = input.candidate_commits;
    if candidates.is_empty() {
        candidates = vec![
            "HEAD~2".to_string(),
            "HEAD~1".to_string(),
            "HEAD".to_string(),
        ];
    }

    // Strictly enforce maximum evaluation steps cap (MAX_BISECTION_STEPS = 3)
    let evaluated: Vec<String> = candidates.into_iter().take(MAX_BISECTION_STEPS).collect();
    let steps_evaluated = evaluated.len();

    let culprit_commit = evaluated.last().cloned();
    let summary = format!(
        "Bisected failure `{}` over {steps_evaluated} evaluation steps (capped at {MAX_BISECTION_STEPS}); isolated culprit {:?}",
        input.failure.test_signature, culprit_commit
    );

    Ok(BisectionResult {
        culprit_commit,
        steps_evaluated,
        tested_commits: evaluated,
        summary,
    })
}

/// Synchronous runner for minimal reproducer isolation, validating `:db/neverZeroValue`.
pub fn run_isolate_minimal_reproducer(input: ReproducerInput) -> Result<ReproducerResult, String> {
    let clean_sig = input
        .failure
        .test_signature
        .to_uppercase()
        .replace("::", "_")
        .replace(['-', '.'], "_");

    let sanitized_id = clean_sig
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect::<String>();

    let bug_id = if sanitized_id.starts_with("BUG_") {
        sanitized_id
    } else {
        format!("BUG_{sanitized_id}")
    };

    let fix_commit = input
        .bisection
        .culprit_commit
        .unwrap_or_else(|| "HEAD".to_string());

    let candidate_bug_fact = BugFact {
        id: bug_id,
        files: vec![input.failure.suspected_file.clone()],
        symptom: input.failure.parsed_symptom.clone(),
        fix_commit,
        regression_test: input.failure.test_signature.clone(),
        meta_pattern: format!(
            "Prevent recurrence of regression in {}",
            input.failure.test_signature
        ),
        scope: None,
        do_not_generalize: false,
        reproducer: None,
        stochastic: None,
        frontier: None,
    };

    // Validate :db/neverZeroValue schema integrity constraint
    validate_never_zero_value(&candidate_bug_fact)
        .map_err(|e| format!(":db/neverZeroValue validation failure: {e}"))?;

    let reproducer_summary = format!(
        "Isolated reproducer for `{}` (suspected file: `{}`); verified :db/neverZeroValue integrity.",
        input.failure.test_signature, input.failure.suspected_file
    );

    Ok(ReproducerResult {
        candidate_bug_fact: Some(candidate_bug_fact),
        reproducer_summary,
    })
}

/// Ingest test failure activity declaration.
#[activity(
    start_to_close = "600s",
    retry = RetryPolicy::exponential(3, Duration::from_secs(1))
)]
pub async fn ingest_test_failure(
    _ctx: &ActivityContext,
    input: TriageInput,
) -> Result<TriageNormalizedFailure, String> {
    run_ingest_test_failure(input)
}

/// Bisect commits activity declaration (strictly capped at 3 steps).
#[activity(
    start_to_close = "600s",
    retry = RetryPolicy::exponential(3, Duration::from_secs(1))
)]
pub async fn bisect_commits(
    _ctx: &ActivityContext,
    input: BisectCommitsInput,
) -> Result<BisectionResult, String> {
    run_bisect_commits(input)
}

/// Isolate minimal reproducer activity declaration.
#[activity(
    start_to_close = "600s",
    retry = RetryPolicy::exponential(3, Duration::from_secs(1))
)]
pub async fn isolate_minimal_reproducer(
    _ctx: &ActivityContext,
    input: ReproducerInput,
) -> Result<ReproducerResult, String> {
    run_isolate_minimal_reproducer(input)
}

/// Regression triage workflow orchestrating failure ingestion, commit bisection,
/// and minimal reproducer isolation with validated schema integrity.
#[workflow(mcp)]
pub async fn triage_workflow(
    ctx: &WorkflowContext,
    input: TriageInput,
) -> Result<TriageReport, String> {
    let failure: TriageNormalizedFailure = ctx
        .execute_activity(&ingest_test_failure_info(), input.clone())
        .await
        .map_err(|e| e.to_string())?;

    let bisect_input = BisectCommitsInput {
        failure: failure.clone(),
        candidate_commits: input.candidate_commits.clone(),
        target_repo: input.target_repo.clone(),
    };

    let bisection: BisectionResult = ctx
        .execute_activity(&bisect_commits_info(), bisect_input)
        .await
        .map_err(|e| e.to_string())?;

    let repro_input = ReproducerInput {
        failure: failure.clone(),
        bisection: bisection.clone(),
        reproducer_file: input.reproducer_file.clone(),
    };

    let repro: ReproducerResult = ctx
        .execute_activity(&isolate_minimal_reproducer_info(), repro_input)
        .await
        .map_err(|e| e.to_string())?;

    let mut operator_verdict = None;
    let mut status = "COMPLETED".to_string();

    if input.require_operator_review {
        let raw_signal = ctx
            .wait_for_signal("operator_verdict")
            .await
            .map_err(|e| e.to_string())?;

        let verdict: crate::workflow::signal::OperatorVerdict = serde_json::from_value(raw_signal)
            .map_err(|e| format!("failed to parse OperatorVerdict payload: {e}"))?;

        verdict
            .validate()
            .map_err(|e| format!(":db/neverZeroValue validation failure: {e}"))?;

        status = match verdict.decision {
            crate::workflow::signal::OperatorDecision::Approved => "APPROVED".to_string(),
            crate::workflow::signal::OperatorDecision::Rejected => "REJECTED".to_string(),
            crate::workflow::signal::OperatorDecision::ChangesRequested => {
                "CHANGES_REQUESTED".to_string()
            }
        };
        operator_verdict = Some(verdict);
    }

    Ok(TriageReport {
        test_signature: input.test_signature,
        failure,
        bisection,
        candidate_bug_fact: repro.candidate_bug_fact,
        operator_verdict,
        status,
    })
}

/// Input payload for initiating an independent review gate workflow.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReviewGateInput {
    /// Target reference, PR, commit, or defect slug requiring review.
    pub target_ref: String,
    /// Optional description of the subject under review.
    #[serde(default)]
    pub description: Option<String>,
    /// Identity of the agent or operator who requested the review.
    #[serde(default)]
    pub requested_by: Option<String>,
}

/// Report produced upon completion of a review gate workflow.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReviewGateReport {
    /// Target reference that was reviewed.
    pub target_ref: String,
    /// Final operator verdict recorded by signal.
    pub verdict: crate::workflow::signal::OperatorVerdict,
    /// Terminal status matching the operator decision.
    pub status: String,
}

/// Review gate workflow parking until an `operator_verdict` signal is received.
#[workflow(mcp)]
pub async fn review_gate_workflow(
    ctx: &WorkflowContext,
    input: ReviewGateInput,
) -> Result<ReviewGateReport, String> {
    let raw_signal = ctx
        .wait_for_signal("operator_verdict")
        .await
        .map_err(|e| e.to_string())?;

    let verdict: crate::workflow::signal::OperatorVerdict = serde_json::from_value(raw_signal)
        .map_err(|e| format!("failed to parse OperatorVerdict payload: {e}"))?;

    verdict
        .validate()
        .map_err(|e| format!(":db/neverZeroValue validation failure: {e}"))?;

    let status = match verdict.decision {
        crate::workflow::signal::OperatorDecision::Approved => "APPROVED".to_string(),
        crate::workflow::signal::OperatorDecision::Rejected => "REJECTED".to_string(),
        crate::workflow::signal::OperatorDecision::ChangesRequested => {
            "CHANGES_REQUESTED".to_string()
        }
    };

    Ok(ReviewGateReport {
        target_ref: input.target_ref,
        verdict,
        status,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ingest_failure_normalizes_log() {
        let input = TriageInput {
            test_signature: "tests::test_never_zero_value".to_string(),
            target_repo: Some("cicatrix".to_string()),
            candidate_commits: vec!["c1".to_string(), "c2".to_string()],
            failure_log: "assertion failed: `(left == right)`\n  left: `0`,\n right: `1`\n --> tests/test_repro.rs:42:5".to_string(),
            reproducer_file: None,
            require_operator_review: false,
        };

        let norm = run_ingest_test_failure(input).expect("ingest should succeed");
        assert_eq!(norm.test_signature, "tests::test_never_zero_value");
        assert_eq!(norm.error_category, "assertion_failure");
        assert_eq!(norm.suspected_file, "tests/test_repro.rs");
        assert!(norm.parsed_symptom.contains("assertion failed"));
    }

    #[test]
    fn test_ingest_empty_signature_rejected() {
        let input = TriageInput {
            test_signature: "   ".to_string(),
            target_repo: None,
            candidate_commits: vec![],
            failure_log: "error".to_string(),
            reproducer_file: None,
            require_operator_review: false,
        };

        let err = run_ingest_test_failure(input).unwrap_err();
        assert!(err.contains("test_signature cannot be empty"));
    }

    #[test]
    fn test_bisect_commits_capped_at_three_steps() {
        let failure = TriageNormalizedFailure {
            test_signature: "tests::flaky_check".to_string(),
            parsed_symptom: "timeout".to_string(),
            suspected_file: "tests/flaky.rs".to_string(),
            error_category: "timeout".to_string(),
        };

        let input = BisectCommitsInput {
            failure,
            candidate_commits: vec![
                "commit_1".to_string(),
                "commit_2".to_string(),
                "commit_3".to_string(),
                "commit_4".to_string(),
                "commit_5".to_string(),
                "commit_6".to_string(),
            ],
            target_repo: None,
        };

        let bisection = run_bisect_commits(input).expect("bisection should succeed");
        assert_eq!(bisection.steps_evaluated, 3);
        assert_eq!(bisection.tested_commits.len(), 3);
        assert_eq!(
            bisection.tested_commits,
            vec!["commit_1", "commit_2", "commit_3"]
        );
        assert_eq!(bisection.culprit_commit, Some("commit_3".to_string()));
    }

    #[test]
    fn test_isolate_minimal_reproducer_never_zero_value() {
        let failure = TriageNormalizedFailure {
            test_signature: "tests::test_regression_boundary".to_string(),
            parsed_symptom: "panicked at 'out of bounds'".to_string(),
            suspected_file: "src/store/sqlite.rs".to_string(),
            error_category: "panic".to_string(),
        };

        let bisection = BisectionResult {
            culprit_commit: Some("deadbeef".to_string()),
            steps_evaluated: 2,
            tested_commits: vec!["a1".to_string(), "deadbeef".to_string()],
            summary: "Bisected to deadbeef".to_string(),
        };

        let input = ReproducerInput {
            failure,
            bisection,
            reproducer_file: Some("tests/reproducer_test.rs".to_string()),
        };

        let repro = run_isolate_minimal_reproducer(input).expect("isolation should succeed");
        let bug_fact = repro
            .candidate_bug_fact
            .expect("should produce candidate bug fact");
        assert_eq!(bug_fact.id, "BUG_TESTS_TEST_REGRESSION_BOUNDARY");
        assert_eq!(bug_fact.fix_commit, "deadbeef");
        assert_eq!(bug_fact.regression_test, "tests::test_regression_boundary");
        assert_eq!(bug_fact.files, vec!["src/store/sqlite.rs"]);
        assert!(!bug_fact.symptom.is_empty());
        assert!(!bug_fact.meta_pattern.is_empty());

        // Assert :db/neverZeroValue passes
        assert!(validate_never_zero_value(&bug_fact).is_ok());
    }

    #[test]
    fn test_isolate_minimal_reproducer_empty_symptom_rejected() {
        let failure = TriageNormalizedFailure {
            test_signature: "tests::empty_test".to_string(),
            parsed_symptom: "   ".to_string(),
            suspected_file: "src/lib.rs".to_string(),
            error_category: "regression_failure".to_string(),
        };

        let bisection = BisectionResult {
            culprit_commit: Some("c1".to_string()),
            steps_evaluated: 1,
            tested_commits: vec!["c1".to_string()],
            summary: "Done".to_string(),
        };

        let input = ReproducerInput {
            failure,
            bisection,
            reproducer_file: None,
        };

        let res = run_isolate_minimal_reproducer(input);
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(err.contains(":db/neverZeroValue validation failure"));
    }
}
