//! Tripwire intrusion guard and audit logging (CER-2760, Phase 3.2).
//!
//! Enforces fail-closed circuit breaking, audit persistence to `tripwire_touches`,
//! and Cortex security alert broadcasting when unauthorized touches occur.

use std::fmt;
use std::io;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::tripwire::canary::{default_synthetic_canaries, TripwireCanary, TripwireTouch};
use crate::tripwire::notify::notify_cortex_intrusion;

/// Inner payload of an intrusion error.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TripwireIntrusionData {
    /// Identifier of the tripped canary.
    pub canary_id: String,
    /// Targeted sensitive path or sentinel.
    pub target_path: String,
    /// Actor attempting the intrusion.
    pub actor: String,
    /// Type of action attempted.
    pub action: String,
    /// Audit log touch ID recorded in database.
    pub touch_id: String,
    /// Whether the Cortex security alert was successfully delivered.
    pub cortex_notified: bool,
    /// Fail-closed violation explanation.
    pub message: String,
}

/// Error returned when an unauthorized agent trips a registered canary.
/// Boxed internally to maintain minimal Result error size.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TripwireIntrusionError(pub Box<TripwireIntrusionData>);

impl std::ops::Deref for TripwireIntrusionError {
    type Target = TripwireIntrusionData;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl fmt::Display for TripwireIntrusionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "TRIPWIRE INTRUSION DETECTED: Canary `{}` touched by `{}` on `{}` [action: {}]. Session circuit broken (touch_id: {}).",
            self.canary_id, self.actor, self.target_path, self.action, self.touch_id
        )
    }
}

impl std::error::Error for TripwireIntrusionError {}

/// Generate a unique timestamped touch ID.
pub fn generate_touch_id() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("touch_{:x}_{:x}", now.as_secs(), now.subsec_nanos())
}

/// Generate current ISO 8601 UTC timestamp.
pub fn current_iso_timestamp() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let days = secs / 86400;
    let rem_secs = secs % 86400;
    let hours = rem_secs / 3600;
    let mins = (rem_secs % 3600) / 60;
    let s = rem_secs % 60;

    // Approximate ISO 8601 string
    let mut year = 1970;
    let mut d = days;
    loop {
        let leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
        let days_in_year = if leap { 366 } else { 365 };
        if d < days_in_year {
            let days_in_months = [
                31,
                if leap { 29 } else { 28 },
                31,
                30,
                31,
                30,
                31,
                31,
                30,
                31,
                30,
                31,
            ];
            for (month, &dim) in (1..=12).zip(days_in_months.iter()) {
                if d < dim {
                    return format!(
                        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
                        year,
                        month,
                        d + 1,
                        hours,
                        mins,
                        s
                    );
                }
                d -= dim;
            }
            break;
        }
        d -= days_in_year;
        year += 1;
    }
    format!("2026-10-10T{:02}:{:02}:{:02}Z", hours, mins, s)
}

/// Seed default synthetic canaries into the database if not already present.
pub fn seed_canaries(conn: &Connection) -> io::Result<usize> {
    let defaults = default_synthetic_canaries();
    let mut seeded = 0;
    for canary in defaults {
        canary.validate_never_zero_value()?;
        let roles_str = canary.authorized_roles.join(",");
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tripwire_canaries WHERE id = ?1;",
                params![&canary.id],
                |row| row.get(0),
            )
            .map_err(|e| io::Error::other(e.to_string()))?;

        if count == 0 {
            conn.execute(
                "INSERT INTO tripwire_canaries (id, sentinel_marker, target_path, description, authorized_roles, is_active, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7);",
                params![
                    &canary.id,
                    &canary.sentinel_marker,
                    &canary.target_path,
                    &canary.description,
                    &roles_str,
                    if canary.is_active { 1 } else { 0 },
                    &canary.created_at,
                ],
            )
            .map_err(|e| io::Error::other(e.to_string()))?;
            seeded += 1;
        }
    }
    Ok(seeded)
}

/// List all registered canaries from the database.
pub fn list_canaries(conn: &Connection) -> io::Result<Vec<TripwireCanary>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, sentinel_marker, target_path, description, authorized_roles, is_active, created_at
             FROM tripwire_canaries ORDER BY id ASC;",
        )
        .map_err(|e| io::Error::other(e.to_string()))?;

    let rows = stmt
        .query_map([], |row| {
            let roles_raw: String = row.get(4)?;
            let authorized_roles: Vec<String> = roles_raw
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect();
            let is_active_int: i32 = row.get(5)?;
            Ok(TripwireCanary {
                id: row.get(0)?,
                sentinel_marker: row.get(1)?,
                target_path: row.get(2)?,
                description: row.get(3)?,
                authorized_roles,
                is_active: is_active_int != 0,
                created_at: row.get(6)?,
            })
        })
        .map_err(|e| io::Error::other(e.to_string()))?;

    let mut list = Vec::new();
    for r in rows {
        list.push(r.map_err(|e| io::Error::other(e.to_string()))?);
    }
    Ok(list)
}

