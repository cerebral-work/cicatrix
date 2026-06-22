//! cicatrix — regression-memory + convention-drift CLI.
mod bug_md;
mod corpus;
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
        "drift" => {
            println!("drift/convention-drift-2026-06-16.md");
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!(
                "usage: cicatrix <inject [--target <path>] | record [<BUG_*.md>...] | \
                 query <changed-file>... [--as-of <commit>] | project-meta [--apply] | drift>"
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
