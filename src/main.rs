//! cicatrix — regression-memory + convention-drift CLI.
pub mod autonomy;
pub mod branch;
mod bug_md;
pub mod context;
mod corpus;
mod drift;
pub mod frontier;
mod gitf;
pub mod hooks;
pub mod masking;
pub mod mcp;
mod reverie;
pub mod reversibility;
pub mod store;
pub mod tripwire;
pub mod workflow;

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
        // branch snapshot & forking interface for agent worktrees (CER-2755)
        "branch" => cmd_branch(&args[1..]),
        // regenerate the CLAUDE.md meta-pattern block from grounded facts; diff (or --apply write)
        "project-meta" => cmd_project_meta(&args[1..]),
        // print newest scan path (bare) or regenerate the convention-drift table (`drift scan`)
        "drift" => cmd_drift(&args[1..]),
        // Autumn Harvest durable workflow engine integration (CER-2756, Phase 2.1)
        "workflow" => cmd_workflow(&args[1..]),
        // Model Context Protocol (MCP) server interface (CER-2758, Phase 2.3)
        "mcp" => cmd_mcp(&args[1..]),
        // Soma run-context assembly integration (CER-2763, Phase 4.2)
        "context" => cmd_context(&args[1..]),
        // Wheelhorse reversibility action pipeline (CER-2759, Phase 3.1)
        "reversibility" => cmd_reversibility(&args[1..]),
        // Synthetic canary tripwire registry & intrusion guard (CER-2760, Phase 3.2)
        "tripwire" => cmd_tripwire(&args[1..]),
        // Earned autonomy trust ladder & promotion ledger (CER-2762, Phase 4.1)
        "autonomy" => cmd_autonomy(&args[1..]),
        // Streaming HTTP server for cluster runners
        "serve" => cmd_serve(&args[1..]),
        _ => {
            eprintln!(
                "usage: cicatrix <inject [--target <path>] | record [<BUG_*.md>...] [--branch <id>] | \
                 query <changed-file>... [--as-of <commit>] [--frontier <vector>] [--branch <id>] [--format <human|soma-block|json>] [--limit <N>] | \
                 branch <fork <id> [--from <base>] [--frontier <vec>] | drop <id> | settle <id> | list | path <id>> | \
                 project-meta [--apply] | drift [scan [--repo <path>]] | \
                 workflow <run <triage|audit|review-gate> | signal <id> <verdict> | list | status <id>> | \
                 context <assemble --prompt <p> <paths...> [--limit <N>] [--as-of <c>] [--frontier <f>] [--branch <b>] [--actor <a>] [--fail-soft <bool>] [--json]> | \
                 reversibility <eval|classify|plan|validate> [--diff <path>] [--tier <shadow|supervised|autonomous>] [--json] | \
                 tripwire <list [--json] | check <target> [--content <text>] [--actor <actor>] [--action <action>] [--json] | touches [--limit <N>] [--canary <id>] [--json] | seed> | \
                 autonomy <status [--actor <actor>] [--capability <capability>] [--json] | promote --actor <actor> --capability <capability> --to <tier> --reason <reason> --authorized-by <user> [--evidence <json>] [--json] | demote --actor <actor> --capability <capability> --to <tier> --reason <reason> --authorized-by <user> [--evidence <json>] [--json] | history [--actor <actor>] [--capability <capability>] [--limit <N>] [--json] | check --actor <actor> --capability <capability> --tier <tier> [--json]> | \
                 mcp [--stdio | --http [<bind>]] [--bind <bind>] | serve [--mcp] [--bind <bind>]>"
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
fn open_branch_store(branch_id: &str) -> Result<store::SqliteStore, branch::BranchError> {
    branch::validate_snapshot_id(branch_id)?;
    let trunk_store = store::SqliteStore::open_default()?;
    let manager = branch::BranchManager::new(trunk_store)?;
    let path = manager.branch_path(branch_id)?;
    let branch_store = store::SqliteStore::open(path)?;
    Ok(branch_store)
}

/// `record [<BUG_*.md>...] [--branch <id>]` — project the given bug-docs (or the whole grounded corpus, if none
/// given) into reverie (or isolated branch database if `--branch` or `CICATRIX_BRANCH` is set).
/// Only grounded facts may be projected: an explicit path under the observed tier is refused before
/// anything is projected (observed facts are ungrounded). The markdown is the source of truth;
/// this writes the regenerable projection.
fn cmd_record(rest: &[String]) -> ExitCode {
    let mut paths = Vec::new();
    let mut branch_arg: Option<String> = None;
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--branch" => match it.next() {
                Some(b) => branch_arg = Some(b.clone()),
                None => {
                    eprintln!("cicatrix record: --branch needs a <snapshot_id>");
                    return ExitCode::FAILURE;
                }
            },
            flag if flag.starts_with("--") => {
                eprintln!("cicatrix record: unknown flag {flag}");
                return ExitCode::FAILURE;
            }
            path => paths.push(path.to_string()),
        }
    }

    let branch = branch_arg.or_else(|| {
        std::env::var("CICATRIX_BRANCH")
            .ok()
            .map(|b| b.trim().to_string())
            .filter(|b| !b.is_empty())
    });

    // Poison-the-well gate: refuse any explicit input under the observed (ungrounded) tier
    // BEFORE projecting anything.
    let observed_dir = corpus::resolve_dir(corpus::Tier::Observed);
    for p in &paths {
        if path_is_under(Path::new(p), &observed_dir) {
            eprintln!("observed facts are ungrounded; promote to grounded first");
            return ExitCode::FAILURE;
        }
    }

    let facts = if paths.is_empty() {
        bug_md::parse_dir(&reverie::corpus_dir())
    } else {
        paths
            .iter()
            .map(|p| bug_md::parse_file(Path::new(p)))
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

    if let Some(branch_id) = branch {
        let mut store = match open_branch_store(&branch_id) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("cicatrix record: failed to open branch database `{branch_id}`: {e}");
                return ExitCode::FAILURE;
            }
        };
        let mut recorded = 0usize;
        for f in &facts {
            match store.record(f) {
                Ok(()) => {
                    println!("recorded {} → branch {} (sqlite)", f.id, branch_id);
                    recorded += 1;
                }
                Err(e) => {
                    let masked =
                        crate::masking::MaskedError::internal(format!("{} failed: {e}", f.id));
                    eprintln!("{}", masked.to_cli_string("cicatrix record"));
                }
            }
        }
        if recorded == facts.len() {
            ExitCode::SUCCESS
        } else {
            eprintln!(
                "cicatrix record: {recorded}/{} recorded to branch",
                facts.len()
            );
            ExitCode::FAILURE
        }
    } else {
        let mut store = match store::SqliteStore::from_env() {
            Ok(s) => s,
            Err(e) => {
                eprintln!("cicatrix record: failed to initialize sqlite database: {e}");
                return ExitCode::FAILURE;
            }
        };
        let mut recorded = 0usize;
        for f in &facts {
            match store.record(f) {
                Ok(()) => {
                    if store.has_reverie() {
                        println!("recorded {} → reverie (project=cicatrix)", f.id);
                    } else {
                        println!("recorded {} → sqlite (project=cicatrix)", f.id);
                    }
                    recorded += 1;
                }
                Err(e) => {
                    let masked =
                        crate::masking::MaskedError::internal(format!("{} failed: {e}", f.id));
                    eprintln!("{}", masked.to_cli_string("cicatrix record"));
                }
            }
        }
        if recorded == facts.len() {
            ExitCode::SUCCESS
        } else {
            eprintln!("cicatrix record: {recorded}/{} projected", facts.len());
            ExitCode::FAILURE
        }
    }
}

