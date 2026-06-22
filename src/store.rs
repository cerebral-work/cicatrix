//! Bug-fact store seam.
//!
//! Decided 2026-06-18 (operator interview, see `docs/design/cicatrix-reverie-unsigned-paas-integration.md`):
//! **reverie is the single fact store** (the janus-datalog sidecar is dropped for v1). The markdown
//! corpus (`docs/bugs/grounded/`) is the source of truth; reverie holds a regenerable one-way
//! projection. The trait below is the seam; its sole impl is [`crate::reverie::ReverieBridge`].
//! AsOf(commit) is preserved without janus via git-ancestry over the fix-commit (`crate::gitf`).

use std::collections::BTreeMap;
use std::collections::BTreeSet;

/// A fixed-bug fact, projected from a `docs/bugs/grounded/BUG_*.md` file.
#[derive(Debug, Clone)]
pub struct BugFact {
    pub id: String,
    pub files: Vec<String>,
    pub symptom: String,
    pub fix_commit: String,
    pub regression_test: String,
    pub meta_pattern: String,
    /// Blast-radius scope: a crate/path glob/prefix. When `None`, the effective scope is the set
    /// of parent directories of `files` (see [`BugFact::effective_scopes`]).
    pub scope: Option<String>,
    /// When `true`, this fact's meta-pattern is NOT generalized into the injected/CLAUDE.md block
    /// (too narrow to promote to a project-wide rule).
    pub do_not_generalize: bool,
}

impl BugFact {
    /// The effective scope prefixes for blast-radius matching. Uses an explicit `scope` if set;
    /// otherwise derives the set of parent directories of `files` (stripping any `:LINE` suffix).
    pub fn effective_scopes(&self) -> Vec<String> {
        if let Some(s) = &self.scope {
            return vec![s.clone()];
        }
        let mut dirs: BTreeSet<String> = BTreeSet::new();
        for f in &self.files {
            let path = f.split(':').next().unwrap_or(f);
            let dir = match path.rfind('/') {
                Some(i) => &path[..i],
                None => "",
            };
            dirs.insert(dir.to_string());
        }
        dirs.into_iter().collect()
    }

    /// Does this fact's scope cover `target`? Simple prefix match (a glob trailing `*` is
    /// tolerated by stripping it). An empty scope ("" — repo root) matches everything.
    pub fn scope_matches(&self, target: &str) -> bool {
        self.effective_scopes().iter().any(|s| {
            let prefix = s.trim_end_matches('*').trim_end_matches('/');
            prefix.is_empty() || target == prefix || target.starts_with(&format!("{prefix}/"))
        })
    }
}

/// The fact-store seam. Sole impl: [`crate::reverie::ReverieBridge`].
pub trait BugStore {
    /// Record a fixed-bug fact (idempotent on the bug slug; re-recording updates the projection).
    fn record(&mut self, fact: &BugFact) -> std::io::Result<()>;
    /// The core query: "does this changed-file set touch a known-bug surface?" Backed by reverie
    /// `/search`; `--as-of <commit>` time-travel is applied by the caller via `crate::gitf`.
    fn touches_known_bug(&self, changed_files: &[String]) -> std::io::Result<Vec<BugFact>>;
}

const META_HEADER: &str = "cicatrix meta-patterns (see CLAUDE.md):";

/// Pure render of the meta-pattern block from facts. Groups by `meta_pattern`, one bullet per
/// distinct class, excluding any fact marked `do_not_generalize`. When `target` is `Some`, only
/// facts whose `scope` matches the target contribute (blast-radius filtering). Classes are
/// emitted in first-seen-by-id order (facts arrive sorted by id) for deterministic output.
pub fn render_meta_patterns(facts: &[BugFact], target: Option<&str>) -> String {
    let mut order: Vec<String> = Vec::new();
    let mut seeds: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for f in facts {
        if f.do_not_generalize {
            continue;
        }
        if let Some(t) = target {
            if !f.scope_matches(t) {
                continue;
            }
        }
        if !seeds.contains_key(&f.meta_pattern) {
            order.push(f.meta_pattern.clone());
        }
        seeds
            .entry(f.meta_pattern.clone())
            .or_default()
            .push(f.id.clone());
    }

    let mut out = String::from(META_HEADER);
    out.push('\n');
    for class in &order {
        let ids = seeds.get(class).map(|v| v.join(", ")).unwrap_or_default();
        out.push_str(&format!("- {class} (seed: {ids})\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fact(id: &str, meta_pattern: &str) -> BugFact {
        BugFact {
            id: id.into(),
            files: vec!["src/x.rs".into()],
            symptom: "s".into(),
            fix_commit: "#1".into(),
            regression_test: "g".into(),
            meta_pattern: meta_pattern.into(),
            scope: None,
            do_not_generalize: false,
        }
    }

    /// The rendered block must carry every named meta-pattern class present in the corpus. If a
    /// class is silently dropped, an agent gets injected with an incomplete corpus — the failure
    /// is invisible at the seam (the "type mismatches kill" class, applied to us). Seeds one
    /// synthetic fact per class (no env-var dependency → no parallel-test race).
    #[test]
    fn meta_patterns_carry_every_named_class() {
        let classes = [
            "Type mismatches kill",
            "Two implementations of one fact drift",
            "Edge cases are real cases",
            "Correctness before performance",
            "Test structure, not just outcomes",
        ];
        let facts: Vec<BugFact> = classes
            .iter()
            .enumerate()
            .map(|(i, c)| fact(&format!("BUG_{i}"), c))
            .collect();
        let block = render_meta_patterns(&facts, None);
        assert!(block.contains(META_HEADER), "header missing: {block}");
        for class in classes {
            assert!(
                block.contains(class),
                "render_meta_patterns() dropped class: {class}"
            );
        }
    }

    /// do-not-generalize facts must not surface in the injected block.
    #[test]
    fn do_not_generalize_excluded() {
        let mut narrow = fact("BUG_NARROW", "Too specific to promote");
        narrow.do_not_generalize = true;
        let general = fact("BUG_GENERAL", "Type mismatches kill");
        let block = render_meta_patterns(&[narrow, general], None);
        assert!(block.contains("Type mismatches kill"));
        assert!(
            !block.contains("Too specific to promote"),
            "do-not-generalize leaked: {block}"
        );
    }

    /// Default scope = parent dirs of files; explicit scope wins.
    #[test]
    fn effective_scopes_and_matching() {
        let mut f = fact("BUG_S", "Type mismatches kill");
        f.files = vec!["crates/a/src/x.rs:12".into(), "crates/a/src/y.rs".into()];
        assert_eq!(f.effective_scopes(), vec!["crates/a/src".to_string()]);
        assert!(f.scope_matches("crates/a/src/x.rs"));
        assert!(!f.scope_matches("crates/b/src/x.rs"));

        f.scope = Some("crates/a".into());
        assert!(f.scope_matches("crates/a/anything.rs"));
        assert!(!f.scope_matches("crates/b/x.rs"));
    }

    /// target filtering: a fact scoped to crate A is emitted for an A target, omitted for B.
    #[test]
    fn render_filters_by_target_scope() {
        let mut a = fact("BUG_A", "A-class pattern");
        a.scope = Some("crates/a".into());
        let mut b = fact("BUG_B", "B-class pattern");
        b.scope = Some("crates/b".into());
        let facts = vec![a, b];

        let for_a = render_meta_patterns(&facts, Some("crates/a/src/x.rs"));
        assert!(for_a.contains("A-class pattern"));
        assert!(!for_a.contains("B-class pattern"));
    }
}
