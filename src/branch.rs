//! Branch snapshot & forking interface for agent worktrees (CER-2755, Phase 1.3).
//!
//! Provides isolated branch database instances using SQLite `VACUUM INTO ?1`, lock-free
//! snapshot drops, and invariant-validated (`:db/neverZeroValue`) settlement back into trunk.
//!
//! Lineage: `wbrown/janus-datalog`.

use crate::frontier::{Frontier, FrontierError};
use crate::store::sqlite::validate_never_zero_value;
use crate::store::SqliteStore;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

/// Metadata record for a branched database snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchSnapshot {
    pub snapshot_id: String,
    pub base_frontier: Option<String>,
    pub created_at: String,
    pub db_path: PathBuf,
}

/// Settlement report generated when a branch snapshot merges back into trunk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettleReport {
    pub snapshot_id: String,
    pub merged_facts: usize,
    pub fact_ids: Vec<String>,
    pub dropped_db_path: PathBuf,
}

/// Error type for branch snapshot operations.
#[derive(Debug)]
pub enum BranchError {
    InvalidSnapshotId(String),
    SnapshotAlreadyExists(String),
    SnapshotNotFound(String),
    NeverZeroValue(String),
    DatabaseExists(PathBuf),
    DatabaseNotFound(PathBuf),
    Io(io::Error),
    Sqlite(rusqlite::Error),
    Frontier(FrontierError),
}

impl fmt::Display for BranchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BranchError::InvalidSnapshotId(msg) => write!(f, "invalid snapshot ID: {msg}"),
            BranchError::SnapshotAlreadyExists(id) => {
                write!(f, "snapshot '{id}' already exists in branch registry")
            }
            BranchError::SnapshotNotFound(id) => {
                write!(f, "snapshot '{id}' not found in branch registry")
            }
            BranchError::NeverZeroValue(msg) => write!(f, ":db/neverZeroValue violation: {msg}"),
            BranchError::DatabaseExists(p) => {
                write!(f, "branch database file already exists: {}", p.display())
            }
            BranchError::DatabaseNotFound(p) => {
                write!(f, "branch database file not found: {}", p.display())
            }
            BranchError::Io(e) => write!(f, "I/O error: {e}"),
            BranchError::Sqlite(e) => write!(f, "SQLite error: {e}"),
            BranchError::Frontier(e) => write!(f, "Frontier error: {e}"),
        }
    }
}

impl std::error::Error for BranchError {}

impl From<io::Error> for BranchError {
    fn from(e: io::Error) -> Self {
        BranchError::Io(e)
    }
}

impl From<rusqlite::Error> for BranchError {
    fn from(e: rusqlite::Error) -> Self {
        BranchError::Sqlite(e)
    }
}

impl From<FrontierError> for BranchError {
    fn from(e: FrontierError) -> Self {
        BranchError::Frontier(e)
    }
}

impl From<BranchError> for io::Error {
    fn from(e: BranchError) -> Self {
        match e {
            BranchError::Io(io_err) => io_err,
            other => io::Error::other(other.to_string()),
        }
    }
}

