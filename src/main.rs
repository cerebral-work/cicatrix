//! cicatrix — regression-memory + convention-drift CLI.
mod bug_md;
mod corpus;
mod drift;
mod gitf;
mod reverie;
mod store;

use std::path::Path;
use std::process::ExitCode;

use store::BugStore;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str).unwrap_or("help") {
        // emit the meta-pattern block into an agent's context, upstream of a task
        "inject" => cmd_inject(&args[1..]),
        // project fixed-bug fact(s) from the markdown corpus into reverie (project=cicatrix)
        "record" => cmd_record(&args[1..]),
        // "does this diff touch a known-bug surface?" — query reverie, optionally as-of a commit
        "query" => cmd_query(&args[1..]),
        // regenerate the CLAUDE.md meta-pattern block from grounded facts; diff (or --apply write)
        "project-meta" => cmd_project_meta(&args[1..]),
        // print newest scan path (bare) or regenerate the convention-drift table (`drift scan`)
        "drift" => cmd_drift(&args[1..]),
        _ => {
            eprintln!(
                "usage: cicatrix <inject [--target <path>] | record [<BUG_*.md>...] | \
                 query <changed-file>... [--as-of <commit>] | project-meta [--apply] | \
                 drift [scan [--repo <path>]]>"
            );
            ExitCode::FAILURE
        }
    }
}

/// `inject [--target <path>]` — emit the meta-pattern block. With `--target`, emit only patterns
/// whose fact `scope` matches the target (blast-radius filtering); without it, emit all.
fn cmd_inject(rest: &[String]) -> ExitCode {
    let mut target: Option<String> = None;
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--target" => match it.next() {
                Some(t) => target = Some(t.clone()),
                None => {
                    eprintln!("cicatrix inject: --target needs a <path>");
                    return ExitCode::FAILURE;
                }
            },
            flag if flag.starts_with("--") => {
                eprintln!("cicatrix inject: unknown flag {flag}");
                return ExitCode::FAILURE;
            }
            other => {
                eprintln!("cicatrix inject: unexpected argument {other}");
                return ExitCode::FAILURE;
            }
        }
    }
    let facts = match corpus::read_facts(corpus::Tier::Grounded) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("cicatrix inject: {e}");
            return ExitCode::FAILURE;
        }
    };
    print!("{}", store::render_meta_patterns(&facts, target.as_deref()));
    ExitCode::SUCCESS
}

/// `record [<BUG_*.md>...]` — project the given bug-docs (or the whole grounded corpus, if none
/// given) into reverie. Only grounded facts may be projected: an explicit path under the observed
/// tier is refused before anything is projected (observed facts are ungrounded). The markdown is
/// the source of truth; this writes the regenerable projection.
fn cmd_record(rest: &[String]) -> ExitCode {
    let paths: Vec<&String> = rest.iter().filter(|a| !a.starts_with("--")).collect();

    // Poison-the-well gate: refuse any explicit input under the observed (ungrounded) tier
    // BEFORE projecting anything.
    let observed_dir = corpus::resolve_dir(corpus::Tier::Observed);
    for p in &paths {
        if path_is_under(Path::new(p.as_str()), &observed_dir) {
            eprintln!("observed facts are ungrounded; promote to grounded first");
            return ExitCode::FAILURE;
        }
    }

    let facts = if paths.is_empty() {
        bug_md::parse_dir(&reverie::corpus_dir())
    } else {
        paths
            .iter()
            .map(|p| bug_md::parse_file(Path::new(p.as_str())))
            .collect()
    };
    let facts = match facts {
        Ok(f) => f,
        Err(e) => {
            eprintln!("cicatrix record: {e}");
            return ExitCode::FAILURE;
        }
    };
    if facts.is_empty() {
        eprintln!("cicatrix record: no bug-facts found to project");
        return ExitCode::FAILURE;
    }

    let mut bridge = reverie::ReverieBridge::from_env();
    let mut recorded = 0usize;
    for f in &facts {
        match bridge.record(f) {
            Ok(()) => {
                println!("recorded {} → reverie (project=cicatrix)", f.id);
                recorded += 1;
            }
            Err(e) => eprintln!("cicatrix record: {} failed: {e}", f.id),
        }
    }
    if recorded == facts.len() {
        ExitCode::SUCCESS
    } else {
        eprintln!("cicatrix record: {recorded}/{} projected", facts.len());
        ExitCode::FAILURE
    }
}

