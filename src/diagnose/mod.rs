//! Automated diagnosis and BugFact auto-authoring subsystem (CER-1394).
//!
//! Clean-room Rust port of the hypothesis-fork and fork-and-judge contract
//! from agent-afk (Griffin Long, Apache-2.0).

pub mod fork;
pub mod judge;
pub mod types;

#[cfg(test)]
mod tests;

pub use fork::{extract_suspected_files, fork_hypotheses, MAX_FORK_COUNT, MIN_FORK_COUNT};
pub use judge::{judge_and_converge, sanitize_slug, slug_to_kebab};
pub use types::{
    ConvergenceReport, DiagnoseError, DiagnoseTarget, ForkOptions, HypothesisCategory,
    RootCauseHypothesis,
};

/// Orchestrate hypothesis forking and convergence judgment for a failure target.
pub fn run_diagnose(
    target: DiagnoseTarget,
    options: ForkOptions,
) -> Result<ConvergenceReport, DiagnoseError> {
    let hypotheses = fork_hypotheses(&target, &options)?;
    judge_and_converge(&target, hypotheses, &options)
}
