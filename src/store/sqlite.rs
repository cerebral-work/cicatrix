//! Embedded SQLite storage engine for regression facts (CER-2753, Phase 1.1).
//!
//! Provides durable single-writer SQLite WAL persistence for bug facts, files, and occurrences.
//! Migrations and pragmas align with the `autumn-harvest-sqlite` runtime invariants:
//! - `PRAGMA busy_timeout = 5000;`
//! - `PRAGMA journal_mode = WAL;`
//! - `PRAGMA synchronous = FULL;`
//! - `PRAGMA foreign_keys = ON;`
//!
//! Enforces `:db/neverZeroValue` schema integrity constraints per EARS specification.

use crate::reverie::ReverieBridge;
use crate::store::{BugFact, BugStore, OccurrenceEntry, StochasticSpec};
use rusqlite::Connection;
use std::io;
use std::path::{Path, PathBuf};

/// Initial schema migration embedded at compile time.
pub const INITIAL_SCHEMA: &str = include_str!("../../migrations/0001_initial_schema.sql");
/// Tripwire registry schema migration embedded at compile time (CER-2760, Phase 3.2).
pub const TRIPWIRE_SCHEMA: &str = include_str!("../../migrations/0002_tripwire_registry.sql");
/// Autonomy ledger schema migration embedded at compile time (CER-2762, Phase 4.1).
pub const AUTONOMY_SCHEMA: &str = include_str!("../../migrations/0003_autonomy_ledger.sql");
/// Cortex settle learning loop schema migration embedded at compile time (CER-2764, Phase 4.3).
pub const CORTEX_SCHEMA: &str = include_str!("../../migrations/0004_cortex_settle_events.sql");
/// Replication log schema migration embedded at compile time (CER-2765, Phase 4.4).
pub const REPLICATION_SCHEMA: &str = include_str!("../../migrations/0005_replication_log.sql");

#[cfg(test)]
pub static TEST_ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn sqlite_to_io(e: rusqlite::Error) -> io::Error {
    io::Error::other(e.to_string())
}

fn never_zero_val(field: &str, reason: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!(":db/neverZeroValue violation: field `{field}` {reason}"),
    )
}

/// Validate that a `BugFact` contains no zero-value or empty-string attributes
/// according to `:db/neverZeroValue` schema integrity rules.
pub fn validate_never_zero_value(fact: &BugFact) -> io::Result<()> {
    if fact.id.trim().is_empty() {
        return Err(never_zero_val("id", "is empty or whitespace"));
    }
    if fact.symptom.trim().is_empty() {
        return Err(never_zero_val("symptom", "is empty or whitespace"));
    }
    if fact.fix_commit.trim().is_empty() {
        return Err(never_zero_val("fix_commit", "is empty or whitespace"));
    }
    if fact.regression_test.trim().is_empty() {
        return Err(never_zero_val("regression_test", "is empty or whitespace"));
    }
    if fact.meta_pattern.trim().is_empty() {
        return Err(never_zero_val("meta_pattern", "is empty or whitespace"));
    }
    if fact.files.is_empty() {
        return Err(never_zero_val("files", "cannot be empty"));
    }
    for (i, file) in fact.files.iter().enumerate() {
        if file.trim().is_empty() {
            return Err(never_zero_val(
                &format!("files[{i}]"),
                "is empty or whitespace",
            ));
        }
    }
    if let Some(scope) = &fact.scope {
        if scope.trim().is_empty() {
            return Err(never_zero_val("scope", "must be non-empty or None"));
        }
    }
    if let Some(reproducer) = &fact.reproducer {
        if reproducer.trim().is_empty() {
            return Err(never_zero_val("reproducer", "must be non-empty or None"));
        }
    }
    if let Some(stochastic) = &fact.stochastic {
        if let Some(cmd) = &stochastic.reproducer_command {
            if cmd.trim().is_empty() {
                return Err(never_zero_val(
                    "reproducer_command",
                    "must be non-empty or None",
                ));
            }
        }
        if let Some(policy) = &stochastic.rerun_policy {
            if policy.trim().is_empty() {
                return Err(never_zero_val("rerun_policy", "must be non-empty or None"));
            }
        }
        if let Some(inv) = &stochastic.closing_invariant {
            if inv.trim().is_empty() {
                return Err(never_zero_val(
                    "closing_invariant",
                    "must be non-empty or None",
                ));
            }
        }
        for (i, occ) in stochastic.occurrences.iter().enumerate() {
            if occ.n == 0 {
                return Err(never_zero_val(
                    &format!("occurrences[{i}].n"),
                    "must be greater than zero",
                ));
            }
            if occ.date.trim().is_empty() {
                return Err(never_zero_val(
                    &format!("occurrences[{i}].date"),
                    "is empty or whitespace",
                ));
            }
            if occ.config.trim().is_empty() {
                return Err(never_zero_val(
                    &format!("occurrences[{i}].config"),
                    "is empty or whitespace",
                ));
            }
            if occ.result.trim().is_empty() {
                return Err(never_zero_val(
                    &format!("occurrences[{i}].result"),
                    "is empty or whitespace",
                ));
            }
        }
    }
    if let Some(frontier) = &fact.frontier {
        frontier
            .validate_never_zero()
            .map_err(|e| never_zero_val("frontier", &e.to_string()))?;
    }
    Ok(())
}

