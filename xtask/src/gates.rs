//! The xtask gates of spec 0003 (XT-1 to XT-8): `tools`, `toolchain-check`, `fmt`,
//! `fmt-check`, `clippy`, `test`, `doc-test`, `deny`, `typos`, `machete`, `ci` and `install`.
//!
//! Every gate runs from the repository root with `.tools/bin` first on `PATH`, so the pinned
//! tools that `tools` installs there win over any other copy.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Instant;

/// One command of the gate set (XT-1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// Install the pinned tools that are missing or at another version (XT-3).
    Tools,
    /// Compare `rustc --version` with `rust-toolchain.toml` (XT-5).
    ToolchainCheck,
    /// `cargo fmt --all`.
    Fmt,
    /// `cargo fmt --all -- --check`.
    FmtCheck,
    /// `cargo clippy`, warnings are errors.
    Clippy,
    /// nextest, or `cargo test` when nextest is missing.
    Test,
    /// `cargo test --doc`.
    DocTest,
    /// `cargo deny check`.
    Deny,
    /// `typos`.
    Typos,
    /// `cargo machete`.
    Machete,
    /// Every gate in order, stopping at the first failure (XT-7).
    Ci,
    /// `ci`, then release builds of every binary (XT-8).
    Install,
}

impl Gate {
    /// Every command, in the order `usage` lists them.
    pub const ALL: [Gate; 12] = [
        Gate::Tools,
        Gate::ToolchainCheck,
        Gate::Fmt,
        Gate::FmtCheck,
        Gate::Clippy,
        Gate::Test,
        Gate::DocTest,
        Gate::Deny,
        Gate::Typos,
        Gate::Machete,
        Gate::Ci,
        Gate::Install,
    ];

    /// The command name on the `cargo xtask` command line.
    pub fn name(self) -> &'static str {
        match self {
            Gate::Tools => "tools",
            Gate::ToolchainCheck => "toolchain-check",
            Gate::Fmt => "fmt",
            Gate::FmtCheck => "fmt-check",
            Gate::Clippy => "clippy",
            Gate::Test => "test",
            Gate::DocTest => "doc-test",
            Gate::Deny => "deny",
            Gate::Typos => "typos",
            Gate::Machete => "machete",
            Gate::Ci => "ci",
            Gate::Install => "install",
        }
    }

    /// The gate a command-line name selects, if any.
    pub fn parse(name: &str) -> Option<Gate> {
        Gate::ALL.into_iter().find(|gate| gate.name() == name)
    }
}

/// The steps `ci` runs, in order (XT-7).
pub const CI_STEPS: [Gate; 9] = [
    Gate::Tools,
    Gate::ToolchainCheck,
    Gate::FmtCheck,
    Gate::Clippy,
    Gate::Test,
    Gate::DocTest,
    Gate::Deny,
    Gate::Typos,
    Gate::Machete,
];

/// A cargo tool a gate runs, at its pinned version (XT-2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tool {
    /// The crate `cargo install` builds.
    pub krate: &'static str,
    /// The command that prints the version of the copy a gate runs.
    pub probe: &'static [&'static str],
    /// The pinned version.
    pub version: &'static str,
}

/// cargo-deny. The `ci-rust` image bakes the same version.
pub const DENY: Tool = Tool {
    krate: "cargo-deny",
    probe: &["cargo", "deny", "--version"],
    version: "0.19.9",
};

/// typos. The `ci-rust` image bakes the same version.
pub const TYPOS: Tool = Tool {
    krate: "typos-cli",
    probe: &["typos", "--version"],
    version: "1.49.0",
};

/// cargo-machete.
pub const MACHETE: Tool = Tool {
    krate: "cargo-machete",
    probe: &["cargo", "machete", "--version"],
    version: "0.9.1",
};

/// cargo-nextest.
pub const NEXTEST: Tool = Tool {
    krate: "cargo-nextest",
    probe: &["cargo", "nextest", "--version"],
    version: "0.9.104",
};

/// The one table of pinned tools (XT-2).
pub const TOOLS: [Tool; 4] = [DENY, TYPOS, MACHETE, NEXTEST];

