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

// === Drift scanner (P3) ===

use std::fs;

/// Build a temp working dir containing a fixture markers.json (with RELATIVE repo paths so output
/// is byte-identical regardless of where the temp dir lives) plus a small repo corpus under it.
/// Returns the temp dir; caller spawns the binary with `current_dir(&dir)`.
fn drift_fixture(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("cicatrix_driftcli_{tag}_{}", std::process::id()));
    fs::remove_dir_all(&dir).ok();
    fs::create_dir_all(&dir).unwrap();

    // repo "alpha": rust, CLAUDE present, a Makefile ci target, two workflows.
    let alpha = dir.join("repos/alpha");
    fs::create_dir_all(alpha.join(".github/workflows")).unwrap();
    fs::write(alpha.join("Cargo.toml"), "[package]\n").unwrap();
    fs::write(alpha.join("CLAUDE.md"), "x").unwrap();
    fs::write(alpha.join("Makefile"), "ci:\n\tcargo test\n").unwrap();
    fs::write(alpha.join(".github/workflows/a.yml"), "x").unwrap();
    fs::write(alpha.join(".github/workflows/b.yaml"), "x").unwrap();

    // repo "beta": node, nothing else.
    let beta = dir.join("repos/beta");
    fs::create_dir_all(&beta).unwrap();
    fs::write(beta.join("package.json"), "{}").unwrap();

    // markers.json with RELATIVE paths + an absent repo to exercise the skip path.
    let markers = r#"{
  "root": "fixture-root",
  "repos": [
    { "name": "alpha", "path": "repos/alpha", "lang": "rust" },
    { "name": "beta", "path": "repos/beta" },
    { "name": "ghost", "path": "repos/ghost" }
  ]
}
"#;
    fs::write(dir.join("markers.json"), markers).unwrap();
    dir
}

fn run_in(dir: &std::path::Path, args: &[&str], now: &str) -> Output {
    Command::new(BIN)
        .args(args)
        .current_dir(dir)
        .env("CICATRIX_DRIFT_NOW", now)
        .output()
        .expect("failed to spawn cicatrix binary")
}

/// `drift scan` regenerates a dated table against a FIXTURE markers.json + temp corpus, and a
/// second identical run is BYTE-IDENTICAL (reproduce-on-unchanged). NOT the real ~/projects config.
#[test]
fn drift_scan_regenerates_and_reproduces() {
    let dir = drift_fixture("repro");
    let now = "2026-06-16";

    let out = run_in(&dir, &["drift", "scan"], now);
    assert!(
        out.status.success(),
        "scan should exit 0: {:?}",
        String::from_utf8_lossy(&out.stderr)
    );
    let printed = String::from_utf8_lossy(&out.stdout);
    let printed = printed.trim();
    assert_eq!(
        printed, "drift/convention-drift-2026-06-16.md",
        "printed: {printed}"
    );

    let path = dir.join(printed);
    assert!(path.exists(), "scan must write the dated file");
    let first = fs::read_to_string(&path).unwrap();

    // structure: header at the pinned date, both present repos as rows, the absent repo skipped.
    assert!(
        first.starts_with("# Convention-drift scan \u{2014} fixture-root \u{2014} 2026-06-16\n"),
        "{first}"
    );
    assert!(first.contains("| alpha | rust |"), "{first}");
    assert!(first.contains("| beta | node |"), "{first}");
    assert!(first.contains("## Skipped repos\n\n- ghost:"), "{first}");
    // rows are byte-wise sorted: alpha before beta.
    assert!(first.find("| alpha |").unwrap() < first.find("| beta |").unwrap());
    // single trailing newline.
    assert!(first.ends_with('\n') && !first.ends_with("\n\n"));

    // reproduce-on-unchanged: a second run yields byte-identical output.
    let out2 = run_in(&dir, &["drift", "scan"], now);
    assert!(out2.status.success());
    let second = fs::read_to_string(&path).unwrap();
    assert_eq!(
        first, second,
        "scan must be byte-identical on unchanged corpus"
    );

    fs::remove_dir_all(&dir).ok();
}

/// `drift scan --repo <path>` narrows to the single configured repo whose path matches exactly,
/// full-regenerating a single-row table (no merge of the others).
#[test]
fn drift_scan_repo_narrows_to_one() {
    let dir = drift_fixture("narrow");
    let out = run_in(
        &dir,
        &["drift", "scan", "--repo", "repos/alpha"],
        "2026-06-16",
    );
    assert!(
        out.status.success(),
        "scan --repo should exit 0: {:?}",
        String::from_utf8_lossy(&out.stderr)
    );
    let body = fs::read_to_string(dir.join("drift/convention-drift-2026-06-16.md")).unwrap();
    assert!(body.contains("| alpha | rust |"), "{body}");
    assert!(
        !body.contains("| beta |"),
        "must narrow to one repo: {body}"
    );
    fs::remove_dir_all(&dir).ok();
}

/// Bare `drift` (after the static arm was replaced) still prints a path that exists — proven here
/// by scanning a freshly-regenerated drift/ dir and joining the path.
#[test]
fn drift_bare_prints_newest_after_scan() {
    let dir = drift_fixture("newest");
    // generate two dated files; bare drift must print the lexically-greatest (newest).
    run_in(&dir, &["drift", "scan"], "2026-06-15");
    run_in(&dir, &["drift", "scan"], "2026-06-17");
    let out = Command::new(BIN)
        .arg("drift")
        .current_dir(&dir)
        .output()
        .expect("spawn");
    assert!(out.status.success());
    let printed = String::from_utf8_lossy(&out.stdout);
    let printed = printed.trim();
    assert_eq!(
        printed, "drift/convention-drift-2026-06-17.md",
        "newest: {printed}"
    );
    assert!(dir.join(printed).exists());
    fs::remove_dir_all(&dir).ok();
}

/// The markers.json hard-error seam (Frozen Decision #7): a missing OR malformed config is a hard
/// error at the CLI boundary — non-zero exit + a `cicatrix drift:` diagnostic on stderr. This is
/// the seam the loader-unit-tests can't reach; it proves `drift_scan` turns the loader Err into
/// the CLI failure (no silent success). Asserts the prefix, not OS-specific io text (portable).
#[test]
fn drift_scan_missing_or_malformed_markers_is_a_hard_error() {
    // missing markers.json: fresh empty temp dir, no config written.
    let missing =
        std::env::temp_dir().join(format!("cicatrix_driftcli_missing_{}", std::process::id()));
    fs::remove_dir_all(&missing).ok();
    fs::create_dir_all(&missing).unwrap();
    let out = run_in(&missing, &["drift", "scan"], "2026-06-16");
    assert!(
        !out.status.success(),
        "missing markers.json must be a hard error"
    );
    let e = String::from_utf8_lossy(&out.stderr);
    assert!(e.contains("cicatrix drift:"), "missing: stderr {e}");
    fs::remove_dir_all(&missing).ok();

    // malformed markers.json: present but not valid JSON.
    let bad = std::env::temp_dir().join(format!("cicatrix_driftcli_bad_{}", std::process::id()));
    fs::remove_dir_all(&bad).ok();
    fs::create_dir_all(&bad).unwrap();
    fs::write(bad.join("markers.json"), "{ not json").unwrap();
    let out = run_in(&bad, &["drift", "scan"], "2026-06-16");
    assert!(
        !out.status.success(),
        "malformed markers.json must be a hard error"
    );
    let e = String::from_utf8_lossy(&out.stderr);
    assert!(e.contains("cicatrix drift:"), "malformed: stderr {e}");
    fs::remove_dir_all(&bad).ok();
}

/// Usage errors: unknown subcommand, unknown flag, and `--repo` with no value each exit FAILURE
/// with a diagnostic on stderr.
#[test]
fn drift_usage_errors() {
    // unknown subcommand
    let out = run(&["drift", "bogus"]);
    assert!(!out.status.success(), "unknown subcommand should fail");
    let e = String::from_utf8_lossy(&out.stderr);
    assert!(
        e.contains("unknown subcommand") || e.contains("usage:"),
        "stderr: {e}"
    );

    // unknown flag (run in a fixture dir so markers.json loads; the flag error must fire first)
    let dir = drift_fixture("usage");
    let out = run_in(&dir, &["drift", "scan", "--nope"], "2026-06-16");
    assert!(!out.status.success(), "unknown flag should fail");
    let e = String::from_utf8_lossy(&out.stderr);
    assert!(e.contains("unknown flag"), "stderr: {e}");

    // --repo with no value
    let out = run_in(&dir, &["drift", "scan", "--repo"], "2026-06-16");
    assert!(!out.status.success(), "--repo without value should fail");
    let e = String::from_utf8_lossy(&out.stderr);
    assert!(
        e.contains("--repo needs a <path>") || e.contains("needs a"),
        "stderr: {e}"
    );

    fs::remove_dir_all(&dir).ok();
}

/// `query` with invalid `--frontier` prints a diagnostic and exits non-zero without network.
#[test]
fn query_invalid_frontier_fails_cleanly() {
    let out = run(&["query", "src/lib.rs", "--frontier", "nodeA:0"]);
    assert!(
        !out.status.success(),
        "query with zero timestamp frontier should fail"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("invalid --frontier"),
        "expected invalid --frontier in: {stderr}"
    );

    let out2 = run(&["query", "src/lib.rs", "--frontier", ""]);
    assert!(
        !out2.status.success(),
        "query with empty frontier should fail"
    );
}