fn apply_pragmas(conn: &Connection) -> io::Result<()> {
    conn.execute_batch(
        "PRAGMA busy_timeout = 5000;
         PRAGMA journal_mode = WAL;
         PRAGMA synchronous = FULL;
         PRAGMA foreign_keys = ON;",
    )
    .map_err(sqlite_to_io)?;
    Ok(())
}

fn apply_migrations(conn: &Connection) -> io::Result<()> {
    conn.execute_batch(INITIAL_SCHEMA).map_err(sqlite_to_io)?;
    // Idempotent migration for existing pre-frontier databases:
    let _ = conn.execute("ALTER TABLE bug_facts ADD COLUMN frontier TEXT;", []);
    conn.execute_batch(TRIPWIRE_SCHEMA).map_err(sqlite_to_io)?;
    let _ = crate::tripwire::seed_canaries(conn)?;
    conn.execute_batch(AUTONOMY_SCHEMA).map_err(sqlite_to_io)?;
    crate::autonomy::seed_default_capabilities(conn)
        .map_err(|e| io::Error::other(e.to_string()))?;
    conn.execute_batch(CORTEX_SCHEMA).map_err(sqlite_to_io)?;
    conn.execute_batch(REPLICATION_SCHEMA)
        .map_err(sqlite_to_io)?;
    Ok(())
}

/// The default database path: `CICATRIX_DB_PATH` or `~/.cicatrix/cicatrix.db`.
pub fn default_db_path() -> PathBuf {
    if let Ok(p) = std::env::var("CICATRIX_DB_PATH") {
        let trimmed = p.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".cicatrix").join("cicatrix.db")
}

/// Embedded SQLite implementation of the regression bug fact store.
pub struct SqliteStore {
    conn: Connection,
    #[allow(dead_code)]
    reverie: Option<ReverieBridge>,
    #[allow(dead_code)]
    db_path: Option<PathBuf>,
}

