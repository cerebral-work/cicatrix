//! Bug-fact store seam.
//!
//! Decided 2026-06-18 (operator interview, see `docs/design/cicatrix-reverie-unsigned-paas-integration.md`):
//! **reverie is the single fact store** (the janus-datalog sidecar is dropped for v1). The markdown
//! corpus (`docs/bugs/resolved/`) is the source of truth; reverie holds a regenerable one-way
//! projection. The trait below is the seam; its sole impl is [`crate::reverie::ReverieBridge`].
//! AsOf(commit) is preserved without janus via git-ancestry over the fix-commit (`crate::gitf`).

/// A fixed-bug fact, projected from a `docs/bugs/resolved/BUG_*.md` file.
#[derive(Debug, Clone)]
pub struct BugFact {
    pub id: String,
    pub files: Vec<String>,
    pub symptom: String,
    pub fix_commit: String,
    pub regression_test: String,
    pub meta_pattern: String,
}

/// The fact-store seam. Sole impl: [`crate::reverie::ReverieBridge`].
pub trait BugStore {
    /// Record a fixed-bug fact (idempotent on the bug slug; re-recording updates the projection).
    fn record(&mut self, fact: &BugFact) -> std::io::Result<()>;
    /// The core query: "does this changed-file set touch a known-bug surface?" Backed by reverie
    /// `/search`; `--as-of <commit>` time-travel is applied by the caller via `crate::gitf`.
    fn touches_known_bug(&self, changed_files: &[String]) -> std::io::Result<Vec<BugFact>>;
}

/// The meta-pattern block injected upstream of an edit. v0 returns the rolled-up rules from
/// CLAUDE.md; next it regenerates from the recorded facts after each `record`.
pub fn meta_patterns() -> &'static str {
    "cicatrix meta-patterns (see CLAUDE.md):\n\
     - Type mismatches kill — validate at the seam; choose an explicit empty representation.\n\
     - Two implementations of one fact drift — single source of truth; read path sees every write.\n\
     - Edge cases are real cases — test empty / first / last / zero-length.\n\
     - Correctness before performance — a fast wrong answer is a bug.\n\
     - Test structure, not just outcomes — assert the invariant, not only the happy path.\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The injected block must carry every named meta-pattern class. If a rule is silently
    /// dropped when this string is edited, an agent gets injected with an incomplete corpus —
    /// the failure is invisible at the seam (the "type mismatches kill" class, applied to us).
    #[test]
    fn meta_patterns_carry_every_named_class() {
        let block = meta_patterns();
        for class in [
            "Type mismatches kill",
            "Two implementations of one fact drift",
            "Edge cases are real cases",
            "Correctness before performance",
            "Test structure, not just outcomes",
        ] {
            assert!(
                block.contains(class),
                "meta_patterns() dropped class: {class}"
            );
        }
    }
}
