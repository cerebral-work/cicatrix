//! Hypothesis forking engine for cicatrix diagnose.
//!
//! Spawns N divergent root-cause hypotheses across failure categories
//! (implementation defects, test specification flaws, boundary/concurrency issues).
//!
//! Clean-room Rust port of the hypothesis-fork contract from agent-afk (Apache-2.0).

use crate::diagnose::types::{
    DiagnoseError, DiagnoseTarget, ForkOptions, HypothesisCategory, RootCauseHypothesis,
};

/// Maximum allowed concurrent hypothesis forks.
pub const MAX_FORK_COUNT: usize = 5;

/// Minimum allowed concurrent hypothesis forks.
pub const MIN_FORK_COUNT: usize = 1;

/// Validate input options and parameters before hypothesis generation.
pub fn validate_fork_input(
    target: &DiagnoseTarget,
    options: &ForkOptions,
) -> Result<(), DiagnoseError> {
    if target.name.trim().is_empty() {
        return Err(DiagnoseError::EmptyTarget);
    }
    if options.num_forks < MIN_FORK_COUNT || options.num_forks > MAX_FORK_COUNT {
        return Err(DiagnoseError::InvalidForks(format!(
            "fork count {} must be within [{}, {}]",
            options.num_forks, MIN_FORK_COUNT, MAX_FORK_COUNT
        )));
    }
    Ok(())
}

/// Extract suspected file paths from a failure log or test output.
pub fn extract_suspected_files(log: &str) -> Vec<String> {
    let mut files = Vec::new();
    for line in log.lines() {
        let trimmed = line.trim();
        // Check Rust compiler / backtrace format: --> path/to/file.rs:line:col
        if let Some(pos) = trimmed.find("--> ") {
            let rest = &trimmed[pos + 4..].trim_start();
            let file_part = rest.split(':').next().unwrap_or(rest).trim();
            if file_part.ends_with(".rs") && !files.contains(&file_part.to_string()) {
                files.push(file_part.to_string());
            }
        }
        // Check panic locations: at src/foo.rs:123:45
        if let Some(pos) = trimmed.find(" at ") {
            let rest = &trimmed[pos + 4..].trim_start();
            let file_part = rest.split(':').next().unwrap_or(rest).trim();
            if file_part.ends_with(".rs") && !files.contains(&file_part.to_string()) {
                files.push(file_part.to_string());
            }
        }
    }
    files
}