impl SqliteStore {
    /// Open or create the SQLite database at `path`.
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let path_ref = path.as_ref();
        if let Some(parent) = path_ref.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let conn = Connection::open(path_ref).map_err(sqlite_to_io)?;
        apply_pragmas(&conn)?;
        apply_migrations(&conn)?;
        Ok(Self {
            conn,
            reverie: None,
            db_path: Some(path_ref.to_path_buf()),
        })
    }

    /// Open an in-memory SQLite database (useful for isolated testing).
    #[allow(dead_code)]
    pub fn open_in_memory() -> io::Result<Self> {
        let conn = Connection::open_in_memory().map_err(sqlite_to_io)?;
        apply_pragmas(&conn)?;
        apply_migrations(&conn)?;
        Ok(Self {
            conn,
            reverie: None,
            db_path: None,
        })
    }

    /// Open the default database path (`CICATRIX_DB_PATH` or `~/.cicatrix/cicatrix.db`).
    pub fn open_default() -> io::Result<Self> {
        Self::open(default_db_path())
    }

    /// Attach a secondary `ReverieBridge` projection.
    pub fn with_reverie(mut self, bridge: ReverieBridge) -> Self {
        self.reverie = Some(bridge);
        self
    }

    /// Whether a secondary Reverie projection is attached.
    pub fn has_reverie(&self) -> bool {
        self.reverie.is_some()
    }

    /// Path to the database file on disk, if not in-memory.
    #[allow(dead_code)]
    pub fn db_path(&self) -> Option<&Path> {
        self.db_path.as_deref()
    }

    /// Reference to the underlying SQLite connection.
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Mutable reference to the underlying SQLite connection.
    pub fn conn_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

    /// Construct `SqliteStore` from environment settings.
    /// Uses `CICATRIX_DB_PATH` (or `~/.cicatrix/cicatrix.db`).
    /// Configures `ReverieBridge` unless `CICATRIX_NO_REVERIE=1` or `CICATRIX_OFFLINE=1`.
    pub fn from_env() -> io::Result<Self> {
        let mut store = Self::open_default()?;
        let offline = std::env::var("CICATRIX_OFFLINE")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let no_reverie = std::env::var("CICATRIX_NO_REVERIE")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

        if !offline && !no_reverie {
            store = store.with_reverie(ReverieBridge::from_env());
        }
        Ok(store)
    }

    /// Record a bug fact locally into SQLite within a transaction.
    pub fn record_local(&mut self, fact: &BugFact) -> io::Result<()> {
        validate_never_zero_value(fact)?;

        let tx = self.conn.transaction().map_err(sqlite_to_io)?;

        let (reproducer_command, rerun_policy, closing_invariant, stochastic_json) =
            if let Some(st) = &fact.stochastic {
                (
                    st.reproducer_command.as_deref(),
                    st.rerun_policy.as_deref(),
                    st.closing_invariant.as_deref(),
                    serde_json::to_string(st).ok(),
                )
            } else {
                (None, None, None, None)
            };

        let frontier_str = fact.frontier.as_ref().map(|f| f.to_string());

        tx.execute(
            "INSERT INTO bug_facts (
                id, symptom, fix_commit, regression_test, meta_pattern, scope,
                do_not_generalize, reproducer, reproducer_command, rerun_policy,
                closing_invariant, stochastic_json, frontier
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
             ON CONFLICT(id) DO UPDATE SET
                symptom = excluded.symptom,
                fix_commit = excluded.fix_commit,
                regression_test = excluded.regression_test,
                meta_pattern = excluded.meta_pattern,
                scope = excluded.scope,
                do_not_generalize = excluded.do_not_generalize,
                reproducer = excluded.reproducer,
                reproducer_command = excluded.reproducer_command,
                rerun_policy = excluded.rerun_policy,
                closing_invariant = excluded.closing_invariant,
                stochastic_json = excluded.stochastic_json,
                frontier = excluded.frontier;",
            rusqlite::params![
                fact.id,
                fact.symptom,
                fact.fix_commit,
                fact.regression_test,
                fact.meta_pattern,
                fact.scope,
                if fact.do_not_generalize { 1 } else { 0 },
                fact.reproducer,
                reproducer_command,
                rerun_policy,
                closing_invariant,
                stochastic_json,
                frontier_str,
            ],
        )
        .map_err(sqlite_to_io)?;

        tx.execute(
            "DELETE FROM bug_fact_files WHERE bug_id = ?1;",
            rusqlite::params![fact.id],
        )
        .map_err(sqlite_to_io)?;

        tx.execute(
            "DELETE FROM bug_occurrences WHERE bug_id = ?1;",
            rusqlite::params![fact.id],
        )
        .map_err(sqlite_to_io)?;

        {
            let mut stmt = tx
                .prepare("INSERT INTO bug_fact_files (bug_id, path) VALUES (?1, ?2);")
                .map_err(sqlite_to_io)?;
            for file in &fact.files {
                stmt.execute(rusqlite::params![fact.id, file])
                    .map_err(sqlite_to_io)?;
            }
        }

        if let Some(st) = &fact.stochastic {
            let mut stmt = tx
                .prepare(
                    "INSERT INTO bug_occurrences (bug_id, n, date, config, result) \
                     VALUES (?1, ?2, ?3, ?4, ?5);",
                )
                .map_err(sqlite_to_io)?;
            for occ in &st.occurrences {
                stmt.execute(rusqlite::params![
                    fact.id, occ.n, occ.date, occ.config, occ.result
                ])
                .map_err(sqlite_to_io)?;
            }
        }

        let frontier_for_log = frontier_str
            .clone()
            .unwrap_or_else(|| format!("{}:1", crate::replication::store::local_node_id()));
        let payload_json = serde_json::to_string(fact)
            .map_err(|e| io::Error::other(format!("JSON serialization error: {e}")))?;
        crate::replication::store::append_log_entry(
            &tx,
            &crate::replication::store::local_node_id(),
            "bug_fact",
            &fact.id,
            "upsert",
            &payload_json,
            &frontier_for_log,
        )
        .map_err(|e| io::Error::other(format!("replication log append failed: {e}")))?;

        tx.commit().map_err(sqlite_to_io)?;
        Ok(())
    }

    /// Retrieve a single bug fact by its unique ID.
    pub fn get_fact(&self, id: &str) -> io::Result<Option<BugFact>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, symptom, fix_commit, regression_test, meta_pattern, scope,
                        do_not_generalize, reproducer, reproducer_command, rerun_policy,
                        closing_invariant, stochastic_json, frontier
                 FROM bug_facts
                 WHERE id = ?1;",
            )
            .map_err(sqlite_to_io)?;

        let mut rows = stmt.query(rusqlite::params![id]).map_err(sqlite_to_io)?;
        let Some(row) = rows.next().map_err(sqlite_to_io)? else {
            return Ok(None);
        };

        let fact_id: String = row.get(0).map_err(sqlite_to_io)?;
        let symptom: String = row.get(1).map_err(sqlite_to_io)?;
        let fix_commit: String = row.get(2).map_err(sqlite_to_io)?;
        let regression_test: String = row.get(3).map_err(sqlite_to_io)?;
        let meta_pattern: String = row.get(4).map_err(sqlite_to_io)?;
        let scope: Option<String> = row.get(5).map_err(sqlite_to_io)?;
        let do_not_generalize_int: i32 = row.get(6).map_err(sqlite_to_io)?;
        let reproducer: Option<String> = row.get(7).map_err(sqlite_to_io)?;
        let reproducer_command: Option<String> = row.get(8).map_err(sqlite_to_io)?;
        let rerun_policy: Option<String> = row.get(9).map_err(sqlite_to_io)?;
        let closing_invariant: Option<String> = row.get(10).map_err(sqlite_to_io)?;
        let _stochastic_json: Option<String> = row.get(11).map_err(sqlite_to_io)?;
        let frontier_str: Option<String> = row.get(12).map_err(sqlite_to_io)?;

        let frontier = match frontier_str {
            Some(s) if !s.trim().is_empty() => match crate::store::Frontier::parse(&s) {
                Ok(f) => Some(f),
                Err(e) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("corrupt frontier in database for bug {fact_id}: {e}"),
                    ));
                }
            },
            _ => None,
        };

        let mut f_stmt = self
            .conn
            .prepare("SELECT path FROM bug_fact_files WHERE bug_id = ?1 ORDER BY rowid ASC;")
            .map_err(sqlite_to_io)?;
        let f_rows = f_stmt
            .query_map(rusqlite::params![id], |r| r.get::<_, String>(0))
            .map_err(sqlite_to_io)?;
        let mut files = Vec::new();
        for f in f_rows {
            files.push(f.map_err(sqlite_to_io)?);
        }

        let mut occ_stmt = self
            .conn
            .prepare(
                "SELECT n, date, config, result FROM bug_occurrences WHERE bug_id = ?1 ORDER BY n ASC;",
            )
            .map_err(sqlite_to_io)?;
        let occ_rows = occ_stmt
            .query_map(rusqlite::params![id], |r| {
                Ok(OccurrenceEntry {
                    n: r.get(0)?,
                    date: r.get(1)?,
                    config: r.get(2)?,
                    result: r.get(3)?,
                })
            })
            .map_err(sqlite_to_io)?;
        let mut occurrences = Vec::new();
        for occ in occ_rows {
            occurrences.push(occ.map_err(sqlite_to_io)?);
        }

        let stochastic = if reproducer_command.is_some()
            || rerun_policy.is_some()
            || closing_invariant.is_some()
            || !occurrences.is_empty()
        {
            Some(StochasticSpec {
                reproducer_command,
                rerun_policy,
                closing_invariant,
                occurrences,
            })
        } else {
            None
        };

        Ok(Some(BugFact {
            id: fact_id,
            files,
            symptom,
            fix_commit,
            regression_test,
            meta_pattern,
            scope,
            do_not_generalize: do_not_generalize_int != 0,
            reproducer,
            stochastic,
            frontier,
        }))
    }

    /// Record a bug fact. Writes locally to SQLite first, then asynchronously
    /// projects to Reverie if configured.
    pub fn record(&mut self, fact: &BugFact) -> io::Result<()> {
        self.record_local(fact)?;

        if let Some(bridge) = self.reverie.clone() {
            let fact_clone = fact.clone();
            let mut bridge_clone = bridge;
            let handle = std::thread::spawn(move || bridge_clone.record(&fact_clone));
            handle
                .join()
                .map_err(|_| io::Error::other("reverie projection thread panicked"))??;
        }
        Ok(())
    }

    /// Query known bug surfaces touching any of the provided changed files.
    pub fn touches_known_bug(&self, changed_files: &[String]) -> io::Result<Vec<BugFact>> {
        if changed_files.is_empty() {
            return Ok(Vec::new());
        }
        let mut matched_ids = std::collections::BTreeSet::new();
        for file in changed_files {
            let base = file.split(':').next().unwrap_or(file);
            let mut stmt = self
                .conn
                .prepare(
                    "SELECT DISTINCT bug_id FROM bug_fact_files
                     WHERE path = ?1
                        OR path LIKE (?1 || ':%')
                        OR (?1 LIKE (path || ':%'));",
                )
                .map_err(sqlite_to_io)?;
            let rows = stmt
                .query_map(rusqlite::params![base], |row| row.get::<_, String>(0))
                .map_err(sqlite_to_io)?;
            for id in rows {
                matched_ids.insert(id.map_err(sqlite_to_io)?);
            }
        }

        let mut facts = Vec::new();
        for bug_id in matched_ids {
            if let Some(fact) = self.get_fact(&bug_id)? {
                facts.push(fact);
            }
        }
        Ok(facts)
    }

    /// Retrieve all stored regression bug facts ordered by ID.
    pub fn all_facts(&self) -> io::Result<Vec<BugFact>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM bug_facts ORDER BY id ASC;")
            .map_err(sqlite_to_io)?;
        let ids = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(sqlite_to_io)?;
        let mut facts = Vec::new();
        for id_res in ids {
            let id = id_res.map_err(sqlite_to_io)?;
            if let Some(fact) = self.get_fact(&id)? {
                facts.push(fact);
            }
        }
        Ok(facts)
    }

    /// Retrieve all regression bug fact IDs ordered by ID.
    pub fn all_fact_ids(&self) -> io::Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM bug_facts ORDER BY id ASC;")
            .map_err(sqlite_to_io)?;
        let ids = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(sqlite_to_io)?;
        let mut result = Vec::new();
        for id_res in ids {
            result.push(id_res.map_err(sqlite_to_io)?);
        }
        Ok(result)
    }

    /// Atomically copy the current database state to `target` using SQLite `VACUUM INTO ?1`.
    pub fn vacuum_into(&self, target: impl AsRef<Path>) -> io::Result<()> {
        let path = target.as_ref();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let path_str = path.to_str().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "path is not valid unicode")
        })?;
        self.conn
            .execute("VACUUM INTO ?1;", rusqlite::params![path_str])
            .map_err(sqlite_to_io)?;
        Ok(())
    }

    /// Seed default synthetic canaries into the tripwire registry if not present.
    pub fn seed_tripwire_canaries(&self) -> io::Result<usize> {
        crate::tripwire::seed_canaries(&self.conn)
    }

    /// List all registered synthetic canaries.
    pub fn list_tripwire_canaries(&self) -> io::Result<Vec<crate::tripwire::TripwireCanary>> {
        crate::tripwire::list_canaries(&self.conn)
    }

    /// Register or update a synthetic canary in the tripwire registry.
    pub fn register_tripwire_canary(
        &self,
        canary: &crate::tripwire::TripwireCanary,
    ) -> io::Result<()> {
        crate::tripwire::register_canary(&self.conn, canary)
    }

    /// Record a touch event in the `tripwire_touches` audit table.
    pub fn record_tripwire_touch(&self, touch: &crate::tripwire::TripwireTouch) -> io::Result<()> {
        crate::tripwire::record_touch(&self.conn, touch)
    }

    /// List recent tripwire audit touches.
    pub fn list_tripwire_touches(
        &self,
        limit: usize,
        canary_id: Option<&str>,
    ) -> io::Result<Vec<crate::tripwire::TripwireTouch>> {
        crate::tripwire::list_touches(&self.conn, limit, canary_id)
    }

    /// Guard a collection of target paths against tripwire canaries.
    pub fn guard_paths(
        &self,
        paths: &[String],
        actor: &str,
        action: &str,
    ) -> Result<(), crate::tripwire::TripwireIntrusionError> {
        crate::tripwire::guard_paths(&self.conn, paths, actor, action)
    }

    /// Guard a unified diff patch string against tripwire canaries.
    pub fn guard_diff(
        &self,
        diff: &str,
        actor: &str,
    ) -> Result<(), crate::tripwire::TripwireIntrusionError> {
        crate::tripwire::guard_diff(&self.conn, diff, actor)
    }

    /// Guard a single target path and optional content against tripwire canaries.
    pub fn guard_check(
        &self,
        target: &str,
        content: Option<&str>,
        actor: &str,
        action: &str,
    ) -> Result<(), crate::tripwire::TripwireIntrusionError> {
        crate::tripwire::guard_check(&self.conn, target, content, actor, action)
    }

    // --- Autonomy trust ladder & promotion ledger (CER-2762, Phase 4.1) ---

    /// Get current autonomy tier for actor and capability.
    pub fn get_autonomy_tier(
        &self,
        actor: &str,
        capability: &str,
    ) -> Result<crate::autonomy::AutonomyTier, crate::autonomy::AutonomyError> {
        crate::autonomy::get_tier(&self.conn, actor, capability)
    }

    /// Get detailed autonomy state for actor and capability.
    pub fn get_autonomy_state(
        &self,
        actor: &str,
        capability: &str,
    ) -> Result<Option<crate::autonomy::AutonomyState>, crate::autonomy::AutonomyError> {
        crate::autonomy::get_state(&self.conn, actor, capability)
    }

    /// List active autonomy states, optionally filtered by actor.
    pub fn list_autonomy_states(
        &self,
        actor: Option<&str>,
    ) -> Result<Vec<crate::autonomy::AutonomyState>, crate::autonomy::AutonomyError> {
        crate::autonomy::list_states(&self.conn, actor)
    }

    /// Record an autonomy event in the audit ledger.
    pub fn record_autonomy_event(
        &self,
        event: &crate::autonomy::AutonomyEvent,
    ) -> Result<(), crate::autonomy::AutonomyError> {
        crate::autonomy::record_event(&self.conn, event)
    }

    /// List audit history of autonomy events.
    pub fn list_autonomy_history(
        &self,
        actor: Option<&str>,
        capability: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<crate::autonomy::AutonomyEvent>, crate::autonomy::AutonomyError> {
        crate::autonomy::list_events(&self.conn, actor, capability, limit.unwrap_or(50))
    }

    /// Promote an actor's capability to a higher autonomy tier.
    pub fn promote_autonomy(
        &self,
        req: &crate::autonomy::PromotionRequest,
    ) -> Result<crate::autonomy::AutonomyEvent, crate::autonomy::AutonomyError> {
        crate::autonomy::promote_capability(&self.conn, req.clone())
    }

    /// Demote an actor's capability to a lower autonomy tier.
    pub fn demote_autonomy(
        &self,
        req: &crate::autonomy::DemotionRequest,
    ) -> Result<crate::autonomy::AutonomyEvent, crate::autonomy::AutonomyError> {
        crate::autonomy::demote_capability(&self.conn, req.clone())
    }

    /// Check if an actor's capability has sufficient autonomy tier.
    pub fn check_autonomy(
        &self,
        actor: &str,
        capability: &str,
        required_tier: crate::autonomy::AutonomyTier,
    ) -> Result<crate::autonomy::AutonomyCheckResult, crate::autonomy::AutonomyError> {
        crate::autonomy::check_capability_autonomy(
            &self.conn,
            actor,
            capability,
            required_tier,
            None,
        )
    }

    // --- Cortex settle learning loop outbox integration (CER-2764, Phase 4.3) ---

    /// Ingest a single Cortex settle event through the given consumer.
    pub fn ingest_cortex_settle(
        &self,
        consumer: &crate::cortex::SettleConsumer,
        event: &crate::cortex::SettleOutcomeEvent,
    ) -> Result<crate::cortex::SettleIngestResult, crate::cortex::CortexError> {
        consumer.ingest_event(&self.conn, event)
    }

    /// Ingest Cortex settle events from a raw JSON/NDJSON string through the consumer.
    pub fn ingest_cortex_settle_str(
        &self,
        consumer: &crate::cortex::SettleConsumer,
        raw: &str,
    ) -> Result<Vec<crate::cortex::SettleIngestResult>, crate::cortex::CortexError> {
        consumer.ingest_str(&self.conn, raw)
    }

    /// Retrieve summary metrics for Cortex settle events.
    pub fn get_cortex_settle_status(
        &self,
    ) -> Result<crate::cortex::SettleStatusSummary, crate::cortex::CortexError> {
        crate::cortex::get_settle_status_summary(&self.conn)
    }

    /// List recorded Cortex settle events matching filter criteria.
    pub fn list_cortex_settle_events(
        &self,
        filter: &crate::cortex::SettleQueryFilter,
    ) -> Result<Vec<crate::cortex::RecordedSettleEvent>, crate::cortex::CortexError> {
        crate::cortex::list_settle_events(&self.conn, filter)
    }

    /// Get a recorded Cortex settle event by ID or event_id.
    pub fn get_cortex_settle_event(
        &self,
        id: &str,
    ) -> Result<Option<crate::cortex::RecordedSettleEvent>, crate::cortex::CortexError> {
        crate::cortex::get_settle_event(&self.conn, id)
    }
}