/// The Rust that `cargo install` builds the tools with (XT-3). The `ci-rust` image bakes it,
/// and it meets every tool's `rust-version` (typos-cli 1.49.0 needs 1.91).
pub const TOOLS_RUST: &str = "1.94.1";

/// The repository commits `Cargo.lock`, so cargo runs with `--locked`.
const LOCKED: bool = true;

/// True when a `--version` output names exactly the pinned version.
pub fn version_matches(output: &str, pinned: &str) -> bool {
    output
        .split_whitespace()
        .any(|word| word.strip_prefix('v').unwrap_or(word) == pinned)
}

/// The command line that installs a tool into `<root>/.tools` (XT-3).
pub fn install_command(tool: &Tool, root: &Path, jobs: &str) -> Vec<String> {
    let tools_root = root.join(".tools").to_string_lossy().into_owned();
    [
        "rustup",
        "run",
        "--install",
        TOOLS_RUST,
        "cargo",
        "install",
        "--locked",
        "--version",
        tool.version,
        "--root",
        &tools_root,
        "--force",
        "-j",
        jobs,
        tool.krate,
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect()
}

/// `PATH` with `<root>/.tools/bin` first.
pub fn path_with_tools(root: &Path, path: Option<OsString>) -> OsString {
    let mut dirs = vec![root.join(".tools").join("bin")];
    if let Some(path) = path {
        dirs.extend(std::env::split_paths(&path));
    }
    std::env::join_paths(dirs).unwrap_or_default()
}

/// The variables among `names` that cargo set for the xtask binary itself, which no gate may
/// inherit (XT-6).
pub fn package_vars<'a>(names: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    const EXACT: [&str; 5] = [
        "CARGO_MANIFEST_DIR",
        "CARGO_MANIFEST_PATH",
        "CARGO_CRATE_NAME",
        "CARGO_BIN_NAME",
        "CARGO_PRIMARY_PACKAGE",
    ];
    names
        .into_iter()
        .filter(|name| name.starts_with("CARGO_PKG_") || EXACT.contains(name))
        .map(str::to_string)
        .collect()
}

/// The build job count: `JOBS` when it is set and not empty, else 2 (the `-j 2` of `.cicatrix/establish-baseline.sh`).
pub fn jobs(env_jobs: Option<String>) -> String {
    env_jobs
        .filter(|jobs| !jobs.trim().is_empty())
        .unwrap_or_else(|| "2".to_string())
}

/// The command line a gate runs (XT-6), program first. `nextest` says whether cargo-nextest
/// is there. `None` for the gates that are not one command.
pub fn command_line(gate: Gate, jobs: &str, nextest: bool) -> Option<Vec<String>> {
    let locked: &[&str] = if LOCKED { &["--locked"] } else { &[] };
    let line: Vec<&str> = match gate {
        Gate::Fmt => vec!["cargo", "fmt", "--all"],
        Gate::FmtCheck => vec!["cargo", "fmt", "--all", "--", "--check"],
        Gate::Clippy => [
            &["cargo", "clippy", "--workspace", "--all-targets"][..],
            locked,
            &["-j", jobs, "--", "-D", "warnings"],
        ]
        .concat(),
        Gate::Test if nextest => [
            &["cargo", "nextest", "run", "--workspace"][..],
            locked,
            &["--build-jobs", jobs],
        ]
        .concat(),
        Gate::Test => [&["cargo", "test", "--workspace"][..], locked, &["-j", jobs]].concat(),
        Gate::DocTest => [
            &["cargo", "test", "--doc", "--workspace"][..],
            locked,
            &["-j", jobs],
        ]
        .concat(),
        Gate::Deny => vec!["cargo", "deny", "check"],
        Gate::Typos => vec!["typos"],
        Gate::Machete => vec!["cargo", "machete"],
        Gate::Tools | Gate::ToolchainCheck | Gate::Ci | Gate::Install => return None,
    };
    Some(line.iter().map(|s| (*s).to_string()).collect())
}

/// The release build `install` runs after `ci` (XT-8).
pub fn install_build_line(jobs: &str) -> Vec<String> {
    let locked: &[&str] = if LOCKED { &["--locked"] } else { &[] };
    [
        &["cargo", "build", "--release"][..],
        locked,
        &["--workspace", "--exclude", "xtask", "--bins", "-j", jobs],
    ]
    .concat()
    .iter()
    .map(|s| (*s).to_string())
    .collect()
}

