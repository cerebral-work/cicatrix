//! Regression suite for the Agent Jury CI workflow's shell logic (CER-2077).
//!
//! The jury is the repo's automated merge gate. Its steps are inline `run:` blocks that no
//! Rust test reaches, so a bash-level defect made the gate report `failure` on every PR while
//! never producing a verdict — vacuous in exactly the direction that looks like enforcement.
//!
//! These tests extract the *real* step scripts out of `.github/workflows/agent-jury.yml` and
//! execute them against stubbed `curl` / `gh`, so they assert the shipped workflow's behavior
//! rather than a copy of it. Structure over outcome, per CLAUDE.md.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const WORKFLOW: &str = ".github/workflows/agent-jury.yml";

/// Extract one step's `run:` block from the workflow, dedented to column 0.
///
/// Deliberately strict: a miss is a hard panic, never an empty script that would let these
/// tests pass vacuously if the workflow is restructured.
fn extract_run_block(yaml: &str, step_name: &str) -> String {
    let needle = format!("- name: {step_name}");
    let start = yaml
        .find(&needle)
        .unwrap_or_else(|| panic!("step `{step_name}` not found in {WORKFLOW}"));

    let after_name = &yaml[start..];
    let run_at = after_name
        .find("run: |")
        .unwrap_or_else(|| panic!("step `{step_name}` has no `run: |` block"));
    let body = &after_name[run_at + "run: |".len()..];

    // The block runs until the first line whose indentation drops to or below the `run:` key's.
    let run_indent = after_name[..run_at]
        .rsplit('\n')
        .next()
        .expect("run: line")
        .len();

    let mut out = String::new();
    let mut block_indent: Option<usize> = None;
    for line in body.lines().skip(1) {
        if line.trim().is_empty() {
            out.push('\n');
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        if indent <= run_indent {
            break;
        }
        let base = *block_indent.get_or_insert(indent);
        out.push_str(line.get(base..).unwrap_or(line.trim_start()));
        out.push('\n');
    }

    assert!(
        !out.trim().is_empty(),
        "extracted an empty script for `{step_name}` — extractor is out of sync with {WORKFLOW}"
    );
    out
}

fn workflow_text() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(WORKFLOW);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// A throwaway directory holding the step script, stub executables, and `$GITHUB_OUTPUT`.
struct StepHarness {
    dir: PathBuf,
}

impl StepHarness {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("cicatrix-jury-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("bin")).expect("create harness dir");
        fs::write(dir.join("github_output"), "").expect("seed GITHUB_OUTPUT");
        // The review step reads the diff the `pr` step left in $RUNNER_TEMP.
        fs::write(
            dir.join("pr-diff.txt"),
            "--- a/src/bug_md.rs\n+++ b/src/bug_md.rs\n",
        )
        .expect("seed pr-diff");
        Self { dir }
    }

    /// Install a stub executable that shadows the real tool on `$PATH`.
    fn stub(&self, name: &str, script: &str) {
        let path = self.dir.join("bin").join(name);
        fs::write(&path, format!("#!/usr/bin/env bash\n{script}\n")).expect("write stub");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod stub");
        }
    }

    fn run(&self, script: &str, env: &[(&str, &str)]) -> (i32, String, String) {
        let script_path = self.dir.join("step.sh");
        fs::write(&script_path, script).expect("write step script");

        let path_var = format!(
            "{}:{}",
            self.dir.join("bin").display(),
            std::env::var("PATH").unwrap_or_default()
        );

        let mut cmd = Command::new("bash");
        cmd.arg(&script_path)
            .env_clear()
            .env("PATH", path_var)
            .env("HOME", &self.dir)
            .env("GITHUB_OUTPUT", self.dir.join("github_output"))
            .env("RUNNER_TEMP", &self.dir);
        for (k, v) in env {
            cmd.env(k, v);
        }

        let out = cmd.output().expect("spawn bash");
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }

    fn github_output(&self) -> String {
        fs::read_to_string(self.dir.join("github_output")).unwrap_or_default()
    }
}

