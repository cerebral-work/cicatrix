//! Trust ladder promotion/demotion governance and capability gating (CER-2762, Phase 4.1).
//!
//! Provides validation and state machine transitions for the earned autonomy ladder.
//! Enforces the estate invariant: Soma production gates strictly require human verdicts.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::autonomy::error::AutonomyError;
use crate::autonomy::ledger::{
    generate_event_id, get_tier, record_event, AutonomyActionType, AutonomyEvent,
};
use crate::autonomy::soma::{
    is_soma_production_capability, is_soma_production_path, validate_soma_invariant,
};
use crate::autonomy::tier::AutonomyTier;
use crate::tripwire::guard::current_iso_timestamp;

/// Parameters for promoting an actor's capability tier.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromotionRequest {
    /// Actor or agent identifier.
    pub actor: String,
    /// Capability name.
    pub capability: String,
    /// Target higher autonomy tier.
    pub target_tier: AutonomyTier,
    /// Rationale or justification for promotion.
    pub reason: String,
    /// Optional JSON payload containing verification receipts or benchmarks.
    pub evidence_json: Option<String>,
    /// Authority authorizing the promotion.
    pub authorized_by: String,
}

/// Parameters for demoting an actor's capability tier.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DemotionRequest {
    /// Actor or agent identifier.
    pub actor: String,
    /// Capability name.
    pub capability: String,
    /// Target lower autonomy tier.
    pub target_tier: AutonomyTier,
    /// Rationale or root cause for demotion.
    pub reason: String,
    /// Optional JSON payload containing incident or failure details.
    pub evidence_json: Option<String>,
    /// Authority authorizing the demotion.
    pub authorized_by: String,
}

/// Result of evaluating autonomy permission for an action or task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutonomyCheckResult {
    /// Actor evaluated.
    pub actor: String,
    /// Capability evaluated.
    pub capability: String,
    /// Current earned autonomy tier.
    pub current_tier: AutonomyTier,
    /// Alias/copy of current tier for backwards compatibility.
    pub tier: AutonomyTier,
    /// Required autonomy tier for the requested operation.
    pub required_tier: AutonomyTier,
    /// Whether the action is permitted at current autonomy tier without violation.
    pub permitted: bool,
    /// Whether explicit human operator approval is required.
    pub requires_approval: bool,
    /// Whether the action or capability touches a Soma production gate.
    pub is_soma_production: bool,
    /// Whether the action is permitted to execute autonomously without human review.
    pub can_execute_autonomously: bool,
    /// Explanatory justification.
    pub reason: String,
}

/// Validate that a string field satisfies the `:db/neverZeroValue` invariant.
fn validate_non_empty(field_name: &str, value: &str) -> Result<(), AutonomyError> {
    if value.trim().is_empty() {
        Err(AutonomyError::Validation(format!(
            ":db/neverZeroValue violation: `{field_name}` cannot be empty or whitespace"
        )))
    } else {
        Ok(())
    }
}

/// Validate evidence JSON syntax if present.
fn validate_evidence_json(evidence_json: &Option<String>) -> Result<(), AutonomyError> {
    if let Some(ref json_str) = evidence_json {
        serde_json::from_str::<serde_json::Value>(json_str).map_err(|e| {
            AutonomyError::Validation(format!("invalid JSON in evidence_json: {e}"))
        })?;
    }
    Ok(())
}