/// Register a new synthetic canary in the database.
pub fn register_canary(conn: &Connection, canary: &TripwireCanary) -> io::Result<()> {
    canary.validate_never_zero_value()?;
    let roles_str = canary.authorized_roles.join(",");
    conn.execute(
        "INSERT INTO tripwire_canaries (id, sentinel_marker, target_path, description, authorized_roles, is_active, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(id) DO UPDATE SET
            sentinel_marker = excluded.sentinel_marker,
            target_path = excluded.target_path,
            description = excluded.description,
            authorized_roles = excluded.authorized_roles,
            is_active = excluded.is_active;",
        params![
            &canary.id,
            &canary.sentinel_marker,
            &canary.target_path,
            &canary.description,
            &roles_str,
            if canary.is_active { 1 } else { 0 },
            &canary.created_at,
        ],
    )
    .map_err(|e| io::Error::other(e.to_string()))?;
    Ok(())
}

/// Record an audit entry in the `tripwire_touches` table.
pub fn record_touch(conn: &Connection, touch: &TripwireTouch) -> io::Result<()> {
    touch.validate_never_zero_value()?;
    conn.execute(
        "INSERT INTO tripwire_touches (touch_id, canary_id, actor, action_type, context_payload, verdict, cortex_notified, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8);",
        params![
            &touch.touch_id,
            &touch.canary_id,
            &touch.actor,
            &touch.action_type,
            &touch.context_payload,
            &touch.verdict,
            if touch.cortex_notified { 1 } else { 0 },
            &touch.created_at,
        ],
    )
    .map_err(|e| io::Error::other(e.to_string()))?;
    Ok(())
}

/// List recent audit records from `tripwire_touches`.
pub fn list_touches(
    conn: &Connection,
    limit: usize,
    canary_id: Option<&str>,
) -> io::Result<Vec<TripwireTouch>> {
    let sql = match canary_id {
        Some(_) => {
            "SELECT touch_id, canary_id, actor, action_type, context_payload, verdict, cortex_notified, created_at
             FROM tripwire_touches WHERE canary_id = ?1 ORDER BY created_at DESC LIMIT ?2;"
        }
        None => {
            "SELECT touch_id, canary_id, actor, action_type, context_payload, verdict, cortex_notified, created_at
             FROM tripwire_touches ORDER BY created_at DESC LIMIT ?1;"
        }
    };

    let mut list = Vec::new();
    if let Some(cid) = canary_id {
        let mut stmt = conn
            .prepare(sql)
            .map_err(|e| io::Error::other(e.to_string()))?;
        let rows = stmt
            .query_map(params![cid, limit as i64], |row| {
                let notified_int: i32 = row.get(6)?;
                Ok(TripwireTouch {
                    touch_id: row.get(0)?,
                    canary_id: row.get(1)?,
                    actor: row.get(2)?,
                    action_type: row.get(3)?,
                    context_payload: row.get(4)?,
                    verdict: row.get(5)?,
                    cortex_notified: notified_int != 0,
                    created_at: row.get(7)?,
                })
            })
            .map_err(|e| io::Error::other(e.to_string()))?;
        for r in rows {
            list.push(r.map_err(|e| io::Error::other(e.to_string()))?);
        }
    } else {
        let mut stmt = conn
            .prepare(sql)
            .map_err(|e| io::Error::other(e.to_string()))?;
        let rows = stmt
            .query_map(params![limit as i64], |row| {
                let notified_int: i32 = row.get(6)?;
                Ok(TripwireTouch {
                    touch_id: row.get(0)?,
                    canary_id: row.get(1)?,
                    actor: row.get(2)?,
                    action_type: row.get(3)?,
                    context_payload: row.get(4)?,
                    verdict: row.get(5)?,
                    cortex_notified: notified_int != 0,
                    created_at: row.get(7)?,
                })
            })
            .map_err(|e| io::Error::other(e.to_string()))?;
        for r in rows {
            list.push(r.map_err(|e| io::Error::other(e.to_string()))?);
        }
    }

    Ok(list)
}

