//! `--as-of <commit>` temporal filter (CER-1375, design §2.1). cicatrix preserves janus-datalog's
//! AsOf(commit) capability without a second store by layering it over git: a bug was "known as of
//! X" iff its fix-commit is an ancestor of X. Pure git — `git merge-base --is-ancestor`.
//!
//! Caveat (surfaced, not hidden): the corpus may record a fix-commit as a PR ref (`#609 (CER-914)`)
//! rather than a sha. Those can't be placed in commit history, so `--as-of` conservatively EXCLUDES
//! them and reports the count — never a silent drop.

use crate::store::BugFact;
use std::process::Command;

/// Pull a git-resolvable ref out of a free-form fix-commit field. Returns the first token that
/// looks like a sha (≥7 hex chars). PR-number / ticket-id values yield `None`.
pub fn extract_ref(fix_commit: &str) -> Option<String> {
    fix_commit
        .split(|c: char| !c.is_ascii_alphanumeric())
        .find(|tok| tok.len() >= 7 && tok.chars().all(|c| c.is_ascii_hexdigit()))
        .map(str::to_string)
}

/// True iff `ancestor` is an ancestor of (or equal to) `commit`. A commit is its own ancestor, so
/// the boundary is inclusive. Unresolvable refs make git exit non-zero → `false`.
pub fn is_ancestor(ancestor: &str, commit: &str) -> bool {
    Command::new("git")
        .args(["merge-base", "--is-ancestor", ancestor, commit])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Split `facts` into (kept, skipped) where kept = facts whose fix-commit resolves to an ancestor
/// of `commit`. `skipped` carries the slugs dropped because their fix-commit wasn't a resolvable
/// ancestor — the caller reports them so the filter is never silent.
pub fn filter_as_of(facts: Vec<BugFact>, commit: &str) -> (Vec<BugFact>, Vec<String>) {
    let mut kept = Vec::new();
    let mut skipped = Vec::new();
    for f in facts {
        match extract_ref(&f.fix_commit) {
            Some(r) if is_ancestor(&r, commit) => kept.push(f),
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
        assert_eq!(extract_ref("6d05ae94b0c3"), Some("6d05ae94b0c3".to_string()));
        assert_eq!(extract_ref("fix in abc1234 landed"), Some("abc1234".to_string()));
        assert_eq!(extract_ref("#609 (CER-914)"), None); // PR + ticket, no sha
        assert_eq!(extract_ref(""), None);
    }

    #[test]
    fn is_ancestor_is_inclusive_and_rejects_garbage() {
        // HEAD is its own ancestor (inclusive boundary).
        let head = String::from_utf8(
            Command::new("git").args(["rev-parse", "HEAD"]).output().unwrap().stdout,
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
            Command::new("git").args(["rev-parse", "HEAD"]).output().unwrap().stdout,
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
        };
        let sha_fact = BugFact { id: "BUG_SHA".into(), fix_commit: head.clone(), ..pr_fact.clone() };
        let (kept, skipped) = filter_as_of(vec![pr_fact, sha_fact], &head);
        assert_eq!(kept.iter().map(|f| f.id.as_str()).collect::<Vec<_>>(), vec!["BUG_SHA"]);
        assert_eq!(skipped, vec!["BUG_PR"]); // reported, not silently gone
    }
}
