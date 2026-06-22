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

/// Run with `REVERIE_URL` pointed at a dead port so the bridge fails deterministically — keeps
/// these tests (and the green-baseline gate) free of any dependency on a live reveried.
fn run_offline(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .env("REVERIE_URL", "http://127.0.0.1:1")
        .output()
        .expect("failed to spawn cicatrix binary")
}

/// `inject` now renders FROM the grounded corpus (was a hardcoded static). The seed corpus yields
/// exactly the two classes its two bugs carry — assert those, not the former static's five.
#[test]
fn inject_emits_the_meta_pattern_corpus() {
    let out = run(&["inject"]);
    assert!(out.status.success(), "`inject` should exit 0");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("meta-patterns"), "inject stdout: {stdout}");
    for needle in [
        "Type mismatches kill",
        "Two implementations of one fact drift",
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
    assert!(
        abs.exists(),
        "`drift` advertises {rel} but it does not exist on disk"
    );
}

/// `query` with no files is a usage error — exit non-zero, print usage. No network involved.
#[test]
fn query_without_files_is_a_usage_error() {
    let out = run(&["query"]);
    assert!(!out.status.success(), "`query` with no files should fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("usage:"), "expected usage; got: {stderr}");
}

/// `record` against an unreachable reverie fails cleanly (non-zero + diagnostic on stderr) rather
/// than reporting phantom success — premature victory is the named failure class.
#[test]
fn record_fails_cleanly_when_reverie_unreachable() {
    let out = run_offline(&["record", "docs/bugs/grounded/BUG_EMBED_EMPTY_INPUT_400.md"]);
    assert!(
        !out.status.success(),
        "record must fail when reverie is unreachable"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("failed") || stderr.contains("record:"),
        "stderr: {stderr}"
    );
}

/// `query` against an unreachable reverie fails cleanly too (does not print a false all-clear).
#[test]
fn query_fails_cleanly_when_reverie_unreachable() {
    let out = run_offline(&["query", "crates/reverie-store/src/embed.rs"]);
    assert!(
        !out.status.success(),
        "query must fail when reverie is unreachable"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("query:"), "stderr: {stderr}");
}

#[test]
fn unknown_verb_fails_with_usage() {
    let out = run(&["definitely-not-a-verb"]);
    assert!(!out.status.success(), "unknown verb should exit non-zero");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("usage:"),
        "expected usage on stderr; got: {stderr}"
    );
}

// === CER-1397 P1: poison-the-well gate ===

/// Run with a CWD pinned to the crate root so default corpus dirs resolve. Also points reverie at
/// a dead port (so the HTTP POST fails deterministically if the gate lets the request through).
fn run_offline_in_repo(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env("REVERIE_URL", "http://127.0.0.1:1")
        .output()
        .expect("failed to spawn cicatrix binary")
}

/// An explicit input path under the observed (ungrounded) tier is REFUSED before any projection:
/// non-zero exit + the exact diagnostic. The gate fires before the network is touched.
#[test]
fn record_refuses_observed_tier_path() {
    // Use the default observed dir; the file need not exist — the gate is structural (path-based).
    let out = run_offline_in_repo(&["record", "docs/bugs/observed/BUG_SOMETHING.md"]);
    assert!(
        !out.status.success(),
        "record must refuse an observed-tier path"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("observed facts are ungrounded; promote to grounded first"),
        "expected ungrounded refusal; got: {stderr}"
    );
}

