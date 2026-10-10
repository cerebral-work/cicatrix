//! Decision gate for Wheelhorse reversibility pipeline (CER-2759).
//!
//! Evaluates the 4th stage of the pipeline:
//! returns `AutoCommit`, `AutoCommitWithNotice`, `RouteToApproval`, or `Block`
//! based on action classification, speculative validation results, and autonomy tier.

use super::classify::ActionClass;
use super::validate::ValidationResult;
use serde::{Deserialize, Serialize};

pub use crate::autonomy::AutonomyTier;

/// Final gate verdict returned by the reversibility pipeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "verdict", rename_all = "snake_case")]
pub enum ReversibilityVerdict {
    /// Action executes autonomously without interruption.
    AutoCommit,
    /// Action executes autonomously; notice recorded in audit journal.
    AutoCommitWithNotice { notice: String },
    /// Action halts; requires durable approval signal from human operator.
    RouteToApproval {
        signal_name: String,
        timeout_seconds: u64,
    },
    /// Action rejected immediately due to policy violation or canary tripwire.
    Block { violation: String },
}

/// Evaluate final reversibility verdict.
pub fn evaluate_verdict(
    action_class: &ActionClass,
    validation: &ValidationResult,
    tier: AutonomyTier,
    canary_touched: bool,
) -> ReversibilityVerdict {
    evaluate_verdict_with_soma(action_class, validation, tier, canary_touched, false)
}

/// Evaluate final reversibility verdict, enforcing canary tripwires and the Soma invariant.
pub fn evaluate_verdict_with_soma(
    action_class: &ActionClass,
    validation: &ValidationResult,
    tier: AutonomyTier,
    canary_touched: bool,
    soma_production_touched: bool,
) -> ReversibilityVerdict {
    // Tripwire canary sentinels fail closed immediately
    if canary_touched {
        return ReversibilityVerdict::Block {
            violation: "Tripwire canary touched: unauthorized mutation to regression sentinel"
                .to_string(),
        };
    }

    // Estate Invariant: Soma production gates strictly require human operator verdicts
    if soma_production_touched {
        return ReversibilityVerdict::RouteToApproval {
            signal_name: "operator_verdict".to_string(),
            timeout_seconds: 3600,
        };
    }

    match validation {
        ValidationResult::Invalid { reason } => ReversibilityVerdict::Block {
            violation: format!("Compensation plan validation failed: {reason}"),
        },
        ValidationResult::OneWayDoorSkipped { reason: _ } => {
            ReversibilityVerdict::RouteToApproval {
                signal_name: "operator_verdict".to_string(),
                timeout_seconds: 3600,
            }
        }
        ValidationResult::Valid { .. } => match action_class {
            ActionClass::OneWayDoor { .. } => ReversibilityVerdict::RouteToApproval {
                signal_name: "operator_verdict".to_string(),
                timeout_seconds: 3600,
            },
            ActionClass::ReversibleAdditive => match tier {
                AutonomyTier::Autonomous => ReversibilityVerdict::AutoCommit,
                AutonomyTier::Supervised => ReversibilityVerdict::AutoCommitWithNotice {
                    notice: "Additive change verified: automated deletion plan captured"
                        .to_string(),
                },
                AutonomyTier::Shadow => ReversibilityVerdict::AutoCommitWithNotice {
                    notice: "Shadow tier: simulated additive change without side effects"
                        .to_string(),
                },
            },
            ActionClass::ReversibleWithPlan { .. } => match tier {
                AutonomyTier::Autonomous => ReversibilityVerdict::AutoCommitWithNotice {
                    notice: "Reversible change verified against baseline sandbox".to_string(),
                },
                AutonomyTier::Supervised => ReversibilityVerdict::AutoCommitWithNotice {
                    notice: "Supervised tier: verified rollback plan captured".to_string(),
                },
                AutonomyTier::Shadow => ReversibilityVerdict::AutoCommitWithNotice {
                    notice: "Shadow tier: speculative rollback execution verified".to_string(),
                },
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_canary_trips_to_block() {
        let action = ActionClass::ReversibleAdditive;
        let val = ValidationResult::Valid {
            baseline_restored: true,
            side_effects_count: 0,
            audit_receipt: "ok".into(),
        };
        let verdict = evaluate_verdict(&action, &val, AutonomyTier::Autonomous, true);
        assert!(matches!(verdict, ReversibilityVerdict::Block { .. }));
    }

    #[test]
    fn test_invalid_plan_trips_to_block() {
        let action = ActionClass::ReversibleWithPlan {
            rollback_script: "dummy".into(),
        };
        let val = ValidationResult::Invalid {
            reason: "mismatched hash".into(),
        };
        let verdict = evaluate_verdict(&action, &val, AutonomyTier::Autonomous, false);
        match verdict {
            ReversibilityVerdict::Block { violation } => {
                assert!(violation.contains("mismatched hash"));
            }
            other => panic!("expected Block verdict, got {other:?}"),
        }
    }

    #[test]
    fn test_one_way_door_routes_to_approval() {
        let action = ActionClass::OneWayDoor {
            reason: "table drop".into(),
        };
        let val = ValidationResult::OneWayDoorSkipped {
            reason: "table drop".into(),
        };
        let verdict = evaluate_verdict(&action, &val, AutonomyTier::Autonomous, false);
        match verdict {
            ReversibilityVerdict::RouteToApproval { signal_name, .. } => {
                assert_eq!(signal_name, "operator_verdict");
            }
            other => panic!("expected RouteToApproval verdict, got {other:?}"),
        }
    }

    #[test]
    fn test_additive_autonomous_autocommit() {
        let action = ActionClass::ReversibleAdditive;
        let val = ValidationResult::Valid {
            baseline_restored: true,
            side_effects_count: 0,
            audit_receipt: "ok".into(),
        };
        let verdict = evaluate_verdict(&action, &val, AutonomyTier::Autonomous, false);
        assert_eq!(verdict, ReversibilityVerdict::AutoCommit);
    }

    #[test]
    fn test_reversible_supervised_notice() {
        let action = ActionClass::ReversibleWithPlan {
            rollback_script: "revert".into(),
        };
        let val = ValidationResult::Valid {
            baseline_restored: true,
            side_effects_count: 0,
            audit_receipt: "ok".into(),
        };
        let verdict = evaluate_verdict(&action, &val, AutonomyTier::Supervised, false);
        assert!(matches!(
            verdict,
            ReversibilityVerdict::AutoCommitWithNotice { .. }
        ));
    }

    #[test]
    fn test_soma_production_routes_to_approval() {
        // Even for an additive change and Autonomous tier, touching Soma production
        // MUST strictly route to operator approval.
        let action = ActionClass::ReversibleAdditive;
        let val = ValidationResult::Valid {
            baseline_restored: true,
            side_effects_count: 0,
            audit_receipt: "ok".into(),
        };
        let verdict =
            evaluate_verdict_with_soma(&action, &val, AutonomyTier::Autonomous, false, true);
        match verdict {
            ReversibilityVerdict::RouteToApproval { signal_name, .. } => {
                assert_eq!(signal_name, "operator_verdict");
            }
            other => panic!("expected RouteToApproval for Soma production gate, got {other:?}"),
        }
    }
}
