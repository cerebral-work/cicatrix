//! Decision gate for Wheelhorse reversibility pipeline (CER-2759).
//!
//! Evaluates the 4th stage of the pipeline:
//! returns `AutoCommit`, `AutoCommitWithNotice`, `RouteToApproval`, or `Block`
//! based on action classification, speculative validation results, and autonomy tier.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

use super::classify::ActionClass;
use super::validate::ValidationResult;

/// Autonomy capability levels under the earned autonomy trust ladder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutonomyTier {
    /// Shadow mode: executes actions in dry-run / speculative sandbox only.
    Shadow,
    /// Supervised mode: human approval required for one-way doors; notice for reversible changes.
    #[default]
    Supervised,
    /// Autonomous mode: auto-commits pure additions; captures rollback notices for modifications.
    Autonomous,
}

impl FromStr for AutonomyTier {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "shadow" => Ok(Self::Shadow),
            "supervised" => Ok(Self::Supervised),
            "autonomous" => Ok(Self::Autonomous),
            other => Err(format!(
                "invalid autonomy tier `{other}`; expected: shadow, supervised, or autonomous"
            )),
        }
    }
}

impl fmt::Display for AutonomyTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Shadow => write!(f, "shadow"),
            Self::Supervised => write!(f, "supervised"),
            Self::Autonomous => write!(f, "autonomous"),
        }
    }
}

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
    // Tripwire canary sentinels fail closed immediately
    if canary_touched {
        return ReversibilityVerdict::Block {
            violation: "Tripwire canary touched: unauthorized mutation to regression sentinel"
                .to_string(),
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
}