/// A grounded-tier path PASSES the tier gate (the ungrounded refusal is absent). Without a live
/// reveried it then fails at the HTTP POST — proving the gate accepted it and projection began.
#[test]
fn record_accepts_grounded_tier_path() {
    let out = run_offline_in_repo(&["record", "docs/bugs/grounded/BUG_EMBED_EMPTY_INPUT_400.md"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("observed facts are ungrounded"),
        "grounded path must pass the tier gate; got: {stderr}"
    );
    // It fails at HTTP (dead port), not at the gate.
    assert!(
        !out.status.success(),
        "record should still fail at the dead reverie port"
    );
    assert!(
        stderr.contains("failed") || stderr.contains("record:"),
        "expected an HTTP-stage diagnostic; got: {stderr}"
    );
}

/// `inject --target` filters meta-patterns by fact scope: a fact scoped to crate A is emitted for
/// a target in A and omitted for a target in B. Driven via a temp grounded corpus + env override.
#[test]
fn inject_filters_by_target_scope() {
    let dir = std::env::temp_dir().join(format!("cicatrix_scope_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let bug = |slug: &str, scope: &str, mp: &str| {
        format!(
            "# {slug}\n\n\
             - **files:** crates/x/src/x.rs\n\
             - **fix-commit:** #1\n\
             - **regression-test:** g\n\
             - **meta-pattern:** {mp}\n\
             - **scope:** {scope}\n\n\
             ## Symptom\nbroke\n"
        )
    };
    std::fs::write(
        dir.join("BUG_A.md"),
        bug("BUG_A", "crates/alpha", "Alpha-class pattern"),
    )
    .unwrap();
    std::fs::write(
        dir.join("BUG_B.md"),
        bug("BUG_B", "crates/beta", "Beta-class pattern"),
    )
    .unwrap();

    let inject = |target: &str| -> String {
        let out = Command::new(BIN)
            .args(["inject", "--target", target])
            .env("CICATRIX_CORPUS_GROUNDED", &dir)
            .output()
            .expect("spawn");
        assert!(out.status.success(), "inject --target should exit 0");
        String::from_utf8_lossy(&out.stdout).into_owned()
    };

    let for_alpha = inject("crates/alpha/src/x.rs");
    assert!(
        for_alpha.contains("Alpha-class pattern"),
        "alpha: {for_alpha}"
    );
    assert!(
        !for_alpha.contains("Beta-class pattern"),
        "alpha leaked beta: {for_alpha}"
    );

    let for_beta = inject("crates/beta/src/y.rs");
    assert!(for_beta.contains("Beta-class pattern"), "beta: {for_beta}");
    assert!(
        !for_beta.contains("Alpha-class pattern"),
        "beta leaked alpha: {for_beta}"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// `project-meta` (default) prints a unified diff and writes NOTHING; `--apply` writes the
/// delimited block. Runs in a temp CWD with a fake CLAUDE.md + grounded corpus so the real files
/// are never touched.
#[test]
fn project_meta_diffs_then_applies() {
    let root = std::env::temp_dir().join(format!("cicatrix_pm_{}", std::process::id()));
    let corpus = root.join("docs/bugs/grounded");
    std::fs::create_dir_all(&corpus).unwrap();
    std::fs::write(
        corpus.join("BUG_Z.md"),
        "# BUG_Z\n\n\
         - **files:** crates/x/src/x.rs\n\
         - **fix-commit:** #1\n\
         - **regression-test:** g\n\
         - **meta-pattern:** Zeta-class pattern\n\n\
         ## Symptom\nbroke\n",
    )
    .unwrap();
    let claude = root.join("CLAUDE.md");
    let original = "# header\n\nunrelated content\n";
    std::fs::write(&claude, original).unwrap();

    // default: prints a diff, writes nothing
    let out = Command::new(BIN)
        .arg("project-meta")
        .current_dir(&root)
        .output()
        .expect("spawn");
    assert!(out.status.success(), "project-meta default should exit 0");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("+++ b/CLAUDE.md"),
        "expected a diff; got: {stdout}"
    );
    assert!(
        stdout.contains("Zeta-class pattern"),
        "diff omits new class: {stdout}"
    );
    assert_eq!(
        std::fs::read_to_string(&claude).unwrap(),
        original,
        "default mode must NOT mutate CLAUDE.md"
    );

    // --apply: writes the delimited block, preserves unrelated content
    let out = Command::new(BIN)
        .args(["project-meta", "--apply"])
        .current_dir(&root)
        .output()
        .expect("spawn");
    assert!(out.status.success(), "project-meta --apply should exit 0");
    let written = std::fs::read_to_string(&claude).unwrap();
    assert!(
        written.contains("unrelated content"),
        "clobbered unrelated content: {written}"
    );
    assert!(
        written.contains("<!-- cicatrix:meta-patterns:start -->"),
        "no marker: {written}"
    );
    assert!(
        written.contains("Zeta-class pattern"),
        "no class: {written}"
    );

    std::fs::remove_dir_all(&root).ok();
}
