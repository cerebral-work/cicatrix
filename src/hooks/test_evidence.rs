//! Deterministic test evidence discovery for Rust edits.
//!
//! Replaces bounded transcript window searches with verified on-disk ground truth:
//! extracts top-level and impl functions added by an edit and checks whether
//! unit tests (`#[cfg(test)]`) or workspace integration tests (`tests/*.rs`)
//! reference the added functions.

use std::collections::HashSet;
use std::path::Path;

/// Result of evaluating test evidence for added Rust functions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestEvidenceReport {
    /// Whether the file was skipped (e.g. test files, benches, non-Rust files).
    pub skipped: bool,
    /// Functions added by the edit that have verified test references.
    pub tested: Vec<String>,
    /// Functions added by the edit that lack verified test references.
    pub untested: Vec<String>,
}

impl TestEvidenceReport {
    /// Format a concise evidence summary for inclusion in reviewer context.
    pub fn format_evidence(&self) -> Option<String> {
        if self.skipped {
            return None;
        }
        if self.tested.is_empty() && self.untested.is_empty() {
            return None;
        }

        let mut lines = Vec::new();
        if !self.tested.is_empty() {
            lines.push(format!(
                "test-evidence: verified test coverage for added fn: {}",
                self.tested.join(", ")
            ));
        }
        if !self.untested.is_empty() {
            lines.push(format!(
                "test-evidence: NO verified test found for added fn: {}",
                self.untested.join(", ")
            ));
        }
        Some(lines.join("\n"))
    }
}

/// Extract function names declared in Rust source code.
///
/// Matches top-level and impl-block function declarations:
/// `[visibility] [async|const|unsafe] fn <name> ...`
/// Parse a single line to extract function name if it declares a function.
fn parse_fn_declaration(line: &str) -> Option<String> {
    // Match optional visibility: pub, pub(crate), pub(super), pub(in ...), etc.
    let mut rest = line;
    if let Some(after) = rest.strip_prefix("pub") {
        rest = after.trim_start();
        if let Some(after_paren) = rest.strip_prefix('(') {
            if let Some(close_idx) = after_paren.find(')') {
                rest = after_paren[close_idx + 1..].trim_start();
            }
        }
    }

    // Match optional qualifiers in any order: async, const, unsafe, extern "C"
    let mut changed = true;
    while changed {
        changed = false;
        for qual in &["async", "const", "unsafe", "extern"] {
            if let Some(after) = rest.strip_prefix(qual) {
                if after.starts_with(char::is_whitespace) || after.starts_with('"') {
                    rest = after.trim_start();
                    // handle extern "C" or extern "system"
                    if let Some(after_quote) = rest.strip_prefix('"') {
                        if let Some(quote_end) = after_quote.find('"') {
                            rest = after_quote[quote_end + 1..].trim_start();
                        }
                    }
                    changed = true;
                }
            }
        }
    }

    // Now must match `fn `
    if let Some(after_fn) = rest.strip_prefix("fn") {
        if after_fn.starts_with(char::is_whitespace) {
            let id_part = after_fn.trim_start();
            let ident: String = id_part
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();

            if !ident.is_empty() {
                return Some(ident);
            }
        }
    }
    None
}

/// Extract function names declared in Rust source code.
///
/// Matches top-level and impl-block function declarations:
/// `[visibility] [async|const|unsafe] fn <name> ...`
pub fn extract_fn_names(source: &str) -> Vec<String> {
    let mut names = Vec::new();

    for raw_line in source.lines() {
        let line = raw_line.trim();
        if line.is_empty()
            || line.starts_with("//")
            || line.starts_with("/*")
            || line.starts_with('*')
        {
            continue;
        }
        if let Some(ident) = parse_fn_declaration(line) {
            names.push(ident);
        }
    }

    names.sort();
    names.dedup();
    names
}

/// Extract production function names from Rust source, skipping functions
/// defined inside test modules (`#[cfg(test)]`) or annotated with `#[test]`.
pub fn extract_production_fn_names(source: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_cfg_test = false;
    let mut prev_was_test_attr = false;

    for raw_line in source.lines() {
        let line = raw_line.trim();
        if line.is_empty()
            || line.starts_with("//")
            || line.starts_with("/*")
            || line.starts_with('*')
        {
            continue;
        }

        if line.contains("#[cfg(test)]") {
            in_cfg_test = true;
            continue;
        }

        if line.contains("#[test]") || line.contains("#[tokio::test]") {
            prev_was_test_attr = true;
            continue;
        }

        if let Some(ident) = parse_fn_declaration(line) {
            if !in_cfg_test && !prev_was_test_attr {
                names.push(ident);
            }
        }

        prev_was_test_attr = false;
    }

    names.sort();
    names.dedup();
    names
}

