//! Parse `docs/bugs/grounded/BUG_*.md` into [`BugFact`]s (CER-1374, Phase 0).
//!
//! The markdown corpus is the **source of truth**; this parser is the front half of the one-way
//! projection into reverie (see `docs/design/cicatrix-reverie-unsigned-paas-integration.md` §2).
//! Pure + offline: no network, no reverie. Format is fixed by `docs/bugs/grounded/_SCHEMA.md`:
//! an `# BUG_<SLUG>` H1, a `- **key:** value` metadata list, then `## Section` prose.
//!
//! Enforces Datomic-style `:db/neverZeroValue` schema integrity (CER-2751) and stochastic failure
//! occurrences/reruns formalization (CER-2752).

use crate::store::{BugFact, OccurrenceEntry, StochasticSpec};
use std::path::Path;

/// Detailed parse errors enforcing schema integrity and Datomic-style `:db/neverZeroValue`
/// (absence is distinct from setting an explicit empty/zero value).
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum ParseError {
    /// An attribute or table cell was explicitly declared with an empty or whitespace-only zero value.
    NeverZeroValue { field: String, detail: String },
    /// A mandatory attribute is missing entirely.
    MissingRequiredField { slug: String, field: String },
    /// A mandatory section (e.g. `## Symptom`) is missing entirely.
    MissingRequiredSection { slug: String, section: String },
    /// No `# BUG_<SLUG>` heading found and no fallback hint provided.
    MissingSlug,
    /// A header line is malformed (e.g. empty slug or empty section title).
    MalformedHeader { line: String },
    /// A markdown table within a section is malformed.
    InvalidTable { section: String, detail: String },
    /// Filesystem or IO failure.
    Io(String),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::NeverZeroValue { field, detail } => {
                write!(f, ":db/neverZeroValue violation on `{field}`: {detail}")
            }
            ParseError::MissingRequiredField { slug, field } => {
                write!(f, "{slug}: missing required `{field}` field")
            }
            ParseError::MissingRequiredSection { slug, section } => {
                write!(f, "{slug}: missing or empty `## {section}` section")
            }
            ParseError::MissingSlug => {
                write!(f, "no `# BUG_<SLUG>` heading and no filename hint")
            }
            ParseError::MalformedHeader { line } => {
                write!(f, "malformed header: {line}")
            }
            ParseError::InvalidTable { section, detail } => {
                write!(f, "invalid markdown table in section `{section}`: {detail}")
            }
            ParseError::Io(err) => write!(f, "IO error: {err}"),
        }
    }
}

impl std::error::Error for ParseError {}

impl From<ParseError> for String {
    fn from(err: ParseError) -> Self {
        err.to_string()
    }
}

/// Parse an occurrence log table formatted as:
/// | n | date | config | result |
/// |---|---|---|---|
/// | 1 | 2026-10-01 | full gate | crash |
pub fn parse_occurrence_table(table_str: &str) -> Result<Vec<OccurrenceEntry>, ParseError> {
    let mut entries = Vec::new();
    for line in table_str.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('|') {
            continue;
        }
        let stripped = trimmed.strip_prefix('|').unwrap_or(trimmed);
        let stripped = stripped.strip_suffix('|').unwrap_or(stripped);
        let cells: Vec<&str> = stripped.split('|').map(str::trim).collect();
        if cells.is_empty() {
            continue;
        }
        let first_lower = cells[0].to_lowercase();
        if first_lower == "n" || cells[0].starts_with("---") || cells[0].starts_with(':') {
            continue;
        }
        if cells.len() != 4 {
            return Err(ParseError::InvalidTable {
                section: "occurrence log".into(),
                detail: format!(
                    "expected 4 columns (n, date, config, result), found {}",
                    cells.len()
                ),
            });
        }
        // Enforce :db/neverZeroValue on table cells
        if cells[0].is_empty() {
            return Err(ParseError::NeverZeroValue {
                field: "occurrence log".into(),
                detail: "occurrence index `n` is empty".into(),
            });
        }
        if cells[1].is_empty() {
            return Err(ParseError::NeverZeroValue {
                field: "occurrence log".into(),
                detail: format!("date cell in occurrence row {} is empty", cells[0]),
            });
        }
        if cells[2].is_empty() {
            return Err(ParseError::NeverZeroValue {
                field: "occurrence log".into(),
                detail: format!("config cell in occurrence row {} is empty", cells[0]),
            });
        }
        if cells[3].is_empty() {
            return Err(ParseError::NeverZeroValue {
                field: "occurrence log".into(),
                detail: format!("result cell in occurrence row {} is empty", cells[0]),
            });
        }

        let n = cells[0]
            .parse::<u32>()
            .map_err(|e| ParseError::InvalidTable {
                section: "occurrence log".into(),
                detail: format!("invalid integer index `{}`: {e}", cells[0]),
            })?;

        entries.push(OccurrenceEntry {
            n,
            date: cells[1].to_string(),
            config: cells[2].to_string(),
            result: cells[3].to_string(),
        });
    }
    Ok(entries)
}