/// How one step of `ci` ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The step ran and passed.
    Passed,
    /// The step ran and failed, with the reason.
    Failed(String),
    /// An earlier step failed, so this one did not run.
    NotRun,
}

/// One line of the `ci` summary.
#[derive(Debug, Clone, PartialEq)]
pub struct StepResult {
    /// The step.
    pub gate: Gate,
    /// How it ended.
    pub outcome: Outcome,
    /// Seconds it took, when it ran.
    pub secs: Option<f64>,
}

/// Run `steps` in order with `run`, and stop at the first step that fails (XT-7).
pub fn run_steps<F>(steps: &[Gate], run: F) -> Vec<StepResult>
where
    F: FnMut(Gate) -> Result<(), String>,
{
    let mut run = run;
    let mut failed = false;
    steps
        .iter()
        .map(|&gate| {
            if failed {
                return StepResult {
                    gate,
                    outcome: Outcome::NotRun,
                    secs: None,
                };
            }
            let start = Instant::now();
            let outcome = match run(gate) {
                Ok(()) => Outcome::Passed,
                Err(why) => {
                    failed = true;
                    Outcome::Failed(why)
                }
            };
            StepResult {
                gate,
                outcome,
                secs: Some(start.elapsed().as_secs_f64()),
            }
        })
        .collect()
}

/// The summary line for one step (XT-7).
pub fn summary_line(label: &str, result: &StepResult) -> String {
    let head = format!("{label}: {:<15}", result.gate.name());
    let secs = result
        .secs
        .map(|s| format!(" {s:>6.1}s"))
        .unwrap_or_default();
    match &result.outcome {
        Outcome::Passed => format!("{head} {:<6}{secs}", "ok"),
        Outcome::Failed(why) => {
            let why = why.lines().next().unwrap_or("");
            format!("{head} {:<6}{secs}  {why}", "FAILED")
        }
        Outcome::NotRun => format!("{head} not run"),
    }
}

/// What a gate needs at run time.
struct Ctx {
    root: PathBuf,
    path: OsString,
    jobs: String,
}

impl Ctx {
    fn new(root: &Path) -> Ctx {
        Ctx {
            root: root.to_path_buf(),
            path: path_with_tools(root, std::env::var_os("PATH")),
            jobs: jobs(std::env::var("JOBS").ok()),
        }
    }

    /// A command that runs from the root with `.tools/bin` first on `PATH`, without the
    /// xtask binary's own package variables (XT-6).
    fn command(&self, line: &[String]) -> Command {
        let mut cmd = Command::new(&line[0]);
        cmd.args(&line[1..])
            .current_dir(&self.root)
            .env("PATH", &self.path);
        let names: Vec<String> = std::env::vars_os()
            .filter_map(|(name, _)| name.into_string().ok())
            .collect();
        for name in package_vars(names.iter().map(String::as_str)) {
            cmd.env_remove(name);
        }
        cmd
    }

