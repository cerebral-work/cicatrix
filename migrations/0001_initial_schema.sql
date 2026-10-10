-- Schema migration 0001: Initial schema for cicatrix regression database provider.
-- Lineage: autumn-foundation/autumn-harvest & wbrown/janus-datalog.

CREATE TABLE IF NOT EXISTS bug_facts (
    id TEXT PRIMARY KEY NOT NULL,
    symptom TEXT NOT NULL,
    fix_commit TEXT NOT NULL,
    regression_test TEXT NOT NULL,
    meta_pattern TEXT NOT NULL,
    scope TEXT,
    do_not_generalize INTEGER NOT NULL DEFAULT 0,
    reproducer TEXT,
    reproducer_command TEXT,
    rerun_policy TEXT,
    closing_invariant TEXT,
    stochastic_json TEXT,
    frontier TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK (length(trim(id)) > 0),
    CHECK (length(trim(symptom)) > 0),
    CHECK (length(trim(fix_commit)) > 0),
    CHECK (length(trim(regression_test)) > 0),
    CHECK (length(trim(meta_pattern)) > 0)
);

CREATE TABLE IF NOT EXISTS bug_fact_files (
    bug_id TEXT NOT NULL,
    path TEXT NOT NULL,
    PRIMARY KEY (bug_id, path),
    FOREIGN KEY (bug_id) REFERENCES bug_facts(id) ON DELETE CASCADE,
    CHECK (length(trim(path)) > 0)
);

CREATE INDEX IF NOT EXISTS idx_bug_fact_files_path ON bug_fact_files(path);
CREATE INDEX IF NOT EXISTS idx_bug_fact_files_bug_id ON bug_fact_files(bug_id);

CREATE TABLE IF NOT EXISTS bug_occurrences (
    bug_id TEXT NOT NULL,
    n INTEGER NOT NULL,
    date TEXT NOT NULL,
    config TEXT NOT NULL,
    result TEXT NOT NULL,
    PRIMARY KEY (bug_id, n),
    FOREIGN KEY (bug_id) REFERENCES bug_facts(id) ON DELETE CASCADE,
    CHECK (n > 0),
    CHECK (length(trim(date)) > 0),
    CHECK (length(trim(config)) > 0),
    CHECK (length(trim(result)) > 0)
);

CREATE INDEX IF NOT EXISTS idx_bug_occurrences_bug_id ON bug_occurrences(bug_id);

CREATE TABLE IF NOT EXISTS branch_snapshots (
    snapshot_id TEXT PRIMARY KEY NOT NULL,
    base_frontier TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    db_path TEXT NOT NULL,
    CHECK (length(trim(snapshot_id)) > 0),
    CHECK (length(trim(db_path)) > 0)
);

CREATE INDEX IF NOT EXISTS idx_branch_snapshots_created_at ON branch_snapshots(created_at);
