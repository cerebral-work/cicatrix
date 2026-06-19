//! cicatrix — regression-memory + convention-drift CLI.
#![allow(dead_code)]
mod bug_md;
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
        "inject" => {
            print!("{}", store::meta_patterns());
            ExitCode::SUCCESS
        }
        // project fixed-bug fact(s) from the markdown corpus into reverie (project=cicatrix)
        "record" => cmd_record(&args[1..]),
        // "does this diff touch a known-bug surface?" — query reverie, optionally as-of a commit
        "query" => cmd_query(&args[1..]),
        "drift" => {
            println!("drift/convention-drift-2026-06-16.md");
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!(
                "usage: cicatrix <inject | record [<BUG_*.md>...] | \
                 query <changed-file>... [--as-of <commit>] | drift>"
            );
            ExitCode::FAILURE
        }
    }
}

/// `record [<BUG_*.md>...]` — project the given bug-docs (or the whole corpus, if none given) into
/// reverie. The markdown is the source of truth; this writes the regenerable projection.
fn cmd_record(rest: &[String]) -> ExitCode {
    let paths: Vec<&String> = rest.iter().filter(|a| !a.starts_with("--")).collect();
    let facts = if paths.is_empty() {
        bug_md::parse_dir(&reverie::corpus_dir())
    } else {
        paths.iter().map(|p| bug_md::parse_file(Path::new(p.as_str()))).collect()
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
