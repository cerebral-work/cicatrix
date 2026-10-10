-- Schema migration 0004: Cortex settle learning loop outbox integration.
-- Lineage: CER-1827 / CER-1852 (Cortex Settle Learning Loop) & wbrown/janus-datalog.
-- Enforces :db/neverZeroValue schema integrity, append-only deduplication ledger,
-- and tracks negative settle verdicts linked to observed defect facts.

CREATE TABLE IF NOT EXISTS cortex_settle_events (
    id TEXT PRIMARY KEY NOT NULL,
    event_id TEXT NOT NULL,
    job_id TEXT NOT NULL,
    source TEXT NOT NULL,
    action_type TEXT NOT NULL,
    guard_tier TEXT NOT NULL DEFAULT 'tier1',
    guard_decision TEXT,
    operator_decision TEXT NOT NULL,
    settle_action TEXT NOT NULL,
    proposal_summary TEXT NOT NULL,
    rejection_reason TEXT,
    edit_delta TEXT,
    files_json TEXT,
    meta_pattern TEXT,
    is_negative INTEGER NOT NULL DEFAULT 0,
    fact_id TEXT,
    fact_path TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK (length(trim(id)) > 0),
    CHECK (length(trim(event_id)) > 0),
    CHECK (length(trim(job_id)) > 0),
    CHECK (length(trim(source)) > 0),
    CHECK (length(trim(action_type)) > 0),
    CHECK (length(trim(operator_decision)) > 0),
    CHECK (length(trim(settle_action)) > 0),
    CHECK (is_negative IN (0, 1))
);

CREATE INDEX IF NOT EXISTS idx_cortex_settle_job_action ON cortex_settle_events(job_id, settle_action);
CREATE INDEX IF NOT EXISTS idx_cortex_settle_source ON cortex_settle_events(source);
CREATE INDEX IF NOT EXISTS idx_cortex_settle_created_at ON cortex_settle_events(created_at);
CREATE INDEX IF NOT EXISTS idx_cortex_settle_is_negative ON cortex_settle_events(is_negative);

-- Immutability enforcement: prevent updates to settle audit ledger
CREATE TRIGGER IF NOT EXISTS prevent_cortex_settle_update
BEFORE UPDATE ON cortex_settle_events
BEGIN
    SELECT RAISE(ABORT, 'cortex_settle_events is append-only: updates are prohibited');
END;

-- Immutability enforcement: prevent deletes from settle audit ledger
CREATE TRIGGER IF NOT EXISTS prevent_cortex_settle_delete
BEFORE DELETE ON cortex_settle_events
BEGIN
    SELECT RAISE(ABORT, 'cortex_settle_events is append-only: deletions are prohibited');
END;