/// `query <changed-file>... [--as-of <commit>]` — ask reverie which known-bug surfaces the changed
/// files touch; with `--as-of`, keep only bugs fixed at or before `<commit>` (git-ancestry).
fn cmd_query(rest: &[String]) -> ExitCode {
    let mut files = Vec::new();
    let mut as_of: Option<String> = None;
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--as-of" => match it.next() {
                Some(c) => as_of = Some(c.clone()),
                None => {
                    eprintln!("cicatrix query: --as-of needs a <commit>");
                    return ExitCode::FAILURE;
                }
            },
            flag if flag.starts_with("--") => {
                eprintln!("cicatrix query: unknown flag {flag}");
                return ExitCode::FAILURE;
            }
            _ => files.push(a.clone()),
        }
    }
    if files.is_empty() {
        eprintln!("usage: cicatrix query <changed-file>... [--as-of <commit>]");
        return ExitCode::FAILURE;
    }

    let bridge = reverie::ReverieBridge::from_env();
    let mut hits = match bridge.touches_known_bug(&files) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("cicatrix query: {e}");
            return ExitCode::FAILURE;
        }
    };

    if let Some(commit) = &as_of {
        let (kept, skipped) = gitf::filter_as_of(hits, commit);
        hits = kept;
        if !skipped.is_empty() {
            eprintln!(
                "cicatrix query: --as-of {commit} excluded {} fact(s) with unresolvable fix-commit: {}",
                skipped.len(),
                skipped.join(", ")
            );
        }
    }

    if hits.is_empty() {
        println!("no known-bug surface touched");
        return ExitCode::SUCCESS;
    }
    for f in &hits {
        println!("⚠ {}: known-bug surface ({})", f.id, f.meta_pattern);
        println!("  files: {}", f.files.join(", "));
        println!("  guard: {} — don't reintroduce it", f.regression_test);
    }
    ExitCode::SUCCESS
}

const MARKERS_JSON: &str = "markers.json";
const DRIFT_DIR: &str = "drift";

/// `drift [scan [--repo <path>]]`.
///
/// - bare `drift`: print the NEWEST `drift/convention-drift-*.md` path (back-compat). The lexically
///   greatest filename is newest given the `yyyy-mm-dd` naming.
/// - `drift scan [--repo <path>]`: load `markers.json` (CWD-relative; missing/malformed → hard
///   error), scan, render with `resolve_now()`, write `drift/convention-drift-<date>.md`, print
///   that path. `--repo <path>` narrows to the ONE configured repo whose `path` matches exactly,
///   full-regenerating that single-repo table (no merge/patch).
///
/// All resolution is CWD-relative (markers.json and the drift/ output dir), so a caller can pin a
/// scan at any root via its working directory — this is how the reproduce-on-unchanged test works.
fn cmd_drift(rest: &[String]) -> ExitCode {
    match rest.first().map(String::as_str) {
        None => drift_print_newest(),
        Some("scan") => drift_scan(&rest[1..]),
        Some(other) => {
            eprintln!("cicatrix drift: unknown subcommand {other}");
            eprintln!("usage: cicatrix drift [scan [--repo <path>]]");
            ExitCode::FAILURE
        }
    }
}

/// Print the newest `drift/convention-drift-*.md` path. With no scan dir or no matching file,
/// it's a clean error (the command would otherwise advertise a path that does not exist).
fn drift_print_newest() -> ExitCode {
    let dir = Path::new(DRIFT_DIR);
    let mut newest: Option<String> = None;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if name.starts_with("convention-drift-") && name.ends_with(".md") {
                let rel = format!("{DRIFT_DIR}/{name}");
                // lexically greatest == newest under yyyy-mm-dd naming
                if newest.as_deref().map(|n| rel.as_str() > n).unwrap_or(true) {
                    newest = Some(rel);
                }
            }
        }
    }
    match newest {
        Some(path) => {
            println!("{path}");
            ExitCode::SUCCESS
        }
        None => {
            eprintln!("cicatrix drift: no scan found under {DRIFT_DIR}/");
            ExitCode::FAILURE
        }
    }
}