/// `query` with `--frontier` missing argument exits non-zero with diagnostic.
#[test]
fn query_frontier_missing_arg_is_usage_error() {
    let out = run(&["query", "src/lib.rs", "--frontier"]);
    assert!(
        !out.status.success(),
        "query --frontier without value should fail"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--frontier needs a <vector>"),
        "stderr: {stderr}"
    );
}

// === CER-2755 P1.3: branch snapshot & forking interface ===

fn branch_test_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("cicatrix_branchcli_{tag}_{}", std::process::id()));
    fs::remove_dir_all(&dir).ok();
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn run_branch_cmd(db_path: &Path, args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env("CICATRIX_DB_PATH", db_path)
        .env("CICATRIX_NO_REVERIE", "1")
        .env("REVERIE_URL", "http://127.0.0.1:1")
        .output()
        .expect("failed to spawn cicatrix binary")
}

fn run_branch_cmd_with_env(db_path: &Path, args: &[&str], envs: &[(&str, &str)]) -> Output {
    let mut cmd = Command::new(BIN);
    cmd.args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env("CICATRIX_DB_PATH", db_path)
        .env("CICATRIX_NO_REVERIE", "1")
        .env("REVERIE_URL", "http://127.0.0.1:1");
    for (k, v) in envs {
        cmd.env(k, v);
    }
    cmd.output().expect("failed to spawn cicatrix binary")
}

