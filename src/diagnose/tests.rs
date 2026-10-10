//! Unit tests for cicatrix diagnose auto-authoring.

use tempfile::tempdir;

use super::*;
use crate::bug_md;

#[test]
fn test_sanitize_slug_conversions() {
    assert_eq!(
        sanitize_slug("tests::test_never_zero_value"),
        "TESTS_TEST_NEVER_ZERO_VALUE"
    );
    assert_eq!(sanitize_slug("BUG_LCM_COLLISION_500"), "LCM_COLLISION_500");
    assert_eq!(
        sanitize_slug("bug:wasm-gc-bad-pointer"),
        "WASM_GC_BAD_POINTER"
    );
    assert_eq!(sanitize_slug(""), "DIAGNOSED_DEFECT");
    assert_eq!(slug_to_kebab("TEST_NEVER_ZERO"), "test-never-zero");
}

#[test]
fn test_extract_suspected_files_from_rustc_log() {
    let log = r#"
error[E0308]: mismatched types
  --> src/store/sqlite.rs:142:18
   |
142 |     let x: u32 = "hello";
   |                  ^^^^^^^ expected `u32`, found `&str`
thread 'test_foo' panicked at src/replication/engine.rs:88:9:
assertion failed: `(left == right)`
"#;
    let files = extract_suspected_files(log);
    assert_eq!(files.len(), 2);
    assert_eq!(files[0], "src/store/sqlite.rs");
    assert_eq!(files[1], "src/replication/engine.rs");
}

#[test]
fn test_fork_hypotheses_bounds_validation() {
    let target = DiagnoseTarget {
        name: "test_failure".to_string(),
        failure_log: "assertion failed".to_string(),
        repro_command: None,
        candidate_files: vec![],
    };

    // 0 forks rejected
    let opts_zero = ForkOptions {
        num_forks: 0,
        ..Default::default()
    };
    assert!(matches!(
        fork_hypotheses(&target, &opts_zero),
        Err(DiagnoseError::InvalidForks(_))
    ));

    // 6 forks rejected
    let opts_six = ForkOptions {
        num_forks: 6,
        ..Default::default()
    };
    assert!(matches!(
        fork_hypotheses(&target, &opts_six),
        Err(DiagnoseError::InvalidForks(_))
    ));

    // Empty target rejected
    let empty_target = DiagnoseTarget {
        name: "   ".to_string(),
        failure_log: "log".to_string(),
        repro_command: None,
        candidate_files: vec![],
    };
    assert_eq!(
        fork_hypotheses(&empty_target, &ForkOptions::default()),
        Err(DiagnoseError::EmptyTarget)
    );
}

#[test]
fn test_fork_hypotheses_generation_and_convergence() {
    let tmp = tempdir().expect("tempdir");
    let target = DiagnoseTarget {
        name: "tests::test_reverie_ingest_invisible_channel".to_string(),
        failure_log: "thread 'test_invisible' panicked at 'assertion failed: `(left == right)`\n  --> crates/reverie-store/src/ingest.rs:45:9".to_string(),
        repro_command: Some("cargo test -j 2 test_invisible".to_string()),
        candidate_files: vec!["crates/reverie-store/src/ingest.rs".to_string()],
    };

    let opts = ForkOptions {
        num_forks: 3,
        target_dir: Some(tmp.path().to_path_buf()),
        write_file: true,
        ..Default::default()
    };

    let report = run_diagnose(target, opts).expect("run_diagnose succeeds");

    assert_eq!(report.hypotheses.len(), 3);
    assert_eq!(report.winning_hypothesis_id, "hyp-1");
    assert!(report.convergence_rationale.contains("hyp-1"));
    assert_eq!(
        report.bug_fact.id,
        "BUG_TESTS_TEST_REVERIE_INGEST_INVISIBLE_CHANNEL"
    );
    assert_eq!(
        report.bug_fact.files,
        vec!["crates/reverie-store/src/ingest.rs"]
    );
    assert_eq!(
        report.bug_fact.scope,
        Some("crates/reverie-store/src".to_string())
    );
    assert_eq!(
        report.bug_fact.reproducer,
        Some("cargo test -j 2 test_invisible".to_string())
    );

    // Verify written file exists and parses cleanly
    let written = report.output_file.expect("output file recorded");
    let content = std::fs::read_to_string(&written).expect("read written file");
    let parsed = bug_md::parse(&content, None).expect("parses through bug_md");
    assert_eq!(parsed.id, report.bug_fact.id);
    assert_eq!(parsed.files, report.bug_fact.files);
    assert_eq!(parsed.scope, report.bug_fact.scope);
}

#[test]
fn test_mock_hypotheses_override() {
    let mock_hyp = RootCauseHypothesis {
        id: "mock-1".to_string(),
        category: HypothesisCategory::ContractDefect,
        title: "Mock interface divergence".to_string(),
        mechanism: "Interface drift detected".to_string(),
        mental_model_error: "Assumed static contract".to_string(),
        suggested_regression_test: "test_contract_guard".to_string(),
        suggested_fix: "Pin protocol version".to_string(),
        meta_pattern: "Contract drift".to_string(),
        confidence: 0.95,
        evidence: vec!["Schema drift receipt".to_string()],
    };

    let target = DiagnoseTarget {
        name: "test_mock".to_string(),
        failure_log: "some failure".to_string(),
        repro_command: None,
        candidate_files: vec!["src/protocol.rs".to_string()],
    };

    let opts = ForkOptions {
        num_forks: 1,
        write_file: false,
        mock_hypotheses: Some(vec![mock_hyp.clone()]),
        ..Default::default()
    };

    let report = run_diagnose(target, opts).expect("run_diagnose with mocks");
    assert_eq!(report.hypotheses.len(), 1);
    assert_eq!(report.winning_hypothesis_id, "mock-1");
    assert_eq!(report.bug_fact.regression_test, "test_contract_guard");
    assert_eq!(report.bug_fact.meta_pattern, "Contract drift");
}
