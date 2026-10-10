//! Multi-vector temporal evaluation and version-vector frontier filtering (CER-2754, Phase 1.2).
//!
//! Replaces scalar git ancestry checks with multi-vector evaluation:
//! - Multi-head commit vectors across branch cuts (`is_ancestor_multi`, `filter_as_of_multi`).
//! - Multi-replica version-vector causality (`filter_by_frontier`) based on Lamport clocks
//!   and causal dominance ($A \ge B \iff \forall r \in \text{dom}(B), A(r) \ge B(r)$).
//!
//! Lineage: `wbrown/janus-datalog`.

use crate::store::{BugFact, Frontier};
use std::process::{Command, Stdio};

/// Pull a git-resolvable ref out of a free-form fix-commit field. Returns the first token that
/// looks like a sha: ≥7 hex chars **with at least one `a`–`f` letter**. The letter requirement
/// distinguishes a sha from a long all-decimal PR/issue number (`#1234567`), which is hex-valid
/// but never a commit ref here — so those yield `None` (conservatively excluded, like a PR ref).
pub fn extract_ref(fix_commit: &str) -> Option<String> {
    fix_commit
        .split(|c: char| !c.is_ascii_alphanumeric())
        .find(|tok| {
            tok.len() >= 7
                && tok.chars().all(|c| c.is_ascii_hexdigit())
                && tok.chars().any(|c| c.is_ascii_alphabetic())
        })
        .map(str::to_string)
}

