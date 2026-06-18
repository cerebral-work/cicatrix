//! CLI behavior suite — exercises every `cicatrix` verb through the built binary.
//!
//! This is the suite the green-baseline invariant (`.cicatrix/baseline-green`) stands on:
//! `.cicatrix/establish-baseline.sh` writes the marker only when these pass. Tests assert
//! *structure/invariants*, not just happy-path strings (see CLAUDE.md meta-patterns).

use std::path::Path;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_cicatrix");

fn run(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .output()
        .expect("failed to spawn cicatrix binary")
}

#[test]
fn inject_emits_the_meta_pattern_corpus() {
    let out = run(&["inject"]);
    assert!(out.status.success(), "`inject` should exit 0");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("meta-patterns"), "inject stdout: {stdout}");
    for needle in [
        "Type mismatches kill",
        "Edge cases are real cases",
        "Correctness before performance",
    ] {
        assert!(stdout.contains(needle), "inject dropped: {needle}");
    }
}

/// Structural invariant: the `drift` verb advertises a scan file. That path must actually
/// exist on disk — otherwise the command points at a renamed/deleted artifact and lies.
/// Asserting the file exists (not just that *some* string printed) catches that drift.
#[test]
fn drift_advertises_a_path_that_exists() {
    let out = run(&["drift"]);
    assert!(out.status.success(), "`drift` should exit 0");
    let rel = String::from_utf8_lossy(&out.stdout);
    let rel = rel.trim();
    assert!(!rel.is_empty(), "`drift` printed nothing");
    let abs = Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    assert!(abs.exists(), "`drift` advertises {rel} but it does not exist on disk");
}

/// `record` and `query` are honest stubs in v0: they must exit 0 *and* say so on stderr,
/// not silently pretend to have done work (premature victory is the named failure class).
#[test]
fn record_and_query_are_honest_stubs() {
    for verb in ["record", "query"] {
        let out = run(&[verb]);
        assert!(out.status.success(), "`{verb}` (stub) should exit 0");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("not yet wired"),
            "`{verb}` should admit it is unwired; stderr: {stderr}"
        );
    }
}

#[test]
fn unknown_verb_fails_with_usage() {
    let out = run(&["definitely-not-a-verb"]);
    assert!(!out.status.success(), "unknown verb should exit non-zero");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("usage:"), "expected usage on stderr; got: {stderr}");
}
