//! Append-only audit ledger and state persistence for earned autonomy (CER-2762, Phase 4.1).
//!
//! Enforces:
//! - Append-only immutability (updates and deletions prohibited at engine level).
//! - `:db/neverZeroValue` schema integrity.
//! - Automatic synchronization of current state per `(actor, capability)`.

use std::fmt;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static EVENT_COUNTER: AtomicU64 = AtomicU64::new(1);

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::autonomy::error::AutonomyError;
use crate::autonomy::tier::AutonomyTier;

/// Action category recorded in the autonomy audit ledger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutonomyActionType {
    /// Capability promoted to a higher trust tier.
    Promote,
    /// Capability demoted to a lower trust tier.
    Demote,
    /// Baseline capability state initialized.
    Initialize,
}

impl AutonomyActionType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Promote => "promote",
            Self::Demote => "demote",
            Self::Initialize => "initialize",
        }
    }
}

impl FromStr for AutonomyActionType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "promote" => Ok(Self::Promote),
            "demote" => Ok(Self::Demote),
            "initialize" => Ok(Self::Initialize),
            other => Err(format!(
                "invalid autonomy action type `{other}`; expected: promote, demote, or initialize"
            )),
        }
    }
}

impl fmt::Display for AutonomyActionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Immutable record appended to `autonomy_ledger`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutonomyEvent {
    /// Unique identifier for the audit event (e.g. `aut_evt_...`).
    pub id: String,
    /// Actor or agent identifier (e.g. `agent-claude`, `ceres-runner`, `default`).
    pub actor: String,
    /// Capability name (e.g. `speculative_sandbox`, `background_ops`, `regression_triage`).
    pub capability: String,
    /// Source tier prior to transition.
    pub from_tier: AutonomyTier,
    /// Target tier following transition.
    pub to_tier: AutonomyTier,
    /// Action type (promote, demote, initialize).
    pub action_type: AutonomyActionType,
    /// Justification or evidence rationale.
    pub reason: String,
    /// Optional JSON payload containing verification receipts, test results, or approvals.
    pub evidence_json: Option<String>,
    /// Authority authorizing the transition (operator, harness, automated).
    pub authorized_by: String,
    /// ISO 8601 UTC creation timestamp.
    pub created_at: String,
}

/// Materialized current state for an actor's capability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutonomyState {
    /// Actor or agent identifier.
    pub actor: String,
    /// Capability name.
    pub capability: String,
    /// Current earned autonomy tier.
    pub current_tier: AutonomyTier,
    /// Alias/copy of current tier.
    pub tier: AutonomyTier,
    /// Identifier of the latest audit event that set this tier.
    pub last_event_id: String,
    /// Timestamp of the last state update.
    pub updated_at: String,
}

/// Generate a unique timestamped event ID.
pub fn generate_event_id() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let seq = EVENT_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!(
        "aut_evt_{:016x}_{:08x}_{:08x}",
        now.as_secs(),
        now.subsec_nanos(),
        seq
    )
}

/// Append an event to the immutable `autonomy_ledger`.
pub fn record_event(conn: &Connection, event: &AutonomyEvent) -> Result<(), AutonomyError> {
    if event.id.trim().is_empty()
        || event.actor.trim().is_empty()
        || event.capability.trim().is_empty()
        || event.reason.trim().is_empty()
        || event.authorized_by.trim().is_empty()
    {
        return Err(AutonomyError::Validation(
            ":db/neverZeroValue constraint violation: id, actor, capability, reason, and authorized_by cannot be empty"
                .to_string(),
        ));
    }

    conn.execute(
        "INSERT INTO autonomy_ledger (
            id, actor, capability, from_tier, to_tier, action_type, reason, evidence_json, authorized_by, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            event.id,
            event.actor,
            event.capability,
            event.from_tier.as_str(),
            event.to_tier.as_str(),
            event.action_type.as_str(),
            event.reason,
            event.evidence_json,
            event.authorized_by,
            event.created_at,
        ],
    )?;

    Ok(())
}

/// Retrieve the current autonomy tier for a given actor and capability.
///
/// If no state has been recorded, defaults to `AutonomyTier::Shadow`.
pub fn get_tier(
    conn: &Connection,
    actor: &str,
    capability: &str,
) -> Result<AutonomyTier, AutonomyError> {
    let mut stmt = conn
        .prepare("SELECT current_tier FROM autonomy_state WHERE actor = ?1 AND capability = ?2")?;

    let mut rows = stmt.query(params![actor, capability])?;
    if let Some(row) = rows.next()? {
        let tier_str: String = row.get(0)?;
        tier_str
            .parse::<AutonomyTier>()
            .map_err(AutonomyError::Database)
    } else {
        Ok(AutonomyTier::Shadow)
    }
}