/// Promote an actor's capability to a higher trust tier.
///
/// Enforces:
/// - `:db/neverZeroValue` parameter validation.
/// - Valid JSON syntax for `evidence_json`.
/// - Soma invariant: Soma production gates can NEVER be promoted to Autonomous.
/// - Ladder monotonicity: Target tier must be strictly higher than current tier.
pub fn promote_capability(
    conn: &Connection,
    req: PromotionRequest,
) -> Result<AutonomyEvent, AutonomyError> {
    validate_non_empty("actor", &req.actor)?;
    validate_non_empty("capability", &req.capability)?;
    validate_non_empty("reason", &req.reason)?;
    validate_non_empty("authorized_by", &req.authorized_by)?;
    validate_evidence_json(&req.evidence_json)?;

    // Invariant: Soma production gates strictly require human verdicts
    validate_soma_invariant(&req.capability, req.target_tier)?;

    let current_tier = get_tier(conn, &req.actor, &req.capability)?;
    if req.target_tier <= current_tier {
        return Err(AutonomyError::InvalidTransition {
            from: current_tier.to_string(),
            to: req.target_tier.to_string(),
            reason: format!(
                "promotion requires target tier `{}` to be strictly higher than current tier `{}`",
                req.target_tier, current_tier
            ),
        });
    }

    let event = AutonomyEvent {
        id: generate_event_id(),
        actor: req.actor,
        capability: req.capability,
        from_tier: current_tier,
        to_tier: req.target_tier,
        action_type: AutonomyActionType::Promote,
        reason: req.reason,
        evidence_json: req.evidence_json,
        authorized_by: req.authorized_by,
        created_at: current_iso_timestamp(),
    };

    record_event(conn, &event)?;
    Ok(event)
}

/// Demote an actor's capability to a lower trust tier.
///
/// Enforces:
/// - `:db/neverZeroValue` parameter validation.
/// - Valid JSON syntax for `evidence_json`.
/// - Ladder monotonicity: Target tier must be strictly lower than current tier.
pub fn demote_capability(
    conn: &Connection,
    req: DemotionRequest,
) -> Result<AutonomyEvent, AutonomyError> {
    validate_non_empty("actor", &req.actor)?;
    validate_non_empty("capability", &req.capability)?;
    validate_non_empty("reason", &req.reason)?;
    validate_non_empty("authorized_by", &req.authorized_by)?;
    validate_evidence_json(&req.evidence_json)?;

    let current_tier = get_tier(conn, &req.actor, &req.capability)?;
    if req.target_tier >= current_tier {
        return Err(AutonomyError::InvalidTransition {
            from: current_tier.to_string(),
            to: req.target_tier.to_string(),
            reason: format!(
                "demotion requires target tier `{}` to be strictly lower than current tier `{}`",
                req.target_tier, current_tier
            ),
        });
    }

    let event = AutonomyEvent {
        id: generate_event_id(),
        actor: req.actor,
        capability: req.capability,
        from_tier: current_tier,
        to_tier: req.target_tier,
        action_type: AutonomyActionType::Demote,
        reason: req.reason,
        evidence_json: req.evidence_json,
        authorized_by: req.authorized_by,
        created_at: current_iso_timestamp(),
    };

    record_event(conn, &event)?;
    Ok(event)
}

/// Check autonomy permissions for a given actor, capability, required tier, and optional touched path.
///
/// Strictly enforces the Soma invariant:
/// Soma production gates strictly require human operator verdicts.
pub fn check_capability_autonomy(
    conn: &Connection,
    actor: &str,
    capability: &str,
    required_tier: AutonomyTier,
    target_path: Option<&str>,
) -> Result<AutonomyCheckResult, AutonomyError> {
    let current_tier = get_tier(conn, actor, capability)?;
    let is_soma = is_soma_production_capability(capability)
        || target_path.is_some_and(is_soma_production_path);

    if is_soma {
        let permitted = if required_tier == AutonomyTier::Autonomous {
            false
        } else {
            current_tier >= required_tier
        };
        let reason = if required_tier == AutonomyTier::Autonomous {
            "Soma production gates strictly require human operator verdicts: autonomous execution is prohibited".to_string()
        } else {
            format!("Soma production gates strictly require human operator verdicts (current tier: `{current_tier}`, required: `{required_tier}`)")
        };

        Ok(AutonomyCheckResult {
            actor: actor.to_string(),
            capability: capability.to_string(),
            current_tier,
            tier: current_tier,
            required_tier,
            permitted,
            requires_approval: true,
            is_soma_production: true,
            can_execute_autonomously: false,
            reason,
        })
    } else {
        let permitted = current_tier >= required_tier;
        let can_execute_autonomously = current_tier.is_autonomous() && permitted;
        let requires_approval = !can_execute_autonomously;
        let reason = if permitted {
            format!("Actor `{actor}` has earned tier `{current_tier}` which meets or exceeds required tier `{required_tier}`")
        } else {
            format!("Actor `{actor}` has earned tier `{current_tier}` which is insufficient for required tier `{required_tier}`")
        };

        Ok(AutonomyCheckResult {
            actor: actor.to_string(),
            capability: capability.to_string(),
            current_tier,
            tier: current_tier,
            required_tier,
            permitted,
            requires_approval,
            is_soma_production: false,
            can_execute_autonomously,
            reason,
        })
    }
}

