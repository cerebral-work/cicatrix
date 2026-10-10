-- Schema migration 0003: Earned autonomy trust ladder & append-only promotion ledger.
-- Lineage: wheelhorsedev/nexus (Mark Masterson).
-- Enforces :db/neverZeroValue schema integrity, append-only ledger immutability,
-- and the Soma invariant: Soma production gates strictly require human operator verdicts.

CREATE TABLE IF NOT EXISTS autonomy_ledger (
    id TEXT PRIMARY KEY NOT NULL,
    actor TEXT NOT NULL,
    capability TEXT NOT NULL,
    from_tier TEXT NOT NULL,
    to_tier TEXT NOT NULL,
    action_type TEXT NOT NULL,
    reason TEXT NOT NULL,
    evidence_json TEXT,
    authorized_by TEXT NOT NULL DEFAULT 'operator',
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK (length(trim(id)) > 0),
    CHECK (length(trim(actor)) > 0),
    CHECK (length(trim(capability)) > 0),
    CHECK (from_tier IN ('shadow', 'supervised', 'autonomous')),
    CHECK (to_tier IN ('shadow', 'supervised', 'autonomous')),
    CHECK (action_type IN ('promote', 'demote', 'initialize')),
    CHECK (length(trim(reason)) > 0),
    CHECK (length(trim(authorized_by)) > 0),
    -- Soma invariant: Soma production gates strictly require human verdicts; cannot be autonomous
    CHECK (NOT (lower(capability) LIKE 'soma%' AND to_tier = 'autonomous'))
);

CREATE INDEX IF NOT EXISTS idx_autonomy_ledger_actor_capability ON autonomy_ledger(actor, capability);
CREATE INDEX IF NOT EXISTS idx_autonomy_ledger_created_at ON autonomy_ledger(created_at);

-- Immutability enforcement: prevent updates to audit ledger
CREATE TRIGGER IF NOT EXISTS prevent_autonomy_ledger_update
BEFORE UPDATE ON autonomy_ledger
BEGIN
    SELECT RAISE(ABORT, 'autonomy_ledger is append-only: updates are prohibited');
END;

-- Immutability enforcement: prevent deletes from audit ledger
CREATE TRIGGER IF NOT EXISTS prevent_autonomy_ledger_delete
BEFORE DELETE ON autonomy_ledger
BEGIN
    SELECT RAISE(ABORT, 'autonomy_ledger is append-only: deletions are prohibited');
END;

-- Materialized current autonomy state per (actor, capability)
CREATE TABLE IF NOT EXISTS autonomy_state (
    actor TEXT NOT NULL,
    capability TEXT NOT NULL,
    current_tier TEXT NOT NULL,
    last_event_id TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (actor, capability),
    FOREIGN KEY (last_event_id) REFERENCES autonomy_ledger(id) ON DELETE RESTRICT,
    CHECK (length(trim(actor)) > 0),
    CHECK (length(trim(capability)) > 0),
    CHECK (current_tier IN ('shadow', 'supervised', 'autonomous')),
    -- Soma invariant: Soma production gates strictly require human verdicts; cannot be autonomous
    CHECK (NOT (lower(capability) LIKE 'soma%' AND current_tier = 'autonomous'))
);

CREATE INDEX IF NOT EXISTS idx_autonomy_state_tier ON autonomy_state(current_tier);

-- Synchronize autonomy_state atomically upon ledger insertion
CREATE TRIGGER IF NOT EXISTS update_autonomy_state_after_insert
AFTER INSERT ON autonomy_ledger
BEGIN
    INSERT INTO autonomy_state (actor, capability, current_tier, last_event_id, updated_at)
    VALUES (NEW.actor, NEW.capability, NEW.to_tier, NEW.id, NEW.created_at)
    ON CONFLICT(actor, capability) DO UPDATE SET
        current_tier = NEW.to_tier,
        last_event_id = NEW.id,
        updated_at = NEW.created_at;
END;