/// Run `drift scan`: parse flags, load + scan markers.json, render, write the dated file, print it.
fn drift_scan(rest: &[String]) -> ExitCode {
    let mut repo: Option<String> = None;
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--repo" => match it.next() {
                Some(p) => repo = Some(p.clone()),
                None => {
                    eprintln!("cicatrix drift: --repo needs a <path>");
                    return ExitCode::FAILURE;
                }
            },
            flag if flag.starts_with("--") => {
                eprintln!("cicatrix drift: unknown flag {flag}");
                return ExitCode::FAILURE;
            }
            other => {
                eprintln!("cicatrix drift: unexpected argument {other}");
                return ExitCode::FAILURE;
            }
        }
    }

    let mut config = match drift::Config::load(Path::new(MARKERS_JSON)) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("cicatrix drift: {e}");
            return ExitCode::FAILURE;
        }
    };

    // --repo narrows to the single configured repo whose path matches (full-regenerate). Compare
    // through `expand_home` on BOTH sides: markers.json stores `~/repos/x`, but a shell expands an
    // unquoted `--repo ~/repos/x` to an absolute path before argv — comparing the raw strings would
    // never match. Normalizing both sides lets either the `~/...` or the expanded form match.
    if let Some(target) = &repo {
        let want = drift::expand_home(target);
        config.repos.retain(|r| drift::expand_home(&r.path) == want);
        if config.repos.is_empty() {
            eprintln!("cicatrix drift: --repo {target} matches no configured repo");
            return ExitCode::FAILURE;
        }
    }

    // Single source of truth for the scan date: `scan_config` calls `resolve_now()` once and stores
    // it in `table.generated`; BOTH the rendered header and the output filename derive from that one
    // value. Computing the date a second time here would be cicatrix meta-pattern #2 ("two
    // implementations of one fact drift") — the filename could disagree with the in-table header.
    let table = drift::scan_config(&config);
    let rendered = render_drift_table(&table);

    if let Err(e) = std::fs::create_dir_all(DRIFT_DIR) {
        eprintln!("cicatrix drift: create {DRIFT_DIR}/: {e}");
        return ExitCode::FAILURE;
    }
    let out_path = format!("{DRIFT_DIR}/convention-drift-{}.md", table.generated);
    if let Err(e) = std::fs::write(&out_path, rendered) {
        eprintln!("cicatrix drift: write {out_path}: {e}");
        return ExitCode::FAILURE;
    }
    println!("{out_path}");
    ExitCode::SUCCESS
}

/// Render a `MarkerTable` to the machine-structured convention-drift markdown. PURE: reads `root`
/// from the table, no clock, no fs. The header em-dash is U+2014 (space-padded); the legend/columns
/// separators are U+00B7 — both copied byte-for-byte from the seed. Always emits a `## Skipped
/// repos` section (`None.` when empty), the `_No repos found._` body for an empty scan, and exactly
/// one trailing newline.
fn render_drift_table(table: &drift::MarkerTable) -> String {
    let mut out = String::new();
    // Line 1: header — em-dash U+2014 space-padded. The date is `table.generated` (the single
    // source); the renderer never consults the clock, so header and filename can never diverge.
    out.push_str(&format!(
        "# Convention-drift scan \u{2014} {} \u{2014} {}\n",
        table.root.display(),
        table.generated,
    ));
    out.push('\n');
    // Legend + Columns block — middle-dot U+00B7 separators, verbatim from the seed.
    out.push_str("Legend: \u{2713} present \u{00b7} \u{2717} missing \u{00b7} ~ partial.\n");
    out.push_str(
        "Columns: CLAUDE=CLAUDE.md \u{00b7} MK=Makefile/justfile w/ ci target \u{00b7} \
         PC=pre-commit \u{00b7} CI=#workflows \u{00b7}\n",
    );
    out.push_str(
        "LIC=LICENSE \u{00b7} TOOL=toolchain pin \u{00b7} SR=signed-release config \u{00b7} \
         CHG=CHANGELOG.\n",
    );
    out.push('\n');
    // Table header + separator.
    out.push_str("| Repo | lang | CLAUDE | MK | PC | CI | LIC | TOOL | SR | CHG |\n");
    out.push_str("|---|---|---|---|---|---|---|---|---|---|\n");
    if table.rows.is_empty() {
        out.push_str("_No repos found._\n");
    } else {
        for r in &table.rows {
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
                r.name,
                r.lang,
                r.claude.glyph(),
                r.mk.glyph(),
                r.pc.glyph(),
                r.ci,
                r.lic.glyph(),
                r.tool.glyph(),
                r.sr.glyph(),
                r.chg.glyph(),
            ));
        }
    }
    out.push('\n');
    // Always a Skipped section.
    out.push_str("## Skipped repos\n\n");
    if table.skipped.is_empty() {
        out.push_str("None.\n");
    } else {
        for s in &table.skipped {
            out.push_str(&format!("- {}: {}\n", s.name, s.reason));
        }
    }
    out
}

