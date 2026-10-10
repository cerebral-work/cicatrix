-- Schema migration 0002: Synthetic canary tripwire registry & intrusion guard.
-- Lineage: wheelhorsedev/nexus (Mark Masterson).

CREATE TABLE IF NOT EXISTS tripwire_canaries (
    id TEXT PRIMARY KEY NOT NULL,
    sentinel_marker TEXT NOT NULL,
    target_path TEXT NOT NULL,
    description TEXT NOT NULL,
    authorized_roles TEXT NOT NULL DEFAULT 'operator,harness',
    is_active INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK (length(trim(id)) > 0),
    CHECK (length(trim(sentinel_marker)) > 0),
    CHECK (length(trim(target_path)) > 0),
    CHECK (length(trim(description)) > 0)
);

CREATE INDEX IF NOT EXISTS idx_tripwire_canaries_target_path ON tripwire_canaries(target_path);
CREATE INDEX IF NOT EXISTS idx_tripwire_canaries_sentinel_marker ON tripwire_canaries(sentinel_marker);

CREATE TABLE IF NOT EXISTS tripwire_touches (
    touch_id TEXT PRIMARY KEY NOT NULL,
    canary_id TEXT NOT NULL,
    actor TEXT NOT NULL,
    action_type TEXT NOT NULL,
    context_payload TEXT,
    verdict TEXT NOT NULL,
    cortex_notified INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    FOREIGN KEY (canary_id) REFERENCES tripwire_canaries(id) ON DELETE CASCADE,
    CHECK (length(trim(touch_id)) > 0),
    CHECK (length(trim(canary_id)) > 0),
    CHECK (length(trim(actor)) > 0),
    CHECK (length(trim(action_type)) > 0)
);

CREATE INDEX IF NOT EXISTS idx_tripwire_touches_canary_id ON tripwire_touches(canary_id);
CREATE INDEX IF NOT EXISTS idx_tripwire_touches_created_at ON tripwire_touches(created_at);