/// `query <changed-file>... [--as-of <commit>] [--frontier <vector>] [--branch <id>] [--actor <name>] [--format <human|soma-block|json>] [--limit <N>]` —
/// ask reverie (or branch database) which known-bug surfaces the changed files touch;
/// with `--as-of`, keep only bugs fixed at or before `<commit>` (git-ancestry);
/// with `--frontier`, keep only bugs causally dominated by the given version-vector frontier cut;
/// with `--format`, format output as human (default), soma-block, or json;
/// with `--limit`, truncate output facts to at most N entries.
fn cmd_query(rest: &[String]) -> ExitCode {
    let mut files = Vec::new();
    let mut as_of: Option<String> = None;
    let mut frontier_arg: Option<String> = None;
    let mut branch_arg: Option<String> = None;
    let mut actor_arg: Option<String> = None;
    let mut format_arg: Option<String> = None;
    let mut limit_arg: Option<usize> = None;
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
            "--frontier" => match it.next() {
                Some(f) => frontier_arg = Some(f.clone()),
                None => {
                    eprintln!("cicatrix query: --frontier needs a <vector>");
                    return ExitCode::FAILURE;
                }
            },
            "--branch" => match it.next() {
                Some(b) => branch_arg = Some(b.clone()),
                None => {
                    eprintln!("cicatrix query: --branch needs a <snapshot_id>");
                    return ExitCode::FAILURE;
                }
            },
            "--actor" => match it.next() {
                Some(act) => actor_arg = Some(act.clone()),
                None => {
                    eprintln!("cicatrix query: --actor needs a <name>");
                    return ExitCode::FAILURE;
                }
            },
            "--format" => match it.next() {
                Some(fmt) => match fmt.as_str() {
                    "human" | "soma-block" | "json" => format_arg = Some(fmt.clone()),
                    other => {
                        eprintln!(
                            "cicatrix query: invalid --format `{other}` (expected human, soma-block, or json)"
                        );
                        return ExitCode::FAILURE;
                    }
                },
                None => {
                    eprintln!(
                        "cicatrix query: --format needs an argument (human, soma-block, json)"
                    );
                    return ExitCode::FAILURE;
                }
            },
            "--limit" => match it.next() {
                Some(lim) => match lim.parse::<usize>() {
                    Ok(val) => limit_arg = Some(val),
                    Err(e) => {
                        eprintln!("cicatrix query: invalid --limit `{lim}`: {e}");
                        return ExitCode::FAILURE;
                    }
                },
                None => {
                    eprintln!("cicatrix query: --limit needs an integer");
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
        eprintln!(
            "usage: cicatrix query <changed-file>... [--as-of <commit>] [--frontier <vector>] [--branch <id>] [--actor <name>] [--format <human|soma-block|json>] [--limit <N>]"
        );
        return ExitCode::FAILURE;
    }

    let actor = actor_arg
        .or_else(|| {
            std::env::var("CICATRIX_ACTOR")
                .ok()
                .map(|a| a.trim().to_string())
                .filter(|a| !a.is_empty())
        })
        .unwrap_or_else(|| "agent".to_string());

    let branch = branch_arg.or_else(|| {
        std::env::var("CICATRIX_BRANCH")
            .ok()
            .map(|b| b.trim().to_string())
            .filter(|b| !b.is_empty())
    });

    let parsed_frontier = if let Some(f_str) = &frontier_arg {
        match store::Frontier::parse(f_str) {
            Ok(f) => Some(f),
            Err(e) => {
                eprintln!("cicatrix query: invalid --frontier `{f_str}`: {e}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        None
    };

    let mut hits = if let Some(branch_id) = branch {
        let branch_store = match open_branch_store(&branch_id) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("cicatrix query: failed to open branch database `{branch_id}`: {e}");
                return ExitCode::FAILURE;
            }
        };
        // Fail-closed canary tripwire check (CER-2760, Phase 3.2)
        if let Err(e) = branch_store.guard_paths(&files, &actor, "query") {
            eprintln!("cicatrix query: tripwire intrusion detected: {e}");
            return ExitCode::FAILURE;
        }
        match branch_store.touches_known_bug(&files) {
            Ok(h) => h,
            Err(e) => {
                let masked = crate::masking::MaskedError::internal(format!("{e}"));
                eprintln!("{}", masked.to_cli_string("cicatrix query"));
                return ExitCode::FAILURE;
            }
        }
    } else {
        let store = match store::SqliteStore::from_env() {
            Ok(s) => s,
            Err(e) => {
                let masked = crate::masking::MaskedError::internal(format!(
                    "failed to open sqlite database for tripwire check: {e}"
                ));
                eprintln!("{}", masked.to_cli_string("cicatrix query"));
                return ExitCode::FAILURE;
            }
        };
        // Fail-closed canary tripwire check (CER-2760, Phase 3.2)
        if let Err(e) = store.guard_paths(&files, &actor, "query") {
            eprintln!("cicatrix query: tripwire intrusion detected: {e}");
            return ExitCode::FAILURE;
        }
        if store.has_reverie() {
            let bridge = reverie::ReverieBridge::from_env();
            match bridge.touches_known_bug(&files) {
                Ok(h) => h,
                Err(e) => {
                    let masked = crate::masking::MaskedError::internal(format!("{e}"));
                    eprintln!("{}", masked.to_cli_string("cicatrix query"));
                    return ExitCode::FAILURE;
                }
            }
        } else {
            match store.touches_known_bug(&files) {
                Ok(h) => h,
                Err(e) => {
                    let masked = crate::masking::MaskedError::internal(format!("{e}"));
                    eprintln!("{}", masked.to_cli_string("cicatrix query"));
                    return ExitCode::FAILURE;
                }
            }
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

    if let Some(frontier) = &parsed_frontier {
        let (kept, skipped) = gitf::filter_by_frontier(hits, frontier);
        hits = kept;
        if !skipped.is_empty() {
            eprintln!(
                "cicatrix query: --frontier excluded {} fact(s) not dominated by frontier: {}",
                skipped.len(),
                skipped.join(", ")
            );
        }
    }

    if let Some(limit) = limit_arg {
        hits.truncate(limit);
    }

    let format = format_arg.as_deref().unwrap_or("human");
    match format {
        "json" => match serde_json::to_string_pretty(&hits) {
            Ok(s) => {
                println!("{s}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("cicatrix query: failed to serialize json: {e}");
                ExitCode::FAILURE
            }
        },
        "soma-block" => {
            let limit = limit_arg.unwrap_or(context::DEFAULT_LIMIT);
            if let Some(block) = context::render_known_bugs_block(&hits, limit) {
                println!("{block}");
            }
            ExitCode::SUCCESS
        }
        "human" => {
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
        _ => unreachable!(),
    }
}

/// `context assemble --prompt <p> <paths...> [--limit <N>] [--as-of <c>] [--frontier <f>] [--branch <b>] [--actor <a>] [--fail-soft <bool>] [--json]` —
/// Assemble a task prompt by prepending a <known-bugs> regression block for touched files,
/// following the Soma run-context assembly specification (CER-2611).
fn cmd_context(rest: &[String]) -> ExitCode {
    let sub = match rest.first().map(String::as_str) {
        Some(s) => s,
        None => {
            eprintln!(
                "usage: cicatrix context assemble --prompt <p> <paths...> [--limit <N>] [--as-of <c>] [--frontier <f>] [--branch <b>] [--actor <a>] [--fail-soft <bool>] [--json]"
            );
            return ExitCode::FAILURE;
        }
    };

    match sub {
        "assemble" => {
            let mut prompt_opt: Option<String> = None;
            let mut paths = Vec::new();
            let mut limit: Option<usize> = None;
            let mut as_of: Option<String> = None;
            let mut frontier: Option<String> = None;
            let mut branch: Option<String> = None;
            let mut actor: Option<String> = None;
            let mut fail_soft = true;
            let mut json_output = false;

            let mut it = rest[1..].iter().peekable();
            while let Some(arg) = it.next() {
                match arg.as_str() {
                    "--prompt" => match it.next() {
                        Some(p) => prompt_opt = Some(p.clone()),
                        None => {
                            eprintln!("cicatrix context assemble: --prompt requires an argument");
                            return ExitCode::FAILURE;
                        }
                    },
                    "--limit" => match it.next() {
                        Some(l) => match l.parse::<usize>() {
                            Ok(val) => limit = Some(val),
                            Err(e) => {
                                eprintln!("cicatrix context assemble: invalid --limit `{l}`: {e}");
                                return ExitCode::FAILURE;
                            }
                        },
                        None => {
                            eprintln!("cicatrix context assemble: --limit requires an integer");
                            return ExitCode::FAILURE;
                        }
                    },
                    "--as-of" => match it.next() {
                        Some(c) => as_of = Some(c.clone()),
                        None => {
                            eprintln!("cicatrix context assemble: --as-of requires a commit SHA");
                            return ExitCode::FAILURE;
                        }
                    },
                    "--frontier" => match it.next() {
                        Some(f) => frontier = Some(f.clone()),
                        None => {
                            eprintln!("cicatrix context assemble: --frontier requires a vector");
                            return ExitCode::FAILURE;
                        }
                    },
                    "--branch" => match it.next() {
                        Some(b) => branch = Some(b.clone()),
                        None => {
                            eprintln!("cicatrix context assemble: --branch requires an ID");
                            return ExitCode::FAILURE;
                        }
                    },
                    "--actor" => match it.next() {
                        Some(a) => actor = Some(a.clone()),
                        None => {
                            eprintln!("cicatrix context assemble: --actor requires a name");
                            return ExitCode::FAILURE;
                        }
                    },
                    "--fail-soft" => {
                        if let Some(next) = it.peek() {
                            if *next == "true" {
                                fail_soft = true;
                                it.next();
                            } else if *next == "false" {
                                fail_soft = false;
                                it.next();
                            } else {
                                fail_soft = true;
                            }
                        } else {
                            fail_soft = true;
                        }
                    }
                    "--no-fail-soft" => {
                        fail_soft = false;
                    }
                    "--json" => {
                        json_output = true;
                    }
                    "--path" => match it.next() {
                        Some(p) => paths.push(p.clone()),
                        None => {
                            eprintln!("cicatrix context assemble: --path requires a file path");
                            return ExitCode::FAILURE;
                        }
                    },
                    flag if flag.starts_with("--") => {
                        eprintln!("cicatrix context assemble: unknown flag `{flag}`");
                        return ExitCode::FAILURE;
                    }
                    path => {
                        paths.push(path.to_string());
                    }
                }
            }

            let prompt = match prompt_opt {
                Some(p) => p,
                None => {
                    eprintln!(
                        "cicatrix context assemble: missing required argument `--prompt <text>`"
                    );
                    return ExitCode::FAILURE;
                }
            };

            let mut opts = context::AssembleOptions::new(prompt, paths).with_fail_soft(fail_soft);
            if let Some(l) = limit {
                opts = opts.with_limit(l);
            }
            if let Some(a) = as_of {
                opts = opts.with_as_of(a);
            }
            if let Some(f) = frontier {
                opts = opts.with_frontier(f);
            }
            if let Some(b) = branch {
                opts = opts.with_branch(b);
            }
            if let Some(act) = actor {
                opts = opts.with_actor(act);
            }

            match context::assemble_context(&opts) {
                Ok(res) => {
                    if json_output {
                        match serde_json::to_string_pretty(&res) {
                            Ok(s) => println!("{s}"),
                            Err(e) => {
                                eprintln!(
                                    "cicatrix context assemble: JSON serialization failed: {e}"
                                );
                                return ExitCode::FAILURE;
                            }
                        }
                    } else {
                        println!("{}", res.prompt);
                    }
                    ExitCode::SUCCESS
                }
                Err(context::ContextError::TripwireIntrusion(msg)) => {
                    eprintln!("cicatrix context assemble: tripwire intrusion detected: {msg}");
                    ExitCode::FAILURE
                }
                Err(context::ContextError::InvalidArgument(msg)) => {
                    eprintln!("cicatrix context assemble: invalid argument: {msg}");
                    ExitCode::FAILURE
                }
                Err(context::ContextError::Database(msg)) => {
                    let masked = crate::masking::MaskedError::internal(msg);
                    eprintln!("{}", masked.to_cli_string("cicatrix context assemble"));
                    ExitCode::FAILURE
                }
            }
        }
        other => {
            eprintln!("cicatrix context: unknown subcommand `{other}`");
            eprintln!(
                "usage: cicatrix context assemble --prompt <p> <paths...> [--limit <N>] [--as-of <c>] [--frontier <f>] [--branch <b>] [--actor <a>] [--fail-soft <bool>] [--json]"
            );
            ExitCode::FAILURE
        }
    }
}

/// `branch <fork <id> [--from <base>] [--frontier <vec>] | drop <id> | settle <id> | list | path <id>>` —
/// snapshot and forking engine for agent worktrees and DeltaDB virtual threads.
fn cmd_branch(rest: &[String]) -> ExitCode {
    let sub = match rest.first().map(String::as_str) {
        Some(s) => s,
        None => {
            eprintln!(
                "usage: cicatrix branch <fork <id> [--from <base>] [--frontier <vec>] | drop <id> | settle <id> | list | path <id>>"
            );
            return ExitCode::FAILURE;
        }
    };

    let trunk_store = match store::SqliteStore::from_env() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cicatrix branch: failed to open primary database: {e}");
            return ExitCode::FAILURE;
        }
    };

    let mut manager = match branch::BranchManager::new(trunk_store) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("cicatrix branch: failed to initialize branch manager: {e}");
            return ExitCode::FAILURE;
        }
    };

    match sub {
        "fork" => {
            let mut id: Option<String> = None;
            let mut from: Option<String> = None;
            let mut frontier: Option<String> = None;
            let mut it = rest[1..].iter();
            while let Some(arg) = it.next() {
                match arg.as_str() {
                    "--from" => match it.next() {
                        Some(f) => from = Some(f.clone()),
                        None => {
                            eprintln!("cicatrix branch fork: --from needs a <base>");
                            return ExitCode::FAILURE;
                        }
                    },
                    "--frontier" => match it.next() {
                        Some(f) => frontier = Some(f.clone()),
                        None => {
                            eprintln!("cicatrix branch fork: --frontier needs a <vector>");
                            return ExitCode::FAILURE;
                        }
                    },
                    flag if flag.starts_with("--") => {
                        eprintln!("cicatrix branch fork: unknown flag {flag}");
                        return ExitCode::FAILURE;
                    }
                    val => {
                        if id.is_none() {
                            id = Some(val.to_string());
                        } else {
                            eprintln!("cicatrix branch fork: unexpected extra argument `{val}`");
                            return ExitCode::FAILURE;
                        }
                    }
                }
            }
            let snapshot_id = match id {
                Some(i) => i,
                None => {
                    eprintln!(
                        "usage: cicatrix branch fork <snapshot_id> [--from <base>] [--frontier <vector>]"
                    );
                    return ExitCode::FAILURE;
                }
            };

            match manager.fork(&snapshot_id, from.as_deref(), frontier.as_deref()) {
                Ok(snapshot) => {
                    println!(
                        "forked branch snapshot `{}` ({})",
                        snapshot.snapshot_id,
                        snapshot.db_path.display()
                    );
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("cicatrix branch fork: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        "drop" => {
            if rest.len() != 2 {
                eprintln!("usage: cicatrix branch drop <snapshot_id>");
                return ExitCode::FAILURE;
            }
            let snapshot_id = &rest[1];
            match manager.drop_snapshot(snapshot_id) {
                Ok(path) => {
                    println!(
                        "dropped branch snapshot `{}` ({})",
                        snapshot_id,
                        path.display()
                    );
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("cicatrix branch drop: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        "settle" => {
            if rest.len() != 2 {
                eprintln!("usage: cicatrix branch settle <snapshot_id>");
                return ExitCode::FAILURE;
            }
            let snapshot_id = &rest[1];
            match manager.settle(snapshot_id) {
                Ok(report) => {
                    println!(
                        "settled branch snapshot `{}`: merged {} fact(s)",
                        report.snapshot_id, report.merged_facts
                    );
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("cicatrix branch settle: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        "list" => {
            if rest.len() > 1 {
                eprintln!("usage: cicatrix branch list");
                return ExitCode::FAILURE;
            }
            match manager.list() {
                Ok(snapshots) => {
                    if snapshots.is_empty() {
                        println!("no active branch snapshots");
                    } else {
                        for snap in snapshots {
                            if let Some(f) = &snap.base_frontier {
                                println!(
                                    "{}\t{}\t{}\t{}",
                                    snap.snapshot_id,
                                    snap.created_at,
                                    f,
                                    snap.db_path.display()
                                );
                            } else {
                                println!(
                                    "{}\t{}\t{}",
                                    snap.snapshot_id,
                                    snap.created_at,
                                    snap.db_path.display()
                                );
                            }
                        }
                    }
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("cicatrix branch list: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        "path" => {
            if rest.len() != 2 {
                eprintln!("usage: cicatrix branch path <snapshot_id>");
                return ExitCode::FAILURE;
            }
            let snapshot_id = &rest[1];
            match manager.branch_path(snapshot_id) {
                Ok(path) => {
                    println!("{}", path.display());
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("cicatrix branch path: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        other => {
            eprintln!("cicatrix branch: unknown subcommand `{other}`");
            eprintln!(
                "usage: cicatrix branch <fork <id> [--from <base>] [--frontier <vec>] | drop <id> | settle <id> | list | path <id>>"
            );
            ExitCode::FAILURE
        }
    }
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

/// Dispatch workflow subcommands for the Autumn Harvest engine (CER-2756).
fn cmd_workflow(rest: &[String]) -> ExitCode {
    match rest.first().map(String::as_str) {
        Some("run") => cmd_workflow_run(&rest[1..]),
        Some("signal") => cmd_workflow_signal(&rest[1..]),
        Some("list") => cmd_workflow_list(&rest[1..]),
        Some("status") => cmd_workflow_status(&rest[1..]),
        Some(other) => {
            eprintln!("cicatrix workflow: unknown subcommand `{other}`");
            eprintln!("usage: cicatrix workflow <run <triage|audit|review-gate> [options] | signal <id> <verdict> [options] | list [--db <path>] | status <id> [--db <path>]>");
            ExitCode::FAILURE
        }
        None => {
            eprintln!("usage: cicatrix workflow <run <triage|audit|review-gate> [options] | signal <id> <verdict> [options] | list [--db <path>] | status <id> [--db <path>]>");
            ExitCode::FAILURE
        }
    }
}

/// Run a registered workflow by name.
fn cmd_workflow_run(rest: &[String]) -> ExitCode {
    match rest.first().map(String::as_str) {
        Some("triage") => cmd_workflow_run_triage(&rest[1..]),
        Some("audit") => cmd_workflow_run_audit(&rest[1..]),
        Some("review-gate") => cmd_workflow_run_review_gate(&rest[1..]),
        Some(other) => {
            eprintln!("cicatrix workflow run: unknown workflow `{other}`");
            eprintln!(
                "usage: cicatrix workflow run <triage <signature> [options] | audit [options] | review-gate <target_ref> [options]>"
            );
            ExitCode::FAILURE
        }
        None => {
            eprintln!(
                "usage: cicatrix workflow run <triage <signature> [options] | audit [options] | review-gate <target_ref> [options]>"
            );
            ExitCode::FAILURE
        }
    }
}

/// Execute regression triage workflow.
fn cmd_workflow_run_triage(rest: &[String]) -> ExitCode {
    let mut signature: Option<String> = None;
    let mut repo: Option<String> = None;
    let mut candidates = Vec::new();
    let mut failure_log = String::new();
    let mut reproducer_file: Option<String> = None;
    let mut require_operator_review = false;
    let mut db_path_opt: Option<String> = None;

    let mut it = rest.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--repo" => match it.next() {
                Some(r) => repo = Some(r.clone()),
                None => {
                    eprintln!("cicatrix workflow run triage: --repo needs a path");
                    return ExitCode::FAILURE;
                }
            },
            "--candidate" => match it.next() {
                Some(c) => candidates.push(c.clone()),
                None => {
                    eprintln!("cicatrix workflow run triage: --candidate needs a commit sha/ref");
                    return ExitCode::FAILURE;
                }
            },
            "--failure-log" => match it.next() {
                Some(l) => failure_log = l.clone(),
                None => {
                    eprintln!("cicatrix workflow run triage: --failure-log needs text");
                    return ExitCode::FAILURE;
                }
            },
            "--file" => match it.next() {
                Some(f) => reproducer_file = Some(f.clone()),
                None => {
                    eprintln!("cicatrix workflow run triage: --file needs a path");
                    return ExitCode::FAILURE;
                }
            },
            "--require-review" => {
                require_operator_review = true;
            }
            "--db" => match it.next() {
                Some(d) => db_path_opt = Some(d.clone()),
                None => {
                    eprintln!("cicatrix workflow run triage: --db needs a path");
                    return ExitCode::FAILURE;
                }
            },
            flag if flag.starts_with("--") => {
                eprintln!("cicatrix workflow run triage: unknown flag {flag}");
                return ExitCode::FAILURE;
            }
            pos => {
                if signature.is_none() {
                    signature = Some(pos.to_string());
                } else {
                    eprintln!("cicatrix workflow run triage: unexpected extra argument `{pos}`");
                    return ExitCode::FAILURE;
                }
            }
        }
    }

    let test_signature = match signature {
        Some(s) if !s.trim().is_empty() => s,
        _ => {
            eprintln!("usage: cicatrix workflow run triage <signature> [--repo <path>] [--candidate <commit>]... [--failure-log <text>] [--file <path>] [--require-review] [--db <path>]");
            return ExitCode::FAILURE;
        }
    };

    let db_path = match db_path_opt {
        Some(p) => std::path::PathBuf::from(p),
        None => match workflow::default_workflow_db_path() {
            Ok(p) => p,
            Err(e) => {
                eprintln!("cicatrix workflow run triage: cannot resolve workflow db path: {e}");
                return ExitCode::FAILURE;
            }
        },
    };

    let input = workflow::TriageInput {
        test_signature,
        target_repo: repo,
        candidate_commits: candidates,
        failure_log,
        reproducer_file,
        require_operator_review,
    };

    let rt = match tokio::runtime::Runtime::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("cicatrix workflow run triage: failed to initialize async runtime: {e}");
            return ExitCode::FAILURE;
        }
    };

    let result = rt.block_on(async {
        let mut engine = workflow::WorkflowEngine::open(&db_path)?;
        engine
            .run_workflow::<_, workflow::TriageReport>("triage_workflow", input)
            .await
    });

    match result {
        Ok(report) => {
            println!("Workflow execution: {}", report.exec_id);
            println!("State: {}", report.state);
            println!(
                "Events: {} ({} bytes)",
                report.event_count, report.byte_size
            );
            if let Some(output) = report.output {
                println!("Culprit commit: {:?}", output.bisection.culprit_commit);
                if let Some(fact) = output.candidate_bug_fact {
                    println!("Candidate BugFact: {} (files: {:?})", fact.id, fact.files);
                }
                println!("Bisection summary: {}", output.bisection.summary);
                if let Some(ref verdict) = output.operator_verdict {
                    println!(
                        "Operator Verdict: {:?} by {} at {}",
                        verdict.decision, verdict.operator, verdict.timestamp
                    );
                    if let Some(ref comments) = verdict.comments {
                        println!("Comments: {}", comments);
                    }
                }
            }
            if let Some(err) = report.error {
                eprintln!("Workflow error: {err}");
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(e) => {
            eprintln!("cicatrix workflow run triage: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Execute review-gate workflow.
fn cmd_workflow_run_review_gate(rest: &[String]) -> ExitCode {
    let mut target_ref: Option<String> = None;
    let mut description = "Operator review gate".to_string();
    let mut requested_by = std::env::var("USER").unwrap_or_else(|_| "operator".to_string());
    let mut db_path_opt: Option<String> = None;

    let mut it = rest.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--description" => match it.next() {
                Some(d) => description = d.clone(),
                None => {
                    eprintln!("cicatrix workflow run review-gate: --description needs text");
                    return ExitCode::FAILURE;
                }
            },
            "--requested-by" => match it.next() {
                Some(r) => requested_by = r.clone(),
                None => {
                    eprintln!("cicatrix workflow run review-gate: --requested-by needs a name");
                    return ExitCode::FAILURE;
                }
            },
            "--db" => match it.next() {
                Some(d) => db_path_opt = Some(d.clone()),
                None => {
                    eprintln!("cicatrix workflow run review-gate: --db needs a path");
                    return ExitCode::FAILURE;
                }
            },
            flag if flag.starts_with("--") => {
                eprintln!("cicatrix workflow run review-gate: unknown flag {flag}");
                return ExitCode::FAILURE;
            }
            pos => {
                if target_ref.is_none() {
                    target_ref = Some(pos.to_string());
                } else {
                    eprintln!(
                        "cicatrix workflow run review-gate: unexpected extra argument `{pos}`"
                    );
                    return ExitCode::FAILURE;
                }
            }
        }
    }

    let target_ref = match target_ref {
        Some(r) if !r.trim().is_empty() => r,
        _ => {
            eprintln!("usage: cicatrix workflow run review-gate <target_ref> [--description <text>] [--requested-by <name>] [--db <path>]");
            return ExitCode::FAILURE;
        }
    };

    let db_path = match db_path_opt {
        Some(p) => std::path::PathBuf::from(p),
        None => match workflow::default_workflow_db_path() {
            Ok(p) => p,
            Err(e) => {
                eprintln!(
                    "cicatrix workflow run review-gate: cannot resolve workflow db path: {e}"
                );
                return ExitCode::FAILURE;
            }
        },
    };

    let input = workflow::ReviewGateInput {
        target_ref,
        description: Some(description),
        requested_by: Some(requested_by),
    };

    let rt = match tokio::runtime::Runtime::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("cicatrix workflow run review-gate: failed to initialize async runtime: {e}");
            return ExitCode::FAILURE;
        }
    };

    let result = rt.block_on(async {
        let mut engine = workflow::WorkflowEngine::open(&db_path)?;
        engine
            .run_workflow::<_, workflow::ReviewGateReport>("review_gate_workflow", input)
            .await
    });

    match result {
        Ok(report) => {
            println!("Workflow execution: {}", report.exec_id);
            println!("State: {}", report.state);
            println!(
                "Events: {} ({} bytes)",
                report.event_count, report.byte_size
            );
            if let Some(output) = report.output {
                println!("Target Ref:   {}", output.target_ref);
                println!(
                    "Verdict:      {:?} by {} at {}",
                    output.verdict.decision, output.verdict.operator, output.verdict.timestamp
                );
                if let Some(ref comments) = output.verdict.comments {
                    println!("Comments:     {}", comments);
                }
                println!("Status:       {}", output.status);
            }
            if let Some(err) = report.error {
                eprintln!("Workflow error: {err}");
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(e) => {
            eprintln!("cicatrix workflow run review-gate: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Deliver a durable signal to a parked or running workflow execution.
fn cmd_workflow_signal(rest: &[String]) -> ExitCode {
    let mut exec_id: Option<String> = None;
    let mut verdict_str: Option<String> = None;
    let mut idempotency_key_opt: Option<String> = None;
    let mut operator = std::env::var("USER").unwrap_or_else(|_| "operator".to_string());
    let mut comments = "Operator sign-off".to_string();
    let mut db_path_opt: Option<String> = None;

    let mut it = rest.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--idempotency-key" => match it.next() {
                Some(k) => idempotency_key_opt = Some(k.clone()),
                None => {
                    eprintln!("cicatrix workflow signal: --idempotency-key needs a string");
                    return ExitCode::FAILURE;
                }
            },
            "--operator" => match it.next() {
                Some(o) => operator = o.clone(),
                None => {
                    eprintln!("cicatrix workflow signal: --operator needs a name");
                    return ExitCode::FAILURE;
                }
            },
            "--comments" => match it.next() {
                Some(c) => comments = c.clone(),
                None => {
                    eprintln!("cicatrix workflow signal: --comments needs text");
                    return ExitCode::FAILURE;
                }
            },
            "--db" => match it.next() {
                Some(d) => db_path_opt = Some(d.clone()),
                None => {
                    eprintln!("cicatrix workflow signal: --db needs a path");
                    return ExitCode::FAILURE;
                }
            },
            flag if flag.starts_with("--") => {
                eprintln!("cicatrix workflow signal: unknown flag {flag}");
                return ExitCode::FAILURE;
            }
            pos => {
                if exec_id.is_none() {
                    exec_id = Some(pos.to_string());
                } else if verdict_str.is_none() {
                    verdict_str = Some(pos.to_string());
                } else {
                    eprintln!("cicatrix workflow signal: unexpected extra argument `{pos}`");
                    return ExitCode::FAILURE;
                }
            }
        }
    }

    let (id, verdict_input) = match (exec_id, verdict_str) {
        (Some(i), Some(v)) if !i.trim().is_empty() && !v.trim().is_empty() => (i, v),
        _ => {
            eprintln!("usage: cicatrix workflow signal <id> <verdict> [--idempotency-key <key>] [--operator <name>] [--comments <text>] [--db <path>]");
            eprintln!("verdict: approved | rejected | changes_requested");
            return ExitCode::FAILURE;
        }
    };

    let decision = match verdict_input.parse::<workflow::OperatorDecision>() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("cicatrix workflow signal: invalid verdict `{verdict_input}`: {e}");
            eprintln!("allowed verdicts: approved, rejected, changes_requested");
            return ExitCode::FAILURE;
        }
    };

    let verdict = match workflow::OperatorVerdict::new(decision, operator, Some(comments)) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("cicatrix workflow signal: validation failed: {e}");
            return ExitCode::FAILURE;
        }
    };

    let idempotency_key = idempotency_key_opt.unwrap_or_else(|| {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        format!("sig-{ts}")
    });

    let signal = match workflow::DurableSignal::new("operator_verdict", idempotency_key, verdict) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cicatrix workflow signal: invalid signal: {e}");
            return ExitCode::FAILURE;
        }
    };

    let db_path = match db_path_opt {
        Some(p) => std::path::PathBuf::from(p),
        None => match workflow::default_workflow_db_path() {
            Ok(p) => p,
            Err(e) => {
                eprintln!("cicatrix workflow signal: cannot resolve workflow db path: {e}");
                return ExitCode::FAILURE;
            }
        },
    };

    let rt = match tokio::runtime::Runtime::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("cicatrix workflow signal: failed to initialize async runtime: {e}");
            return ExitCode::FAILURE;
        }
    };

    let result = rt.block_on(async {
        let mut engine = workflow::WorkflowEngine::open(&db_path)?;
        engine.deliver_signal(&id, &signal).await
    });

    match result {
        Ok(report) => {
            println!("Signal delivery report:");
            println!("Execution ID:    {}", report.exec_id);
            println!("Signal:          {}", report.signal_name);
            println!("Idempotency Key: {}", report.idempotency_key);
            println!(
                "Status:          {}",
                match report.status {
                    workflow::SignalDeliveryStatus::Delivered => "DELIVERED",
                    workflow::SignalDeliveryStatus::Duplicate => "DUPLICATE",
                }
            );
            println!("Run State:       {}", report.run_state);
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("cicatrix workflow signal: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Execute convention audit workflow.
fn cmd_workflow_run_audit(rest: &[String]) -> ExitCode {
    let mut repo_paths = Vec::new();
    let mut marker: Option<String> = None;
    let mut db_path_opt: Option<String> = None;

    let mut it = rest.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--repo" => match it.next() {
                Some(r) => repo_paths.push(r.clone()),
                None => {
                    eprintln!("cicatrix workflow run audit: --repo needs a path");
                    return ExitCode::FAILURE;
                }
            },
            "--marker" => match it.next() {
                Some(m) => marker = Some(m.clone()),
                None => {
                    eprintln!("cicatrix workflow run audit: --marker needs a string");
                    return ExitCode::FAILURE;
                }
            },
            "--db" => match it.next() {
                Some(d) => db_path_opt = Some(d.clone()),
                None => {
                    eprintln!("cicatrix workflow run audit: --db needs a path");
                    return ExitCode::FAILURE;
                }
            },
            flag if flag.starts_with("--") => {
                eprintln!("cicatrix workflow run audit: unknown flag {flag}");
                return ExitCode::FAILURE;
            }
            pos => {
                eprintln!("cicatrix workflow run audit: unexpected argument `{pos}`");
                return ExitCode::FAILURE;
            }
        }
    }

    let db_path = match db_path_opt {
        Some(p) => std::path::PathBuf::from(p),
        None => match workflow::default_workflow_db_path() {
            Ok(p) => p,
            Err(e) => {
                eprintln!("cicatrix workflow run audit: cannot resolve workflow db path: {e}");
                return ExitCode::FAILURE;
            }
        },
    };

    let input = workflow::AuditInput {
        repo_paths,
        convention_marker: marker,
        scan_labels: Vec::new(),
    };

    let rt = match tokio::runtime::Runtime::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("cicatrix workflow run audit: failed to initialize async runtime: {e}");
            return ExitCode::FAILURE;
        }
    };

    let result = rt.block_on(async {
        let mut engine = workflow::WorkflowEngine::open(&db_path)?;
        engine
            .run_workflow::<_, workflow::AuditReport>("audit_workflow", input)
            .await
    });

    match result {
        Ok(report) => {
            println!("Workflow execution: {}", report.exec_id);
            println!("State: {}", report.state);
            println!(
                "Events: {} ({} bytes)",
                report.event_count, report.byte_size
            );
            if let Some(output) = report.output {
                println!("Scanned targets: {}", output.scanned_targets);
                println!("Drift detected: {}", output.drift_count);
                println!("Summary: {}", output.summary);
                for target_res in output.results {
                    if target_res.drift_detected {
                        println!(
                            " - {}: {} marker(s)",
                            target_res.repo_path,
                            target_res.markers_found.len()
                        );
                    }
                }
            }
            if let Some(err) = report.error {
                eprintln!("Workflow error: {err}");
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(e) => {
            eprintln!("cicatrix workflow run audit: {e}");
            ExitCode::FAILURE
        }
    }
}

/// List workflow executions from the database.
fn cmd_workflow_list(rest: &[String]) -> ExitCode {
    let mut db_path_opt: Option<String> = None;
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--db" => match it.next() {
                Some(d) => db_path_opt = Some(d.clone()),
                None => {
                    eprintln!("cicatrix workflow list: --db needs a path");
                    return ExitCode::FAILURE;
                }
            },
            flag if flag.starts_with("--") => {
                eprintln!("cicatrix workflow list: unknown flag {flag}");
                return ExitCode::FAILURE;
            }
            other => {
                eprintln!("cicatrix workflow list: unexpected argument `{other}`");
                return ExitCode::FAILURE;
            }
        }
    }

    let db_path = match db_path_opt {
        Some(p) => std::path::PathBuf::from(p),
        None => match workflow::default_workflow_db_path() {
            Ok(p) => p,
            Err(e) => {
                eprintln!("cicatrix workflow list: cannot resolve workflow db path: {e}");
                return ExitCode::FAILURE;
            }
        },
    };

    match workflow::list_executions_from_db(&db_path) {
        Ok(list) => {
            if list.is_empty() {
                println!("No workflow executions found in {}", db_path.display());
            } else {
                println!(
                    "{:<36}\t{:<20}\t{:<10}\t{:<8}\t{:<8}",
                    "EXEC_ID", "WORKFLOW", "STATE", "EVENTS", "BYTES"
                );
                for item in list {
                    println!(
                        "{:<36}\t{:<20}\t{:<10}\t{:<8}\t{:<8}",
                        item.exec_id,
                        item.workflow_name,
                        item.state,
                        item.event_count,
                        item.byte_size
                    );
                }
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("cicatrix workflow list: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Inspect detailed workflow execution state from the database.
fn cmd_workflow_status(rest: &[String]) -> ExitCode {
    let mut exec_id: Option<String> = None;
    let mut db_path_opt: Option<String> = None;

    let mut it = rest.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--db" => match it.next() {
                Some(d) => db_path_opt = Some(d.clone()),
                None => {
                    eprintln!("cicatrix workflow status: --db needs a path");
                    return ExitCode::FAILURE;
                }
            },
            flag if flag.starts_with("--") => {
                eprintln!("cicatrix workflow status: unknown flag {flag}");
                return ExitCode::FAILURE;
            }
            other => {
                if exec_id.is_none() {
                    exec_id = Some(other.to_string());
                } else {
                    eprintln!("cicatrix workflow status: unexpected argument `{other}`");
                    return ExitCode::FAILURE;
                }
            }
        }
    }

    let id = match exec_id {
        Some(i) => i,
        None => {
            eprintln!("usage: cicatrix workflow status <exec_id> [--db <path>]");
            return ExitCode::FAILURE;
        }
    };

    let db_path = match db_path_opt {
        Some(p) => std::path::PathBuf::from(p),
        None => match workflow::default_workflow_db_path() {
            Ok(p) => p,
            Err(e) => {
                eprintln!("cicatrix workflow status: cannot resolve workflow db path: {e}");
                return ExitCode::FAILURE;
            }
        },
    };

    match workflow::get_execution_detail_from_db(&db_path, &id) {
        Ok(detail) => {
            println!("Execution ID:  {}", detail.exec_id);
            println!("Workflow:      {}", detail.workflow_name);
            println!("Business ID:   {}", detail.workflow_id);
            println!("State:         {}", detail.state);
            println!(
                "Events:        {} ({} bytes)",
                detail.event_count, detail.byte_size
            );
            println!("Input:         {}", detail.input_json);
            if let Some(out) = detail.output_json {
                println!("Output:        {}", out);
            }
            if let Some(err) = detail.error {
                println!("Error:         {}", err);
            }
            if !detail.events.is_empty() {
                println!("Event Log ({} total):", detail.events.len());
                for (idx, ev) in detail.events.iter().enumerate() {
                    println!("  [{idx}] {}", ev);
                }
            }
            if !detail.signals.is_empty() {
                println!("Durable Signals ({} total):", detail.signals.len());
                for sig in &detail.signals {
                    println!(
                        "  [{}] signal={} key={} payload={}",
                        sig.received_at, sig.signal_name, sig.idempotency_key, sig.payload_json
                    );
                }
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("cicatrix workflow status: {e}");
            ExitCode::FAILURE
        }
    }
}

/// `mcp [--stdio | --http [<bind>]] [--bind <bind>]` — run the Model Context Protocol (MCP) server.
fn cmd_mcp(rest: &[String]) -> ExitCode {
    let mut mode = "stdio";
    let mut http_bind = "127.0.0.1:8080".to_string();
    let mut it = rest.iter().peekable();

    while let Some(a) = it.next() {
        match a.as_str() {
            "--stdio" => {
                mode = "stdio";
            }
            "--http" => {
                mode = "http";
                if let Some(next) = it.peek() {
                    if !next.starts_with("--") {
                        http_bind = it.next().unwrap().clone();
                    }
                }
            }
            "--bind" => {
                mode = "http";
                match it.next() {
                    Some(b) => http_bind = b.clone(),
                    None => {
                        eprintln!("cicatrix mcp: --bind requires <address:port>");
                        return ExitCode::FAILURE;
                    }
                }
            }
            flag if flag.starts_with("--") => {
                eprintln!("cicatrix mcp: unknown flag {flag}");
                return ExitCode::FAILURE;
            }
            other => {
                eprintln!("cicatrix mcp: unexpected argument {other}");
                return ExitCode::FAILURE;
            }
        }
    }

    let rt = match create_tokio_runtime() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("cicatrix mcp: failed to initialize tokio runtime: {e}");
            return ExitCode::FAILURE;
        }
    };

    match mode {
        "stdio" => {
            if let Err(e) = rt.block_on(mcp::run_stdio_server()) {
                eprintln!("cicatrix mcp: stdio server exited with error: {e}");
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            }
        }
        "http" => {
            if let Err(e) = rt.block_on(mcp::run_http_server(&http_bind)) {
                eprintln!("cicatrix mcp: http server failed on {http_bind}: {e}");
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            }
        }
        _ => ExitCode::FAILURE,
    }
}

/// `serve [--mcp] [--bind <bind>]` — run HTTP server for cluster runners.
fn cmd_serve(rest: &[String]) -> ExitCode {
    let mut http_bind = "127.0.0.1:8080".to_string();
    let mut it = rest.iter();

    while let Some(a) = it.next() {
        match a.as_str() {
            "--mcp" => {
                // MCP HTTP service is currently the default and primary service
            }
            "--bind" => match it.next() {
                Some(b) => http_bind = b.clone(),
                None => {
                    eprintln!("cicatrix serve: --bind requires <address:port>");
                    return ExitCode::FAILURE;
                }
            },
            flag if flag.starts_with("--") => {
                eprintln!("cicatrix serve: unknown flag {flag}");
                return ExitCode::FAILURE;
            }
            other => {
                eprintln!("cicatrix serve: unexpected argument {other}");
                return ExitCode::FAILURE;
            }
        }
    }

    let rt = match create_tokio_runtime() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("cicatrix serve: failed to initialize tokio runtime: {e}");
            return ExitCode::FAILURE;
        }
    };

    if let Err(e) = rt.block_on(mcp::run_http_server(&http_bind)) {
        eprintln!("cicatrix serve: http server failed on {http_bind}: {e}");
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// `reversibility <eval|classify|plan|validate> [--diff <path>] [--tier <shadow|supervised|autonomous>] [--json]`
fn cmd_reversibility(rest: &[String]) -> ExitCode {
    use reversibility::{
        AutonomyTier, ReversibilityPipeline, ReversibilityVerdict, ValidationResult,
    };
    use std::io::{IsTerminal, Read};

    if rest.is_empty() || rest.iter().any(|a| a == "--help" || a == "-h") {
        eprintln!(
            "usage: cicatrix reversibility <eval|classify|plan|validate> \
             [--diff <path|->] [--tier <shadow|supervised|autonomous>] [--json]"
        );
        return ExitCode::FAILURE;
    }

    let subcmd = &rest[0];
    if subcmd != "eval"
        && subcmd != "evaluate"
        && subcmd != "classify"
        && subcmd != "plan"
        && subcmd != "validate"
    {
        eprintln!(
            "cicatrix reversibility: unknown subcommand `{subcmd}`; expected eval, classify, plan, or validate"
        );
        return ExitCode::FAILURE;
    }
    let mut diff_source: Option<String> = None;
    let mut tier = AutonomyTier::Supervised;
    let mut json_output = false;
    let mut actor_arg: Option<String> = None;

    let mut it = rest[1..].iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--diff" => match it.next() {
                Some(d) => diff_source = Some(d.clone()),
                None => {
                    eprintln!("cicatrix reversibility: --diff requires a path or '-' for stdin");
                    return ExitCode::FAILURE;
                }
            },
            "--tier" => match it.next() {
                Some(t) => match t.parse::<AutonomyTier>() {
                    Ok(parsed) => tier = parsed,
                    Err(e) => {
                        eprintln!("cicatrix reversibility: {e}");
                        return ExitCode::FAILURE;
                    }
                },
                None => {
                    eprintln!(
                        "cicatrix reversibility: --tier requires shadow, supervised, or autonomous"
                    );
                    return ExitCode::FAILURE;
                }
            },
            "--actor" => match it.next() {
                Some(a) => actor_arg = Some(a.clone()),
                None => {
                    eprintln!("cicatrix reversibility: --actor requires a name");
                    return ExitCode::FAILURE;
                }
            },
            "--json" => {
                json_output = true;
            }
            flag if flag.starts_with("--") => {
                eprintln!("cicatrix reversibility: unknown flag {flag}");
                return ExitCode::FAILURE;
            }
            pos if diff_source.is_none() => {
                diff_source = Some(pos.to_string());
            }
            other => {
                eprintln!("cicatrix reversibility: unexpected argument `{other}`");
                return ExitCode::FAILURE;
            }
        }
    }

    let diff_content = match diff_source.as_deref() {
        Some("-") => {
            let mut buf = String::new();
            if let Err(e) = std::io::stdin().read_to_string(&mut buf) {
                eprintln!("cicatrix reversibility: failed to read stdin: {e}");
                return ExitCode::FAILURE;
            }
            buf
        }
        Some(path_or_str) => {
            if Path::new(path_or_str).exists() {
                match std::fs::read_to_string(path_or_str) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!(
                            "cicatrix reversibility: failed to read file `{path_or_str}`: {e}"
                        );
                        return ExitCode::FAILURE;
                    }
                }
            } else if path_or_str.contains("diff --git") || path_or_str.contains("--- ") {
                path_or_str.to_string()
            } else {
                match std::fs::read_to_string(path_or_str) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("cicatrix reversibility: failed to read `{path_or_str}`: {e}");
                        return ExitCode::FAILURE;
                    }
                }
            }
        }
        None => {
            if !std::io::stdin().is_terminal() {
                let mut buf = String::new();
                if let Err(e) = std::io::stdin().read_to_string(&mut buf) {
                    eprintln!("cicatrix reversibility: failed to read stdin: {e}");
                    return ExitCode::FAILURE;
                }
                buf
            } else {
                eprintln!("cicatrix reversibility: missing diff input (provide via --diff <path|-> or piped stdin)");
                return ExitCode::FAILURE;
            }
        }
    };

    if diff_content.trim().is_empty() {
        eprintln!("cicatrix reversibility: diff content is empty");
        return ExitCode::FAILURE;
    }

    let actor = actor_arg
        .or_else(|| {
            std::env::var("CICATRIX_ACTOR")
                .ok()
                .map(|a| a.trim().to_string())
                .filter(|a| !a.is_empty())
        })
        .unwrap_or_else(|| "agent".to_string());

    let store = match store::SqliteStore::from_env() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cicatrix reversibility: failed to open store for tripwire check: {e}");
            return ExitCode::FAILURE;
        }
    };

    // Fail-closed synthetic canary tripwire check (CER-2760, Phase 3.2)
    if let Err(e) = store.guard_diff(&diff_content, &actor) {
        if json_output {
            eprintln!(
                "{}",
                serde_json::json!({
                    "error": "tripwire_intrusion_detected",
                    "canary_id": e.canary_id,
                    "target_path": e.target_path,
                    "actor": e.actor,
                    "action": e.action,
                    "touch_id": e.touch_id,
                    "cortex_notified": e.cortex_notified,
                    "message": e.message,
                })
            );
        } else {
            eprintln!("cicatrix reversibility: tripwire intrusion detected: {e}");
        }
        return ExitCode::FAILURE;
    }

    let pipeline = ReversibilityPipeline::new(tier);

    match subcmd.as_str() {
        "classify" => match pipeline.classify(&diff_content) {
            Ok(report) => {
                if json_output {
                    match serde_json::to_string_pretty(&report) {
                        Ok(j) => println!("{j}"),
                        Err(e) => {
                            eprintln!("cicatrix reversibility: failed to format json: {e}");
                            return ExitCode::FAILURE;
                        }
                    }
                } else {
                    println!("Action Class: {:?}", report.action_class);
                    println!("Canary Touched: {}", report.canary_touched);
                    println!("Touched Files:");
                    for f in &report.touched_files {
                        println!("  - {f}");
                    }
                    println!("Reasons:");
                    for r in &report.reasons {
                        println!("  - {r}");
                    }
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("cicatrix reversibility classify failed: {e}");
                ExitCode::FAILURE
            }
        },
        "plan" => match pipeline.plan(&diff_content) {
            Ok(plan) => {
                if json_output {
                    match serde_json::to_string_pretty(&plan) {
                        Ok(j) => println!("{j}"),
                        Err(e) => {
                            eprintln!("cicatrix reversibility: failed to format json: {e}");
                            return ExitCode::FAILURE;
                        }
                    }
                } else {
                    println!("Compensation Plan ({} ops):", plan.total_ops);
                    for (idx, op) in plan.ops.iter().enumerate() {
                        println!("  {}. {:?}", idx + 1, op);
                    }
                    println!("\nRollback Script:\n{}", plan.rollback_script);
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("cicatrix reversibility plan failed: {e}");
                ExitCode::FAILURE
            }
        },
        "validate" => match pipeline.validate(&diff_content) {
            Ok(val) => {
                let is_err = matches!(val, ValidationResult::Invalid { .. });
                if json_output {
                    match serde_json::to_string_pretty(&val) {
                        Ok(j) => println!("{j}"),
                        Err(e) => {
                            eprintln!("cicatrix reversibility: failed to format json: {e}");
                            return ExitCode::FAILURE;
                        }
                    }
                } else {
                    println!("Validation Outcome: {:?}", val);
                }
                if is_err {
                    ExitCode::FAILURE
                } else {
                    ExitCode::SUCCESS
                }
            }
            Err(e) => {
                eprintln!("cicatrix reversibility validate failed: {e}");
                ExitCode::FAILURE
            }
        },
        "eval" | "evaluate" => match pipeline.evaluate(&diff_content) {
            Ok(report) => {
                let is_block = matches!(report.verdict, ReversibilityVerdict::Block { .. });
                if json_output {
                    match serde_json::to_string_pretty(&report) {
                        Ok(j) => println!("{j}"),
                        Err(e) => {
                            eprintln!("cicatrix reversibility: failed to format json: {e}");
                            return ExitCode::FAILURE;
                        }
                    }
                } else {
                    println!("Reversibility Evaluation (Tier: {})", report.tier);
                    println!("Verdict: {:?}", report.verdict);
                    println!("Action Class: {:?}", report.action_class);
                    println!("Validation: {:?}", report.validation);
                    println!("Canary Touched: {}", report.canary_touched);
                    println!("Touched Files:");
                    for f in &report.touched_files {
                        println!("  - {f}");
                    }
                    println!("Summary: {}", report.summary);
                }
                if is_block {
                    ExitCode::FAILURE
                } else {
                    ExitCode::SUCCESS
                }
            }
            Err(e) => {
                eprintln!("cicatrix reversibility eval failed: {e}");
                ExitCode::FAILURE
            }
        },
        other => {
            eprintln!(
                "cicatrix reversibility: unknown subcommand `{other}`; expected eval, classify, plan, or validate"
            );
            ExitCode::FAILURE
        }
    }
}

fn cmd_tripwire(rest: &[String]) -> ExitCode {
    if rest.is_empty() {
        eprintln!(
            "usage: cicatrix tripwire <list [--json] | check <target> [--content <text>] [--actor <actor>] [--action <action>] [--json] | touches [--limit <N>] [--canary <id>] [--json] | seed>"
        );
        return ExitCode::FAILURE;
    }

    let subcmd = rest[0].as_str();
    let sub_args = &rest[1..];

    let store = match store::SqliteStore::from_env() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cicatrix tripwire: failed to open store: {e}");
            return ExitCode::FAILURE;
        }
    };

    match subcmd {
        "list" => {
            let mut json_output = false;
            for a in sub_args {
                if a == "--json" {
                    json_output = true;
                }
            }
            match store.list_tripwire_canaries() {
                Ok(canaries) => {
                    if json_output {
                        match serde_json::to_string_pretty(&canaries) {
                            Ok(j) => println!("{j}"),
                            Err(e) => {
                                eprintln!("cicatrix tripwire list: json serialization error: {e}");
                                return ExitCode::FAILURE;
                            }
                        }
                    } else if canaries.is_empty() {
                        println!("No synthetic canaries registered.");
                    } else {
                        println!("Registered Synthetic Canaries ({}):", canaries.len());
                        for c in &canaries {
                            println!(
                                "  [{}] target: `{}` (roles: {}, active: {})",
                                c.id,
                                c.target_path,
                                c.authorized_roles.join(", "),
                                c.is_active
                            );
                            println!("    marker: `{}`", c.sentinel_marker);
                            println!("    description: {}", c.description);
                        }
                    }
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("cicatrix tripwire list failed: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        "seed" => match store.seed_tripwire_canaries() {
            Ok(count) => {
                println!("Seeded {count} synthetic canaries into tripwire registry.");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("cicatrix tripwire seed failed: {e}");
                ExitCode::FAILURE
            }
        },
        "check" => {
            if sub_args.is_empty() {
                eprintln!(
                    "usage: cicatrix tripwire check <target> [--content <text>] [--actor <actor>] [--action <action>] [--json]"
                );
                return ExitCode::FAILURE;
            }

            let target = &sub_args[0];
            let mut content: Option<String> = None;
            let mut actor_arg: Option<String> = None;
            let mut action = "check".to_string();
            let mut json_output = false;

            let mut it = sub_args[1..].iter();
            while let Some(arg) = it.next() {
                match arg.as_str() {
                    "--content" => match it.next() {
                        Some(c) => content = Some(c.clone()),
                        None => {
                            eprintln!("cicatrix tripwire check: --content requires <text>");
                            return ExitCode::FAILURE;
                        }
                    },
                    "--actor" => match it.next() {
                        Some(a) => actor_arg = Some(a.clone()),
                        None => {
                            eprintln!("cicatrix tripwire check: --actor requires <actor>");
                            return ExitCode::FAILURE;
                        }
                    },
                    "--action" => match it.next() {
                        Some(act) => action = act.clone(),
                        None => {
                            eprintln!("cicatrix tripwire check: --action requires <action>");
                            return ExitCode::FAILURE;
                        }
                    },
                    "--json" => {
                        json_output = true;
                    }
                    unknown => {
                        eprintln!("cicatrix tripwire check: unknown option `{unknown}`");
                        return ExitCode::FAILURE;
                    }
                }
            }

            let actor = actor_arg
                .or_else(|| {
                    std::env::var("CICATRIX_ACTOR")
                        .ok()
                        .map(|a| a.trim().to_string())
                        .filter(|a| !a.is_empty())
                })
                .unwrap_or_else(|| "agent".to_string());

            match store.guard_check(target, content.as_deref(), &actor, &action) {
                Ok(()) => {
                    if json_output {
                        println!(
                            "{}",
                            serde_json::json!({
                                "status": "clear",
                                "verdict": "permitted",
                                "target": target,
                                "actor": actor,
                                "action": action,
                            })
                        );
                    } else {
                        println!(
                            "Tripwire check clear: permitted for actor `{actor}` on `{target}`"
                        );
                    }
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    if json_output {
                        println!(
                            "{}",
                            serde_json::json!({
                                "status": "tripped",
                                "error": "tripwire_intrusion_detected",
                                "canary_id": e.canary_id,
                                "target_path": e.target_path,
                                "actor": e.actor,
                                "action": e.action,
                                "touch_id": e.touch_id,
                                "cortex_notified": e.cortex_notified,
                                "message": e.message,
                            })
                        );
                    } else {
                        eprintln!("cicatrix tripwire check: intrusion detected: {e}");
                    }
                    ExitCode::FAILURE
                }
            }
        }
        "touches" => {
            let mut limit = 50usize;
            let mut canary_id: Option<String> = None;
            let mut json_output = false;

            let mut it = sub_args.iter();
            while let Some(arg) = it.next() {
                match arg.as_str() {
                    "--limit" => {
                        match it.next().and_then(|s| s.parse::<usize>().ok()) {
                            Some(l) => limit = l,
                            None => {
                                eprintln!("cicatrix tripwire touches: --limit requires a positive integer");
                                return ExitCode::FAILURE;
                            }
                        }
                    }
                    "--canary" => match it.next() {
                        Some(cid) => canary_id = Some(cid.clone()),
                        None => {
                            eprintln!("cicatrix tripwire touches: --canary requires <id>");
                            return ExitCode::FAILURE;
                        }
                    },
                    "--json" => {
                        json_output = true;
                    }
                    unknown => {
                        eprintln!("cicatrix tripwire touches: unknown option `{unknown}`");
                        return ExitCode::FAILURE;
                    }
                }
            }

            match store.list_tripwire_touches(limit, canary_id.as_deref()) {
                Ok(touches) => {
                    if json_output {
                        match serde_json::to_string_pretty(&touches) {
                            Ok(j) => println!("{j}"),
                            Err(e) => {
                                eprintln!("cicatrix tripwire touches: json error: {e}");
                                return ExitCode::FAILURE;
                            }
                        }
                    } else if touches.is_empty() {
                        println!("No tripwire touches recorded.");
                    } else {
                        println!("Tripwire Touch Events ({}):", touches.len());
                        for t in &touches {
                            println!(
                                "  [{}] touch_id={} canary={} actor={} action={} verdict={} cortex_notified={}",
                                t.created_at,
                                t.touch_id,
                                t.canary_id,
                                t.actor,
                                t.action_type,
                                t.verdict,
                                t.cortex_notified
                            );
                            if let Some(ref ctx) = t.context_payload {
                                println!("      context: {ctx}");
                            }
                        }
                    }
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("cicatrix tripwire touches failed: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        other => {
            eprintln!(
                "cicatrix tripwire: unknown subcommand `{other}`; expected list, check, touches, or seed"
            );
            ExitCode::FAILURE
        }
    }
}

fn cmd_autonomy(rest: &[String]) -> ExitCode {
    if rest.is_empty() {
        eprintln!(
            "usage: cicatrix autonomy <status [--actor <actor>] [--capability <capability>] [--json] | \
             promote --actor <actor> --capability <capability> --to <tier> --reason <reason> --authorized-by <user> [--evidence <json>] [--json] | \
             demote --actor <actor> --capability <capability> --to <tier> --reason <reason> --authorized-by <user> [--evidence <json>] [--json] | \
             history [--actor <actor>] [--capability <capability>] [--limit <N>] [--json] | \
             check --actor <actor> --capability <capability> --tier <tier> [--json]>"
        );
        return ExitCode::FAILURE;
    }

    let subcmd = rest[0].as_str();
    let sub_args = &rest[1..];

    let store = match store::SqliteStore::from_env() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cicatrix autonomy: failed to open store: {e}");
            return ExitCode::FAILURE;
        }
    };

    match subcmd {
        "status" => {
            let mut actor: Option<String> = None;
            let mut capability: Option<String> = None;
            let mut json_output = false;

            let mut it = sub_args.iter();
            while let Some(arg) = it.next() {
                match arg.as_str() {
                    "--actor" => match it.next() {
                        Some(a) => actor = Some(a.clone()),
                        None => {
                            eprintln!("cicatrix autonomy status: --actor requires <actor>");
                            return ExitCode::FAILURE;
                        }
                    },
                    "--capability" => match it.next() {
                        Some(c) => capability = Some(c.clone()),
                        None => {
                            eprintln!(
                                "cicatrix autonomy status: --capability requires <capability>"
                            );
                            return ExitCode::FAILURE;
                        }
                    },
                    "--json" => json_output = true,
                    unknown => {
                        eprintln!("cicatrix autonomy status: unknown option `{unknown}`");
                        return ExitCode::FAILURE;
                    }
                }
            }

            if let (Some(a), Some(c)) = (actor.as_deref(), capability.as_deref()) {
                match store.get_autonomy_state(a, c) {
                    Ok(Some(st)) => {
                        if json_output {
                            match serde_json::to_string_pretty(&st) {
                                Ok(j) => println!("{j}"),
                                Err(e) => {
                                    eprintln!("cicatrix autonomy status: json error: {e}");
                                    return ExitCode::FAILURE;
                                }
                            }
                        } else {
                            println!("Actor: {}", st.actor);
                            println!("Capability: {}", st.capability);
                            println!("Current Tier: {}", st.current_tier);
                            println!("Last Event: {}", st.last_event_id);
                            println!("Updated At: {}", st.updated_at);
                        }
                        ExitCode::SUCCESS
                    }
                    Ok(None) => {
                        if json_output {
                            println!(
                                "{}",
                                serde_json::json!({
                                    "actor": a,
                                    "capability": c,
                                    "tier": "shadow",
                                    "last_action": "default",
                                    "authorized_by": "system",
                                    "updated_at": null
                                })
                            );
                        } else {
                            println!("Actor: {a}");
                            println!("Capability: {c}");
                            println!("Current Tier: shadow (unrecorded, default)");
                        }
                        ExitCode::SUCCESS
                    }
                    Err(e) => {
                        eprintln!("cicatrix autonomy status failed: {e}");
                        ExitCode::FAILURE
                    }
                }
            } else {
                match store.list_autonomy_states(actor.as_deref()) {
                    Ok(states) => {
                        if json_output {
                            match serde_json::to_string_pretty(&states) {
                                Ok(j) => println!("{j}"),
                                Err(e) => {
                                    eprintln!("cicatrix autonomy status: json error: {e}");
                                    return ExitCode::FAILURE;
                                }
                            }
                        } else if states.is_empty() {
                            println!("No autonomy capability states recorded.");
                        } else {
                            println!("Autonomy Trust Ladder States ({}):", states.len());
                            for s in &states {
                                println!(
                                    "  [{}] actor: `{}` capability: `{}` (last event: {}, at: {})",
                                    s.current_tier,
                                    s.actor,
                                    s.capability,
                                    s.last_event_id,
                                    s.updated_at
                                );
                            }
                        }
                        ExitCode::SUCCESS
                    }
                    Err(e) => {
                        eprintln!("cicatrix autonomy status failed: {e}");
                        ExitCode::FAILURE
                    }
                }
            }
        }
        "promote" => {
            let mut actor: Option<String> = None;
            let mut capability: Option<String> = None;
            let mut to_tier_str: Option<String> = None;
            let mut reason: Option<String> = None;
            let mut authorized_by: Option<String> = None;
            let mut evidence_json: Option<String> = None;
            let mut json_output = false;

            let mut it = sub_args.iter();
            while let Some(arg) = it.next() {
                match arg.as_str() {
                    "--actor" => match it.next() {
                        Some(a) => actor = Some(a.clone()),
                        None => {
                            eprintln!("cicatrix autonomy promote: --actor requires <actor>");
                            return ExitCode::FAILURE;
                        }
                    },
                    "--capability" => match it.next() {
                        Some(c) => capability = Some(c.clone()),
                        None => {
                            eprintln!(
                                "cicatrix autonomy promote: --capability requires <capability>"
                            );
                            return ExitCode::FAILURE;
                        }
                    },
                    "--to" => match it.next() {
                        Some(t) => to_tier_str = Some(t.clone()),
                        None => {
                            eprintln!("cicatrix autonomy promote: --to requires <tier>");
                            return ExitCode::FAILURE;
                        }
                    },
                    "--reason" => match it.next() {
                        Some(r) => reason = Some(r.clone()),
                        None => {
                            eprintln!("cicatrix autonomy promote: --reason requires <reason>");
                            return ExitCode::FAILURE;
                        }
                    },
                    "--authorized-by" => match it.next() {
                        Some(ab) => authorized_by = Some(ab.clone()),
                        None => {
                            eprintln!("cicatrix autonomy promote: --authorized-by requires <user>");
                            return ExitCode::FAILURE;
                        }
                    },
                    "--evidence" => match it.next() {
                        Some(ev) => {
                            // Validate json syntax
                            if let Err(e) = serde_json::from_str::<serde_json::Value>(ev) {
                                eprintln!("cicatrix autonomy promote: invalid evidence json: {e}");
                                return ExitCode::FAILURE;
                            }
                            evidence_json = Some(ev.clone());
                        }
                        None => {
                            eprintln!("cicatrix autonomy promote: --evidence requires <json>");
                            return ExitCode::FAILURE;
                        }
                    },
                    "--json" => json_output = true,
                    unknown => {
                        eprintln!("cicatrix autonomy promote: unknown option `{unknown}`");
                        return ExitCode::FAILURE;
                    }
                }
            }

            let actor = match actor {
                Some(a) => a,
                None => {
                    eprintln!("cicatrix autonomy promote: missing required --actor");
                    return ExitCode::FAILURE;
                }
            };
            let capability = match capability {
                Some(c) => c,
                None => {
                    eprintln!("cicatrix autonomy promote: missing required --capability");
                    return ExitCode::FAILURE;
                }
            };
            let to_tier_str = match to_tier_str {
                Some(t) => t,
                None => {
                    eprintln!("cicatrix autonomy promote: missing required --to <tier>");
                    return ExitCode::FAILURE;
                }
            };
            let to_tier: crate::autonomy::AutonomyTier = match to_tier_str.parse() {
                Ok(t) => t,
                Err(e) => {
                    eprintln!(
                        "cicatrix autonomy promote: invalid target tier `{to_tier_str}`: {e}"
                    );
                    return ExitCode::FAILURE;
                }
            };
            let reason = match reason {
                Some(r) => r,
                None => {
                    eprintln!("cicatrix autonomy promote: missing required --reason");
                    return ExitCode::FAILURE;
                }
            };
            let authorized_by = match authorized_by {
                Some(ab) => ab,
                None => {
                    eprintln!("cicatrix autonomy promote: missing required --authorized-by");
                    return ExitCode::FAILURE;
                }
            };

            let req = crate::autonomy::PromotionRequest {
                actor,
                capability,
                target_tier: to_tier,
                reason,
                evidence_json,
                authorized_by,
            };

            match store.promote_autonomy(&req) {
                Ok(event) => {
                    if json_output {
                        match serde_json::to_string_pretty(&event) {
                            Ok(j) => println!("{j}"),
                            Err(e) => {
                                eprintln!("cicatrix autonomy promote: json error: {e}");
                                return ExitCode::FAILURE;
                            }
                        }
                    } else {
                        println!(
                            "PROMOTED: actor `{}` capability `{}` from `{}` to `{}` (event_id: {})",
                            event.actor, event.capability, event.from_tier, event.to_tier, event.id
                        );
                    }
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    if json_output {
                        println!(
                            "{}",
                            serde_json::json!({
                                "status": "error",
                                "error": e.to_string()
                            })
                        );
                    } else {
                        eprintln!("cicatrix autonomy promote failed: {e}");
                    }
                    ExitCode::FAILURE
                }
            }
        }
        "demote" => {
            let mut actor: Option<String> = None;
            let mut capability: Option<String> = None;
            let mut to_tier_str: Option<String> = None;
            let mut reason: Option<String> = None;
            let mut authorized_by: Option<String> = None;
            let mut evidence_json: Option<String> = None;
            let mut json_output = false;

            let mut it = sub_args.iter();
            while let Some(arg) = it.next() {
                match arg.as_str() {
                    "--actor" => match it.next() {
                        Some(a) => actor = Some(a.clone()),
                        None => {
                            eprintln!("cicatrix autonomy demote: --actor requires <actor>");
                            return ExitCode::FAILURE;
                        }
                    },
                    "--capability" => match it.next() {
                        Some(c) => capability = Some(c.clone()),
                        None => {
                            eprintln!(
                                "cicatrix autonomy demote: --capability requires <capability>"
                            );
                            return ExitCode::FAILURE;
                        }
                    },
                    "--to" => match it.next() {
                        Some(t) => to_tier_str = Some(t.clone()),
                        None => {
                            eprintln!("cicatrix autonomy demote: --to requires <tier>");
                            return ExitCode::FAILURE;
                        }
                    },
                    "--reason" => match it.next() {
                        Some(r) => reason = Some(r.clone()),
                        None => {
                            eprintln!("cicatrix autonomy demote: --reason requires <reason>");
                            return ExitCode::FAILURE;
                        }
                    },
                    "--authorized-by" => match it.next() {
                        Some(ab) => authorized_by = Some(ab.clone()),
                        None => {
                            eprintln!("cicatrix autonomy demote: --authorized-by requires <user>");
                            return ExitCode::FAILURE;
                        }
                    },
                    "--evidence" => match it.next() {
                        Some(ev) => {
                            if let Err(e) = serde_json::from_str::<serde_json::Value>(ev) {
                                eprintln!("cicatrix autonomy demote: invalid evidence json: {e}");
                                return ExitCode::FAILURE;
                            }
                            evidence_json = Some(ev.clone());
                        }
                        None => {
                            eprintln!("cicatrix autonomy demote: --evidence requires <json>");
                            return ExitCode::FAILURE;
                        }
                    },
                    "--json" => json_output = true,
                    unknown => {
                        eprintln!("cicatrix autonomy demote: unknown option `{unknown}`");
                        return ExitCode::FAILURE;
                    }
                }
            }

            let actor = match actor {
                Some(a) => a,
                None => {
                    eprintln!("cicatrix autonomy demote: missing required --actor");
                    return ExitCode::FAILURE;
                }
            };
            let capability = match capability {
                Some(c) => c,
                None => {
                    eprintln!("cicatrix autonomy demote: missing required --capability");
                    return ExitCode::FAILURE;
                }
            };
            let to_tier_str = match to_tier_str {
                Some(t) => t,
                None => {
                    eprintln!("cicatrix autonomy demote: missing required --to <tier>");
                    return ExitCode::FAILURE;
                }
            };
            let to_tier: crate::autonomy::AutonomyTier = match to_tier_str.parse() {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("cicatrix autonomy demote: invalid target tier `{to_tier_str}`: {e}");
                    return ExitCode::FAILURE;
                }
            };
            let reason = match reason {
                Some(r) => r,
                None => {
                    eprintln!("cicatrix autonomy demote: missing required --reason");
                    return ExitCode::FAILURE;
                }
            };
            let authorized_by = match authorized_by {
                Some(ab) => ab,
                None => {
                    eprintln!("cicatrix autonomy demote: missing required --authorized-by");
                    return ExitCode::FAILURE;
                }
            };

            let req = crate::autonomy::DemotionRequest {
                actor,
                capability,
                target_tier: to_tier,
                reason,
                evidence_json,
                authorized_by,
            };

            match store.demote_autonomy(&req) {
                Ok(event) => {
                    if json_output {
                        match serde_json::to_string_pretty(&event) {
                            Ok(j) => println!("{j}"),
                            Err(e) => {
                                eprintln!("cicatrix autonomy demote: json error: {e}");
                                return ExitCode::FAILURE;
                            }
                        }
                    } else {
                        println!(
                            "DEMOTED: actor `{}` capability `{}` from `{}` to `{}` (event_id: {})",
                            event.actor, event.capability, event.from_tier, event.to_tier, event.id
                        );
                    }
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    if json_output {
                        println!(
                            "{}",
                            serde_json::json!({
                                "status": "error",
                                "error": e.to_string()
                            })
                        );
                    } else {
                        eprintln!("cicatrix autonomy demote failed: {e}");
                    }
                    ExitCode::FAILURE
                }
            }
        }
        "history" => {
            let mut actor: Option<String> = None;
            let mut capability: Option<String> = None;
            let mut limit: usize = 50;
            let mut json_output = false;

            let mut it = sub_args.iter();
            while let Some(arg) = it.next() {
                match arg.as_str() {
                    "--actor" => match it.next() {
                        Some(a) => actor = Some(a.clone()),
                        None => {
                            eprintln!("cicatrix autonomy history: --actor requires <actor>");
                            return ExitCode::FAILURE;
                        }
                    },
                    "--capability" => match it.next() {
                        Some(c) => capability = Some(c.clone()),
                        None => {
                            eprintln!(
                                "cicatrix autonomy history: --capability requires <capability>"
                            );
                            return ExitCode::FAILURE;
                        }
                    },
                    "--limit" => {
                        match it.next().and_then(|s| s.parse::<usize>().ok()) {
                            Some(l) => limit = l,
                            None => {
                                eprintln!("cicatrix autonomy history: --limit requires a positive integer");
                                return ExitCode::FAILURE;
                            }
                        }
                    }
                    "--json" => json_output = true,
                    unknown => {
                        eprintln!("cicatrix autonomy history: unknown option `{unknown}`");
                        return ExitCode::FAILURE;
                    }
                }
            }

            match store.list_autonomy_history(actor.as_deref(), capability.as_deref(), Some(limit))
            {
                Ok(events) => {
                    if json_output {
                        match serde_json::to_string_pretty(&events) {
                            Ok(j) => println!("{j}"),
                            Err(e) => {
                                eprintln!("cicatrix autonomy history: json error: {e}");
                                return ExitCode::FAILURE;
                            }
                        }
                    } else if events.is_empty() {
                        println!("No autonomy audit history recorded.");
                    } else {
                        println!("Autonomy Audit History ({}):", events.len());
                        for ev in &events {
                            println!(
                                "  [{}] event_id: {} actor: `{}` capability: `{}` action: {} ({} -> {}) by: {}",
                                ev.created_at,
                                ev.id,
                                ev.actor,
                                ev.capability,
                                ev.action_type,
                                ev.from_tier,
                                ev.to_tier,
                                ev.authorized_by
                            );
                            println!("      reason: {}", ev.reason);
                        }
                    }
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("cicatrix autonomy history failed: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        "check" => {
            let mut actor: Option<String> = None;
            let mut capability: Option<String> = None;
            let mut required_tier_str: Option<String> = None;
            let mut json_output = false;

            let mut it = sub_args.iter();
            while let Some(arg) = it.next() {
                match arg.as_str() {
                    "--actor" => match it.next() {
                        Some(a) => actor = Some(a.clone()),
                        None => {
                            eprintln!("cicatrix autonomy check: --actor requires <actor>");
                            return ExitCode::FAILURE;
                        }
                    },
                    "--capability" => match it.next() {
                        Some(c) => capability = Some(c.clone()),
                        None => {
                            eprintln!(
                                "cicatrix autonomy check: --capability requires <capability>"
                            );
                            return ExitCode::FAILURE;
                        }
                    },
                    "--tier" => match it.next() {
                        Some(t) => required_tier_str = Some(t.clone()),
                        None => {
                            eprintln!("cicatrix autonomy check: --tier requires <tier>");
                            return ExitCode::FAILURE;
                        }
                    },
                    "--json" => json_output = true,
                    unknown => {
                        eprintln!("cicatrix autonomy check: unknown option `{unknown}`");
                        return ExitCode::FAILURE;
                    }
                }
            }

            let actor = match actor {
                Some(a) => a,
                None => {
                    eprintln!("cicatrix autonomy check: missing required --actor");
                    return ExitCode::FAILURE;
                }
            };
            let capability = match capability {
                Some(c) => c,
                None => {
                    eprintln!("cicatrix autonomy check: missing required --capability");
                    return ExitCode::FAILURE;
                }
            };
            let required_tier_str = match required_tier_str {
                Some(t) => t,
                None => {
                    eprintln!("cicatrix autonomy check: missing required --tier");
                    return ExitCode::FAILURE;
                }
            };
            let required_tier: crate::autonomy::AutonomyTier = match required_tier_str.parse() {
                Ok(t) => t,
                Err(e) => {
                    eprintln!(
                        "cicatrix autonomy check: invalid required tier `{required_tier_str}`: {e}"
                    );
                    return ExitCode::FAILURE;
                }
            };

            match store.check_autonomy(&actor, &capability, required_tier) {
                Ok(res) => {
                    if json_output {
                        match serde_json::to_string_pretty(&res) {
                            Ok(j) => println!("{j}"),
                            Err(e) => {
                                eprintln!("cicatrix autonomy check: json error: {e}");
                                return ExitCode::FAILURE;
                            }
                        }
                    } else if res.permitted {
                        println!(
                            "GRANTED: actor `{}` has sufficient tier `{}` for capability `{}` (required: `{}`)",
                            actor, res.current_tier, capability, res.required_tier
                        );
                    } else {
                        eprintln!(
                            "DENIED: actor `{}` denied for capability `{}`: {}",
                            actor, capability, res.reason
                        );
                    }
                    if res.permitted {
                        ExitCode::SUCCESS
                    } else {
                        ExitCode::FAILURE
                    }
                }
                Err(e) => {
                    eprintln!("cicatrix autonomy check failed: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        other => {
            eprintln!(
                "cicatrix autonomy: unknown subcommand `{other}`; expected status, promote, demote, history, or check"
            );
            ExitCode::FAILURE
        }
    }
}

fn create_tokio_runtime() -> std::io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .or_else(|_| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
        })
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