/// Parse one bug-doc's text into a [`BugFact`]. `slug_hint` (the filename stem) is used as the
/// slug when the H1 is absent. Validates at the seam: every required field must be present and
/// non-empty, and all explicit attributes must obey `:db/neverZeroValue` schema integrity.
pub fn parse(text: &str, slug_hint: Option<&str>) -> Result<BugFact, ParseError> {
    let mut slug: Option<String> = None;
    let mut meta: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut sections: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut current_section: Option<String> = None;
    // Fenced-code state (CER-2078). Inside a ``` fence, markdown structural prefixes
    // (`# `, `## `, `- **`) are literal code, not headers/metadata — a real BugFact's
    // Reproduction routinely embeds shell/YAML whose comment lines start with `#`. The
    // fence line itself and its contents are preserved as prose under the current section.
    let mut in_fence = false;

    for line in text.lines() {
        let trimmed = line.trim_end();
        if trimmed.trim_start().starts_with("```") {
            in_fence = !in_fence;
            // fall through to append the fence delimiter to the current section
        } else if !in_fence {
            if let Some(rest) = trimmed.strip_prefix("## ") {
                let s = rest.trim();
                if s.is_empty() {
                    return Err(ParseError::MalformedHeader {
                        line: line.to_string(),
                    });
                }
                current_section = Some(s.to_lowercase());
                continue;
            }
            if let Some(rest) = trimmed.strip_prefix("# ") {
                let s = rest.trim();
                if s.is_empty() {
                    return Err(ParseError::MalformedHeader {
                        line: line.to_string(),
                    });
                }
                slug = Some(s.to_string());
                current_section = None;
                continue;
            }
            if let Some(rest) = trimmed.strip_prefix("- **") {
                if let Some((raw_key, raw_val)) = rest.split_once(":**") {
                    let key = raw_key.trim().to_lowercase();
                    if key.is_empty() {
                        return Err(ParseError::MalformedHeader {
                            line: line.to_string(),
                        });
                    }
                    let val = raw_val.trim();
                    if val.is_empty() || val == "\"\"" || val == "''" {
                        return Err(ParseError::NeverZeroValue {
                            field: key,
                            detail: "explicit empty zero value is disallowed; omit the attribute instead".into(),
                        });
                    }
                    let unquoted = if (val.starts_with('"') && val.ends_with('"') && val.len() >= 2)
                        || (val.starts_with('\'') && val.ends_with('\'') && val.len() >= 2)
                    {
                        val[1..val.len() - 1].trim()
                    } else {
                        val
                    };
                    if unquoted.is_empty() {
                        return Err(ParseError::NeverZeroValue {
                            field: key,
                            detail: "explicit empty zero value is disallowed; omit the attribute instead".into(),
                        });
                    }
                    meta.insert(key, unquoted.to_string());
                } else if let Some((raw_key, _)) = rest.split_once("**") {
                    let key = raw_key.trim().to_lowercase();
                    return Err(ParseError::NeverZeroValue {
                        field: key,
                        detail:
                            "explicit key without value is disallowed; omit the attribute instead"
                                .into(),
                    });
                }
                continue;
            }
        }
        // Prose (outside a fence), or any line inside a fence, or a fence delimiter:
        // literal content of the current section.
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
        .ok_or(ParseError::MissingSlug)?;

    let req = |key: &str| -> Result<String, ParseError> {
        match meta.get(key) {
            Some(s) if !s.trim().is_empty() => Ok(s.trim().to_string()),
            Some(_) => Err(ParseError::NeverZeroValue {
                field: key.to_string(),
                detail: "field has empty zero value".into(),
            }),
            None => Err(ParseError::MissingRequiredField {
                slug: slug.clone(),
                field: key.to_string(),
            }),
        }
    };

    let files_raw = req("files")?;
    let mut files: Vec<String> = Vec::new();
    for item in files_raw.split(',') {
        let trimmed_item = item.trim();
        if trimmed_item.is_empty() {
            return Err(ParseError::NeverZeroValue {
                field: "files".into(),
                detail: "comma-separated `files` list contains an empty zero-value element".into(),
            });
        }
        files.push(trimmed_item.to_string());
    }
    if files.is_empty() {
        return Err(ParseError::NeverZeroValue {
            field: "files".into(),
            detail: "`files` field is empty".into(),
        });
    }

    let symptom = match sections.get("symptom") {
        Some(s) if !s.trim().is_empty() => s.trim().to_string(),
        Some(_) => {
            return Err(ParseError::NeverZeroValue {
                field: "symptom".into(),
                detail: "## Symptom section contains only empty whitespace".into(),
            })
        }
        None => {
            return Err(ParseError::MissingRequiredSection {
                slug: slug.clone(),
                section: "symptom".into(),
            })
        }
    };

    // Resolve every `req` field before moving `slug` into `id` (the closure borrows `slug`).
    let fix_commit = req("fix-commit")?;
    let regression_test = req("regression-test")?;
    let meta_pattern = req("meta-pattern")?;

    // Optional fields — absent is fine (the seed corpus has neither).
    let scope = meta.get("scope").cloned();
    let do_not_generalize = meta
        .get("do-not-generalize")
        .map(|v| matches!(v.trim().to_lowercase().as_str(), "true" | "yes" | "1"))
        .unwrap_or(false);
    let reproducer = meta.get("reproducer").cloned();

    // Stochastic failure extension (CER-2752)
    let occurrences = if let Some(table_str) = sections.get("occurrence log") {
        parse_occurrence_table(table_str)?
    } else {
        Vec::new()
    };
    let rerun_policy = sections
        .get("sanctioned reruns")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let closing_invariant = meta.get("closing-invariant").cloned().or_else(|| {
        sections
            .get("closing invariant")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    });

    let stochastic = if reproducer.is_some()
        || rerun_policy.is_some()
        || closing_invariant.is_some()
        || !occurrences.is_empty()
    {
        Some(StochasticSpec {
            reproducer_command: reproducer.clone(),
            rerun_policy,
            closing_invariant,
            occurrences,
        })
    } else {
        None
    };

    Ok(BugFact {
        id: slug,
        files,
        symptom,
        fix_commit,
        regression_test,
        meta_pattern,
        scope,
        do_not_generalize,
        reproducer,
        stochastic,
    })
}

/// Parse a single `BUG_*.md` file; slug falls back to the filename stem.
pub fn parse_file(path: &Path) -> Result<BugFact, ParseError> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| ParseError::Io(format!("{}: {e}", path.display())))?;
    let stem = path.file_stem().and_then(|s| s.to_str());
    parse(&text, stem)
}