const META_MARK_START: &str = "<!-- cicatrix:meta-patterns:start -->";
const META_MARK_END: &str = "<!-- cicatrix:meta-patterns:end -->";
const CLAUDE_MD: &str = "CLAUDE.md";

/// Is `candidate` the same path as, or nested under, `dir`? Compared lexically (no fs touch) so
/// it works for not-yet-existing paths; tolerant of `./` and trailing slashes via component walk.
fn path_is_under(candidate: &Path, dir: &Path) -> bool {
    use std::path::Component;
    let norm = |p: &Path| -> Vec<std::ffi::OsString> {
        p.components()
            .filter_map(|c| match c {
                Component::Normal(s) => Some(s.to_os_string()),
                _ => None,
            })
            .collect()
    };
    let (cand, base) = (norm(candidate), norm(dir));
    if base.is_empty() || cand.len() < base.len() {
        return false;
    }
    cand[..base.len()] == base[..]
}

/// `project-meta [--apply]` — regenerate the delimited CLAUDE.md meta-pattern block from grounded
/// facts, print a unified diff vs the current block, and (only with `--apply`) write CLAUDE.md.
/// Default: print diff, write nothing, exit 0. Never silently mutates CLAUDE.md.
fn cmd_project_meta(rest: &[String]) -> ExitCode {
    let mut apply = false;
    for a in rest {
        match a.as_str() {
            "--apply" => apply = true,
            other => {
                eprintln!("cicatrix project-meta: unknown argument {other}");
                return ExitCode::FAILURE;
            }
        }
    }

    let facts = match corpus::read_facts(corpus::Tier::Grounded) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("cicatrix project-meta: {e}");
            return ExitCode::FAILURE;
        }
    };
    let new_block = format!(
        "{META_MARK_START}\n{}{META_MARK_END}\n",
        store::render_meta_patterns(&facts, None)
    );

    let current = match std::fs::read_to_string(CLAUDE_MD) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cicatrix project-meta: {CLAUDE_MD}: {e}");
            return ExitCode::FAILURE;
        }
    };

    let (old_block, updated) = replace_marker_block(&current, &new_block);
    print!("{}", unified_diff(&old_block, &new_block, CLAUDE_MD));

    if !apply {
        return ExitCode::SUCCESS;
    }
    if updated == current {
        println!("cicatrix project-meta: CLAUDE.md already up to date");
        return ExitCode::SUCCESS;
    }
    match std::fs::write(CLAUDE_MD, updated) {
        Ok(()) => {
            println!("cicatrix project-meta: wrote {CLAUDE_MD}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("cicatrix project-meta: write {CLAUDE_MD}: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Return `(old_block, full_text_with_new_block)`. If the delimited block exists, swap its
/// contents in place; otherwise the old block is empty and the new block is appended (so the diff
/// shows a pure insertion and `--apply` never clobbers unrelated CLAUDE.md content).
fn replace_marker_block(text: &str, new_block: &str) -> (String, String) {
    if let (Some(s), Some(e)) = (text.find(META_MARK_START), text.find(META_MARK_END)) {
        let end = e + META_MARK_END.len();
        // extend through the trailing newline so the block round-trips cleanly
        let end = if text[end..].starts_with('\n') {
            end + 1
        } else {
            end
        };
        let old_block = text[s..end].to_string();
        let updated = format!("{}{new_block}{}", &text[..s], &text[end..]);
        (old_block, updated)
    } else {
        let sep = if text.ends_with('\n') || text.is_empty() {
            ""
        } else {
            "\n"
        };
        (String::new(), format!("{text}{sep}{new_block}"))
    }
}

/// Minimal unified diff of two blocks (whole-block replacement; no LCS). Empty if identical.
fn unified_diff(old: &str, new: &str, label: &str) -> String {
    if old == new {
        return String::new();
    }
    let mut out = format!("--- a/{label}\n+++ b/{label}\n");
    for line in old.lines() {
        out.push_str(&format!("-{line}\n"));
    }
    for line in new.lines() {
        out.push_str(&format!("+{line}\n"));
    }
    out
}

#[cfg(test)]
mod render_tests {
    use super::*;
    use drift::{Lang, Mark, MarkerTable, RepoRow, RepoSkip};
    use std::path::PathBuf;

    fn row(name: &str) -> RepoRow {
        RepoRow {
            name: name.into(),
            lang: Lang::Rust,
            claude: Mark::Present,
            mk: Mark::Partial,
            pc: Mark::Missing,
            ci: 0,
            lic: Mark::Present,
            tool: Mark::Missing,
            sr: Mark::Partial,
            chg: Mark::Present,
        }
    }

    /// Structure invariants: em-dash header, middle-dot legend separators, the CI=0 digit (never a
    /// glyph), an always-present Skipped section, and exactly one trailing newline.
    #[test]
    fn render_structure_and_separators() {
        let table = MarkerTable {
            generated: "2026-06-16".into(),
            root: PathBuf::from("~/projects"),
            rows: vec![row("reverie")],
            skipped: vec![RepoSkip {
                name: "ghost".into(),
                reason: "not a directory: ~/projects/ghost".into(),
            }],
        };
        let s = render_drift_table(&table);
        // header em-dash U+2014, space-padded
        assert!(
            s.starts_with("# Convention-drift scan \u{2014} ~/projects \u{2014} 2026-06-16\n"),
            "header: {:?}",
            s.lines().next()
        );
        // legend middle-dot U+00B7
        assert!(
            s.contains("Legend: \u{2713} present \u{00b7} \u{2717} missing \u{00b7} ~ partial.\n")
        );
        // columns block both lines
        assert!(s.contains("Columns: CLAUDE=CLAUDE.md \u{00b7}"));
        assert!(s.contains("LIC=LICENSE \u{00b7} TOOL=toolchain pin \u{00b7}"));
        // CI=0 renders the digit, NOT the missing glyph
        assert!(s.contains("| reverie | rust | \u{2713} | ~ | \u{2717} | 0 | \u{2713} | \u{2717} | ~ | \u{2713} |\n"));
        // Skipped section present with the skip line
        assert!(s.contains("## Skipped repos\n\n- ghost: not a directory: ~/projects/ghost\n"));
        // exactly one trailing newline
        assert!(s.ends_with('\n') && !s.ends_with("\n\n"));
    }

    /// No skips → the literal `None.` line; the section is ALWAYS emitted.
    #[test]
    fn render_skipped_none_when_empty() {
        let table = MarkerTable {
            generated: "2026-06-16".into(),
            root: PathBuf::from("~/projects"),
            rows: vec![row("a")],
            skipped: vec![],
        };
        let s = render_drift_table(&table);
        assert!(s.contains("## Skipped repos\n\nNone.\n"), "{s}");
        assert!(s.ends_with("None.\n"));
    }

    /// Empty scan → header+legend+columns+table-header, then `_No repos found._`, then Skipped.
    #[test]
    fn render_empty_scan_body() {
        let table = MarkerTable {
            generated: "2026-06-16".into(),
            root: PathBuf::from("~/projects"),
            rows: vec![],
            skipped: vec![],
        };
        let s = render_drift_table(&table);
        assert!(
            s.contains("|---|---|---|---|---|---|---|---|---|---|\n_No repos found._\n"),
            "{s}"
        );
        // no data rows, but the Skipped section still appears
        assert!(s.contains("## Skipped repos\n\nNone.\n"));
        assert!(s.ends_with('\n') && !s.ends_with("\n\n"));
    }

    /// The renderer does NOT sort — rows render in table order as given.
    #[test]
    fn render_preserves_row_order() {
        let table = MarkerTable {
            generated: "2026-06-16".into(),
            root: PathBuf::from("~/projects"),
            rows: vec![row("zeta"), row("alpha")],
            skipped: vec![],
        };
        let s = render_drift_table(&table);
        let zeta = s.find("| zeta |").unwrap();
        let alpha = s.find("| alpha |").unwrap();
        assert!(zeta < alpha, "renderer must not reorder rows");
    }
}