/// Fork N distinct root-cause hypotheses for the target.
pub fn fork_hypotheses(
    target: &DiagnoseTarget,
    options: &ForkOptions,
) -> Result<Vec<RootCauseHypothesis>, DiagnoseError> {
    validate_fork_input(target, options)?;

    if let Some(ref mocks) = options.mock_hypotheses {
        if mocks.is_empty() {
            return Err(DiagnoseError::ConvergenceFailure(
                "mock hypotheses list is empty".to_string(),
            ));
        }
        return Ok(mocks.clone());
    }

    // Determine primary file under diagnosis
    let mut files = target.candidate_files.clone();
    if files.is_empty() {
        files = extract_suspected_files(&target.failure_log);
    }
    let primary_file = files
        .first()
        .cloned()
        .unwrap_or_else(|| "src/lib.rs".to_string());

    let clean_target = target.name.trim();
    let log = target.failure_log.trim();

    // Scan failure log characteristics
    let has_assertion = log.contains("assertion failed")
        || log.contains("assert_eq!")
        || log.contains("left == right");
    let has_panic =
        log.contains("panicked at") || log.contains("unwrap()") || log.contains("expect(");
    let has_timeout =
        log.contains("timeout") || log.contains("timed out") || log.contains("deadline");
    let has_boundary =
        log.contains("out of bounds") || log.contains("overflow") || log.contains("underflow");
    let has_schema =
        log.contains("neverZeroValue") || log.contains("schema") || log.contains("parse error");

    let mut hypotheses = Vec::new();

    // Fork 1: Core Implementation Defect
    {
        let mechanism = if has_boundary {
            format!("Index or boundary condition violated during execution in `{primary_file}`.")
        } else if has_schema {
            format!("Datomic `:db/neverZeroValue` schema integrity constraint rejected empty zero value in `{primary_file}`.")
        } else if has_panic {
            format!("Unchecked unwrap on None or Err variant encountered in `{primary_file}`.")
        } else {
            format!("Internal invariant or state transformation logic broken in `{primary_file}`.")
        };

        let mental_model_error = if has_schema {
            "Assumed empty strings or zeroes are benign default values rather than invalid datoms."
        } else if has_boundary {
            "Assumed collection length or index range is always strictly positive and non-empty."
        } else {
            "Assumed preconditions are guaranteed by callers without defensive validation."
        };

        let confidence = if has_panic || has_schema || has_boundary {
            0.88
        } else {
            0.75
        };

        let mut evidence = Vec::new();
        if !log.is_empty() {
            evidence.push(
                log.lines()
                    .next()
                    .unwrap_or("Failure logged")
                    .chars()
                    .take(120)
                    .collect(),
            );
        }
        evidence.push(format!("Target unit: `{clean_target}`"));
        evidence.push(format!("Suspected implementation path: `{primary_file}`"));

        hypotheses.push(RootCauseHypothesis {
            id: "hyp-1".to_string(),
            category: HypothesisCategory::ImplementationDefect,
            title: format!("Implementation defect in {primary_file}"),
            mechanism,
            mental_model_error: mental_model_error.to_string(),
            suggested_regression_test: format!("{clean_target}_regression_boundary_guard"),
            suggested_fix: format!(
                "Add defensive validation and explicit error propagation in `{primary_file}`."
            ),
            meta_pattern: "Contracts break silently downstream".to_string(),
            confidence,
            evidence,
        });
    }

    // Fork 2: Test Specification / Brittle Assertion Defect
    if options.num_forks >= 2 {
        let mechanism = if has_assertion {
            format!("Test `{clean_target}` asserts rigid exact-match expectation against valid updated behavior.")
        } else {
            format!("Test setup in `{clean_target}` constructs stale environment state or mock discrepancy.")
        };

        let mental_model_error =
            "Assumed test harness fixture state reflects live environment invariants.";
        let confidence = if has_assertion { 0.70 } else { 0.55 };

        let mut evidence = Vec::new();
        evidence.push(format!("Assertion structure in `{clean_target}`"));
        if has_assertion {
            evidence.push("Assertion failure marker detected in failure log".to_string());
        }

        hypotheses.push(RootCauseHypothesis {
            id: "hyp-2".to_string(),
            category: HypothesisCategory::AssertionDefect,
            title: format!("Brittle assertion in {clean_target}"),
            mechanism,
            mental_model_error: mental_model_error.to_string(),
            suggested_regression_test: format!("{clean_target}_relaxed_contract_verification"),
            suggested_fix: format!(
                "Update test assertion in `{clean_target}` to match specification tolerances."
            ),
            meta_pattern: "Test mirrors implementation bug".to_string(),
            confidence,
            evidence,
        });
    }

    // Fork 3: Concurrency, Timing, or Contract Interface Defect
    if options.num_forks >= 3 {
        let (cat, title, mechanism, pattern, conf) = if has_timeout {
            (
                HypothesisCategory::ConcurrencyDefect,
                format!("Asynchronous task timeout or deadlock in {clean_target}"),
                format!("Async operation in `{primary_file}` exceeded deadline or suffered lock contention."),
                "Async task starvation under load".to_string(),
                0.85,
            )
        } else {
            (
                HypothesisCategory::ContractDefect,
                format!("Contract interface mismatch in {primary_file}"),
                format!("Caller and callee exchange incompatible schema or protocol representations across `{primary_file}`."),
                "Unvalidated boundary payload".to_string(),
                0.65,
            )
        };

        let mental_model_error =
            "Assumed synchronous completion without task isolation or boundary timeout.";

        let mut evidence = Vec::new();
        evidence.push(format!("Inter-subsystem boundary around `{primary_file}`"));
        if has_timeout {
            evidence.push("Timeout keyword observed in log trace".to_string());
        }

        hypotheses.push(RootCauseHypothesis {
            id: "hyp-3".to_string(),
            category: cat,
            title,
            mechanism,
            mental_model_error: mental_model_error.to_string(),
            suggested_regression_test: format!("{clean_target}_contract_timeout_boundary"),
            suggested_fix: format!("Enforce explicit timeout, cancellation token, or schema guard in `{primary_file}`."),
            meta_pattern: pattern,
            confidence: conf,
            evidence,
        });
    }

    // Fork 4: State Mutation / Cache Poisoning Defect
    if options.num_forks >= 4 {
        hypotheses.push(RootCauseHypothesis {
            id: "hyp-4".to_string(),
            category: HypothesisCategory::StateMutationDefect,
            title: format!("Shared state pollution or uncommitted mutation in {primary_file}"),
            mechanism: format!("Concurrent or previous execution contaminated shared cache or store in `{primary_file}`."),
            mental_model_error: "Assumed stateful cache is isolated between successive test invocations.".to_string(),
            suggested_regression_test: format!("{clean_target}_state_isolation_check"),
            suggested_fix: format!("Reset mutable caches and ensure atomic rollback on failure in `{primary_file}`."),
            meta_pattern: "Shared state leak between runs".to_string(),
            confidence: 0.60,
            evidence: vec![format!("State mutation vectors in `{primary_file}`")],
        });
    }

    // Fork 5: Boundary & Numerical Overflow Defect
    if options.num_forks >= 5 {
        hypotheses.push(RootCauseHypothesis {
            id: "hyp-5".to_string(),
            category: HypothesisCategory::BoundaryDefect,
            title: format!("Boundary limit or numeric range error in {primary_file}"),
            mechanism: format!("Edge-case value exceeded integer or buffer capacity boundaries in `{primary_file}`."),
            mental_model_error: "Assumed values always fit inside standard allocated bounds.".to_string(),
            suggested_regression_test: format!("{clean_target}_extreme_value_fuzz"),
            suggested_fix: format!("Add saturating arithmetic and explicit bound checks in `{primary_file}`."),
            meta_pattern: "Unchecked numeric boundary".to_string(),
            confidence: 0.58,
            evidence: vec![format!("Capacity limits in `{primary_file}`")],
        });
    }

    Ok(hypotheses)
}