/// Retrieve the complete `AutonomyState` for a given actor and capability.
pub fn get_state(
    conn: &Connection,
    actor: &str,
    capability: &str,
) -> Result<Option<AutonomyState>, AutonomyError> {
    let mut stmt = conn.prepare(
        "SELECT actor, capability, current_tier, last_event_id, updated_at
         FROM autonomy_state WHERE actor = ?1 AND capability = ?2",
    )?;

    let mut rows = stmt.query(params![actor, capability])?;
    if let Some(row) = rows.next()? {
        let actor: String = row.get(0)?;
        let capability: String = row.get(1)?;
        let tier_str: String = row.get(2)?;
        let last_event_id: String = row.get(3)?;
        let updated_at: String = row.get(4)?;
        let current_tier = tier_str
            .parse::<AutonomyTier>()
            .map_err(AutonomyError::Database)?;

        Ok(Some(AutonomyState {
            actor,
            capability,
            current_tier,
            tier: current_tier,
            last_event_id,
            updated_at,
        }))
    } else {
        Ok(None)
    }
}

/// List all materialized autonomy states, optionally filtered by actor.
pub fn list_states(
    conn: &Connection,
    actor: Option<&str>,
) -> Result<Vec<AutonomyState>, AutonomyError> {
    let mut results = Vec::new();

    if let Some(act) = actor {
        let mut stmt = conn.prepare(
            "SELECT actor, capability, current_tier, last_event_id, updated_at
             FROM autonomy_state WHERE actor = ?1 ORDER BY capability ASC",
        )?;
        let mut rows = stmt.query(params![act])?;
        while let Some(row) = rows.next()? {
            let a: String = row.get(0)?;
            let c: String = row.get(1)?;
            let t: String = row.get(2)?;
            let l: String = row.get(3)?;
            let u: String = row.get(4)?;
            let current_tier = t.parse::<AutonomyTier>().map_err(AutonomyError::Database)?;
            results.push(AutonomyState {
                actor: a,
                capability: c,
                current_tier,
                tier: current_tier,
                last_event_id: l,
                updated_at: u,
            });
        }
    } else {
        let mut stmt = conn.prepare(
            "SELECT actor, capability, current_tier, last_event_id, updated_at
             FROM autonomy_state ORDER BY actor ASC, capability ASC",
        )?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let a: String = row.get(0)?;
            let c: String = row.get(1)?;
            let t: String = row.get(2)?;
            let l: String = row.get(3)?;
            let u: String = row.get(4)?;
            let current_tier = t.parse::<AutonomyTier>().map_err(AutonomyError::Database)?;
            results.push(AutonomyState {
                actor: a,
                capability: c,
                current_tier,
                tier: current_tier,
                last_event_id: l,
                updated_at: u,
            });
        }
    }

    Ok(results)
}

/// List recent audit ledger events, ordered by timestamp descending.
pub fn list_events(
    conn: &Connection,
    actor: Option<&str>,
    capability: Option<&str>,
    limit: usize,
) -> Result<Vec<AutonomyEvent>, AutonomyError> {
    let lim = if limit == 0 { 50 } else { limit };
    let mut results = Vec::new();

    let query = match (actor, capability) {
        (Some(_), Some(_)) => {
            "SELECT id, actor, capability, from_tier, to_tier, action_type, reason, evidence_json, authorized_by, created_at
             FROM autonomy_ledger WHERE actor = ?1 AND capability = ?2 ORDER BY created_at DESC, id DESC LIMIT ?3"
        }
        (Some(_), None) => {
            "SELECT id, actor, capability, from_tier, to_tier, action_type, reason, evidence_json, authorized_by, created_at
             FROM autonomy_ledger WHERE actor = ?1 ORDER BY created_at DESC, id DESC LIMIT ?2"
        }
        (None, Some(_)) => {
            "SELECT id, actor, capability, from_tier, to_tier, action_type, reason, evidence_json, authorized_by, created_at
             FROM autonomy_ledger WHERE capability = ?1 ORDER BY created_at DESC, id DESC LIMIT ?2"
        }
        (None, None) => {
            "SELECT id, actor, capability, from_tier, to_tier, action_type, reason, evidence_json, authorized_by, created_at
             FROM autonomy_ledger ORDER BY created_at DESC, id DESC LIMIT ?1"
        }
    };

    let mut stmt = conn.prepare(query)?;
    let mut rows = match (actor, capability) {
        (Some(a), Some(c)) => stmt.query(params![a, c, lim as i64])?,
        (Some(a), None) => stmt.query(params![a, lim as i64])?,
        (None, Some(c)) => stmt.query(params![c, lim as i64])?,
        (None, None) => stmt.query(params![lim as i64])?,
    };

    while let Some(row) = rows.next()? {
        let id: String = row.get(0)?;
        let a: String = row.get(1)?;
        let c: String = row.get(2)?;
        let ft: String = row.get(3)?;
        let tt: String = row.get(4)?;
        let at: String = row.get(5)?;
        let reason: String = row.get(6)?;
        let ev: Option<String> = row.get(7)?;
        let auth: String = row.get(8)?;
        let ts: String = row.get(9)?;

        results.push(AutonomyEvent {
            id,
            actor: a,
            capability: c,
            from_tier: ft
                .parse::<AutonomyTier>()
                .map_err(AutonomyError::Database)?,
            to_tier: tt
                .parse::<AutonomyTier>()
                .map_err(AutonomyError::Database)?,
            action_type: at
                .parse::<AutonomyActionType>()
                .map_err(AutonomyError::Database)?,
            reason,
            evidence_json: ev,
            authorized_by: auth,
            created_at: ts,
        });
    }

    Ok(results)
}