impl BugStore for SqliteStore {
    fn record(&mut self, fact: &BugFact) -> io::Result<()> {
        SqliteStore::record(self, fact)
    }

    fn touches_known_bug(&self, changed_files: &[String]) -> io::Result<Vec<BugFact>> {
        SqliteStore::touches_known_bug(self, changed_files)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn sample_fact() -> BugFact {
        BugFact {
            id: "BUG_TEST_101".into(),
            files: vec!["src/lib.rs:42".into(), "src/store.rs".into()],
            symptom: "Memory buffer underflow".into(),
            fix_commit: "abc1234".into(),
            regression_test: "cargo test -p cicatrix".into(),
            meta_pattern: "Edge cases are real cases".into(),
            scope: Some("src".into()),
            do_not_generalize: false,
            reproducer: Some("ENV=1 cargo test".into()),
            stochastic: Some(StochasticSpec {
                reproducer_command: Some("GOGC=1 cargo test".into()),
                rerun_policy: Some("3 runs".into()),
                closing_invariant: Some("assert!(len > 0)".into()),
                occurrences: vec![OccurrenceEntry {
                    n: 1,
                    date: "2026-10-10".into(),
                    config: "default".into(),
                    result: "FAIL".into(),
                }],
            }),
            frontier: None,
        }
    }

    #[test]
    fn wal_mode_and_pragmas_enabled() {
        let store = SqliteStore::open_in_memory().expect("open in-memory store");
        let synchronous: i32 = store
            .conn
            .query_row("PRAGMA synchronous;", [], |r| r.get(0))
            .expect("pragma synchronous");
        assert_eq!(synchronous, 2, "PRAGMA synchronous should be FULL (2)");

        let foreign_keys: i32 = store
            .conn
            .query_row("PRAGMA foreign_keys;", [], |r| r.get(0))
            .expect("pragma foreign_keys");
        assert_eq!(foreign_keys, 1, "PRAGMA foreign_keys should be ON (1)");

        let busy_timeout: i32 = store
            .conn
            .query_row("PRAGMA busy_timeout;", [], |r| r.get(0))
            .expect("pragma busy_timeout");
        assert_eq!(busy_timeout, 5000, "PRAGMA busy_timeout should be 5000ms");
    }

    #[test]
    fn wal_mode_on_disk() {
        let tmp = NamedTempFile::new().expect("create temp file");
        let store = SqliteStore::open(tmp.path()).expect("open disk store");
        let journal_mode: String = store
            .conn
            .query_row("PRAGMA journal_mode;", [], |r| r.get(0))
            .expect("pragma journal_mode");
        assert_eq!(
            journal_mode.to_lowercase(),
            "wal",
            "Disk database must use WAL journal mode"
        );
    }

    #[test]
    fn never_zero_value_enforcement() {
        let mut store = SqliteStore::open_in_memory().expect("open store");

        // Empty ID
        let mut bad = sample_fact();
        bad.id = "   ".into();
        assert!(store.record_local(&bad).is_err());

        // Empty symptom
        let mut bad = sample_fact();
        bad.symptom = "".into();
        assert!(store.record_local(&bad).is_err());

        // Empty files list
        let mut bad = sample_fact();
        bad.files = vec![];
        assert!(store.record_local(&bad).is_err());

        // Empty file entry
        let mut bad = sample_fact();
        bad.files = vec!["src/x.rs".into(), "".into()];
        assert!(store.record_local(&bad).is_err());

        // Zero occurrence n
        let mut bad = sample_fact();
        if let Some(st) = &mut bad.stochastic {
            st.occurrences[0].n = 0;
        }
        assert!(store.record_local(&bad).is_err());

        // Empty occurrence date
        let mut bad = sample_fact();
        if let Some(st) = &mut bad.stochastic {
            st.occurrences[0].date = "  ".into();
        }
        assert!(store.record_local(&bad).is_err());

        // Empty frontier
        let mut bad = sample_fact();
        bad.frontier = Some(crate::store::Frontier::new());
        assert!(store.record_local(&bad).is_err());
    }

    #[test]
    fn roundtrip_record_and_get() {
        let mut store = SqliteStore::open_in_memory().expect("open store");
        let fact = sample_fact();

        store.record_local(&fact).expect("record locally");
        let retrieved = store
            .get_fact(&fact.id)
            .expect("get fact")
            .expect("fact exists");

        assert_eq!(retrieved.id, fact.id);
        assert_eq!(retrieved.files, fact.files);
        assert_eq!(retrieved.symptom, fact.symptom);
        assert_eq!(retrieved.fix_commit, fact.fix_commit);
        assert_eq!(retrieved.regression_test, fact.regression_test);
        assert_eq!(retrieved.meta_pattern, fact.meta_pattern);
        assert_eq!(retrieved.scope, fact.scope);
        assert_eq!(retrieved.do_not_generalize, fact.do_not_generalize);
        assert_eq!(retrieved.reproducer, fact.reproducer);
        assert_eq!(retrieved.stochastic, fact.stochastic);
        assert_eq!(retrieved.frontier, None);
    }

    #[test]
    fn roundtrip_record_and_get_with_frontier() {
        let mut store = SqliteStore::open_in_memory().expect("open store");
        let mut fact = sample_fact();
        let frontier =
            crate::store::Frontier::parse("ceres:42, cygnus:18").expect("parse frontier");
        fact.frontier = Some(frontier.clone());

        store
            .record_local(&fact)
            .expect("record locally with frontier");
        let retrieved = store
            .get_fact(&fact.id)
            .expect("get fact")
            .expect("fact exists");

        assert_eq!(retrieved.id, fact.id);
        assert_eq!(retrieved.frontier, Some(frontier));
    }

    #[test]
    fn touches_known_bug_matching() {
        let mut store = SqliteStore::open_in_memory().expect("open store");
        let fact = sample_fact();
        store.record_local(&fact).expect("record fact");

        // Exact match with file containing line number
        let hits = store
            .touches_known_bug(&["src/lib.rs:42".into()])
            .expect("query");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, fact.id);

        // Match base file against path with line number
        let hits = store
            .touches_known_bug(&["src/lib.rs".into()])
            .expect("query");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, fact.id);

