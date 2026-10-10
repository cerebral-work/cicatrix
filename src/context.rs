//! Run-context assembly engine (CER-2763, Phase 4.2).
//!
//! Wires Cicatrix regression queries into the Soma run-context assembly window
//! (D2 contract, CER-2611). Prepends `<known-bugs>` block to task prompts for
//! touched paths when matches exist:
//!
//! ```text
//! <known-bugs>
//! - BUG_... (meta-pattern): <regression guard> — files: ...
//! </known-bugs>
//!
//! {prompt}
//! ```
//!
//! Failure semantics:
//! - Non-blocking fail-soft by default for infrastructure / database queries:
//!   if the backend or reveried is unreachable, a warning is logged and the
//!   prompt proceeds unchanged without blocking agent dispatch.
//! - Hard fail-closed for security violations: unauthorized access to synthetic
//!   canary tripwire paths breaks the circuit immediately and halts execution.

use std::fmt::Write as _;

use crate::branch::{validate_snapshot_id, BranchManager};
use crate::frontier::Frontier;
use crate::gitf::{filter_as_of, filter_by_frontier};
use crate::store::{BugFact, SqliteStore};

/// Default limit on the number of bug facts rendered into `<known-bugs>`.
pub const DEFAULT_LIMIT: usize = 3;

/// Starting tag for the known bugs context block.
pub const KNOWN_BUGS_TAG_START: &str = "<known-bugs>";

/// Ending tag for the known bugs context block.
pub const KNOWN_BUGS_TAG_END: &str = "</known-bugs>";

fn default_true() -> bool {
    true
}

/// Options configuring run-context assembly.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AssembleOptions {
    /// Base prompt or task instruction.
    pub prompt: String,
    /// List of modified or touched file paths.
    pub paths: Vec<String>,
    /// Maximum number of bug facts to prepend (default: 3).
    #[serde(default)]
    pub limit: Option<usize>,
    /// Optional historical git commit SHA filter.
    #[serde(default)]
    pub as_of: Option<String>,
    /// Optional causal frontier version vector filter.
    #[serde(default)]
    pub frontier: Option<String>,
    /// Optional isolated branch snapshot identifier.
    #[serde(default)]
    pub branch: Option<String>,
    /// Optional actor identity for authorization checks (default: "agent").
    #[serde(default)]
    pub actor: Option<String>,
    /// Non-blocking fail-soft semantics on query error (default: true).
    #[serde(default = "default_true")]
    pub fail_soft: bool,
}

impl AssembleOptions {
    /// Create new assemble options with required prompt and paths.
    pub fn new(prompt: impl Into<String>, paths: Vec<String>) -> Self {
        Self {
            prompt: prompt.into(),
            paths,
            limit: None,
            as_of: None,
            frontier: None,
            branch: None,
            actor: None,
            fail_soft: true,
        }
    }

    /// Set the maximum number of facts to render.
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Set an as-of commit SHA filter.
    pub fn with_as_of(mut self, as_of: impl Into<String>) -> Self {
        self.as_of = Some(as_of.into());
        self
    }

    /// Set a version-vector causal frontier filter.
    pub fn with_frontier(mut self, frontier: impl Into<String>) -> Self {
        self.frontier = Some(frontier.into());
        self
    }

    /// Set an isolated branch snapshot identifier.
    pub fn with_branch(mut self, branch: impl Into<String>) -> Self {
        self.branch = Some(branch.into());
        self
    }

    /// Set the actor identity for tripwire authorization.
    pub fn with_actor(mut self, actor: impl Into<String>) -> Self {
        self.actor = Some(actor.into());
        self
    }

    /// Set fail-soft behavior (true = advisory non-blocking, false = fail-closed on query error).
    pub fn with_fail_soft(mut self, fail_soft: bool) -> Self {
        self.fail_soft = fail_soft;
        self
    }
}

/// Result of run-context assembly.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AssembleResult {
    /// Final assembled prompt (augmented or unmodified).
    pub prompt: String,
    /// Total count of matching bug facts before truncation.
    pub matched_facts: usize,
    /// Whether `<known-bugs>` block was prepended.
    pub block_rendered: bool,
    /// Bug facts included in the assembled block.
    pub facts: Vec<BugFact>,
    /// Warning message if fail-soft was triggered on query error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

/// Errors occurring during run-context assembly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextError {
    /// Security violation: unauthorized attempt to query or touch canary tripwire.
    TripwireIntrusion(String),
    /// Database error during store resolution or querying.
    Database(String),
    /// Invalid arguments (e.g., malformed frontier or branch ID).
    InvalidArgument(String),
}

