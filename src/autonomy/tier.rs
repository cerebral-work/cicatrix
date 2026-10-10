//! Autonomy capability levels under the earned autonomy trust ladder (CER-2762, Phase 4.1).
//!
//! Three-tier ladder:
//! 1. `Shadow`: Dry-run and speculative sandbox execution only. No outward mutation.
//! 2. `Supervised`: Human approval required for one-way doors / production gates; notice for reversible changes.
//! 3. `Autonomous`: Auto-commits reversible additive changes, captures rollback notices for reversible modifications.
//!
//! Enforces the estate invariant: Soma production gates strictly require human operator verdicts;
//! earned autonomy applies only to background operational tasks and speculative sandboxes.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Autonomy capability levels under the earned autonomy trust ladder.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize, Hash,
)]
#[serde(rename_all = "snake_case")]
pub enum AutonomyTier {
    /// Shadow mode: executes actions in dry-run / speculative sandbox only.
    Shadow = 0,
    /// Supervised mode: human approval required for one-way doors; notice for reversible changes.
    #[default]
    Supervised = 1,
    /// Autonomous mode: auto-commits pure additions; captures rollback notices for modifications.
    Autonomous = 2,
}

impl AutonomyTier {
    pub const ALL: &'static [AutonomyTier] = &[
        AutonomyTier::Shadow,
        AutonomyTier::Supervised,
        AutonomyTier::Autonomous,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Shadow => "shadow",
            Self::Supervised => "supervised",
            Self::Autonomous => "autonomous",
        }
    }

    pub fn is_shadow(&self) -> bool {
        matches!(self, Self::Shadow)
    }

    pub fn is_supervised(&self) -> bool {
        matches!(self, Self::Supervised)
    }

    pub fn is_autonomous(&self) -> bool {
        matches!(self, Self::Autonomous)
    }

    pub fn can_auto_commit(&self) -> bool {
        self.is_autonomous()
    }

    pub fn rank(&self) -> u8 {
        *self as u8
    }
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
        write!(f, "{}", self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tier_ordering() {
        assert!(AutonomyTier::Shadow < AutonomyTier::Supervised);
        assert!(AutonomyTier::Supervised < AutonomyTier::Autonomous);
        assert!(AutonomyTier::Shadow < AutonomyTier::Autonomous);
    }

    #[test]
    fn test_tier_from_str() {
        assert_eq!(
            "shadow".parse::<AutonomyTier>().unwrap(),
            AutonomyTier::Shadow
        );
        assert_eq!(
            "SUPERVISED".parse::<AutonomyTier>().unwrap(),
            AutonomyTier::Supervised
        );
        assert_eq!(
            "Autonomous".parse::<AutonomyTier>().unwrap(),
            AutonomyTier::Autonomous
        );
        assert!("invalid".parse::<AutonomyTier>().is_err());
    }

    #[test]
    fn test_tier_display() {
        assert_eq!(AutonomyTier::Shadow.to_string(), "shadow");
        assert_eq!(AutonomyTier::Supervised.to_string(), "supervised");
        assert_eq!(AutonomyTier::Autonomous.to_string(), "autonomous");
    }
}