/// True iff `ancestor` is an ancestor of (or equal to) `commit`. A commit is its own ancestor, so
/// the boundary is inclusive. Unresolvable refs make git exit non-zero → `false`. stderr is
/// silenced so an unresolvable ref doesn't leak git's `fatal:` into the caller's terminal during
/// `query --as-of` — the exclusion is reported by `filter_as_of`, not by git's noise.
pub fn is_ancestor(ancestor: &str, commit: &str) -> bool {
    Command::new("git")
        .args(["merge-base", "--is-ancestor", ancestor, commit])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Multi-head ancestry check: returns true iff `ancestor` is an ancestor of (or equal to)
/// ANY of the target heads in `target_heads`.
pub fn is_ancestor_multi(ancestor: &str, target_heads: &[&str]) -> bool {
    target_heads
        .iter()
        .any(|head| is_ancestor(ancestor, head.trim()))
}

/// Split `facts` into (kept, skipped) where kept = facts whose fix-commit resolves to an ancestor
/// of ANY of `target_heads`. `skipped` carries the slugs dropped because their fix-commit wasn't a
/// resolvable ancestor of any target head — the caller reports them so the filter is never silent.
pub fn filter_as_of_multi(
    facts: Vec<BugFact>,
    target_heads: &[&str],
) -> (Vec<BugFact>, Vec<String>) {
    if target_heads.is_empty() {
        return (facts, Vec::new());
    }
    let mut kept = Vec::new();
    let mut skipped = Vec::new();
    for f in facts {
        match extract_ref(&f.fix_commit) {
            Some(r) if is_ancestor_multi(&r, target_heads) => kept.push(f),
            _ => skipped.push(f.id.clone()),
        }
    }
    (kept, skipped)
}

/// Backwards-compatible single-commit wrapper delegating to [`filter_as_of_multi`].
/// Supports either a single commit hash or comma-separated commit heads (e.g. `head1,head2`).
pub fn filter_as_of(facts: Vec<BugFact>, commit: &str) -> (Vec<BugFact>, Vec<String>) {
    let heads: Vec<&str> = commit
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    filter_as_of_multi(facts, &heads)
}

/// Filter bug facts by a target version-vector [`Frontier`].
///
/// Under version-vector causality, a bug is known as-of `target` iff `target.dominates(fact_frontier)`.
/// Facts with no recorded frontier cannot be placed in the version-vector causal history,
/// so they are conservatively reported in `skipped` (never silently dropped).
pub fn filter_by_frontier(facts: Vec<BugFact>, target: &Frontier) -> (Vec<BugFact>, Vec<String>) {
    let mut kept = Vec::new();
    let mut skipped = Vec::new();
    for f in facts {
        match &f.frontier {
            Some(fact_frontier) if target.dominates(fact_frontier) => kept.push(f),
            _ => skipped.push(f.id.clone()),
        }
    }
    (kept, skipped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_ref_finds_sha_rejects_pr_and_ticket() {
        assert_eq!(
            extract_ref("6d05ae94b0c3"),
            Some("6d05ae94b0c3".to_string())
        );
        assert_eq!(
            extract_ref("fix in abc1234 landed"),
            Some("abc1234".to_string())
        );
        assert_eq!(extract_ref("#609 (CER-914)"), None); // PR + ticket, no sha
        assert_eq!(extract_ref(""), None);
        // A long all-decimal PR/issue number is hex-valid but must NOT be read as a sha — it has
        // no a–f letter, so it's conservatively excluded (regression guard for the false-positive).
        assert_eq!(extract_ref("#1234567"), None);
        assert_eq!(extract_ref("see PR 9876543 for the fix"), None);
        // …but a real sha that happens to start with digits still resolves (has hex letters).
        assert_eq!(extract_ref("123abc7"), Some("123abc7".to_string()));
    }

    #[test]
    fn is_ancestor_is_inclusive_and_rejects_garbage() {
        // HEAD is its own ancestor (inclusive boundary).
        let head = String::from_utf8(
            Command::new("git")
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap();
        let head = head.trim();
        assert!(is_ancestor(head, head), "a commit must be its own ancestor");
        // An unresolvable ref is not an ancestor of anything.
        assert!(!is_ancestor("0000000nonexistentref", head));
    }

    #[test]
    fn filter_reports_unresolvable_fixcommits_instead_of_dropping_silently() {
        let head = String::from_utf8(
            Command::new("git")
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap();
        let head = head.trim().to_string();
        let pr_fact = BugFact {
            id: "BUG_PR".into(),
            files: vec!["x.rs".into()],
            symptom: "s".into(),
            fix_commit: "#609 (CER-914)".into(), // unresolvable
            regression_test: "t".into(),
            meta_pattern: "m".into(),
            scope: None,
            do_not_generalize: false,
            reproducer: None,
            stochastic: None,
            frontier: None,
        };
        let sha_fact = BugFact {
            id: "BUG_SHA".into(),
            fix_commit: head.clone(),
            ..pr_fact.clone()
        };
        let (kept, skipped) = filter_as_of(vec![pr_fact, sha_fact], &head);
        assert_eq!(
            kept.iter().map(|f| f.id.as_str()).collect::<Vec<_>>(),
            vec!["BUG_SHA"]
        );
        assert_eq!(skipped, vec!["BUG_PR"]); // reported, not silently gone
    }

    #[test]
    fn multi_head_ancestry_and_filter() {
        let head = String::from_utf8(
            Command::new("git")
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap();
        let head = head.trim().to_string();

        assert!(is_ancestor_multi(&head, &["nonexistentref", &head]));
        assert!(!is_ancestor_multi(
            "0000000nonexistentref",
            &["badref1", "badref2"]
        ));

        let fact1 = BugFact {
            id: "BUG_HEAD".into(),
            files: vec!["a.rs".into()],
            symptom: "s".into(),
            fix_commit: head.clone(),
            regression_test: "t".into(),
            meta_pattern: "m".into(),
            scope: None,
            do_not_generalize: false,
            reproducer: None,
            stochastic: None,
            frontier: None,
        };
        let fact2 = BugFact {
            id: "BUG_GARBAGE".into(),
            fix_commit: "0000000nonexistentref".into(),
            ..fact1.clone()
        };

        // Comma-separated multi-head
        let multi_spec = format!("0000000nonexistentref, {head}");
        let (kept, skipped) = filter_as_of(vec![fact1, fact2], &multi_spec);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].id, "BUG_HEAD");
        assert_eq!(skipped, vec!["BUG_GARBAGE"]);
    }

    #[test]
    fn filter_by_version_vector_frontier() {
        let mut target = Frontier::new();
        target.set("ceres", 50).unwrap();
        target.set("cygnus", 20).unwrap();

        // Dominated: ceres:40, cygnus:15 (<= 50, 20)
        let f_dominated = Frontier::parse("ceres:40, cygnus:15").unwrap();
        // Exceeding: ceres:60 (> 50)
        let f_exceeding = Frontier::parse("ceres:60, cygnus:10").unwrap();
        // Concurrent: ceres:30, cygnus:25 (cygnus 25 > 20)
        let f_concurrent = Frontier::parse("ceres:30, cygnus:25").unwrap();

        let base_fact = BugFact {
            id: "".into(),
            files: vec!["x.rs".into()],
            symptom: "s".into(),
            fix_commit: "abc1234".into(),
            regression_test: "t".into(),
            meta_pattern: "m".into(),
            scope: None,
            do_not_generalize: false,
            reproducer: None,
            stochastic: None,
            frontier: None,
        };

        let fact_dom = BugFact {
            id: "BUG_DOMINATED".into(),
            frontier: Some(f_dominated),
            ..base_fact.clone()
        };
        let fact_exc = BugFact {
            id: "BUG_EXCEEDING".into(),
            frontier: Some(f_exceeding),
            ..base_fact.clone()
        };
        let fact_con = BugFact {
            id: "BUG_CONCURRENT".into(),
            frontier: Some(f_concurrent),
            ..base_fact.clone()
        };
        let fact_no_frontier = BugFact {
            id: "BUG_NO_FRONTIER".into(),
            frontier: None,
            ..base_fact.clone()
        };

        let (kept, skipped) = filter_by_frontier(
            vec![fact_dom, fact_exc, fact_con, fact_no_frontier],
            &target,
        );

        assert_eq!(
            kept.iter().map(|f| f.id.as_str()).collect::<Vec<_>>(),
            vec!["BUG_DOMINATED"]
        );
        assert_eq!(
            skipped,
            vec!["BUG_EXCEEDING", "BUG_CONCURRENT", "BUG_NO_FRONTIER"]
        );
    }
}