impl std::fmt::Display for ContextError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TripwireIntrusion(msg) => write!(f, "tripwire intrusion detected: {msg}"),
            Self::Database(msg) => write!(f, "database query error: {msg}"),
            Self::InvalidArgument(msg) => write!(f, "invalid argument: {msg}"),
        }
    }
}

impl std::error::Error for ContextError {}

/// Render `<known-bugs>` block from a slice of bug facts up to `limit`.
/// Returns `None` if `facts` is empty or `limit == 0`.
#[must_use]
pub fn render_known_bugs_block(facts: &[BugFact], limit: usize) -> Option<String> {
    if facts.is_empty() || limit == 0 {
        return None;
    }
    let mut block = String::from(KNOWN_BUGS_TAG_START);
    block.push('\n');
    for f in facts.iter().take(limit) {
        let files_str = f.files.join(", ");
        let _ = writeln!(
            block,
            "- {} ({}): {} — files: {}",
            f.id, f.meta_pattern, f.regression_test, files_str
        );
    }
    block.push_str(KNOWN_BUGS_TAG_END);
    Some(block)
}

/// Prepend a `<known-bugs>` block to `prompt` if facts exist; otherwise return `prompt` unchanged.
#[must_use]
pub fn assemble_prompt(prompt: &str, facts: &[BugFact], limit: usize) -> String {
    match render_known_bugs_block(facts, limit) {
        Some(block) => format!("{block}\n\n{prompt}"),
        None => prompt.to_string(),
    }
}

/// Helper to open the appropriate `SqliteStore` for run-context assembly.
pub fn open_context_store(branch: Option<&str>) -> Result<SqliteStore, ContextError> {
    if let Some(branch_id) = branch {
        validate_snapshot_id(branch_id).map_err(|e| {
            ContextError::InvalidArgument(format!("invalid branch id `{branch_id}`: {e}"))
        })?;
        let trunk_store = SqliteStore::open_default()
            .map_err(|e| ContextError::Database(format!("failed to open trunk database: {e}")))?;
        let manager = BranchManager::new(trunk_store)
            .map_err(|e| ContextError::Database(format!("failed to init branch manager: {e}")))?;
        let path = manager.branch_path(branch_id).map_err(|e| {
            ContextError::InvalidArgument(format!(
                "failed to resolve branch path for `{branch_id}`: {e}"
            ))
        })?;
        SqliteStore::open(path)
            .map_err(|e| ContextError::Database(format!("failed to open branch database: {e}")))
    } else {
        SqliteStore::from_env()
            .map_err(|e| ContextError::Database(format!("failed to open sqlite database: {e}")))
    }
}