/// Compute test evidence for an edited Rust file.
///
/// - `file_path`: Path to the modified file.
/// - `old_content`: Previous contents of the file.
/// - `new_content`: Proposed new contents of the file.
/// - `repo_root`: Root path of the repository for integration test checks.
pub fn compute_test_evidence(
    file_path: &Path,
    old_content: &str,
    new_content: &str,
    repo_root: Option<&Path>,
) -> TestEvidenceReport {
    let path_str = file_path.to_string_lossy();

    // Skip tests, benches, or non-rust files
    let is_test_or_bench = path_str.starts_with("tests/")
        || path_str.contains("/tests/")
        || path_str.starts_with("benches/")
        || path_str.contains("/benches/");

    if is_test_or_bench || !path_str.ends_with(".rs") {
        return TestEvidenceReport {
            skipped: true,
            tested: Vec::new(),
            untested: Vec::new(),
        };
    }

    let old_fns: HashSet<String> = extract_production_fn_names(old_content)
        .into_iter()
        .collect();
    let new_fns = extract_production_fn_names(new_content);

    // Added functions are in new_fns but not in old_fns
    let added_fns: Vec<String> = new_fns
        .into_iter()
        .filter(|name| !old_fns.contains(name))
        .collect();

    if added_fns.is_empty() {
        return TestEvidenceReport {
            skipped: false,
            tested: Vec::new(),
            untested: Vec::new(),
        };
    }

    let has_cfg_test = new_content.contains("#[cfg(test)]");
    let mut tested = Vec::new();
    let mut untested = Vec::new();

    for fn_name in added_fns {
        let mut hit = false;

        // (a) In-file #[cfg(test)]: fn name appears at least twice (def + ref)
        if has_cfg_test {
            let count = count_word_occurrences(new_content, &fn_name);
            if count >= 2 {
                hit = true;
            }
        }

        // (b) Workspace integration tests (<repo-root>/tests/*.rs)
        if !hit {
            if let Some(root) = repo_root {
                let tests_dir = root.join("tests");
                if tests_dir.is_dir() {
                    if let Ok(entries) = std::fs::read_dir(&tests_dir) {
                        for entry in entries.flatten() {
                            let p = entry.path();
                            if p.extension().and_then(|s| s.to_str()) == Some("rs") {
                                if let Ok(test_src) = std::fs::read_to_string(&p) {
                                    if count_word_occurrences(&test_src, &fn_name) > 0 {
                                        hit = true;
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        if hit {
            tested.push(fn_name);
        } else {
            untested.push(fn_name);
        }
    }

    TestEvidenceReport {
        skipped: false,
        tested,
        untested,
    }
}

/// Count occurrences of a word as a standalone token or substring.
fn count_word_occurrences(haystack: &str, needle: &str) -> usize {
    let mut count = 0;
    let mut search_from = 0;
    while let Some(idx) = haystack[search_from..].find(needle) {
        let absolute_idx = search_from + idx;
        // Verify word boundaries
        let before_ok = if absolute_idx == 0 {
            true
        } else {
            let prev = haystack[..absolute_idx].chars().next_back().unwrap();
            !prev.is_alphanumeric() && prev != '_'
        };

        let after_idx = absolute_idx + needle.len();
        let after_ok = if after_idx >= haystack.len() {
            true
        } else {
            let next = haystack[after_idx..].chars().next().unwrap();
            !next.is_alphanumeric() && next != '_'
        };

        if before_ok && after_ok {
            count += 1;
        }
        search_from = absolute_idx + needle.len();
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn extracts_diverse_fn_signatures() {
        let src = r#"
            // fn commented_out() {}
            /* fn block_commented() {} */
            fn standard_fn() {}
            pub fn public_fn() {}
            pub(crate) async fn async_crate_fn() {}
            pub(super) const fn const_super_fn() {}
            pub unsafe fn unsafe_fn() {}
            extern "C" fn extern_c_fn() {}
            impl Foo {
                pub fn method_fn(&self) {}
                fn helper_fn() {}
            }
        "#;

        let names = extract_fn_names(src);
        assert_eq!(
            names,
            vec![
                "async_crate_fn",
                "const_super_fn",
                "extern_c_fn",
                "helper_fn",
                "method_fn",
                "public_fn",
                "standard_fn",
                "unsafe_fn",
            ]
        );
    }

    #[test]
    fn skips_tests_and_benches_dirs() {
        let report =
            compute_test_evidence(Path::new("tests/cli.rs"), "", "fn some_test() {}", None);
        assert!(report.skipped);
    }

    #[test]
    fn detects_in_file_test_evidence() {
        let old_src = "pub fn old_fn() {}\n";
        let new_src = r#"
            pub fn old_fn() {}
            pub fn new_fn() {}

            #[cfg(test)]
            mod tests {
                use super::*;
                #[test]
                fn test_new() {
                    new_fn();
                }
            }
        "#;

        let report = compute_test_evidence(Path::new("src/lib.rs"), old_src, new_src, None);

        assert!(!report.skipped);
        assert_eq!(report.tested, vec!["new_fn"]);
        assert!(report.untested.is_empty());
        assert!(report
            .format_evidence()
            .unwrap()
            .contains("verified test coverage"));
    }

    #[test]
    fn detects_integration_test_evidence() {
        let temp =
            std::env::temp_dir().join(format!("cicatrix_test_evidence_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp);
        let tests_dir = temp.join("tests");
        fs::create_dir_all(&tests_dir).unwrap();
        fs::write(
            tests_dir.join("integration.rs"),
            "#[test] fn test_integration() { service::added_fn(); }\n",
        )
        .unwrap();

        let old_src = "";
        let new_src = "pub fn added_fn() {}\npub fn lonely_fn() {}\n";

        let report =
            compute_test_evidence(Path::new("src/service.rs"), old_src, new_src, Some(&temp));

        assert!(!report.skipped);
        assert_eq!(report.tested, vec!["added_fn"]);
        assert_eq!(report.untested, vec!["lonely_fn"]);

        let _ = fs::remove_dir_all(&temp);
    }
}