impl Drop for StepHarness {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn review_env() -> Vec<(&'static str, &'static str)> {
    vec![
        ("LITELLM_URL", "http://stub.invalid/v1"),
        ("LITELLM_KEY", "stub-value-not-a-credential"),
        ("AGENT_JURY_MODEL", "glm-5.2-fast"),
        ("PR_NUMBER", "13"),
        ("PR_TITLE", "fix(parser): skip fenced code blocks"),
        ("PR_BODY", "body"),
        ("FILES_CHANGED", "src/bug_md.rs"),
    ]
}

/// The gateway answered 200 with a body that is not JSON at all (proxy/HTML error page).
///
/// `jq` exits 5 on malformed input; under `set -euo pipefail` that aborts the step at the
/// assignment, so the `[ -z "$CONTENT" ]` guard below it never executes and `review_failed`
/// is never written. The step must instead reach its guard and exit 0 advisory.
#[test]
fn review_step_survives_non_json_gateway_body() {
    let script = extract_run_block(&workflow_text(), "Run GLM-5.2 review");
    let h = StepHarness::new("nonjson");
    h.stub(
        "curl",
        r#"printf '%s' '<html>504 upstream timeout</html>'; exit 0"#,
    );

    let (code, _out, err) = h.run(&script, &review_env());

    assert_eq!(
        code, 0,
        "review step must not abort — it is advisory. stderr: {err}"
    );
    assert!(
        h.github_output().contains("review_failed=true"),
        "guard must record review_failed; GITHUB_OUTPUT was: {:?}",
        h.github_output()
    );
}

/// The gateway answered correctly but the model returned prose instead of the strict JSON
/// object. Same dead-guard mechanism, one `jq` call further down.
#[test]
fn review_step_survives_non_json_model_content() {
    let script = extract_run_block(&workflow_text(), "Run GLM-5.2 review");
    let h = StepHarness::new("prose");
    h.stub(
        "curl",
        r#"printf '%s' '{"choices":[{"message":{"content":"Sure! Here is my review: looks good to me."}}]}'; exit 0"#,
    );

    let (code, _out, err) = h.run(&script, &review_env());

    assert_eq!(
        code, 0,
        "review step must not abort on unparseable model output. stderr: {err}"
    );
    assert!(
        h.github_output().contains("review_failed=true"),
        "guard must record review_failed; GITHUB_OUTPUT was: {:?}",
        h.github_output()
    );
}

/// A well-formed review still has to produce a verdict — guards must not swallow the happy path.
#[test]
fn review_step_emits_verdict_on_well_formed_response() {
    let script = extract_run_block(&workflow_text(), "Run GLM-5.2 review");
    let h = StepHarness::new("happy");
    h.stub(
        "curl",
        r#"printf '%s' '{"choices":[{"message":{"content":"{\"verdict\":\"approved\",\"confidence\":\"high\",\"findings\":[],\"summary\":\"clean\"}"}}]}'; exit 0"#,
    );

    let (code, _out, err) = h.run(&script, &review_env());
    let output = h.github_output();

    assert_eq!(code, 0, "happy path must exit 0. stderr: {err}");
    assert!(
        output.contains("verdict=approved"),
        "expected a verdict; GITHUB_OUTPUT was: {output:?}"
    );
    assert!(
        !output.contains("review_failed=true"),
        "happy path must not report failure; GITHUB_OUTPUT was: {output:?}"
    );
}

/// The post step is `if: always()`, so it runs even when the review step died before writing
/// its parsed-JSON file. It must report the failure as a comment, not blow up on a missing
/// file and turn the whole gate red for a reason no reviewer can read.
#[test]
fn post_step_reports_when_review_left_no_parsed_json() {
    let script = extract_run_block(&workflow_text(), "Post review comment + stamp label");
    let h = StepHarness::new("noparse");
    h.stub("gh", "exit 0");
    // Deliberately do NOT create the parsed-JSON file, and leave every review output empty —
    // exactly the state an aborted review step leaves behind.

    let (code, _out, err) = h.run(
        &script,
        &[
            ("GH_TOKEN", "stub-value-not-a-credential"),
            ("PR_NUMBER", "13"),
            ("VERDICT", ""),
            ("CONFIDENCE", ""),
            ("FINDINGS_COUNT", ""),
            ("REVIEW_FAILED", ""),
            ("MODEL", "glm-5.2-fast"),
        ],
    );

    assert_eq!(
        code, 0,
        "post step must tolerate a missing parsed-JSON file. stderr: {err}"
    );
}
