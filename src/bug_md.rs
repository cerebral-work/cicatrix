//! Parse `docs/bugs/grounded/BUG_*.md` into [`BugFact`]s (CER-1374, Phase 0).
//!
//! The markdown corpus is the **source of truth**; this parser is the front half of the one-way
//! projection into reverie (see `docs/design/cicatrix-reverie-unsigned-paas-integration.md` §2).
//! Pure + offline: no network, no reverie. Format is fixed by `docs/bugs/grounded/_SCHEMA.md`:
//! an `# BUG_<SLUG>` H1, a `- **key:** value` metadata list, then `## Section` prose.

use crate::store::BugFact;
use std::path::Path;

/// Parse one bug-doc's text into a [`BugFact`]. `slug_hint` (the filename stem) is used as the
/// slug when the H1 is absent. Validates at the seam: every required field must be present and
/// non-empty, else `Err` — an incomplete fact must never silently project a degenerate observation.
pub fn parse(text: &str, slug_hint: Option<&str>) -> Result<BugFact, String> {
    let mut slug: Option<String> = None;
    let mut meta: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut sections: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut current_section: Option<String> = None;

    for line in text.lines() {
        let trimmed = line.trim_end();
        if let Some(rest) = trimmed.strip_prefix("## ") {
            current_section = Some(rest.trim().to_lowercase());
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("# ") {
            // H1 (single hash) — the bug slug. `strip_prefix("## ")` above already consumed H2s.
            slug = Some(rest.trim().to_string());
            current_section = None;
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("- **") {
            // `key:** value`
            if let Some((key, value)) = rest.split_once(":** ") {
                meta.insert(key.trim().to_lowercase(), value.trim().to_string());
            }
            continue;
        }
        if let Some(sec) = &current_section {
            let buf = sections.entry(sec.clone()).or_default();
            if !buf.is_empty() {
                buf.push('\n');
            }
            buf.push_str(line);
        }
    }

    let slug = slug
        .or_else(|| slug_hint.map(str::to_string))
        .ok_or("no `# BUG_<SLUG>` heading and no filename hint")?;

    let req = |key: &str| -> Result<String, String> {
        meta.get(key)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| format!("{slug}: missing required `{key}` field"))
    };

    let files: Vec<String> = req("files")?
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if files.is_empty() {
        return Err(format!("{slug}: `files` field is empty"));
    }

    let symptom = sections
        .get("symptom")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("{slug}: missing or empty `## Symptom` section"))?;

    // Resolve every `req` field before moving `slug` into `id` (the closure borrows `slug`).
    let fix_commit = req("fix-commit")?;
    let regression_test = req("regression-test")?;
    let meta_pattern = req("meta-pattern")?;

    // Optional fields — absent is fine (the seed corpus has neither).
    let scope = meta
        .get("scope")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let do_not_generalize = meta
        .get("do-not-generalize")
        .map(|v| matches!(v.trim().to_lowercase().as_str(), "true" | "yes" | "1"))
        .unwrap_or(false);

    Ok(BugFact {
        id: slug,
        files,
        symptom,
        fix_commit,
        regression_test,
        meta_pattern,
        scope,
        do_not_generalize,
    })
}

/// Parse a single `BUG_*.md` file; slug falls back to the filename stem.
pub fn parse_file(path: &Path) -> Result<BugFact, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let stem = path.file_stem().and_then(|s| s.to_str());
    parse(&text, stem)
}

/// Parse every `BUG_*.md` under `dir` (skips `_SCHEMA.md` and non-`BUG_` files). Sorted by id
/// for deterministic output. Returns the first parse error encountered.
pub fn parse_dir(dir: &Path) -> Result<Vec<BugFact>, String> {
    let mut facts = Vec::new();
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for entry in entries {
        let path = entry.map_err(|e| e.to_string())?.path();
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if !name.starts_with("BUG_") || !name.ends_with(".md") {
            continue;
        }
        facts.push(parse_file(&path)?);
    }
    facts.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(facts)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# BUG_SAMPLE\n\
        \n\
        - **id:** bug:sample\n\
        - **files:** src/a.rs:12, src/b.rs\n\
        - **fix-commit:** #42 (CER-1)\n\
        - **regression-test:** sample guard\n\
        - **meta-pattern:** Type mismatches kill\n\
        - **status:** resolved\n\
        \n\
        ## Symptom\n\
        The thing broke.\n\
        \n\
        ## Root cause\n\
        A boundary error.\n";

    #[test]
    fn parses_all_fields() {
        let f = parse(SAMPLE, None).expect("should parse");
        assert_eq!(f.id, "BUG_SAMPLE");
        assert_eq!(f.files, vec!["src/a.rs:12", "src/b.rs"]);
        assert_eq!(f.fix_commit, "#42 (CER-1)");
        assert_eq!(f.regression_test, "sample guard");
        assert_eq!(f.meta_pattern, "Type mismatches kill");
        assert_eq!(f.symptom, "The thing broke.");
    }

    #[test]
    fn missing_required_field_is_an_error() {
        // drop the fix-commit line — the seam must reject, not project a degenerate fact
        let text = SAMPLE.replace("- **fix-commit:** #42 (CER-1)\n", "");
        let err = parse(&text, None).unwrap_err();
        assert!(err.contains("fix-commit"), "err was: {err}");
    }

    #[test]
    fn empty_symptom_is_an_error() {
        let text = SAMPLE.replace("The thing broke.\n", "");
        assert!(parse(&text, None).is_err());
    }

    #[test]
    fn slug_falls_back_to_filename_hint() {
        let no_h1 = SAMPLE.replacen("# BUG_SAMPLE\n", "", 1);
        let f = parse(&no_h1, Some("BUG_FROM_FILENAME")).expect("hint slug");
        assert_eq!(f.id, "BUG_FROM_FILENAME");
    }

    /// Seed corpus has no `scope`/`do-not-generalize` — the optional fields default cleanly.
    #[test]
    fn optional_fields_default_when_absent() {
        let f = parse(SAMPLE, None).expect("should parse");
        assert_eq!(f.scope, None);
        assert!(!f.do_not_generalize);
    }

    /// Optional `scope` + `do-not-generalize` markers parse when present.
    #[test]
    fn parses_optional_scope_and_do_not_generalize() {
        let text = SAMPLE.replace(
            "- **status:** resolved\n",
            "- **status:** resolved\n\
             - **scope:** crates/reverie-store\n\
             - **do-not-generalize:** true\n",
        );
        let f = parse(&text, None).expect("should parse");
        assert_eq!(f.scope.as_deref(), Some("crates/reverie-store"));
        assert!(f.do_not_generalize);
    }

    /// Structural guard against schema drift: every real seed bug must parse cleanly.
    #[test]
    fn real_corpus_parses() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/bugs/grounded");
        let facts = parse_dir(&dir).expect("corpus should parse");
        assert!(
            facts.len() >= 2,
            "expected >=2 seed bugs, got {}",
            facts.len()
        );
        for f in &facts {
            assert!(f.id.starts_with("BUG_"), "slug not BUG_*: {}", f.id);
            assert!(!f.files.is_empty() && !f.symptom.is_empty() && !f.meta_pattern.is_empty());
        }
    }
}