    fn status(&self, line: &[String]) -> Result<(), String> {
        let shown = line.join(" ");
        println!("xtask: running `{shown}`");
        let status = self
            .command(line)
            .status()
            .map_err(|e| format!("cannot run `{shown}`: {e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("`{shown}` exited with {status}"))
        }
    }

    /// The `--version` output of the copy of `tool` a gate would run, if it runs.
    fn probe(&self, tool: &Tool) -> Option<String> {
        let line: Vec<String> = tool.probe.iter().map(|s| (*s).to_string()).collect();
        let out = self.command(&line).output().ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    }
}

/// Install each pinned tool that is missing or at another version (XT-3, XT-4).
fn tools(ctx: &Ctx) -> Result<(), String> {
    for tool in &TOOLS {
        match ctx.probe(tool) {
            Some(found) if version_matches(&found, tool.version) => {
                println!("tools: {} {} ok", tool.krate, tool.version);
                continue;
            }
            Some(found) => println!(
                "tools: {} reports `{}`, installing {}",
                tool.krate,
                found.lines().next().unwrap_or(""),
                tool.version
            ),
            None => println!("tools: {} missing, installing {}", tool.krate, tool.version),
        }
        ctx.status(&install_command(tool, &ctx.root, &ctx.jobs))?;
        match ctx.probe(tool) {
            Some(found) if version_matches(&found, tool.version) => {
                println!("tools: {} {} installed", tool.krate, tool.version);
            }
            found => {
                return Err(format!(
                    "{} still reports `{}` after installing {}; another copy is ahead of .tools/bin",
                    tool.krate,
                    found
                        .as_deref()
                        .and_then(|f| f.lines().next())
                        .unwrap_or("nothing"),
                    tool.version
                ));
            }
        }
    }
    Ok(())
}

/// Run one gate that is a single step.
fn step(ctx: &Ctx, gate: Gate) -> Result<(), String> {
    match gate {
        Gate::Tools => tools(ctx),
        Gate::ToolchainCheck => crate::toolchain::check(&ctx.root),
        Gate::Ci | Gate::Install => Err(format!("{} is not a single step", gate.name())),
        _ => {
            let nextest = gate == Gate::Test && ctx.probe(&NEXTEST).is_some();
            let line = command_line(gate, &ctx.jobs, nextest)
                .ok_or_else(|| format!("{} has no command line", gate.name()))?;
            ctx.status(&line)
        }
    }
}

/// Run `ci`'s steps, print the summary, and say whether every step passed.
fn ci(ctx: &Ctx, label: &str) -> bool {
    let results = run_steps(&CI_STEPS, |gate| {
        println!("==> {label}: {}", gate.name());
        step(ctx, gate)
    });
    for result in &results {
        println!("{}", summary_line(label, result));
    }
    results.iter().all(|r| r.outcome == Outcome::Passed)
}

/// Run one command of the gate set.
pub fn run(root: &Path, gate: Gate) -> ExitCode {
    let ctx = Ctx::new(root);
    let ok = match gate {
        Gate::Ci => ci(&ctx, "xtask ci"),
        Gate::Install => {
            ci(&ctx, "xtask install")
                && match ctx.status(&install_build_line(&ctx.jobs)) {
                    Ok(()) => {
                        println!("xtask install: release build ok");
                        true
                    }
                    Err(err) => {
                        eprintln!("xtask install: release build FAILED: {err}");
                        false
                    }
                }
        }
        _ => match step(&ctx, gate) {
            Ok(()) => true,
            Err(err) => {
                eprintln!("{}: {err}", gate.name());
                false
            }
        },
    };
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn xt1_every_command_parses_by_its_name() {
        let names: Vec<&str> = Gate::ALL.iter().map(|g| g.name()).collect();
        assert_eq!(
            names,
            [
                "tools",
                "toolchain-check",
                "fmt",
                "fmt-check",
                "clippy",
                "test",
                "doc-test",
                "deny",
                "typos",
                "machete",
                "ci",
                "install"
            ]
        );
        for gate in Gate::ALL {
            assert_eq!(Gate::parse(gate.name()), Some(gate));
        }
    }

    #[test]
    fn xt1_other_names_are_not_gates() {
        for name in ["", "spec-check", "CI", "fmt-check ", "nextest"] {
            assert_eq!(Gate::parse(name), None, "{name:?}");
        }
    }

    #[test]
    fn xt2_one_table_of_four_pinned_tools() {
        let table: Vec<(&str, &str)> = TOOLS.iter().map(|t| (t.krate, t.version)).collect();
        assert_eq!(
            table,
            [
                ("cargo-deny", "0.19.9"),
                ("typos-cli", "1.49.0"),
                ("cargo-machete", "0.9.1"),
                ("cargo-nextest", "0.9.104")
            ]
        );
    }

    #[test]
    fn xt3_version_lines() {
        assert!(version_matches("cargo-deny 0.19.9", "0.19.9"));
        assert!(version_matches("typos-cli 1.49.0\n", "1.49.0"));
        assert!(version_matches("0.9.1", "0.9.1"));
        assert!(version_matches("cargo-machete v0.9.1", "0.9.1"));
        assert!(version_matches(
            "cargo-nextest 0.9.104 (abc123 2025-09-15)\nrelease: 0.9.104\nhost: x86_64",
            "0.9.104"
        ));
        assert!(!version_matches("0.9.10", "0.9.1"));
        assert!(!version_matches("0.9.2", "0.9.1"));
        assert!(!version_matches("cargo-deny 0.20.2", "0.19.9"));
        assert!(!version_matches("typos-cli 1.49", "1.49.0"));
        assert!(!version_matches("", "0.9.1"));
    }

    #[test]
    fn xt3_install_builds_the_pin_into_dot_tools_with_rust_1_94_1() {
        let line = install_command(&TYPOS, Path::new("/r"), "2");
        assert_eq!(
            line,
            strings(&[
                "rustup",
                "run",
                "--install",
                "1.94.1",
                "cargo",
                "install",
                "--locked",
                "--version",
                "1.49.0",
                "--root",
                "/r/.tools",
                "--force",
                "-j",
                "2",
                "typos-cli"
            ])
        );
    }

    #[test]
    fn xt3_tools_come_first_on_path() {
        let path = path_with_tools(Path::new("/r"), Some(OsString::from("/usr/bin:/bin")));
        let parts: Vec<PathBuf> = std::env::split_paths(&path).collect();
        assert_eq!(
            parts,
            [
                PathBuf::from("/r/.tools/bin"),
                PathBuf::from("/usr/bin"),
                PathBuf::from("/bin")
            ]
        );
        let path = path_with_tools(Path::new("/r"), None);
        assert_eq!(
            std::env::split_paths(&path).collect::<Vec<_>>(),
            [PathBuf::from("/r/.tools/bin")]
        );
    }

    #[test]
    fn xt6_gates_do_not_inherit_the_xtask_package_variables() {
        let names = [
            "CARGO",
            "CARGO_HOME",
            "CARGO_TARGET_DIR",
            "CARGO_BUILD_JOBS",
            "CARGO_PKG_NAME",
            "CARGO_PKG_VERSION",
            "CARGO_PKG_AUTHORS",
            "CARGO_MANIFEST_DIR",
            "CARGO_MANIFEST_PATH",
            "CARGO_CRATE_NAME",
            "CARGO_BIN_NAME",
            "CARGO_PRIMARY_PACKAGE",
            "PATH",
            "RUSTUP_TOOLCHAIN",
        ];
        assert_eq!(
            package_vars(names),
            strings(&[
                "CARGO_PKG_NAME",
                "CARGO_PKG_VERSION",
                "CARGO_PKG_AUTHORS",
                "CARGO_MANIFEST_DIR",
                "CARGO_MANIFEST_PATH",
                "CARGO_CRATE_NAME",
                "CARGO_BIN_NAME",
                "CARGO_PRIMARY_PACKAGE"
            ])
        );
    }

    #[test]
    fn xt6_jobs_default_to_two() {
        assert_eq!(jobs(None), "2");
        assert_eq!(jobs(Some(String::new())), "2");
        assert_eq!(jobs(Some("10".to_string())), "10");
    }

    #[test]
    fn xt6_command_lines() {
        let line = |gate| command_line(gate, "2", true).unwrap_or_default();
        assert_eq!(line(Gate::Fmt), strings(&["cargo", "fmt", "--all"]));
        assert_eq!(
            line(Gate::FmtCheck),
            strings(&["cargo", "fmt", "--all", "--", "--check"])
        );
        assert_eq!(
            line(Gate::Clippy),
            strings(&[
                "cargo",
                "clippy",
                "--workspace",
                "--all-targets",
                "--locked",
                "-j",
                "2",
                "--",
                "-D",
                "warnings"
            ])
        );
        assert_eq!(
            line(Gate::Test),
            strings(&[
                "cargo",
                "nextest",
                "run",
                "--workspace",
                "--locked",
                "--build-jobs",
                "2"
            ])
        );
        assert_eq!(
            line(Gate::DocTest),
            strings(&[
                "cargo",
                "test",
                "--doc",
                "--workspace",
                "--locked",
                "-j",
                "2"
            ])
        );
        assert_eq!(line(Gate::Deny), strings(&["cargo", "deny", "check"]));
        assert_eq!(line(Gate::Typos), strings(&["typos"]));
        assert_eq!(line(Gate::Machete), strings(&["cargo", "machete"]));
    }

    #[test]
    fn xt6_test_falls_back_to_cargo_test_without_nextest() {
        assert_eq!(
            command_line(Gate::Test, "10", false),
            Some(strings(&[
                "cargo",
                "test",
                "--workspace",
                "--locked",
                "-j",
                "10"
            ]))
        );
    }

    #[test]
    fn xt6_composite_gates_have_no_single_command_line() {
        for gate in [Gate::Tools, Gate::ToolchainCheck, Gate::Ci, Gate::Install] {
            assert_eq!(command_line(gate, "2", true), None, "{gate:?}");
        }
    }

    #[test]
    fn xt7_ci_runs_the_nine_steps_in_contract_order() {
        let names: Vec<&str> = CI_STEPS.iter().map(|g| g.name()).collect();
        assert_eq!(
            names,
            [
                "tools",
                "toolchain-check",
                "fmt-check",
                "clippy",
                "test",
                "doc-test",
                "deny",
                "typos",
                "machete"
            ]
        );
    }

    #[test]
    fn xt7_ci_stops_at_the_first_failure() {
        let mut ran = Vec::new();
        let results = run_steps(&CI_STEPS, |gate| {
            ran.push(gate);
            if gate == Gate::Clippy {
                Err("clippy found 3 warnings".to_string())
            } else {
                Ok(())
            }
        });
        assert_eq!(
            ran,
            [
                Gate::Tools,
                Gate::ToolchainCheck,
                Gate::FmtCheck,
                Gate::Clippy
            ]
        );
        let outcomes: Vec<&Outcome> = results.iter().map(|r| &r.outcome).collect();
        assert_eq!(results.len(), 9);
        assert_eq!(outcomes[..3], [&Outcome::Passed; 3]);
        assert_eq!(
            outcomes[3],
            &Outcome::Failed("clippy found 3 warnings".to_string())
        );
        assert!(outcomes[4..].iter().all(|o| **o == Outcome::NotRun));
        assert!(results[3].secs.is_some());
        assert!(results[4].secs.is_none());
    }

    #[test]
    fn xt7_ci_runs_every_step_when_all_pass() {
        let results = run_steps(&CI_STEPS, |_| Ok(()));
        assert_eq!(results.len(), 9);
        assert!(results.iter().all(|r| r.outcome == Outcome::Passed));
    }

    #[test]
    fn xt7_one_summary_line_per_step() {
        let passed = StepResult {
            gate: Gate::Clippy,
            outcome: Outcome::Passed,
            secs: Some(3.21),
        };
        let failed = StepResult {
            gate: Gate::Test,
            outcome: Outcome::Failed("`cargo nextest run` exited with 100".to_string()),
            secs: Some(12.0),
        };
        let not_run = StepResult {
            gate: Gate::DocTest,
            outcome: Outcome::NotRun,
            secs: None,
        };
        assert_eq!(
            summary_line("xtask ci", &passed),
            "xtask ci: clippy          ok        3.2s"
        );
        assert_eq!(
            summary_line("xtask ci", &failed),
            "xtask ci: test            FAILED   12.0s  `cargo nextest run` exited with 100"
        );
        assert_eq!(
            summary_line("xtask ci", &not_run),
            "xtask ci: doc-test        not run"
        );
        let two_lines = StepResult {
            gate: Gate::Deny,
            outcome: Outcome::Failed("advisories FAILED\nlicenses ok".to_string()),
            secs: Some(1.0),
        };
        for line in [&passed, &failed, &not_run, &two_lines].map(|r| summary_line("xtask ci", r)) {
            assert_eq!(line.lines().count(), 1, "{line:?}");
        }
    }

    #[test]
    fn xt8_install_builds_every_binary_in_release() {
        assert_eq!(
            install_build_line("2"),
            strings(&[
                "cargo",
                "build",
                "--release",
                "--locked",
                "--workspace",
                "--exclude",
                "xtask",
                "--bins",
                "-j",
                "2"
            ])
        );
    }
}