/// Seed initial baseline capabilities if not already present in `autonomy_state`.
pub fn seed_default_capabilities(conn: &Connection) -> Result<(), AutonomyError> {
    let defaults = [
        (
            "default",
            "speculative_sandbox",
            AutonomyTier::Autonomous,
            "Speculative sandboxes execute autonomously without side effects",
        ),
        (
            "default",
            "background_ops",
            AutonomyTier::Supervised,
            "Background operational tasks require supervision by default",
        ),
        (
            "default",
            "soma_production_gate",
            AutonomyTier::Shadow,
            "Soma production gates default strictly to shadow/operator verdict",
        ),
        (
            "harness",
            "speculative_sandbox",
            AutonomyTier::Autonomous,
            "Harness speculative sandbox verification",
        ),
        (
            "harness",
            "background_ops",
            AutonomyTier::Supervised,
            "Harness background operations",
        ),
        (
            "harness",
            "soma_production_gate",
            AutonomyTier::Shadow,
            "Harness Soma production gate compliance",
        ),
        (
            "wheelhorse",
            "code_edit",
            AutonomyTier::Autonomous,
            "Code editing in speculative sandbox operates autonomously",
        ),
        (
            "wheelhorse",
            "speculative_sandbox",
            AutonomyTier::Autonomous,
            "Speculative sandboxes execute autonomously without side effects",
        ),
        (
            "wheelhorse",
            "background_ops",
            AutonomyTier::Supervised,
            "Background operational tasks require supervision by default",
        ),
        (
            "wheelhorse",
            "soma_production_gate",
            AutonomyTier::Supervised,
            "Soma production gates require human supervision/operator verdict",
        ),
    ];

    for (actor, cap, tier, reason) in defaults {
        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM autonomy_state WHERE actor = ?1 AND capability = ?2",
                [actor, cap],
                |_| Ok(true),
            )
            .unwrap_or(false);

        if !exists {
            let event = AutonomyEvent {
                id: generate_event_id(),
                actor: actor.to_string(),
                capability: cap.to_string(),
                from_tier: AutonomyTier::Shadow,
                to_tier: tier,
                action_type: AutonomyActionType::Initialize,
                reason: reason.to_string(),
                evidence_json: None,
                authorized_by: "system_init".to_string(),
                created_at: current_iso_timestamp(),
            };
            record_event(conn, &event)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::sqlite::SqliteStore;

    #[test]
    fn test_promotion_and_demotion_lifecycle() {
        let store = SqliteStore::open_in_memory().unwrap();
        let conn = store.conn();

        // 1. Initial query on unrecorded capability returns Shadow
        let tier = get_tier(conn, "test-agent", "regression_triage").unwrap();
        assert_eq!(tier, AutonomyTier::Shadow);

        // 2. Promote Shadow -> Supervised
        let req1 = PromotionRequest {
            actor: "test-agent".to_string(),
            capability: "regression_triage".to_string(),
            target_tier: AutonomyTier::Supervised,
            reason: "Completed 10 shadow triage passes without errors".to_string(),
            evidence_json: Some(r#"{"passes": 10, "error_count": 0}"#.to_string()),
            authorized_by: "operator".to_string(),
        };
        let evt1 = promote_capability(conn, req1).unwrap();
        assert_eq!(evt1.from_tier, AutonomyTier::Shadow);
        assert_eq!(evt1.to_tier, AutonomyTier::Supervised);

        let tier = get_tier(conn, "test-agent", "regression_triage").unwrap();
        assert_eq!(tier, AutonomyTier::Supervised);

        // 3. Promote Supervised -> Autonomous
        let req2 = PromotionRequest {
            actor: "test-agent".to_string(),
            capability: "regression_triage".to_string(),
            target_tier: AutonomyTier::Autonomous,
            reason: "Passed all regression verification gates and benchmarks".to_string(),
            evidence_json: Some(r#"{"bench_score": 99.4}"#.to_string()),
            authorized_by: "operator".to_string(),
        };
        let evt2 = promote_capability(conn, req2).unwrap();
        assert_eq!(evt2.from_tier, AutonomyTier::Supervised);
        assert_eq!(evt2.to_tier, AutonomyTier::Autonomous);

        let tier = get_tier(conn, "test-agent", "regression_triage").unwrap();
        assert_eq!(tier, AutonomyTier::Autonomous);

        // 4. Invalid promotion: cannot promote to equal or lower tier
        let req_invalid = PromotionRequest {
            actor: "test-agent".to_string(),
            capability: "regression_triage".to_string(),
            target_tier: AutonomyTier::Autonomous,
            reason: "Redundant promotion".to_string(),
            evidence_json: None,
            authorized_by: "operator".to_string(),
        };
        assert!(promote_capability(conn, req_invalid).is_err());

        // 5. Demote Autonomous -> Supervised
        let demote_req = DemotionRequest {
            actor: "test-agent".to_string(),
            capability: "regression_triage".to_string(),
            target_tier: AutonomyTier::Supervised,
            reason: "Detected false positive triage report".to_string(),
            evidence_json: Some(r#"{"incident_id": "INC-104"}"#.to_string()),
            authorized_by: "harness".to_string(),
        };
        let evt_demote = demote_capability(conn, demote_req).unwrap();
        assert_eq!(evt_demote.from_tier, AutonomyTier::Autonomous);
        assert_eq!(evt_demote.to_tier, AutonomyTier::Supervised);

        let tier = get_tier(conn, "test-agent", "regression_triage").unwrap();
        assert_eq!(tier, AutonomyTier::Supervised);
    }

    #[test]
    fn test_soma_invariant_rejection() {
        let store = SqliteStore::open_in_memory().unwrap();
        let conn = store.conn();

        let req = PromotionRequest {
            actor: "test-agent".to_string(),
            capability: "soma_production_gate".to_string(),
            target_tier: AutonomyTier::Autonomous,
            reason: "Attempt unauthorized promotion of production gate".to_string(),
            evidence_json: None,
            authorized_by: "rogue-agent".to_string(),
        };

        let result = promote_capability(conn, req);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, AutonomyError::SomaInvariantViolation(_)));
    }

    #[test]
    fn test_autonomy_check_soma_surface() {
        let store = SqliteStore::open_in_memory().unwrap();
        let conn = store.conn();

        // Promote background_ops to Autonomous
        let req = PromotionRequest {
            actor: "agent-1".to_string(),
            capability: "background_ops".to_string(),
            target_tier: AutonomyTier::Autonomous,
            reason: "Promote for background work".to_string(),
            evidence_json: None,
            authorized_by: "operator".to_string(),
        };
        promote_capability(conn, req).unwrap();

        // 1. Check ordinary path -> can execute autonomously
        let check1 = check_capability_autonomy(
            conn,
            "agent-1",
            "background_ops",
            AutonomyTier::Autonomous,
            Some("src/main.rs"),
        )
        .unwrap();
        assert!(check1.can_execute_autonomously);
        assert!(!check1.is_soma_production);
        assert!(check1.permitted);
        assert!(!check1.requires_approval);

        // 2. Check Soma production path -> rejected, requires human verdict
        let check2 = check_capability_autonomy(
            conn,
            "agent-1",
            "background_ops",
            AutonomyTier::Autonomous,
            Some(".soma/deploy.yaml"),
        )
        .unwrap();
        assert!(!check2.can_execute_autonomously);
        assert!(check2.is_soma_production);
        assert!(!check2.permitted);
        assert!(check2.requires_approval);
        assert!(check2
            .reason
            .contains("Soma production gates strictly require human operator verdicts"));
    }
}