/// Validate that a snapshot identifier is safe, non-empty, and free of path-traversal tokens.
pub fn validate_snapshot_id(id: &str) -> Result<(), BranchError> {
    let trimmed = id.trim();
    if trimmed.is_empty() {
        return Err(BranchError::InvalidSnapshotId(
            ":db/neverZeroValue violation: snapshot_id is empty or whitespace".to_string(),
        ));
    }
    if id != trimmed {
        return Err(BranchError::InvalidSnapshotId(
            "snapshot_id cannot contain leading or trailing whitespace".to_string(),
        ));
    }
    if id == "." || id == ".." || id.contains('/') || id.contains('\\') || id.contains("..") {
        return Err(BranchError::InvalidSnapshotId(format!(
            "path traversal detected in snapshot_id `{id}`"
        )));
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
    {
        return Err(BranchError::InvalidSnapshotId(format!(
            "snapshot_id `{id}` contains invalid characters (allowed: [a-zA-Z0-9_.-])"
        )));
    }
    Ok(())
}

fn resolve_default_branches_dir(store: &SqliteStore) -> PathBuf {
    if let Ok(dir) = std::env::var("CICATRIX_BRANCH_DIR") {
        let p = PathBuf::from(dir.trim());
        if !p.as_os_str().is_empty() {
            return p;
        }
    }
    if let Some(path) = store.db_path() {
        path.parent()
            .unwrap_or_else(|| Path::new("."))
            .join("branches")
    } else {
        std::env::temp_dir().join("cicatrix-branches")
    }
}

/// Manager for branch database snapshots and DeltaDB virtual worktrees.
pub struct BranchManager {
    store: SqliteStore,
    branches_dir: PathBuf,
}

impl BranchManager {
    /// Create a new `BranchManager` wrapping the primary trunk `SqliteStore`.
    pub fn new(store: SqliteStore) -> io::Result<Self> {
        let branches_dir = resolve_default_branches_dir(&store);
        std::fs::create_dir_all(&branches_dir)?;
        Ok(Self {
            store,
            branches_dir,
        })
    }

    /// Set an explicit directory for storing branch SQLite databases.
    pub fn with_branches_dir(mut self, dir: impl Into<PathBuf>) -> io::Result<Self> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir)?;
        self.branches_dir = dir;
        Ok(self)
    }

    /// Path to the directory housing branch snapshot database files.
    pub fn branches_dir(&self) -> &Path {
        &self.branches_dir
    }

    /// Access the underlying primary trunk store.
    pub fn store(&self) -> &SqliteStore {
        &self.store
    }

    /// Mutably access the underlying primary trunk store.
    pub fn store_mut(&mut self) -> &mut SqliteStore {
        &mut self.store
    }

    /// Consume manager and return the underlying primary trunk store.
    pub fn into_store(self) -> SqliteStore {
        self.store
    }

    /// Fork an isolated branch database from trunk or another snapshot/database.
    ///
    /// Uses SQLite's `VACUUM INTO ?1` to generate a transactionally consistent,
    /// atomic clone of the database without interrupting ongoing readers.
    pub fn fork(
        &mut self,
        snapshot_id: &str,
        from: Option<&str>,
        frontier: Option<&str>,
    ) -> Result<BranchSnapshot, BranchError> {
        validate_snapshot_id(snapshot_id)?;

        // Check if snapshot_id is already registered
        let exists: bool = self.store.conn().query_row(
            "SELECT EXISTS(SELECT 1 FROM branch_snapshots WHERE snapshot_id = ?1);",
            rusqlite::params![snapshot_id],
            |r| r.get(0),
        )?;
        if exists {
            return Err(BranchError::SnapshotAlreadyExists(snapshot_id.to_string()));
        }

        let target_path = self.branches_dir.join(format!("{snapshot_id}.db"));
        if target_path.exists() {
            return Err(BranchError::DatabaseExists(target_path));
        }

        // Validate frontier if supplied
        let parsed_frontier_str = if let Some(f_str) = frontier {
            let f = Frontier::parse(f_str)?;
            Some(f.to_string())
        } else {
            None
        };

        // Determine source database and perform VACUUM INTO
        if let Some(base) = from {
            // Check if base is a snapshot_id in the registry
            let base_path_opt: Option<String> = self
                .store
                .conn()
                .query_row(
                    "SELECT db_path FROM branch_snapshots WHERE snapshot_id = ?1;",
                    rusqlite::params![base],
                    |r| r.get(0),
                )
                .optional()?;

            if let Some(base_path) = base_path_opt {
                let base_store = SqliteStore::open(&base_path)?;
                base_store.vacuum_into(&target_path)?;
            } else if Path::new(base).exists() {
                let base_store = SqliteStore::open(base)?;
                base_store.vacuum_into(&target_path)?;
            } else {
                return Err(BranchError::SnapshotNotFound(base.to_string()));
            }
        } else {
            self.store.vacuum_into(&target_path)?;
        }

        let created_at: String = self.store.conn().query_row(
            "INSERT INTO branch_snapshots (snapshot_id, base_frontier, db_path)
             VALUES (?1, ?2, ?3)
             RETURNING created_at;",
            rusqlite::params![
                snapshot_id,
                parsed_frontier_str.as_deref(),
                target_path.to_str().unwrap_or("")
            ],
            |r| r.get(0),
        )?;

        Ok(BranchSnapshot {
            snapshot_id: snapshot_id.to_string(),
            base_frontier: parsed_frontier_str,
            created_at,
            db_path: target_path,
        })
    }

    /// Drop a branch snapshot and remove its database files.
    ///
    /// Holds no writer locks on the primary database, performing only a microsecond
    /// registry delete and unlinking filesystem files.
    pub fn drop_snapshot(&mut self, snapshot_id: &str) -> Result<PathBuf, BranchError> {
        validate_snapshot_id(snapshot_id)?;

        let db_path_opt: Option<String> = self
            .store
            .conn()
            .query_row(
                "SELECT db_path FROM branch_snapshots WHERE snapshot_id = ?1;",
                rusqlite::params![snapshot_id],
                |r| r.get(0),
            )
            .optional()?;

        let db_path = match db_path_opt {
            Some(p) => {
                self.store.conn().execute(
                    "DELETE FROM branch_snapshots WHERE snapshot_id = ?1;",
                    rusqlite::params![snapshot_id],
                )?;
                PathBuf::from(p)
            }
            None => {
                let candidate = self.branches_dir.join(format!("{snapshot_id}.db"));
                if candidate.exists() {
                    candidate
                } else {
                    return Err(BranchError::SnapshotNotFound(snapshot_id.to_string()));
                }
            }
        };

        // Remove SQLite DB file and any WAL / SHM files
        let wal_path = PathBuf::from(format!("{}-wal", db_path.display()));
        let shm_path = PathBuf::from(format!("{}-shm", db_path.display()));
        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(&wal_path);
        let _ = std::fs::remove_file(&shm_path);

        Ok(db_path)
    }

    /// Settle a branch snapshot back into the primary trunk database.
    ///
    /// Validates all branch facts against `:db/neverZeroValue` schema integrity constraints,
    /// merges changed facts into trunk, projects to Reverie (if enabled), and drops the branch.
    pub fn settle(&mut self, snapshot_id: &str) -> Result<SettleReport, BranchError> {
        validate_snapshot_id(snapshot_id)?;

        let db_path_str: String = self
            .store
            .conn()
            .query_row(
                "SELECT db_path FROM branch_snapshots WHERE snapshot_id = ?1;",
                rusqlite::params![snapshot_id],
                |r| r.get(0),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    BranchError::SnapshotNotFound(snapshot_id.to_string())
                }
                other => BranchError::Sqlite(other),
            })?;

        let branch_path = PathBuf::from(db_path_str);
        if !branch_path.exists() {
            return Err(BranchError::DatabaseNotFound(branch_path));
        }

        let branch_store = SqliteStore::open(&branch_path)?;
        let branch_facts = branch_store.all_facts()?;

        // Validate all facts against :db/neverZeroValue before mutating trunk
        for fact in &branch_facts {
            validate_never_zero_value(fact)
                .map_err(|e| BranchError::NeverZeroValue(e.to_string()))?;
        }

        // Merge changed/new facts into trunk (projecting to Reverie if enabled)
        let mut merged_ids = Vec::new();
        for fact in &branch_facts {
            let is_changed = match self.store.get_fact(&fact.id)? {
                Some(existing) => existing != *fact,
                None => true,
            };
            if is_changed {
                self.store.record(fact)?;
                merged_ids.push(fact.id.clone());
            }
        }

        let dropped_path = self.drop_snapshot(snapshot_id)?;

        Ok(SettleReport {
            snapshot_id: snapshot_id.to_string(),
            merged_facts: merged_ids.len(),
            fact_ids: merged_ids,
            dropped_db_path: dropped_path,
        })
    }

    /// List all active registered branch snapshots.
    pub fn list(&self) -> Result<Vec<BranchSnapshot>, BranchError> {
        let mut stmt = self.store.conn().prepare(
            "SELECT snapshot_id, base_frontier, created_at, db_path
             FROM branch_snapshots
             ORDER BY created_at ASC;",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(BranchSnapshot {
                snapshot_id: row.get(0)?,
                base_frontier: row.get(1)?,
                created_at: row.get(2)?,
                db_path: PathBuf::from(row.get::<_, String>(3)?),
            })
        })?;
        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
    }

    /// Resolve the database path for a given snapshot ID.
    pub fn branch_path(&self, snapshot_id: &str) -> Result<PathBuf, BranchError> {
        validate_snapshot_id(snapshot_id)?;
        let db_path_opt: Option<String> = self
            .store
            .conn()
            .query_row(
                "SELECT db_path FROM branch_snapshots WHERE snapshot_id = ?1;",
                rusqlite::params![snapshot_id],
                |r| r.get(0),
            )
            .optional()?;

        if let Some(p) = db_path_opt {
            let path = PathBuf::from(p);
            if path.exists() {
                return Ok(path);
            }
        }

        let fallback = self.branches_dir.join(format!("{snapshot_id}.db"));
        if fallback.exists() {
            return Ok(fallback);
        }

        Err(BranchError::SnapshotNotFound(snapshot_id.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::BugFact;
    use tempfile::NamedTempFile;

    fn sample_fact() -> BugFact {
        BugFact {
            id: "BUG_BRANCH_101".into(),
            files: vec!["src/branch.rs".into()],
            symptom: "Test branch bug fact".into(),
            fix_commit: "commit123".into(),
            regression_test: "cargo test".into(),
            meta_pattern: "Test pattern".into(),
            scope: None,
            do_not_generalize: false,
            reproducer: None,
            stochastic: None,
            frontier: None,
        }
    }

    #[test]
    fn snapshot_id_validation_rules() {
        assert!(validate_snapshot_id("valid-snapshot-01").is_ok());
        assert!(validate_snapshot_id("branch_w19_p1").is_ok());
        assert!(validate_snapshot_id("snap.v1").is_ok());

        assert!(validate_snapshot_id("").is_err());
        assert!(validate_snapshot_id("   ").is_err());
        assert!(validate_snapshot_id(" leading_space").is_err());
        assert!(validate_snapshot_id("trailing_space ").is_err());
        assert!(validate_snapshot_id("..").is_err());
        assert!(validate_snapshot_id(".").is_err());
        assert!(validate_snapshot_id("../path").is_err());
        assert!(validate_snapshot_id("sub/path").is_err());
        assert!(validate_snapshot_id("sub\\path").is_err());
        assert!(validate_snapshot_id("foo..bar").is_err());
        assert!(validate_snapshot_id("snap with space").is_err());
        assert!(validate_snapshot_id("snap#1").is_err());
    }

    #[test]
    fn fork_list_path_drop_flow() {
        let trunk_file = NamedTempFile::new().expect("create trunk file");
        let branches_dir = tempfile::tempdir().expect("create temp dir");

        let trunk = SqliteStore::open(trunk_file.path()).expect("open trunk");
        let mut manager = BranchManager::new(trunk)
            .expect("branch manager")
            .with_branches_dir(branches_dir.path())
            .expect("set branches dir");

        // Fork snapshot
        let snap = manager
            .fork("worktree-01", None, Some("ceres:1, cygnus:2"))
            .expect("fork branch");
        assert_eq!(snap.snapshot_id, "worktree-01");
        assert_eq!(snap.base_frontier.as_deref(), Some("ceres:1,cygnus:2"));
        assert!(snap.db_path.exists());

        // List snapshots
        let list = manager.list().expect("list branches");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].snapshot_id, "worktree-01");

        // Branch path
        let resolved = manager.branch_path("worktree-01").expect("branch path");
        assert_eq!(resolved, snap.db_path);

        // Drop snapshot
        let dropped = manager.drop_snapshot("worktree-01").expect("drop snapshot");
        assert_eq!(dropped, snap.db_path);
        assert!(!dropped.exists(), "dropped database file must be removed");

        let list_after = manager.list().expect("list after drop");
        assert!(list_after.is_empty());
    }

    #[test]
    fn fork_duplicate_rejected() {
        let trunk_file = NamedTempFile::new().expect("create trunk file");
        let branches_dir = tempfile::tempdir().expect("create temp dir");

        let trunk = SqliteStore::open(trunk_file.path()).expect("open trunk");
        let mut manager = BranchManager::new(trunk)
            .expect("branch manager")
            .with_branches_dir(branches_dir.path())
            .expect("set branches dir");

        manager.fork("branch_dup", None, None).expect("first fork");
        let err = manager.fork("branch_dup", None, None).unwrap_err();
        match err {
            BranchError::SnapshotAlreadyExists(id) => assert_eq!(id, "branch_dup"),
            other => panic!("expected SnapshotAlreadyExists, got {other:?}"),
        }
    }

    #[test]
    fn settle_merges_branch_facts_and_drops_branch() {
        let trunk_file = NamedTempFile::new().expect("create trunk file");
        let branches_dir = tempfile::tempdir().expect("create temp dir");

        let mut trunk = SqliteStore::open(trunk_file.path()).expect("open trunk");
        let mut trunk_fact = sample_fact();
        trunk_fact.id = "BUG_TRUNK".into();
        trunk.record_local(&trunk_fact).expect("record trunk fact");

        let mut manager = BranchManager::new(trunk)
            .expect("branch manager")
            .with_branches_dir(branches_dir.path())
            .expect("set branches dir");

        let snap = manager.fork("branch_settle", None, None).expect("fork");

        // Write a speculative fact only to the branch DB
        {
            let mut branch_store = SqliteStore::open(&snap.db_path).expect("open branch store");
            let mut branch_fact = sample_fact();
            branch_fact.id = "BUG_SPECULATIVE".into();
            branch_store
                .record_local(&branch_fact)
                .expect("record branch fact");
        }

        // Before settle, trunk does not have BUG_SPECULATIVE
        assert!(manager
            .store()
            .get_fact("BUG_SPECULATIVE")
            .unwrap()
            .is_none());

        // Settle branch into trunk
        let report = manager.settle("branch_settle").expect("settle");
        assert_eq!(report.snapshot_id, "branch_settle");
        assert_eq!(report.merged_facts, 1);
        assert_eq!(report.fact_ids, vec!["BUG_SPECULATIVE"]);

        // After settle, trunk has BUG_SPECULATIVE
        assert!(manager
            .store()
            .get_fact("BUG_SPECULATIVE")
            .unwrap()
            .is_some());
        // Branch DB file is removed
        assert!(!snap.db_path.exists());
        // Branch is dropped from registry
        assert!(manager.list().expect("list").is_empty());
    }

    #[test]
    fn settle_validates_never_zero_value() {
        let trunk_file = NamedTempFile::new().expect("create trunk file");
        let branches_dir = tempfile::tempdir().expect("create temp dir");

        let trunk = SqliteStore::open(trunk_file.path()).expect("open trunk");
        let mut manager = BranchManager::new(trunk)
            .expect("branch manager")
            .with_branches_dir(branches_dir.path())
            .expect("set branches dir");

        let snap = manager.fork("branch_invalid", None, None).expect("fork");

        // Directly bypass validation on branch store via raw SQL to insert an invalid zero-value fact (empty scope)
        {
            let branch_store = SqliteStore::open(&snap.db_path).expect("open branch store");
            branch_store
                .conn()
                .execute(
                    "INSERT INTO bug_facts (id, symptom, fix_commit, regression_test, meta_pattern, scope)
                     VALUES ('BUG_CORRUPT', 's1', 'c1', 't1', 'p1', '');",
                    [],
                )
                .expect("insert corrupt fact");
        }

        let err = manager.settle("branch_invalid").unwrap_err();
        match err {
            BranchError::NeverZeroValue(msg) => {
                assert!(msg.contains("neverZeroValue"));
            }
            other => panic!("expected NeverZeroValue error, got {other:?}"),
        }
    }
}