        // Match path with line number against plain path stored
        let hits = store
            .touches_known_bug(&["src/store.rs:99".into()])
            .expect("query");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, fact.id);

        // Non-matching file
        let misses = store
            .touches_known_bug(&["src/other.rs".into()])
            .expect("query");
        assert!(misses.is_empty());
    }

    #[test]
    fn persistence_and_recovery_across_reopen() {
        let tmp = NamedTempFile::new().expect("create temp file");
        let db_path = tmp.path().to_path_buf();

        let fact = sample_fact();
        {
            let mut store = SqliteStore::open(&db_path).expect("open initial");
            store.record_local(&fact).expect("record initial");
        }

        // Reopen database from same file path
        let store2 = SqliteStore::open(&db_path).expect("reopen store");
        let retrieved = store2
            .get_fact(&fact.id)
            .expect("query reopened")
            .expect("fact exists in reopened");
        assert_eq!(retrieved.id, fact.id);
        assert_eq!(retrieved.symptom, fact.symptom);
    }

    #[test]
    fn coexistence_with_autumn_harvest_sqlite() {
        let tmp = NamedTempFile::new().expect("create temp file");
        let db_path = tmp.path().to_path_buf();

        // 1. Initialize with SqliteStore and record a fact
        {
            let mut store = SqliteStore::open(&db_path).expect("open cicatrix store");
            store.record_local(&sample_fact()).expect("record fact");
        }

        // 2. Open with autumn_harvest_sqlite::SqliteRuntime
        {
            let rt = autumn_harvest_sqlite::SqliteRuntime::open(&db_path)
                .expect("open autumn harvest sqlite runtime");
            drop(rt);
        }

        // 3. Re-open with SqliteStore and verify tables coexist
        let store = SqliteStore::open(&db_path).expect("re-open cicatrix store");
        let retrieved = store
            .get_fact("BUG_TEST_101")
            .expect("query fact")
            .expect("fact still exists");
        assert_eq!(retrieved.id, "BUG_TEST_101");

        // Verify harvest_executions table also exists in the same sqlite database
        let count: i64 = store
            .conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='harvest_executions';",
                [],
                |r| r.get(0),
            )
            .expect("query sqlite_master");
        assert_eq!(
            count, 1,
            "harvest_executions table must coexist with cicatrix schema"
        );
    }

    #[test]
    fn all_facts_and_all_fact_ids_retrieval() {
        let mut store = SqliteStore::open_in_memory().expect("open store");
        assert!(store.all_facts().expect("all facts empty").is_empty());
        assert!(store.all_fact_ids().expect("all fact ids empty").is_empty());

        let mut fact1 = sample_fact();
        fact1.id = "BUG_ALPHA".into();
        let mut fact2 = sample_fact();
        fact2.id = "BUG_BETA".into();

        store.record_local(&fact2).expect("record beta");
        store.record_local(&fact1).expect("record alpha");

        let ids = store.all_fact_ids().expect("all fact ids");
        assert_eq!(ids, vec!["BUG_ALPHA", "BUG_BETA"]);

        let facts = store.all_facts().expect("all facts");
        assert_eq!(facts.len(), 2);
        assert_eq!(facts[0].id, "BUG_ALPHA");
        assert_eq!(facts[1].id, "BUG_BETA");
    }

    #[test]
    fn vacuum_into_creates_valid_and_isolated_database_copy() {
        let tmp = NamedTempFile::new().expect("create temp file");
        let target_dir = tempfile::tempdir().expect("temp dir");
        let target_path = target_dir.path().join("copy.db");

        let mut store = SqliteStore::open(tmp.path()).expect("open initial store");
        store.record_local(&sample_fact()).expect("record fact");

        store.vacuum_into(&target_path).expect("vacuum into copy");
        assert!(target_path.exists());

        let copy_store = SqliteStore::open(&target_path).expect("open copy store");
        let fact = copy_store
            .get_fact("BUG_TEST_101")
            .expect("query fact from copy")
            .expect("fact exists in copy");
        assert_eq!(fact.id, "BUG_TEST_101");

        // Mutating the copy does not affect the original
        let mut copy_store = copy_store;
        let mut new_fact = sample_fact();
        new_fact.id = "BUG_ONLY_IN_COPY".into();
        copy_store.record_local(&new_fact).expect("record to copy");

        assert!(copy_store.get_fact("BUG_ONLY_IN_COPY").unwrap().is_some());
        assert!(store.get_fact("BUG_ONLY_IN_COPY").unwrap().is_none());
    }

    #[test]
    fn branch_snapshots_table_exists() {
        let store = SqliteStore::open_in_memory().expect("open store");
        let count: i64 = store
            .conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='branch_snapshots';",
                [],
                |r| r.get(0),
            )
            .expect("query branch_snapshots");
        assert_eq!(count, 1, "branch_snapshots table must exist in schema");
    }

    #[test]
    fn tripwire_tables_and_canaries_seeded_on_open() {
        let store = SqliteStore::open_in_memory().expect("open store");
        let canaries = store.list_tripwire_canaries().expect("list canaries");
        assert_eq!(canaries.len(), 4, "default 4 canaries must be seeded");

        // Verify tripwire_touches table exists
        let count: i64 = store
            .conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='tripwire_touches';",
                [],
                |r| r.get(0),
            )
            .expect("query tripwire_touches table");
        assert_eq!(count, 1, "tripwire_touches table must exist in schema");
    }

    #[test]
    fn autonomy_tables_and_seeds_on_open() {
        let store = SqliteStore::open_in_memory().expect("open store");
        let states = store.list_autonomy_states(None).expect("list states");
        assert!(!states.is_empty(), "default capabilities should be seeded");

        let code_edit = store
            .get_autonomy_state("wheelhorse", "code_edit")
            .expect("get state");
        assert!(code_edit.is_some());
        assert_eq!(
            code_edit.unwrap().tier,
            crate::autonomy::AutonomyTier::Autonomous
        );

        let soma_gate = store
            .get_autonomy_state("wheelhorse", "soma_production_gate")
            .expect("get state");
        assert!(soma_gate.is_some());
        assert_eq!(
            soma_gate.unwrap().tier,
            crate::autonomy::AutonomyTier::Supervised
        );
    }
}