#[test]
fn branch_usage_errors() {
    let out = run(&["branch"]);
    assert!(!out.status.success(), "bare branch must exit non-zero");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("usage: cicatrix branch"), "stderr: {err}");

    let out = run(&["branch", "bogus"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("unknown subcommand `bogus`") || err.contains("usage: cicatrix branch"),
        "stderr: {err}"
    );

    let out = run(&["branch", "fork"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("usage: cicatrix branch fork"), "stderr: {err}");

    let out = run(&["branch", "fork", "id", "--from"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("--from needs a <base>"), "stderr: {err}");

    let out = run(&["branch", "fork", "id", "--frontier"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("--frontier needs a <vector>"), "stderr: {err}");

    let out = run(&["branch", "fork", "id", "--unknown"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("unknown flag"), "stderr: {err}");

    let out = run(&["branch", "drop"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("usage: cicatrix branch drop"), "stderr: {err}");

    let out = run(&["branch", "settle"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("usage: cicatrix branch settle"),
        "stderr: {err}"
    );

    let out = run(&["branch", "list", "extra"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("usage: cicatrix branch list"), "stderr: {err}");

    let out = run(&["branch", "path"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("usage: cicatrix branch path"), "stderr: {err}");

    let out = run(&["record", "--branch"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("--branch needs a <snapshot_id>"),
        "stderr: {err}"
    );

    let out = run(&["query", "src/lib.rs", "--branch"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("--branch needs a <snapshot_id>"),
        "stderr: {err}"
    );
}

#[test]
fn branch_snapshot_id_validation_errors() {
    let out = run(&["branch", "fork", "../traversal"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("path traversal detected") || err.contains("invalid characters"),
        "stderr: {err}"
    );

    let out = run(&["branch", "fork", "invalid/slash"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("path traversal detected") || err.contains("invalid characters"),
        "stderr: {err}"
    );

    let out = run(&["branch", "fork", "with space"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("invalid characters"), "stderr: {err}");
}

#[test]
fn branch_lifecycle_and_nested_fork_cli() {
    let dir = branch_test_dir("lifecycle");
    let db_path = dir.join("cicatrix.db");

    let out = run_branch_cmd(&db_path, &["branch", "list"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("no active branch snapshots"),
        "stdout: {stdout}"
    );

    let out = run_branch_cmd(&db_path, &["branch", "fork", "b1", "--frontier", "nodeA:1"]);
    assert!(
        out.status.success(),
        "fork b1 failed: {:?}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("forked branch snapshot `b1`"),
        "stdout: {stdout}"
    );

    let out = run_branch_cmd(&db_path, &["branch", "path", "b1"]);
    assert!(out.status.success());
    let b1_path_str = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let b1_path = Path::new(&b1_path_str);
    assert!(
        b1_path.exists(),
        "branch db path does not exist: {b1_path_str}"
    );

    let out = run_branch_cmd(&db_path, &["branch", "fork", "b1"]);
    assert!(!out.status.success(), "duplicate fork should fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("already exists"), "stderr: {stderr}");

    let out = run_branch_cmd(
        &db_path,
        &[
            "branch",
            "fork",
            "b2",
            "--from",
            "b1",
            "--frontier",
            "nodeB:2",
        ],
    );
    assert!(
        out.status.success(),
        "fork b2 failed: {:?}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("forked branch snapshot `b2`"),
        "stdout: {stdout}"
    );

    let out = run_branch_cmd(&db_path, &["branch", "list"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("b1"), "list missing b1: {stdout}");
    assert!(stdout.contains("b2"), "list missing b2: {stdout}");
    assert!(
        stdout.contains("nodeA:1"),
        "list missing frontier nodeA:1: {stdout}"
    );
    assert!(
        stdout.contains("nodeB:2"),
        "list missing frontier nodeB:2: {stdout}"
    );

    let out = run_branch_cmd(&db_path, &["branch", "drop", "b2"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("dropped branch snapshot `b2`"),
        "stdout: {stdout}"
    );

    let out = run_branch_cmd(&db_path, &["branch", "drop", "b1"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("dropped branch snapshot `b1`"),
        "stdout: {stdout}"
    );
    assert!(!b1_path.exists(), "dropped db still exists on disk");

    let out = run_branch_cmd(&db_path, &["branch", "drop", "b1"]);
    assert!(!out.status.success(), "second drop should fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("not found"), "stderr: {stderr}");

    let out = run_branch_cmd(&db_path, &["branch", "list"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("no active branch snapshots"),
        "stdout: {stdout}"
    );

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn branch_record_query_isolation_and_settle() {
    let dir = branch_test_dir("settle");
    let db_path = dir.join("cicatrix.db");

    let out = run_branch_cmd(&db_path, &["branch", "fork", "feat-wt"]);
    assert!(
        out.status.success(),
        "fork failed: {:?}",
        String::from_utf8_lossy(&out.stderr)
    );

    let out = run_branch_cmd(
        &db_path,
        &[
            "record",
            "docs/bugs/grounded/BUG_EMBED_EMPTY_INPUT_400.md",
            "--branch",
            "feat-wt",
        ],
    );
    assert!(
        out.status.success(),
        "record to branch failed: {:?}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("recorded BUG_EMBED_EMPTY_INPUT_400 → branch feat-wt (sqlite)"),
        "stdout: {stdout}"
    );

    let out = run_branch_cmd(
        &db_path,
        &[
            "query",
            "crates/reverie-store/src/embed.rs",
            "--branch",
            "feat-wt",
        ],
    );
    assert!(
        out.status.success(),
        "branch query failed: {:?}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("BUG_EMBED_EMPTY_INPUT_400: known-bug surface"),
        "stdout: {stdout}"
    );

    let out = run_branch_cmd_with_env(
        &db_path,
        &["query", "crates/reverie-store/src/embed.rs"],
        &[("CICATRIX_BRANCH", "feat-wt")],
    );
    assert!(
        out.status.success(),
        "CICATRIX_BRANCH query failed: {:?}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("BUG_EMBED_EMPTY_INPUT_400: known-bug surface"),
        "stdout: {stdout}"
    );

    let out = run_branch_cmd(
        &db_path,
        &[
            "query",
            "crates/unrelated/src/lib.rs",
            "--branch",
            "feat-wt",
        ],
    );
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("no known-bug surface touched"),
        "stdout: {stdout}"
    );

    let out = run_branch_cmd(&db_path, &["branch", "settle", "feat-wt"]);
    assert!(
        out.status.success(),
        "settle failed: {:?}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("settled branch snapshot `feat-wt`: merged 1 fact(s)"),
        "stdout: {stdout}"
    );

    let out = run_branch_cmd(&db_path, &["branch", "list"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("no active branch snapshots"),
        "stdout: {stdout}"
    );

    let out = run_branch_cmd(&db_path, &["branch", "settle", "feat-wt"]);
    assert!(!out.status.success(), "second settle should fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("not found"), "stderr: {stderr}");

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn workflow_usage_errors() {
    let out = run(&["workflow"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("usage: cicatrix workflow"));

    let out = run(&["workflow", "invalid"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("unknown subcommand `invalid`"));

    let out = run(&["workflow", "run"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("usage: cicatrix workflow run"));

    let out = run(&["workflow", "run", "triage"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("usage: cicatrix workflow run triage"));

    let out = run(&["workflow", "status"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("usage: cicatrix workflow status"));

    let out = run(&["workflow", "run", "review-gate"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("usage: cicatrix workflow run review-gate"));

    let out = run(&["workflow", "signal"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("usage: cicatrix workflow signal"));
}

#[test]
fn workflow_run_triage_audit_list_status_cli() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let db_path = temp_dir.path().join("cli_workflows.db");
    let db_str = db_path.display().to_string();

    // 1. Run triage workflow
    let out = run(&[
        "workflow",
        "run",
        "triage",
        "tests::test_cli_workflow",
        "--repo",
        "cicatrix",
        "--candidate",
        "sha_1",
        "--candidate",
        "sha_2",
        "--failure-log",
        "assertion failed: (a == b)\n --> src/workflow/triage.rs:15:1",
        "--db",
        &db_str,
    ]);
    assert!(
        out.status.success(),
        "triage failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Workflow execution:"));
    assert!(stdout.contains("State: COMPLETED"));
    assert!(stdout.contains("Culprit commit: Some(\"sha_2\")"));
    assert!(stdout.contains("Candidate BugFact: BUG_TESTS_TEST_CLI_WORKFLOW"));

    // Extract execution ID from output
    let exec_id_line = stdout
        .lines()
        .find(|l| l.starts_with("Workflow execution:"))
        .expect("exec id line");
    let exec_id = exec_id_line
        .strip_prefix("Workflow execution:")
        .unwrap()
        .trim();

    // 2. Run audit workflow
    let out = run(&[
        "workflow", "run", "audit", "--repo", ".", "--marker", "DRIFT", "--db", &db_str,
    ]);
    assert!(
        out.status.success(),
        "audit failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Workflow execution:"));
    assert!(stdout.contains("State: COMPLETED"));
    assert!(stdout.contains("Scanned targets: 1"));

    // 3. List workflow executions
    let out = run(&["workflow", "list", "--db", &db_str]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("EXEC_ID"));
    assert!(stdout.contains("triage_workflow"));
    assert!(stdout.contains("audit_workflow"));
    assert!(stdout.contains("COMPLETED"));
    assert!(stdout.contains(exec_id));

    // 4. Status of specific execution
    let out = run(&["workflow", "status", exec_id, "--db", &db_str]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains(&format!("Execution ID:  {exec_id}")));
    assert!(stdout.contains("Workflow:      triage_workflow"));
    assert!(stdout.contains("State:         COMPLETED"));
    assert!(stdout.contains("Event Log"));
}

#[test]
fn workflow_review_gate_parking_and_signal_delivery_cli() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let db_path = temp_dir.path().join("cli_review_gate.db");
    let db_str = db_path.display().to_string();

    // 1. Run review-gate workflow -> should park on WAITING_SIGNAL(operator_verdict)
    let out = run(&[
        "workflow",
        "run",
        "review-gate",
        "refs/heads/feature-auth",
        "--description",
        "Authentication gate review",
        "--requested-by",
        "alice",
        "--db",
        &db_str,
    ]);
    assert!(
        out.status.success(),
        "review-gate run failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Workflow execution:"));
    assert!(stdout.contains("State: WAITING_SIGNAL(operator_verdict)"));

    let exec_id_line = stdout
        .lines()
        .find(|l| l.starts_with("Workflow execution:"))
        .expect("exec id line");
    let exec_id = exec_id_line
        .strip_prefix("Workflow execution:")
        .unwrap()
        .trim();

    // 2. Status before signal delivery -> shows WAITING_SIGNAL, no signals
    let out = run(&["workflow", "status", exec_id, "--db", &db_str]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("State:         WAITING_SIGNAL(operator_verdict)"));
    assert!(!stdout.contains("Durable Signals"));

    // 3. Deliver approved signal -> status DELIVERED, new run state COMPLETED
    let out = run(&[
        "workflow",
        "signal",
        exec_id,
        "approved",
        "--idempotency-key",
        "gate-key-001",
        "--operator",
        "ctodie",
        "--comments",
        "Signed off by security operator",
        "--db",
        &db_str,
    ]);
    assert!(
        out.status.success(),
        "signal delivery failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Status:          DELIVERED"));
    assert!(stdout.contains("Run State:       COMPLETED"));
    assert!(stdout.contains("Idempotency Key: gate-key-001"));

    // 4. Deliver duplicate signal -> status DUPLICATE, run state COMPLETED
    let out = run(&[
        "workflow",
        "signal",
        exec_id,
        "approved",
        "--idempotency-key",
        "gate-key-001",
        "--operator",
        "ctodie",
        "--comments",
        "Duplicate call",
        "--db",
        &db_str,
    ]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Status:          DUPLICATE"));
    assert!(stdout.contains("Run State:       COMPLETED"));

    // 5. Status after delivery -> shows COMPLETED and Durable Signals table
    let out = run(&["workflow", "status", exec_id, "--db", &db_str]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("State:         COMPLETED"));
    assert!(stdout.contains("Durable Signals (1 total):"));
    assert!(stdout.contains("signal=operator_verdict"));
    assert!(stdout.contains("key=gate-key-001"));
    assert!(stdout.contains("ctodie"));
    assert!(stdout.contains("Signed off by security operator"));
}

#[test]
fn workflow_triage_require_review_cli() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let db_path = temp_dir.path().join("cli_triage_review.db");
    let db_str = db_path.display().to_string();

    // 1. Run triage with --require-review -> parks on WAITING_SIGNAL(operator_verdict)
    let out = run(&[
        "workflow",
        "run",
        "triage",
        "tests::test_triage_gate",
        "--repo",
        "cicatrix",
        "--candidate",
        "sha_x",
        "--candidate",
        "sha_y",
        "--failure-log",
        "assertion failure at test_triage_gate",
        "--require-review",
        "--db",
        &db_str,
    ]);
    assert!(
        out.status.success(),
        "triage run failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("State: WAITING_SIGNAL(operator_verdict)"));

    let exec_id_line = stdout
        .lines()
        .find(|l| l.starts_with("Workflow execution:"))
        .expect("exec id line");
    let exec_id = exec_id_line
        .strip_prefix("Workflow execution:")
        .unwrap()
        .trim();

    // 2. Deliver rejected verdict
    let out = run(&[
        "workflow",
        "signal",
        exec_id,
        "rejected",
        "--idempotency-key",
        "triage-reject-001",
        "--operator",
        "ctodie",
        "--comments",
        "Culprit rejected pending investigation",
        "--db",
        &db_str,
    ]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Status:          DELIVERED"));
    assert!(stdout.contains("Run State:       COMPLETED"));

    // 3. Status confirms COMPLETED and signal details
    let out = run(&["workflow", "status", exec_id, "--db", &db_str]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("State:         COMPLETED"));
    assert!(stdout.contains("Durable Signals (1 total):"));
    assert!(stdout.contains("rejected"));
    assert!(stdout.contains("Culprit rejected pending investigation"));
}

#[test]
fn mcp_stdio_initialize_and_tools_list_cli() {
    use std::io::{BufRead, BufReader, Write};
    use std::process::Stdio;

    let mut child = Command::new(BIN)
        .arg("mcp")
        .arg("--stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn cicatrix mcp --stdio");

    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut reader = BufReader::new(stdout);

    // 1. Send initialize
    let init_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {}
    });
    let mut init_line = serde_json::to_string(&init_req).unwrap();
    init_line.push('\n');
    stdin.write_all(init_line.as_bytes()).unwrap();
    stdin.flush().unwrap();

    let mut resp_line = String::new();
    reader.read_line(&mut resp_line).unwrap();
    let resp: serde_json::Value = serde_json::from_str(&resp_line).expect("valid json response");
    assert_eq!(resp["id"], 1);
    assert_eq!(resp["result"]["protocolVersion"], "2024-11-05");
    assert_eq!(resp["result"]["serverInfo"]["name"], "cicatrix");

    // 2. Send tools/list
    let list_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list"
    });
    let mut list_line = serde_json::to_string(&list_req).unwrap();
    list_line.push('\n');
    stdin.write_all(list_line.as_bytes()).unwrap();
    stdin.flush().unwrap();

    let mut resp_line2 = String::new();
    reader.read_line(&mut resp_line2).unwrap();
    let resp2: serde_json::Value = serde_json::from_str(&resp_line2).expect("valid json response");
    assert_eq!(resp2["id"], 2);
    let tools = resp2["result"]["tools"].as_array().expect("tools array");
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"cicatrix_query_known_bugs"));
    assert!(names.contains(&"cicatrix_verify_diff"));
    assert!(names.contains(&"cicatrix_record_defect"));
    assert!(names.contains(&"cicatrix_start_workflow"));
    assert!(names.contains(&"cicatrix_workflow_status"));
    assert!(names.contains(&"cicatrix_submit_signal"));

    // 3. Send tools/call for cicatrix_verify_diff
    let call_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": {
            "name": "cicatrix_verify_diff",
            "arguments": {
                "diff": "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,1 +1,2 @@\n+pub fn test() {}\n"
            }
        }
    });
    let mut call_line = serde_json::to_string(&call_req).unwrap();
    call_line.push('\n');
    stdin.write_all(call_line.as_bytes()).unwrap();
    stdin.flush().unwrap();

    let mut resp_line3 = String::new();
    reader.read_line(&mut resp_line3).unwrap();
    let resp3: serde_json::Value = serde_json::from_str(&resp_line3).expect("valid json response");
    assert_eq!(resp3["id"], 3);
    assert!(resp3["result"]["content"].is_array());

    // Drop stdin to close pipe cleanly
    drop(stdin);
    let status = child.wait().expect("wait on child");
    assert!(status.success());
}

#[test]
fn mcp_http_server_endpoints_cli() {
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::time::Duration;

    // Pick a free random port
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind free port");
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let bind_addr = format!("127.0.0.1:{port}");

    let mut child = Command::new(BIN)
        .arg("serve")
        .arg("--mcp")
        .arg("--bind")
        .arg(&bind_addr)
        .spawn()
        .expect("failed to spawn cicatrix serve --mcp");

    // Wait until server is reachable (up to 5 seconds)
    let mut connected = false;
    for _ in 0..50 {
        if let Ok(mut stream) = TcpStream::connect(&bind_addr) {
            let req =
                format!("GET /health HTTP/1.1\r\nHost: {bind_addr}\r\nConnection: close\r\n\r\n");
            if stream.write_all(req.as_bytes()).is_ok() {
                let mut res = String::new();
                if stream.read_to_string(&mut res).is_ok() && res.contains("200 OK") {
                    connected = true;
                    break;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(connected, "HTTP server did not become ready in time");

    // 1. GET /health
    {
        let mut stream = TcpStream::connect(&bind_addr).expect("connect to server");
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let req = format!("GET /health HTTP/1.1\r\nHost: {bind_addr}\r\nConnection: close\r\n\r\n");
        stream.write_all(req.as_bytes()).unwrap();

        let mut res = String::new();
        stream.read_to_string(&mut res).unwrap();
        assert!(res.contains("200 OK"));
        assert!(res.contains("\"status\":\"ok\""));
        assert!(res.contains("\"service\":\"cicatrix-mcp\""));
    }

    // 2. POST /mcp (JSON-RPC initialize)
    {
        let mut stream = TcpStream::connect(&bind_addr).expect("connect to server");
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let json_body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 42,
            "method": "initialize",
            "params": {}
        })
        .to_string();

        let req = format!(
            "POST /mcp HTTP/1.1\r\nHost: {bind_addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            json_body.len(),
            json_body
        );
        stream.write_all(req.as_bytes()).unwrap();

        let mut res = String::new();
        stream.read_to_string(&mut res).unwrap();
        assert!(res.contains("200 OK"));
        assert!(res.contains("\"id\":42"));
        assert!(res.contains("\"protocolVersion\":\"2024-11-05\""));
    }

    // Kill child process
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn mcp_usage_errors() {
    let out = run(&["mcp", "--unknown"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("unknown flag --unknown"));

    let out = run(&["serve", "--invalid"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("unknown flag --invalid"));
}

fn run_with_stdin(args: &[&str], input: &str) -> Output {
    use std::io::Write;
    use std::process::Stdio;

    let mut child = Command::new(BIN)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn cicatrix binary");

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(input.as_bytes());
    }

    child.wait_with_output().expect("failed to wait on child")
}

#[test]
fn reversibility_usage_errors() {
    let out = run(&["reversibility"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("usage: cicatrix reversibility"));

    let out = run(&["reversibility", "unknown"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("unknown subcommand `unknown`"));

    let out = run(&["reversibility", "eval", "--diff"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("--diff requires a path or '-' for stdin"));

    let out = run(&["reversibility", "eval", "--diff", "nonexistent.patch"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("failed to read `nonexistent.patch`"));

    let out = run(&["reversibility", "eval", "--tier"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("--tier requires"));

    let out = run(&["reversibility", "eval", "--tier", "godmode"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("invalid autonomy tier"));
}

#[test]
fn reversibility_classify_cli() {
    let additive_diff = r#"
diff --git a/tests/test_foo.rs b/tests/test_foo.rs
new file mode 100644
index 0000000..1111111
--- /dev/null
+++ b/tests/test_foo.rs
@@ -0,0 +1,3 @@
+#[test]
+fn test_additive() {}
+"#;
    let out = run_with_stdin(
        &["reversibility", "classify", "--diff", "-", "--json"],
        additive_diff,
    );
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("\"type\": \"reversible_additive\""));
    assert!(stdout.contains("tests/test_foo.rs"));

    let plain_out = run_with_stdin(&["reversibility", "classify", "--diff", "-"], additive_diff);
    assert!(plain_out.status.success());
    let plain_stdout = String::from_utf8_lossy(&plain_out.stdout);
    assert!(plain_stdout.contains("Action Class: ReversibleAdditive"));

    let migration_diff = r#"
diff --git a/migrations/0002_drop_legacy_tables.sql b/migrations/0002_drop_legacy_tables.sql
new file mode 100644
index 0000000..2222222
--- /dev/null
+++ b/migrations/0002_drop_legacy_tables.sql
@@ -0,0 +1,2 @@
+DROP TABLE legacy_users;
+DROP TABLE legacy_tokens;
+"#;
    let out = run_with_stdin(
        &["reversibility", "classify", "--diff", "-", "--json"],
        migration_diff,
    );
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("\"type\": \"one_way_door\""));
    assert!(stdout.contains("migration file path touched"));
}

#[test]
fn reversibility_plan_cli() {
    let edit_diff = r#"
diff --git a/src/lib.rs b/src/lib.rs
index 1111111..2222222 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,4 @@
 pub fn answer() -> u32 {
-    41
+    42
+    // comment
 }
"#;
    let out = run_with_stdin(
        &["reversibility", "plan", "--diff", "-", "--json"],
        edit_diff,
    );
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("\"total_ops\": 1"));
    assert!(stdout.contains("rollback_script"));
    assert!(stdout.contains("patch -p1"));

    let plain_out = run_with_stdin(&["reversibility", "plan", "--diff", "-"], edit_diff);
    assert!(plain_out.status.success());
    let plain_stdout = String::from_utf8_lossy(&plain_out.stdout);
    assert!(plain_stdout.contains("Compensation Plan (1 ops)"));
}

#[test]
fn reversibility_validate_cli() {
    let additive_diff = r#"
diff --git a/tests/test_valid.rs b/tests/test_valid.rs
new file mode 100644
index 0000000..1111111
--- /dev/null
+++ b/tests/test_valid.rs
@@ -0,0 +1,3 @@
+#[test]
+fn test_valid() {}
+"#;
    let out = run_with_stdin(
        &["reversibility", "validate", "--diff", "-", "--json"],
        additive_diff,
    );
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("\"status\": \"valid\""));
    assert!(stdout.contains("\"baseline_restored\": true"));
}

#[test]
fn reversibility_eval_tiers_cli() {
    let additive_diff = r#"
diff --git a/tests/test_auto.rs b/tests/test_auto.rs
new file mode 100644
index 0000000..1111111
--- /dev/null
+++ b/tests/test_auto.rs
@@ -0,0 +1,3 @@
+#[test]
+fn test_auto() {}
+"#;
    // Autonomous tier: pure additive -> auto_commit
    let out = run_with_stdin(
        &[
            "reversibility",
            "eval",
            "--diff",
            "-",
            "--tier",
            "autonomous",
            "--json",
        ],
        additive_diff,
    );
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("\"verdict\": \"auto_commit\""));

    // Supervised tier: pure additive -> auto_commit_with_notice
    let out = run_with_stdin(
        &[
            "reversibility",
            "eval",
            "--diff",
            "-",
            "--tier",
            "supervised",
            "--json",
        ],
        additive_diff,
    );
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("\"verdict\": \"auto_commit_with_notice\""));

    let edit_diff = r#"
diff --git a/src/lib.rs b/src/lib.rs
index 1111111..2222222 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,3 @@
-const V: usize = 1;
+const V: usize = 2;
"#;
    let out = run_with_stdin(
        &[
            "reversibility",
            "eval",
            "--diff",
            "-",
            "--tier",
            "autonomous",
            "--json",
        ],
        edit_diff,
    );
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("\"verdict\": \"auto_commit_with_notice\""));

    let migration_diff = r#"
diff --git a/migrations/0003_drop_stuff.sql b/migrations/0003_drop_stuff.sql
new file mode 100644
index 0000000..3333333
--- /dev/null
+++ b/migrations/0003_drop_stuff.sql
@@ -0,0 +1,1 @@
+DROP TABLE accounts;
+"#;
    let out = run_with_stdin(
        &[
            "reversibility",
            "eval",
            "--diff",
            "-",
            "--tier",
            "autonomous",
            "--json",
        ],
        migration_diff,
    );
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("\"verdict\": \"route_to_approval\""));
}

fn run_with_env(args: &[&str], envs: &[(&str, &str)]) -> Output {
    let mut cmd = Command::new(BIN);
    cmd.args(args);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    cmd.output().expect("failed to spawn cicatrix binary")
}

fn run_with_env_and_stdin(args: &[&str], envs: &[(&str, &str)], input: &str) -> Output {
    use std::io::Write;
    use std::process::Stdio;

    let mut cmd = Command::new(BIN);
    cmd.args(args);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().expect("failed to spawn cicatrix binary");
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(input.as_bytes());
    }
    child.wait_with_output().expect("failed to wait on child")
}

#[test]
fn tripwire_usage_errors() {
    let out = run(&["tripwire"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("usage: cicatrix tripwire"));

    let out = run(&["tripwire", "unknown_verb"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("unknown subcommand `unknown_verb`"));

    let out = run(&["tripwire", "check"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("usage: cicatrix tripwire check"));

    let out = run(&["tripwire", "touches", "--limit", "abc"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("--limit requires a positive integer"));
}

#[test]
fn tripwire_list_and_seed_cli() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let db_path = tmp.path().join("tripwire_list.db");
    let db_str = db_path.to_str().unwrap();
    let envs = [("CICATRIX_DB_PATH", db_str)];

    // 1. Text list output
    let out = run_with_env(&["tripwire", "list"], &envs);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Registered Synthetic Canaries"));
    assert!(stdout.contains("TRIPWIRE_CANARY_SENTINEL_ALPHA"));
    assert!(stdout.contains(".cicatrix/sentinel/canary_alpha.rs"));

    // 2. JSON list output
    let out_json = run_with_env(&["tripwire", "list", "--json"], &envs);
    assert!(out_json.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out_json.stdout).expect("valid json");
    let arr = v.as_array().expect("array of canaries");
    assert!(arr.len() >= 4, "expected at least 4 default canaries");
    assert!(arr
        .iter()
        .any(|c| c["id"] == "TRIPWIRE_CANARY_SENTINEL_ALPHA"));

    // 3. Seed idempotent execution
    let out_seed = run_with_env(&["tripwire", "seed"], &envs);
    assert!(out_seed.status.success());
    let stdout = String::from_utf8_lossy(&out_seed.stdout);
    assert!(stdout.contains("Seeded 0 synthetic canaries into tripwire registry"));
}

#[test]
fn tripwire_check_and_touches_cli() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let db_path = tmp.path().join("tripwire_check.db");
    let db_str = db_path.to_str().unwrap();
    let envs = [
        ("CICATRIX_DB_PATH", db_str),
        ("CICATRIX_TRIPWIRE_MOCK_NOTIFY", "1"),
    ];

    // 1. Authorized check passes
    let out = run_with_env(
        &[
            "tripwire",
            "check",
            ".cicatrix/sentinel/canary_alpha.rs",
            "--actor",
            "operator",
        ],
        &envs,
    );
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Tripwire check clear: permitted for actor `operator`"));

    // 2. Authorized check JSON output
    let out_json = run_with_env(
        &[
            "tripwire",
            "check",
            ".cicatrix/sentinel/canary_alpha.rs",
            "--actor",
            "operator",
            "--json",
        ],
        &envs,
    );
    assert!(out_json.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out_json.stdout).expect("valid json");
    assert_eq!(v["status"], "clear");
    assert_eq!(v["verdict"], "permitted");

    // 3. Unauthorized check trips circuit breaker (fail-closed)
    let out_trip = run_with_env(
        &[
            "tripwire",
            "check",
            ".cicatrix/sentinel/canary_alpha.rs",
            "--actor",
            "unauthorized_agent",
        ],
        &envs,
    );
    assert!(!out_trip.status.success());
    let stderr = String::from_utf8_lossy(&out_trip.stderr);
    assert!(stderr.contains("intrusion detected"));
    assert!(stderr.contains("TRIPWIRE_CANARY_SENTINEL_ALPHA"));

    // 4. Unauthorized check JSON output
    let out_trip_json = run_with_env(
        &[
            "tripwire",
            "check",
            ".cicatrix/sentinel/canary_alpha.rs",
            "--actor",
            "unauthorized_agent",
            "--json",
        ],
        &envs,
    );
    assert!(!out_trip_json.status.success());
    let v: serde_json::Value =
        serde_json::from_slice(&out_trip_json.stdout).expect("valid json output on tripped check");
    assert_eq!(v["status"], "tripped");
    assert_eq!(v["error"], "tripwire_intrusion_detected");
    assert_eq!(v["canary_id"], "TRIPWIRE_CANARY_SENTINEL_ALPHA");
    assert_eq!(v["cortex_notified"], true);

    // 5. Touches audit log listing
    let out_touches = run_with_env(&["tripwire", "touches"], &envs);
    assert!(out_touches.status.success());
    let stdout = String::from_utf8_lossy(&out_touches.stdout);
    assert!(stdout.contains("Tripwire Touch Events"));
    assert!(stdout.contains("canary=TRIPWIRE_CANARY_SENTINEL_ALPHA"));
    assert!(stdout.contains("actor=unauthorized_agent"));
    assert!(stdout.contains("verdict=circuit_broken"));

    // 6. Touches filtered by canary with JSON output
    let out_touches_json = run_with_env(
        &[
            "tripwire",
            "touches",
            "--canary",
            "TRIPWIRE_CANARY_SENTINEL_ALPHA",
            "--json",
        ],
        &envs,
    );
    assert!(out_touches_json.status.success());
    let arr: Vec<serde_json::Value> =
        serde_json::from_slice(&out_touches_json.stdout).expect("valid touches array json");
    assert!(!arr.is_empty());
    assert!(arr
        .iter()
        .any(|t| t["verdict"] == "circuit_broken" && t["actor"] == "unauthorized_agent"));
}

#[test]
fn query_cli_fails_closed_on_tripwire_canary() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let db_path = tmp.path().join("query_tripwire.db");
    let db_str = db_path.to_str().unwrap();
    let envs = [
        ("CICATRIX_DB_PATH", db_str),
        ("CICATRIX_TRIPWIRE_MOCK_NOTIFY", "1"),
    ];

    let out = run_with_env(
        &[
            "query",
            ".cicatrix/sentinel/canary_alpha.rs",
            "--actor",
            "unauthorized_reader",
        ],
        &envs,
    );
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("tripwire intrusion detected"));
    assert!(stderr.contains("TRIPWIRE_CANARY_SENTINEL_ALPHA"));
}

#[test]
fn reversibility_eval_fails_closed_on_tripwire_marker() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let db_path = tmp.path().join("rev_tripwire.db");
    let db_str = db_path.to_str().unwrap();
    let envs = [
        ("CICATRIX_DB_PATH", db_str),
        ("CICATRIX_TRIPWIRE_MOCK_NOTIFY", "1"),
    ];

    let sentinel_diff = r#"
diff --git a/src/token.rs b/src/token.rs
--- a/src/token.rs
+++ b/src/token.rs
@@ -1,1 +1,2 @@
+// TRIPWIRE_MARKER_CREDENTIAL_CANARY_DO_NOT_READ
"#;

    // Plain output format
    let out = run_with_env_and_stdin(
        &[
            "reversibility",
            "eval",
            "--diff",
            "-",
            "--actor",
            "untrusted_agent",
        ],
        &envs,
        sentinel_diff,
    );
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("tripwire intrusion detected"));
    assert!(stderr.contains("TRIPWIRE_CANARY_AUTH_TOKEN"));

    // JSON error format
    let out_json = run_with_env_and_stdin(
        &[
            "reversibility",
            "eval",
            "--diff",
            "-",
            "--actor",
            "untrusted_agent",
            "--json",
        ],
        &envs,
        sentinel_diff,
    );
    assert!(!out_json.status.success());
    let stderr = String::from_utf8_lossy(&out_json.stderr);
    let v: serde_json::Value = serde_json::from_str(&stderr)
        .expect("valid json error output on tripped reversibility eval");
    assert_eq!(v["error"], "tripwire_intrusion_detected");
    assert_eq!(v["canary_id"], "TRIPWIRE_CANARY_AUTH_TOKEN");
    assert_eq!(v["cortex_notified"], true);
}

#[test]
fn unified_error_masking_rest_mcp_and_cli() {
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::time::Duration;

    // Pick a free random port
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind free port");
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let bind_addr = format!("127.0.0.1:{port}");

    let mut child = Command::new(BIN)
        .arg("serve")
        .arg("--mcp")
        .arg("--bind")
        .arg(&bind_addr)
        .spawn()
        .expect("failed to spawn cicatrix serve --mcp");

    // Wait until server is reachable
    let mut connected = false;
    for _ in 0..50 {
        if let Ok(mut stream) = TcpStream::connect(&bind_addr) {
            let req =
                format!("GET /health HTTP/1.1\r\nHost: {bind_addr}\r\nConnection: close\r\n\r\n");
            if stream.write_all(req.as_bytes()).is_ok() {
                let mut res = String::new();
                if stream.read_to_string(&mut res).is_ok() && res.contains("200 OK") {
                    connected = true;
                    break;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(connected, "HTTP server did not become ready in time");

    // 1. GET /api/v1/error/simulate_500: Assert 500 status, X-Correlation-Id header, and zero leaks
    {
        let mut stream = TcpStream::connect(&bind_addr).expect("connect to server");
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let req = format!("GET /api/v1/error/simulate_500 HTTP/1.1\r\nHost: {bind_addr}\r\nConnection: close\r\n\r\n");
        stream.write_all(req.as_bytes()).unwrap();

        let mut res = String::new();
        stream.read_to_string(&mut res).unwrap();
        assert!(
            res.contains("500 Internal Server Error"),
            "Expected 500, got: {res}"
        );
        assert!(
            res.contains("X-Correlation-Id: ref_"),
            "Expected X-Correlation-Id header, got: {res}"
        );
        assert!(
            res.contains("\"error\":\"internal_server_error\""),
            "Expected internal_server_error, got: {res}"
        );
        assert!(
            res.contains("internal server error (correlation: ref_"),
            "Expected correlation in body, got: {res}"
        );

        // Invariant: zero path or credential leaks in body
        let body_start = res.find("\r\n\r\n").expect("end of headers") + 4;
        let body = &res[body_start..];
        assert!(!body.contains("/home/"), "Leaked /home/ in body: {body}");
        assert!(!body.contains("/tmp/"), "Leaked /tmp/ in body: {body}");
        assert!(
            !body.contains("secret123"),
            "Leaked secret123 in body: {body}"
        );
        assert!(!body.contains("token="), "Leaked token in body: {body}");
    }

    // 2. POST /api/v1/query with malformed JSON: Assert 400 Bad Request client error
    {
        let mut stream = TcpStream::connect(&bind_addr).expect("connect to server");
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let bad_payload = "{ not valid json";
        let req = format!(
            "POST /api/v1/query HTTP/1.1\r\nHost: {bind_addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            bad_payload.len(),
            bad_payload
        );
        stream.write_all(req.as_bytes()).unwrap();

        let mut res = String::new();
        stream.read_to_string(&mut res).unwrap();
        assert!(res.contains("400 Bad Request"), "Expected 400, got: {res}");
        assert!(
            res.contains("\"error\":\"bad_request\""),
            "Expected bad_request, got: {res}"
        );
    }

    // 3. POST /api/v1/tripwire/check with valid payload: Assert 200 OK
    {
        let mut stream = TcpStream::connect(&bind_addr).expect("connect to server");
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let payload = serde_json::json!({
            "target": "src/normal.rs",
            "actor": "operator"
        })
        .to_string();
        let req = format!(
            "POST /api/v1/tripwire/check HTTP/1.1\r\nHost: {bind_addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            payload.len(),
            payload
        );
        stream.write_all(req.as_bytes()).unwrap();

        let mut res = String::new();
        stream.read_to_string(&mut res).unwrap();
        assert!(res.contains("200 OK"), "Expected 200 OK, got: {res}");
        assert!(
            res.contains("\"verdict\":\"permitted\""),
            "Expected verdict permitted, got: {res}"
        );
    }

    // 4. POST /mcp with invalid JSON: Assert 400 Bad Request with sanitized message
    {
        let mut stream = TcpStream::connect(&bind_addr).expect("connect to server");
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let bad_mcp = "{ invalid mcp json";
        let req = format!(
            "POST /mcp HTTP/1.1\r\nHost: {bind_addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            bad_mcp.len(),
            bad_mcp
        );
        stream.write_all(req.as_bytes()).unwrap();

        let mut res = String::new();
        stream.read_to_string(&mut res).unwrap();
        assert!(res.contains("400 Bad Request"), "Expected 400, got: {res}");
        assert!(
            res.contains("parse error"),
            "Expected parse error, got: {res}"
        );
    }

    let _ = child.kill();
    let _ = child.wait();

    // 5. CLI Error Masking: offline query produces masked correlation ID on stderr
    let out = run_offline(&["query", "crates/reverie-store/src/embed.rs"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("correlation: ref_"),
        "CLI stderr should contain masked correlation: {stderr}"
    );
    assert!(
        stderr.contains("[cicatrix] internal error [ref_"),
        "CLI stderr should log correlation tag: {stderr}"
    );
    assert!(
        !stderr.contains("password"),
        "CLI stderr must not leak credentials"
    );
}

#[test]
fn autonomy_cli_usage_errors() {
    let out = run(&["autonomy"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("usage: cicatrix autonomy"));

    let out = run(&["autonomy", "unknown_subcmd"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("unknown subcommand `unknown_subcmd`"));

    let out = run(&["autonomy", "status", "--unknown"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("unknown option `--unknown`"));

    let out = run(&["autonomy", "promote"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("missing required --actor"));

    let out = run(&["autonomy", "demote"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("missing required --actor"));

    let out = run(&["autonomy", "check"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("missing required --actor"));

    let out = run(&["autonomy", "history", "--limit", "abc"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("--limit requires a positive integer"));
}

#[test]
fn autonomy_cli_status_and_defaults() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let db_path = tmp.path().join("autonomy_status.db");
    let db_str = db_path.to_str().unwrap();
    let envs = [("CICATRIX_DB_PATH", db_str)];

    // 1. Text list output with seeded capabilities
    let out = run_with_env(&["autonomy", "status"], &envs);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Autonomy Trust Ladder States"));
    assert!(stdout.contains("wheelhorse"));
    assert!(stdout.contains("code_edit"));

    // 2. Query seeded autonomous capability
    let out_seeded = run_with_env(
        &[
            "autonomy",
            "status",
            "--actor",
            "wheelhorse",
            "--capability",
            "code_edit",
        ],
        &envs,
    );
    assert!(out_seeded.status.success());
    let stdout = String::from_utf8_lossy(&out_seeded.stdout);
    assert!(stdout.contains("Actor: wheelhorse"));
    assert!(stdout.contains("Capability: code_edit"));
    assert!(stdout.contains("Current Tier: autonomous"));

    // 3. Query seeded autonomous capability as JSON
    let out_json = run_with_env(
        &[
            "autonomy",
            "status",
            "--actor",
            "wheelhorse",
            "--capability",
            "code_edit",
            "--json",
        ],
        &envs,
    );
    assert!(out_json.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out_json.stdout).expect("valid json");
    assert_eq!(v["current_tier"], "autonomous");
    assert_eq!(v["actor"], "wheelhorse");
    assert_eq!(v["capability"], "code_edit");

    // 4. Query unrecorded capability defaults to shadow
    let out_unrec = run_with_env(
        &[
            "autonomy",
            "status",
            "--actor",
            "wheelhorse",
            "--capability",
            "unrecorded_neural_optimizer",
        ],
        &envs,
    );
    assert!(out_unrec.status.success());
    let stdout = String::from_utf8_lossy(&out_unrec.stdout);
    assert!(stdout.contains("Current Tier: shadow (unrecorded, default)"));

    // 5. Query unrecorded capability JSON
    let out_unrec_json = run_with_env(
        &[
            "autonomy",
            "status",
            "--actor",
            "wheelhorse",
            "--capability",
            "unrecorded_neural_optimizer",
            "--json",
        ],
        &envs,
    );
    assert!(out_unrec_json.status.success());
    let v_unrec: serde_json::Value =
        serde_json::from_slice(&out_unrec_json.stdout).expect("valid json");
    assert_eq!(v_unrec["tier"], "shadow");
}

#[test]
fn autonomy_cli_promote_check_demote_history_lifecycle() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let db_path = tmp.path().join("autonomy_lifecycle.db");
    let db_str = db_path.to_str().unwrap();
    let envs = [("CICATRIX_DB_PATH", db_str)];

    // 1. Check initially fails for supervised requirement when tier is shadow (unrecorded)
    let out_check1 = run_with_env(
        &[
            "autonomy",
            "check",
            "--actor",
            "agent_x",
            "--capability",
            "background_ops",
            "--tier",
            "supervised",
        ],
        &envs,
    );
    assert!(!out_check1.status.success());
    let stderr = String::from_utf8_lossy(&out_check1.stderr);
    assert!(stderr.contains("DENIED"));

    // 2. Promote to supervised
    let out_promote1 = run_with_env(
        &[
            "autonomy",
            "promote",
            "--actor",
            "agent_x",
            "--capability",
            "background_ops",
            "--to",
            "supervised",
            "--reason",
            "Completed 50 shadow tasks without failure",
            "--authorized-by",
            "operator",
            "--evidence",
            "{\"tasks_completed\": 50}",
        ],
        &envs,
    );
    assert!(out_promote1.status.success());
    let stdout = String::from_utf8_lossy(&out_promote1.stdout);
    assert!(stdout.contains(
        "PROMOTED: actor `agent_x` capability `background_ops` from `shadow` to `supervised`"
    ));

    // 3. Check now passes for supervised
    let out_check2 = run_with_env(
        &[
            "autonomy",
            "check",
            "--actor",
            "agent_x",
            "--capability",
            "background_ops",
            "--tier",
            "supervised",
        ],
        &envs,
    );
    assert!(out_check2.status.success());
    let stdout = String::from_utf8_lossy(&out_check2.stdout);
    assert!(stdout.contains("GRANTED"));

    // 4. Check fails for autonomous
    let out_check3 = run_with_env(
        &[
            "autonomy",
            "check",
            "--actor",
            "agent_x",
            "--capability",
            "background_ops",
            "--tier",
            "autonomous",
        ],
        &envs,
    );
    assert!(!out_check3.status.success());

    // 5. Promote to autonomous with JSON output
    let out_promote2 = run_with_env(
        &[
            "autonomy",
            "promote",
            "--actor",
            "agent_x",
            "--capability",
            "background_ops",
            "--to",
            "autonomous",
            "--reason",
            "Exceeded benchmark thresholds",
            "--authorized-by",
            "operator",
            "--json",
        ],
        &envs,
    );
    assert!(out_promote2.status.success());
    let v_prom: serde_json::Value =
        serde_json::from_slice(&out_promote2.stdout).expect("valid json");
    assert_eq!(v_prom["to_tier"], "autonomous");
    assert_eq!(v_prom["from_tier"], "supervised");

    // 6. Check now passes for autonomous
    let out_check4 = run_with_env(
        &[
            "autonomy",
            "check",
            "--actor",
            "agent_x",
            "--capability",
            "background_ops",
            "--tier",
            "autonomous",
            "--json",
        ],
        &envs,
    );
    assert!(out_check4.status.success());
    let v_chk: serde_json::Value = serde_json::from_slice(&out_check4.stdout).expect("valid json");
    assert_eq!(v_chk["permitted"], true);
    assert_eq!(v_chk["can_execute_autonomously"], true);
    assert_eq!(v_chk["requires_approval"], false);

    // 7. Demote back to supervised
    let out_demote = run_with_env(
        &[
            "autonomy",
            "demote",
            "--actor",
            "agent_x",
            "--capability",
            "background_ops",
            "--to",
            "supervised",
            "--reason",
            "Incident investigation requested supervisor review",
            "--authorized-by",
            "operator",
        ],
        &envs,
    );
    assert!(out_demote.status.success());
    let stdout = String::from_utf8_lossy(&out_demote.stdout);
    assert!(stdout.contains(
        "DEMOTED: actor `agent_x` capability `background_ops` from `autonomous` to `supervised`"
    ));

    // 8. History reflects promotions and demotions
    let out_hist = run_with_env(
        &[
            "autonomy",
            "history",
            "--actor",
            "agent_x",
            "--capability",
            "background_ops",
            "--json",
        ],
        &envs,
    );
    assert!(out_hist.status.success());
    let v_hist: serde_json::Value = serde_json::from_slice(&out_hist.stdout).expect("valid json");
    let arr = v_hist.as_array().expect("array of history events");
    assert_eq!(arr.len(), 3);
    assert_eq!(arr[0]["action_type"], "demote");
    assert_eq!(arr[1]["action_type"], "promote");
    assert_eq!(arr[2]["action_type"], "promote");
}

#[test]
fn autonomy_cli_soma_production_invariant() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let db_path = tmp.path().join("autonomy_soma.db");
    let db_str = db_path.to_str().unwrap();
    let envs = [("CICATRIX_DB_PATH", db_str)];

    // 1. Soma production capability check fails closed for autonomous tier
    let out_check = run_with_env(
        &[
            "autonomy",
            "check",
            "--actor",
            "wheelhorse",
            "--capability",
            "soma_production_gate",
            "--tier",
            "autonomous",
        ],
        &envs,
    );
    assert!(!out_check.status.success());
    let stderr = String::from_utf8_lossy(&out_check.stderr);
    assert!(stderr.contains("strictly require human operator verdicts"));

    // 2. Promotion of Soma capability to autonomous tier is strictly prohibited and fails closed
    let out_promote = run_with_env(
        &[
            "autonomy",
            "promote",
            "--actor",
            "wheelhorse",
            "--capability",
            "soma_production_gate",
            "--to",
            "autonomous",
            "--reason",
            "Bypass operator gate",
            "--authorized-by",
            "operator",
        ],
        &envs,
    );
    assert!(!out_promote.status.success());
    let stderr = String::from_utf8_lossy(&out_promote.stderr);
    assert!(stderr.contains("strictly require human operator verdicts"));

    // 3. Reversibility verification on Soma production diff routes to human approval
    let soma_diff = r#"diff --git a/crates/soma/src/policy.rs b/crates/soma/src/policy.rs
new file mode 100644
--- /dev/null
+++ b/crates/soma/src/policy.rs
@@ -0,0 +1,5 @@
+// Soma production cluster policy definition
+pub fn enforce_gate() -> bool {
+    true
+}
+"#;
    let out_rev = run_with_env_and_stdin(
        &[
            "reversibility",
            "eval",
            "--diff",
            "-",
            "--tier",
            "autonomous",
            "--json",
        ],
        &envs,
        soma_diff,
    );
    assert!(out_rev.status.success());
    let v_rev: serde_json::Value =
        serde_json::from_slice(&out_rev.stdout).expect("valid json reversibility report");
    assert_eq!(v_rev["verdict"]["verdict"], "route_to_approval");
    assert_eq!(v_rev["verdict"]["signal_name"], "operator_verdict");
    assert_eq!(v_rev["is_soma_production"], true);
}

#[test]
fn autonomy_rest_endpoints_and_error_masking() {
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::time::Duration;

    let tmp = tempfile::tempdir().expect("tempdir");
    let db_path = tmp.path().join("autonomy_rest.db");
    let db_str = db_path.to_str().unwrap().to_string();

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind free port");
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let bind_addr = format!("127.0.0.1:{port}");

    let mut child = Command::new(BIN)
        .arg("serve")
        .arg("--mcp")
        .arg("--bind")
        .arg(&bind_addr)
        .env("CICATRIX_DB_PATH", &db_str)
        .spawn()
        .expect("failed to spawn cicatrix serve --mcp");

    // Wait until server is reachable
    let mut connected = false;
    for _ in 0..50 {
        if let Ok(mut stream) = TcpStream::connect(&bind_addr) {
            let req =
                format!("GET /health HTTP/1.1\r\nHost: {bind_addr}\r\nConnection: close\r\n\r\n");
            if stream.write_all(req.as_bytes()).is_ok() {
                let mut res = String::new();
                if stream.read_to_string(&mut res).is_ok() && res.contains("200 OK") {
                    connected = true;
                    break;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(connected, "HTTP server did not become ready in time");

    // 1. POST /api/v1/autonomy/tier: Retrieve seeded capability tier
    {
        let mut stream = TcpStream::connect(&bind_addr).expect("connect to server");
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let payload = serde_json::json!({
            "actor": "wheelhorse",
            "capability": "code_edit"
        })
        .to_string();
        let req = format!(
            "POST /api/v1/autonomy/tier HTTP/1.1\r\nHost: {bind_addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            payload.len(),
            payload
        );
        stream.write_all(req.as_bytes()).unwrap();

        let mut res = String::new();
        stream.read_to_string(&mut res).unwrap();
        assert!(res.contains("200 OK"), "Expected 200 OK, got: {res}");
        assert!(
            res.contains("\"tier\":\"autonomous\""),
            "Expected tier autonomous, got: {res}"
        );
    }

    // 2. POST /api/v1/autonomy/check: Check autonomy permissions
    {
        let mut stream = TcpStream::connect(&bind_addr).expect("connect to server");
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let payload = serde_json::json!({
            "actor": "wheelhorse",
            "capability": "code_edit",
            "required_tier": "autonomous"
        })
        .to_string();
        let req = format!(
            "POST /api/v1/autonomy/check HTTP/1.1\r\nHost: {bind_addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            payload.len(),
            payload
        );
        stream.write_all(req.as_bytes()).unwrap();

        let mut res = String::new();
        stream.read_to_string(&mut res).unwrap();
        assert!(res.contains("200 OK"), "Expected 200 OK, got: {res}");
        assert!(
            res.contains("\"permitted\":true"),
            "Expected permitted true, got: {res}"
        );
        assert!(
            res.contains("\"requires_approval\":false"),
            "Expected requires_approval false, got: {res}"
        );
    }

    // 3. POST /api/v1/autonomy/record: Record promotion event
    {
        let mut stream = TcpStream::connect(&bind_addr).expect("connect to server");
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let payload = serde_json::json!({
            "actor": "rest_actor",
            "capability": "data_indexing",
            "to_tier": "supervised",
            "reason": "Promoting to supervised for REST test",
            "authorized_by": "operator",
            "event_type": "promote"
        })
        .to_string();
        let req = format!(
            "POST /api/v1/autonomy/record HTTP/1.1\r\nHost: {bind_addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            payload.len(),
            payload
        );
        stream.write_all(req.as_bytes()).unwrap();

        let mut res = String::new();
        stream.read_to_string(&mut res).unwrap();
        assert!(res.contains("200 OK"), "Expected 200 OK, got: {res}");
        assert!(
            res.contains("\"status\":\"recorded\""),
            "Expected status recorded, got: {res}"
        );
    }

    // 4. POST /api/v1/autonomy/history: Query recorded history
    {
        let mut stream = TcpStream::connect(&bind_addr).expect("connect to server");
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let payload = serde_json::json!({
            "actor": "rest_actor",
            "capability": "data_indexing"
        })
        .to_string();
        let req = format!(
            "POST /api/v1/autonomy/history HTTP/1.1\r\nHost: {bind_addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            payload.len(),
            payload
        );
        stream.write_all(req.as_bytes()).unwrap();

        let mut res = String::new();
        stream.read_to_string(&mut res).unwrap();
        assert!(res.contains("200 OK"), "Expected 200 OK, got: {res}");
        assert!(
            res.contains("\"action_type\":\"promote\""),
            "Expected promote action in history, got: {res}"
        );
    }

    // 5. POST /api/v1/autonomy/record: Attempting to promote Soma capability to autonomous fails with 400 Bad Request
    {
        let mut stream = TcpStream::connect(&bind_addr).expect("connect to server");
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let payload = serde_json::json!({
            "actor": "rest_actor",
            "capability": "soma_prod_deploy",
            "to_tier": "autonomous",
            "reason": "Attempting illegal autonomous Soma promotion",
            "authorized_by": "operator",
            "event_type": "promote"
        })
        .to_string();
        let req = format!(
            "POST /api/v1/autonomy/record HTTP/1.1\r\nHost: {bind_addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            payload.len(),
            payload
        );
        stream.write_all(req.as_bytes()).unwrap();

        let mut res = String::new();
        stream.read_to_string(&mut res).unwrap();
        assert!(
            res.contains("400 Bad Request"),
            "Expected 400 Bad Request, got: {res}"
        );
        assert!(
            res.contains("strictly require human operator verdicts"),
            "Expected soma invariant error, got: {res}"
        );
        // Zero path leaks in masked client error
        assert!(!res.contains("/home/"), "Leaked path in body: {res}");
    }

    // 6. POST /api/v1/autonomy/tier with malformed JSON: Assert 400 Bad Request client error
    {
        let mut stream = TcpStream::connect(&bind_addr).expect("connect to server");
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let bad_payload = "{ not valid json";
        let req = format!(
            "POST /api/v1/autonomy/tier HTTP/1.1\r\nHost: {bind_addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            bad_payload.len(),
            bad_payload
        );
        stream.write_all(req.as_bytes()).unwrap();

        let mut res = String::new();
        stream.read_to_string(&mut res).unwrap();
        assert!(res.contains("400 Bad Request"), "Expected 400, got: {res}");
        assert!(
            res.contains("\"error\":\"bad_request\""),
            "Expected bad_request, got: {res}"
        );
    }

    let _ = child.kill();
    let _ = child.wait();
}

// === CER-2763 P4.2: Soma run-context assembly integration ===

#[test]
fn test_context_assemble_cli() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let db_path = tmp.path().join("context_cli.db");
    let db_str = db_path.to_str().unwrap();
    let envs = [("CICATRIX_DB_PATH", db_str), ("CICATRIX_NO_REVERIE", "1")];

    // Seed grounded bug into test database
    let out = run_with_env(
        &["record", "docs/bugs/grounded/BUG_EMBED_EMPTY_INPUT_400.md"],
        &envs,
    );
    assert!(
        out.status.success(),
        "record failed: {:?}",
        String::from_utf8_lossy(&out.stderr)
    );

    // 1. Text output with match prepends <known-bugs> block
    let out = run_with_env(
        &[
            "context",
            "assemble",
            "--prompt",
            "Refactor embed service input handler",
            "crates/reverie-store/src/embed.rs",
        ],
        &envs,
    );
    assert!(
        out.status.success(),
        "assemble failed: {:?}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("<known-bugs>"));
    assert!(stdout.contains("BUG_EMBED_EMPTY_INPUT_400 (Type mismatches kill): embed empty-input → zero-vector (not 400) — files: crates/reverie-store/src/embed.rs"));
    assert!(stdout.contains("</known-bugs>"));
    assert!(stdout.contains("Refactor embed service input handler"));

    // 2. Unmatched path returns prompt unmodified with exit 0
    let out_nomatch = run_with_env(
        &[
            "context",
            "assemble",
            "--prompt",
            "Do unrelated task",
            "crates/reverie-store/src/unrelated.rs",
        ],
        &envs,
    );
    assert!(out_nomatch.status.success());
    let stdout_nomatch = String::from_utf8_lossy(&out_nomatch.stdout);
    assert!(!stdout_nomatch.contains("<known-bugs>"));
    assert_eq!(stdout_nomatch.trim(), "Do unrelated task");

    // 3. No paths returns prompt unmodified with exit 0
    let out_empty = run_with_env(
        &["context", "assemble", "--prompt", "Do empty paths task"],
        &envs,
    );
    assert!(out_empty.status.success());
    let stdout_empty = String::from_utf8_lossy(&out_empty.stdout);
    assert!(!stdout_empty.contains("<known-bugs>"));
    assert_eq!(stdout_empty.trim(), "Do empty paths task");

    // 4. JSON output format with structured metadata
    let out_json = run_with_env(
        &[
            "context",
            "assemble",
            "--prompt",
            "Refactor embed service input handler",
            "crates/reverie-store/src/embed.rs",
            "--json",
        ],
        &envs,
    );
    assert!(out_json.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out_json.stdout).expect("valid json");
    assert_eq!(v["matched_facts"], 1);
    assert_eq!(v["block_rendered"], true);
    assert_eq!(v["facts"][0]["id"], "BUG_EMBED_EMPTY_INPUT_400");
    assert!(v["prompt"].as_str().unwrap().contains("<known-bugs>"));
    assert!(v["prompt"]
        .as_str()
        .unwrap()
        .contains("Refactor embed service input handler"));

    // 5. Fail-soft behavior on missing database path returns prompt unmodified with exit 0
    let missing_db = tmp.path().join("nonexistent_dir").join("missing.db");
    let missing_envs = [
        ("CICATRIX_DB_PATH", missing_db.to_str().unwrap()),
        ("CICATRIX_NO_REVERIE", "1"),
    ];
    let out_failsoft = run_with_env(
        &[
            "context",
            "assemble",
            "--prompt",
            "Continue safely despite DB issue",
            "crates/reverie-store/src/embed.rs",
        ],
        &missing_envs,
    );
    assert!(out_failsoft.status.success());
    let stdout_failsoft = String::from_utf8_lossy(&out_failsoft.stdout);
    assert_eq!(stdout_failsoft.trim(), "Continue safely despite DB issue");
}

#[test]
fn test_context_assemble_tripwire_fail_closed_cli() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let db_path = tmp.path().join("context_tripwire.db");
    let db_str = db_path.to_str().unwrap();
    let envs = [
        ("CICATRIX_DB_PATH", db_str),
        ("CICATRIX_TRIPWIRE_MOCK_NOTIFY", "1"),
    ];

    let out = run_with_env(
        &[
            "context",
            "assemble",
            "--prompt",
            "Secret agent prompt",
            ".cicatrix/sentinel/canary_alpha.rs",
            "--actor",
            "unauthorized_agent",
        ],
        &envs,
    );
    assert!(
        !out.status.success(),
        "unauthorized canary touch must fail closed"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("tripwire intrusion detected"));
    assert!(stderr.contains("TRIPWIRE_CANARY_SENTINEL_ALPHA"));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains("Secret agent prompt"),
        "prompt must never be printed on intrusion"
    );
}

#[test]
fn test_query_soma_block_and_json_format_cli() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let db_path = tmp.path().join("query_format.db");
    let db_str = db_path.to_str().unwrap();
    let envs = [("CICATRIX_DB_PATH", db_str), ("CICATRIX_NO_REVERIE", "1")];

    let out = run_with_env(
        &["record", "docs/bugs/grounded/BUG_EMBED_EMPTY_INPUT_400.md"],
        &envs,
    );
    assert!(out.status.success());

    // 1. --format soma-block
    let out_block = run_with_env(
        &[
            "query",
            "crates/reverie-store/src/embed.rs",
            "--format",
            "soma-block",
        ],
        &envs,
    );
    assert!(out_block.status.success());
    let stdout_block = String::from_utf8_lossy(&out_block.stdout);
    assert!(stdout_block.contains("<known-bugs>"));
    assert!(stdout_block.contains("BUG_EMBED_EMPTY_INPUT_400"));
    assert!(stdout_block.contains("</known-bugs>"));

    // 2. --format json
    let out_json = run_with_env(
        &[
            "query",
            "crates/reverie-store/src/embed.rs",
            "--format",
            "json",
        ],
        &envs,
    );
    assert!(out_json.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out_json.stdout).expect("valid json");
    let arr = v.as_array().expect("array of facts");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["id"], "BUG_EMBED_EMPTY_INPUT_400");
}

#[test]
fn test_rest_context_assemble_endpoint() {
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::time::Duration;

    let tmp = tempfile::tempdir().expect("tempdir");
    let db_path = tmp.path().join("context_rest.db");
    let db_str = db_path.to_str().unwrap().to_string();

    let envs = [
        ("CICATRIX_DB_PATH", db_str.as_str()),
        ("CICATRIX_NO_REVERIE", "1"),
    ];
    let out = run_with_env(
        &["record", "docs/bugs/grounded/BUG_EMBED_EMPTY_INPUT_400.md"],
        &envs,
    );
    assert!(out.status.success());

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind free port");
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let bind_addr = format!("127.0.0.1:{port}");

    let mut child = Command::new(BIN)
        .arg("serve")
        .arg("--mcp")
        .arg("--bind")
        .arg(&bind_addr)
        .env("CICATRIX_DB_PATH", &db_str)
        .env("CICATRIX_NO_REVERIE", "1")
        .spawn()
        .expect("failed to spawn cicatrix serve --mcp");

    // Wait until server is reachable
    let mut connected = false;
    for _ in 0..50 {
        if let Ok(mut stream) = TcpStream::connect(&bind_addr) {
            let req =
                format!("GET /health HTTP/1.1\r\nHost: {bind_addr}\r\nConnection: close\r\n\r\n");
            if stream.write_all(req.as_bytes()).is_ok() {
                let mut res = String::new();
                if stream.read_to_string(&mut res).is_ok() && res.contains("200 OK") {
                    connected = true;
                    break;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(connected, "HTTP server did not become ready in time");

    // 1. POST /api/v1/context/assemble: Augmented prompt
    {
        let mut stream = TcpStream::connect(&bind_addr).expect("connect to server");
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let payload = serde_json::json!({
            "prompt": "Soma execution prompt",
            "paths": ["crates/reverie-store/src/embed.rs"],
            "limit": 3
        })
        .to_string();
        let req = format!(
            "POST /api/v1/context/assemble HTTP/1.1\r\nHost: {bind_addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            payload.len(),
            payload
        );
        stream.write_all(req.as_bytes()).unwrap();

        let mut res = String::new();
        stream.read_to_string(&mut res).unwrap();
        assert!(res.contains("200 OK"), "Expected 200 OK, got: {res}");
        assert!(
            res.contains("<known-bugs>"),
            "Expected <known-bugs> in response, got: {res}"
        );
        assert!(
            res.contains("BUG_EMBED_EMPTY_INPUT_400"),
            "Expected BUG_EMBED_EMPTY_INPUT_400, got: {res}"
        );
        assert!(
            res.contains("Soma execution prompt"),
            "Expected original prompt, got: {res}"
        );
    }

    // 2. POST /api/v1/context/assemble: Canary tripwire intrusion returns 400 Bad Request
    {
        let mut stream = TcpStream::connect(&bind_addr).expect("connect to server");
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let payload = serde_json::json!({
            "prompt": "Unauthorized probe",
            "paths": [".cicatrix/sentinel/canary_alpha.rs"],
            "actor": "rogue_agent"
        })
        .to_string();
        let req = format!(
            "POST /api/v1/context/assemble HTTP/1.1\r\nHost: {bind_addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            payload.len(),
            payload
        );
        stream.write_all(req.as_bytes()).unwrap();

        let mut res = String::new();
        stream.read_to_string(&mut res).unwrap();
        assert!(
            res.contains("400 Bad Request"),
            "Expected 400 Bad Request on canary tripwire, got: {res}"
        );
        assert!(
            res.contains("tripwire intrusion detected"),
            "Expected intrusion error, got: {res}"
        );
        assert!(!res.contains("/home/"), "Leaked path in body: {res}");
    }

    let _ = child.kill();
    let _ = child.wait();
}