/// Core tripwire guard logic: inspect a target path and optional content.
///
/// If an active canary is matched:
/// - If the actor is authorized, returns `Ok(())`.
/// - If the actor is unauthorized:
///   1. Logs touch into `tripwire_touches` with `verdict = "circuit_broken"`.
///   2. Broadcasts a high-priority Cortex security notification.
///   3. Halts execution fail-closed with `Err(TripwireIntrusionError)`.
pub fn guard_check(
    conn: &Connection,
    target: &str,
    content: Option<&str>,
    actor: &str,
    action: &str,
) -> Result<(), TripwireIntrusionError> {
    let canaries = match list_canaries(conn) {
        Ok(c) => c,
        Err(_) => default_synthetic_canaries(),
    };

    for canary in &canaries {
        if !canary.is_active {
            continue;
        }

        let mut matched = canary.matches_path(target);
        if !matched {
            if let Some(c) = content {
                matched = canary.matches_content(c);
            }
        }

        if matched {
            if canary.is_authorized(actor) {
                // Authorized operator touch; record audit entry if needed
                let touch = TripwireTouch {
                    touch_id: generate_touch_id(),
                    canary_id: canary.id.clone(),
                    actor: actor.to_string(),
                    action_type: action.to_string(),
                    context_payload: Some(target.to_string()),
                    verdict: "permitted".to_string(),
                    cortex_notified: false,
                    created_at: current_iso_timestamp(),
                };
                let _ = record_touch(conn, &touch);
                return Ok(());
            }

            // Unauthorized touch! Fail-closed circuit break
            let touch_id = generate_touch_id();
            let notified = notify_cortex_intrusion(
                &canary.id,
                &canary.target_path,
                actor,
                action,
                Some(target),
            );

            let touch = TripwireTouch {
                touch_id: touch_id.clone(),
                canary_id: canary.id.clone(),
                actor: actor.to_string(),
                action_type: action.to_string(),
                context_payload: Some(format!("Target: {target}")),
                verdict: "circuit_broken".to_string(),
                cortex_notified: notified,
                created_at: current_iso_timestamp(),
            };
            let _ = record_touch(conn, &touch);

            return Err(TripwireIntrusionError(Box::new(TripwireIntrusionData {
                canary_id: canary.id.clone(),
                target_path: canary.target_path.clone(),
                actor: actor.to_string(),
                action: action.to_string(),
                touch_id,
                cortex_notified: notified,
                message: format!(
                    "Tripwire canary `{}` touched by unauthorized actor `{}` on `{}`",
                    canary.id, actor, target
                ),
            })));
        }
    }

    Ok(())
}

/// Guard a collection of target paths (e.g. for `cicatrix query <paths...>`).
pub fn guard_paths(
    conn: &Connection,
    paths: &[String],
    actor: &str,
    action: &str,
) -> Result<(), TripwireIntrusionError> {
    for p in paths {
        guard_check(conn, p, None, actor, action)?;
    }
    Ok(())
}

/// Guard a unified diff patch string (e.g. for reversibility evaluations).
pub fn guard_diff(
    conn: &Connection,
    diff: &str,
    actor: &str,
) -> Result<(), TripwireIntrusionError> {
    // Scan raw diff for canary markers
    guard_check(conn, "diff_evaluation", Some(diff), actor, "diff_eval")?;

    // Also extract paths from diff header lines
    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("+++ b/") {
            guard_check(conn, rest.trim(), None, actor, "diff_target_path")?;
        } else if let Some(rest) = line.strip_prefix("--- a/") {
            guard_check(conn, rest.trim(), None, actor, "diff_source_path")?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn in_memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS tripwire_canaries (
                id TEXT PRIMARY KEY NOT NULL,
                sentinel_marker TEXT NOT NULL,
                target_path TEXT NOT NULL,
                description TEXT NOT NULL,
                authorized_roles TEXT NOT NULL DEFAULT 'operator,harness',
                is_active INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
            );
            CREATE TABLE IF NOT EXISTS tripwire_touches (
                touch_id TEXT PRIMARY KEY NOT NULL,
                canary_id TEXT NOT NULL,
                actor TEXT NOT NULL,
                action_type TEXT NOT NULL,
                context_payload TEXT,
                verdict TEXT NOT NULL,
                cortex_notified INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
            );",
        )
        .unwrap();
        seed_canaries(&conn).unwrap();
        conn
    }

    #[test]
    fn test_seed_and_list_canaries() {
        let conn = in_memory_db();
        let canaries = list_canaries(&conn).unwrap();
        assert_eq!(canaries.len(), 4);
    }

    #[test]
    fn test_guard_authorized_operator_passes() {
        let conn = in_memory_db();
        let res = guard_check(
            &conn,
            ".cicatrix/sentinel/canary_alpha.rs",
            None,
            "operator",
            "query",
        );
        assert!(res.is_ok());

        let touches = list_touches(&conn, 10, None).unwrap();
        assert_eq!(touches.len(), 1);
        assert_eq!(touches[0].verdict, "permitted");
    }

    #[test]
    fn test_guard_unauthorized_agent_circuit_breaks() {
        let conn = in_memory_db();
        let res = guard_check(
            &conn,
            ".cicatrix/sentinel/canary_alpha.rs",
            None,
            "session-claude-agent",
            "query",
        );
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert_eq!(err.canary_id, "TRIPWIRE_CANARY_SENTINEL_ALPHA");

        let touches = list_touches(&conn, 10, None).unwrap();
        assert_eq!(touches.len(), 1);
        assert_eq!(touches[0].verdict, "circuit_broken");
        assert_eq!(touches[0].actor, "session-claude-agent");
    }

    #[test]
    fn test_guard_diff_content_circuit_breaks() {
        let conn = in_memory_db();
        let diff = r#"
diff --git a/foo.rs b/foo.rs
--- a/foo.rs
+++ b/foo.rs
@@ -1,3 +1,3 @@
-let a = 1;
+let a = TRIPWIRE_CANARY_SENTINEL;
"#;
        let res = guard_diff(&conn, diff, "agent-runner");
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert_eq!(err.canary_id, "TRIPWIRE_CANARY_SENTINEL_ALPHA");
    }
}