/// Parse every `BUG_*.md` under `dir` (skips `_SCHEMA.md` and non-`BUG_` files). Sorted by id
/// for deterministic output. Returns the first parse error encountered.
pub fn parse_dir(dir: &Path) -> Result<Vec<BugFact>, ParseError> {
    let mut facts = Vec::new();
    let entries =
        std::fs::read_dir(dir).map_err(|e| ParseError::Io(format!("{}: {e}", dir.display())))?;
    for entry in entries {
        let path = entry.map_err(|e| ParseError::Io(e.to_string()))?.path();
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
        assert!(err.to_string().contains("fix-commit"), "err was: {err}");
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
        assert_eq!(f.reproducer, None);
    }

    /// The stochastic-failure extension (janus's occurrence-log genre, ported 2026-10-05):
    /// `- **reproducer:**` parses onto the fact; `## Occurrence log` / `## Sanctioned reruns`
    /// are prose sections that must not disturb the required fields.
    #[test]
    fn parses_stochastic_failure_shape() {
        let text = "# BUG_FLAKE\n\
            \n\
            - **id:** bug:flake\n\
            - **files:** src/wasm.rs\n\
            - **fix-commit:** #9 (CER-2)\n\
            - **regression-test:** wasm gate on pinned toolchain\n\
            - **meta-pattern:** Edge cases are real cases\n\
            - **status:** resolved\n\
            - **reproducer:** GOGC=1 cargo test -p store\n\
            - **closing-invariant:** crash signature on Go >= 1.26.8 is a new bug\n\
            \n\
            ## Symptom\n\
            Crashes at a layout-determined rate.\n\
            \n\
            ## Occurrence log\n\
            | n | date | config | result |\n\
            | 1 | 2026-10-01 | full gate | crash, poison 0x22000000 |\n\
            | 2 | 2026-10-02 | full gate | pass |\n\
            \n\
            ## Sanctioned reruns\n\
            One rerun is sanctioned for poison-shaped crashes only; ends when the pin lands.\n\
            \n\
            ## Root cause\n\
            Upstream runtime defect.\n\
            \n\
            ## Resolution\n\
            Toolchain pin carries the fix. Same signature on the pinned toolchain is a new bug.\n";
        let f = parse(text, None).expect("should parse");
        assert_eq!(f.id, "BUG_FLAKE");
        assert_eq!(f.reproducer.as_deref(), Some("GOGC=1 cargo test -p store"));
        assert_eq!(f.symptom, "Crashes at a layout-determined rate.");
        assert_eq!(f.meta_pattern, "Edge cases are real cases");
        let stoch = f.stochastic.expect("stochastic spec should be present");
        assert_eq!(
            stoch.reproducer_command.as_deref(),
            Some("GOGC=1 cargo test -p store")
        );
        assert_eq!(
            stoch.rerun_policy.as_deref(),
            Some(
                "One rerun is sanctioned for poison-shaped crashes only; ends when the pin lands."
            )
        );
        assert_eq!(
            stoch.closing_invariant.as_deref(),
            Some("crash signature on Go >= 1.26.8 is a new bug")
        );
        assert_eq!(stoch.occurrences.len(), 2);
        assert_eq!(stoch.occurrences[0].n, 1);
        assert_eq!(stoch.occurrences[0].date, "2026-10-01");
        assert_eq!(stoch.occurrences[0].config, "full gate");
        assert_eq!(stoch.occurrences[0].result, "crash, poison 0x22000000");
        assert_eq!(stoch.occurrences[1].n, 2);
        assert_eq!(stoch.occurrences[1].date, "2026-10-02");
        assert_eq!(stoch.occurrences[1].config, "full gate");
        assert_eq!(stoch.occurrences[1].result, "pass");
    }

    #[test]
    fn never_zero_value_rejects_empty_string_metadata() {
        let text = SAMPLE.replace(
            "- **status:** resolved\n",
            "- **status:** resolved\n- **scope:** \"\"\n",
        );
        let err = parse(&text, None).unwrap_err();
        assert!(
            matches!(err, ParseError::NeverZeroValue { ref field, .. } if field == "scope"),
            "expected NeverZeroValue on scope, got: {err}"
        );
        assert!(err.to_string().contains(":db/neverZeroValue violation"));
    }

    #[test]
    fn never_zero_value_rejects_empty_key_value() {
        let text = SAMPLE.replace(
            "- **status:** resolved\n",
            "- **status:** resolved\n- **scope:**\n",
        );
        let err = parse(&text, None).unwrap_err();
        assert!(
            matches!(err, ParseError::NeverZeroValue { ref field, .. } if field == "scope"),
            "expected NeverZeroValue on scope, got: {err}"
        );
    }

    #[test]
    fn never_zero_value_rejects_empty_files_element() {
        let text = SAMPLE.replace(
            "- **files:** src/a.rs:12, src/b.rs\n",
            "- **files:** src/a.rs:12, , src/b.rs\n",
        );
        let err = parse(&text, None).unwrap_err();
        assert!(
            matches!(err, ParseError::NeverZeroValue { ref field, .. } if field == "files"),
            "expected NeverZeroValue on files, got: {err}"
        );
        assert!(err.to_string().contains("empty zero-value element"));
    }

    #[test]
    fn stochastic_table_rejects_empty_cell() {
        let text = "# BUG_FLAKE\n\
            \n\
            - **id:** bug:flake\n\
            - **files:** src/wasm.rs\n\
            - **fix-commit:** #9 (CER-2)\n\
            - **regression-test:** wasm gate on pinned toolchain\n\
            - **meta-pattern:** Edge cases are real cases\n\
            - **status:** resolved\n\
            \n\
            ## Symptom\n\
            Crashes at a layout-determined rate.\n\
            \n\
            ## Occurrence log\n\
            | n | date | config | result |\n\
            | 1 | | full gate | crash |\n";
        let err = parse(text, None).unwrap_err();
        assert!(
            matches!(err, ParseError::NeverZeroValue { ref field, .. } if field == "occurrence log"),
            "expected NeverZeroValue on occurrence log cell, got: {err}"
        );
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

    /// Fenced code blocks are literal content, not markdown structure (CER-2078). A real
    /// BugFact's Reproduction/Root-cause routinely embeds shell/YAML whose lines start with
    /// `# `, `## `, or `- **`; those must NOT be read as the slug / a section header / a
    /// metadata field. Regression for the live break: a `# comment` inside a ```bash fence
    /// clobbered the slug so it no longer started with `BUG_`.
    #[test]
    fn fenced_code_block_content_is_not_parsed_as_structure() {
        let text = "# BUG_FENCE\n\
            \n\
            - **id:** bug:fence\n\
            - **files:** src/a.rs\n\
            - **fix-commit:** #7 (CER-1)\n\
            - **regression-test:** fence guard\n\
            - **meta-pattern:** Edge cases are real cases\n\
            \n\
            ## Symptom\n\
            It broke on deploy.\n\
            \n\
            ## Reproduction\n\
            ```bash\n\
            # The break appears only when the API server validates the object.\n\
            ## not a section either\n\
            - **not-a-field:** and neither is this\n\
            helm template .\n\
            ```\n\
            \n\
            ## Root cause\n\
            A boundary error.\n";
        let f = parse(text, None).expect("should parse");
        // The slug is the real H1, NOT the code comment.
        assert_eq!(f.id, "BUG_FENCE", "slug clobbered by a fenced `# ` line");
        // The fenced `- **not-a-field:**` line did not become metadata.
        assert_eq!(f.meta_pattern, "Edge cases are real cases");
        // The fenced `## not a section` line did not create/switch a section; the real
        // Symptom is intact and the fenced text lives under Reproduction, not as its own key.
        assert_eq!(f.symptom, "It broke on deploy.");
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
