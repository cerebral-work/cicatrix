//! Bug-fact store. v0 = local markdown corpus; the janus-datalog sidecar lands next.
//!
//! Decided 2026-06-16: embed wbrown/janus-datalog as the temporal fact store (run as a sidecar
//! binary this crate drives), bridged into reverie's dream consolidation. The trait below is the
//! seam: a `JanusStore` impl (sidecar) and a `ReverieBridge` impl swap in behind it.

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

/// The fact-store seam. v0 has no impl; `JanusStore` (sidecar) + `ReverieBridge` land next.
pub trait BugStore {
    /// Record a fixed-bug fact (append-only; janus-datalog keeps full immutable history).
    fn record(&mut self, fact: &BugFact) -> std::io::Result<()>;
    /// The core query: "does this changed-file set touch a known-bug surface?"
    /// Backed by a Datalog pattern over the temporal store, evaluable `AsOf(commit)`.
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
            assert!(block.contains(class), "meta_patterns() dropped class: {class}");
        }
    }
}
