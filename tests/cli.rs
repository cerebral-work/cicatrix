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