/// Assemble run-context prompt by querying regression database for touched file paths.
///
/// Follows Soma run-context invariants:
/// 1. Advisory fail-soft by default: unreachable database or queries log a warning
///    and return the original prompt without blocking agent spawn.
/// 2. Security fail-closed: unauthorized touch to synthetic canary tripwires breaks
///    the circuit immediately with `ContextError::TripwireIntrusion` (never swallowed).
pub fn assemble_context(opts: &AssembleOptions) -> Result<AssembleResult, ContextError> {
    if opts.paths.is_empty() {
        return Ok(AssembleResult {
            prompt: opts.prompt.clone(),
            matched_facts: 0,
            block_rendered: false,
            facts: Vec::new(),
            warning: None,
        });
    }

    let actor = opts
        .actor
        .clone()
        .or_else(|| {
            std::env::var("CICATRIX_ACTOR")
                .ok()
                .map(|a| a.trim().to_string())
                .filter(|a| !a.is_empty())
        })
        .unwrap_or_else(|| "agent".to_string());

    let store = match open_context_store(opts.branch.as_deref()) {
        Ok(s) => s,
        Err(e) => {
            if opts.fail_soft {
                let warn_msg = format!("cicatrix known-bugs skipped, spawning without it: {e}");
                eprintln!("warning: {warn_msg}");
                return Ok(AssembleResult {
                    prompt: opts.prompt.clone(),
                    matched_facts: 0,
                    block_rendered: false,
                    facts: Vec::new(),
                    warning: Some(warn_msg),
                });
            } else {
                return Err(e);
            }
        }
    };

    // Fail-closed canary tripwire intrusion check (CER-2760, Phase 3.2).
    // Intrusion violations are ALWAYS fatal and NEVER swallowed by fail-soft.
    if let Err(intrusion) = store.guard_paths(&opts.paths, &actor, "context_assemble") {
        return Err(ContextError::TripwireIntrusion(intrusion.to_string()));
    }

    let mut hits = match store.touches_known_bug(&opts.paths) {
        Ok(h) => h,
        Err(e) => {
            if opts.fail_soft {
                let warn_msg = format!("cicatrix query failed, spawning without it: {e}");
                eprintln!("warning: {warn_msg}");
                return Ok(AssembleResult {
                    prompt: opts.prompt.clone(),
                    matched_facts: 0,
                    block_rendered: false,
                    facts: Vec::new(),
                    warning: Some(warn_msg),
                });
            } else {
                return Err(ContextError::Database(e.to_string()));
            }
        }
    };

    if let Some(f_str) = &opts.frontier {
        match Frontier::parse(f_str) {
            Ok(frontier) => {
                let (kept, _) = filter_by_frontier(hits, &frontier);
                hits = kept;
            }
            Err(e) => {
                if opts.fail_soft {
                    let warn_msg = format!("cicatrix frontier filter invalid `{f_str}`: {e}");
                    eprintln!("warning: {warn_msg}");
                } else {
                    return Err(ContextError::InvalidArgument(format!(
                        "invalid frontier `{f_str}`: {e}"
                    )));
                }
            }
        }
    }

    if let Some(commit) = &opts.as_of {
        let (kept, _) = filter_as_of(hits, commit);
        hits = kept;
    }

    let limit = opts.limit.unwrap_or(DEFAULT_LIMIT);
    let matched_facts = hits.len();
    let mut limited_facts = hits;
    limited_facts.truncate(limit);

    let block_opt = render_known_bugs_block(&limited_facts, limit);
    let block_rendered = block_opt.is_some();
    let final_prompt = match block_opt {
        Some(block) => format!("{block}\n\n{}", opts.prompt),
        None => opts.prompt.clone(),
    };

    Ok(AssembleResult {
        prompt: final_prompt,
        matched_facts,
        block_rendered,
        facts: limited_facts,
        warning: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn test_fact(id: &str, files: &[&str], meta: &str, test_guard: &str) -> BugFact {
        BugFact {
            id: id.to_string(),
            files: files.iter().map(|s| s.to_string()).collect(),
            symptom: format!("Symptom for {id}"),
            fix_commit: "abc1234".to_string(),
            regression_test: test_guard.to_string(),
            meta_pattern: meta.to_string(),
            scope: None,
            do_not_generalize: false,
            reproducer: None,
            stochastic: None,
            frontier: None,
        }
    }

    #[test]
    fn test_render_empty_facts_returns_none() {
        assert_eq!(render_known_bugs_block(&[], 3), None);
        let facts = vec![test_fact(
            "BUG_001",
            &["src/main.rs"],
            "bounds-check",
            "tests/guard.rs",
        )];
        assert_eq!(render_known_bugs_block(&facts, 0), None);
    }

    #[test]
    fn test_render_known_bugs_block_format() {
        let facts = vec![
            test_fact(
                "BUG_001",
                &["src/lib.rs", "src/util.rs"],
                "null-pointer",
                "tests/guard_1.rs",
            ),
            test_fact(
                "BUG_002",
                &["src/api.rs"],
                "header-overflow",
                "tests/guard_2.rs",
            ),
        ];

        let block = render_known_bugs_block(&facts, 3).expect("block rendered");
        let expected = "<known-bugs>\n\
            - BUG_001 (null-pointer): tests/guard_1.rs — files: src/lib.rs, src/util.rs\n\
            - BUG_002 (header-overflow): tests/guard_2.rs — files: src/api.rs\n\
            </known-bugs>";
        assert_eq!(block, expected);
    }

    #[test]
    fn test_assemble_prompt_prepends_block() {
        let facts = vec![test_fact(
            "BUG_001",
            &["src/lib.rs"],
            "null-pointer",
            "tests/guard_1.rs",
        )];
        let base_prompt = "Refactor the authentication handler.";
        let assembled = assemble_prompt(base_prompt, &facts, 3);
        let expected = "<known-bugs>\n\
            - BUG_001 (null-pointer): tests/guard_1.rs — files: src/lib.rs\n\
            </known-bugs>\n\n\
            Refactor the authentication handler.";
        assert_eq!(assembled, expected);
    }

    #[test]
    fn test_assemble_prompt_unmodified_when_empty() {
        let base_prompt = "Refactor the authentication handler.";
        let assembled = assemble_prompt(base_prompt, &[], 3);
        assert_eq!(assembled, base_prompt);
    }

    #[test]
    fn test_assemble_context_empty_paths() {
        let opts = AssembleOptions::new("Test prompt", vec![]);
        let res = assemble_context(&opts).expect("success");
        assert_eq!(res.prompt, "Test prompt");
        assert_eq!(res.matched_facts, 0);
        assert!(!res.block_rendered);
        assert!(res.facts.is_empty());
        assert!(res.warning.is_none());
    }

    #[test]
    fn test_assemble_context_with_hits_and_truncation() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let mut store = SqliteStore::open(&db_path).unwrap();

        let f1 = test_fact(
            "BUG_001",
            &["src/auth.rs"],
            "token-expiry",
            "tests/auth_test.rs",
        );
        let f2 = test_fact(
            "BUG_002",
            &["src/auth.rs"],
            "session-fixation",
            "tests/session_test.rs",
        );
        let f3 = test_fact(
            "BUG_003",
            &["src/auth.rs"],
            "replay-attack",
            "tests/replay_test.rs",
        );
        store.record(&f1).unwrap();
        store.record(&f2).unwrap();
        store.record(&f3).unwrap();

        std::env::set_var("CICATRIX_DB_PATH", db_path.to_str().unwrap());

        // Test with limit 2
        let opts = AssembleOptions::new("Fix auth bug", vec!["src/auth.rs".to_string()])
            .with_limit(2)
            .with_actor("operator");
        let res = assemble_context(&opts).expect("assemble success");

        assert_eq!(res.matched_facts, 3);
        assert_eq!(res.facts.len(), 2);
        assert!(res.block_rendered);
        assert!(res.prompt.starts_with("<known-bugs>\n"));
        assert!(res.prompt.ends_with("\n\nFix auth bug"));
        assert!(res.prompt.contains("BUG_001"));
        assert!(res.prompt.contains("BUG_002"));
        assert!(!res.prompt.contains("BUG_003"));

        std::env::remove_var("CICATRIX_DB_PATH");
    }

    #[test]
    fn test_assemble_context_no_hits_leaves_prompt_unmodified() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let store = SqliteStore::open(&db_path).unwrap();
        let _ = store;

        std::env::set_var("CICATRIX_DB_PATH", db_path.to_str().unwrap());

        let opts =
            AssembleOptions::new("Work on unrelated module", vec!["src/other.rs".to_string()])
                .with_actor("operator");
        let res = assemble_context(&opts).expect("assemble success");

        assert_eq!(res.prompt, "Work on unrelated module");
        assert_eq!(res.matched_facts, 0);
        assert!(!res.block_rendered);
        assert!(res.facts.is_empty());

        std::env::remove_var("CICATRIX_DB_PATH");
    }

    #[test]
    fn test_assemble_context_fail_soft_on_db_error() {
        let invalid_db_path = "/dev/null/forbidden/db.sqlite";
        std::env::set_var("CICATRIX_DB_PATH", invalid_db_path);

        // Fail-soft = true: returns original prompt with warning
        let opts = AssembleOptions::new("Execute safely", vec!["src/file.rs".to_string()])
            .with_fail_soft(true);
        let res = assemble_context(&opts).expect("fail-soft returns Ok");
        assert_eq!(res.prompt, "Execute safely");
        assert_eq!(res.matched_facts, 0);
        assert!(!res.block_rendered);
        assert!(res.warning.is_some());

        // Fail-soft = false: returns ContextError::Database
        let opts_strict = AssembleOptions::new("Execute safely", vec!["src/file.rs".to_string()])
            .with_fail_soft(false);
        let err = assemble_context(&opts_strict).expect_err("fail-closed returns Err");
        match err {
            ContextError::Database(_) => {}
            other => panic!("expected ContextError::Database, got {other:?}"),
        }

        std::env::remove_var("CICATRIX_DB_PATH");
    }

    #[test]
    fn test_assemble_context_canary_tripwire_fail_closed() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let store = SqliteStore::open(&db_path).unwrap();
        let _ = store;

        std::env::set_var("CICATRIX_DB_PATH", db_path.to_str().unwrap());

        // Unauthorized actor attempting to touch synthetic canary path
        let canary_path = ".cicatrix/sentinel/canary_alpha.rs";
        let opts = AssembleOptions::new("Attempt canary probe", vec![canary_path.to_string()])
            .with_actor("unauthorized_agent")
            .with_fail_soft(true); // Even with fail_soft = true, tripwire must fail closed!

        let err = assemble_context(&opts).expect_err("tripwire must fail closed");
        match err {
            ContextError::TripwireIntrusion(msg) => {
                assert!(msg.contains("circuit broken"));
            }
            other => panic!("expected ContextError::TripwireIntrusion, got {other:?}"),
        }

        std::env::remove_var("CICATRIX_DB_PATH");
    }
}
