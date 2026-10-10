//! Soma production gate invariant enforcement (CER-2762, Phase 4.1).
//!
//! Estate Invariant:
//! Soma production gates strictly require human operator verdicts.
//! Earned autonomy applies only to background operational tasks and speculative sandboxes.
//! Under no circumstance may any Soma production gate, release pipeline, or cluster deployment
//! be promoted to autonomous execution.

use crate::autonomy::error::AutonomyError;
use crate::autonomy::tier::AutonomyTier;

/// Check whether a capability name identifies a Soma production gate.
pub fn is_soma_production_capability(capability: &str) -> bool {
    let lower = capability.trim().to_lowercase();
    lower.starts_with("soma")
        || lower.contains("soma_gate")
        || lower.contains("soma_prod")
        || lower.contains("production_gate")
        || lower.contains("prod_deploy")
        || lower.contains("cluster_mutation")
}

/// Check whether a file or directory path identifies a Soma production surface.
pub fn is_soma_production_path(path: &str) -> bool {
    let p = path.trim().to_lowercase();
    p.starts_with(".soma/")
        || p.starts_with("soma/")
        || p.starts_with("crates/soma/")
        || p.contains("/soma/")
        || p.ends_with("/soma")
        || p == "soma.yaml"
        || p == "soma.yml"
        || p == "soma.toml"
        || p.contains("soma_gate")
        || p.contains("soma_production")
        || p.contains("deploy/prod")
        || p.contains("deploy/production")
        || p.contains("k8s/prod")
        || p.contains("k8s/production")
        || p.contains("infra/prod")
        || p.contains("clusters/prod")
}

/// Enforce the Soma invariant for promotion requests.
///
/// Returns Err(AutonomyError::SomaInvariantViolation) if attempting to promote
/// a Soma production capability to Autonomous.
pub fn validate_soma_invariant(
    capability: &str,
    target_tier: AutonomyTier,
) -> Result<(), AutonomyError> {
    if is_soma_production_capability(capability) && target_tier == AutonomyTier::Autonomous {
        return Err(AutonomyError::SomaInvariantViolation(format!(
            "capability `{capability}` identifies a Soma production gate: Soma production gates strictly require human operator verdicts and cannot be promoted to autonomous tier"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_soma_capability_detection() {
        assert!(is_soma_production_capability("soma_gate"));
        assert!(is_soma_production_capability("soma_prod_deploy"));
        assert!(is_soma_production_capability("soma-release"));
        assert!(is_soma_production_capability("production_gate"));
        assert!(!is_soma_production_capability("background_ops"));
        assert!(!is_soma_production_capability("speculative_sandbox"));
        assert!(!is_soma_production_capability("regression_triage"));
    }

    #[test]
    fn test_soma_path_detection() {
        assert!(is_soma_production_path(".soma/config.yaml"));
        assert!(is_soma_production_path("soma.yaml"));
        assert!(is_soma_production_path("deploy/prod/release.yaml"));
        assert!(is_soma_production_path("k8s/production/manifest.yaml"));
        assert!(!is_soma_production_path("src/main.rs"));
        assert!(!is_soma_production_path("tests/cli.rs"));
    }

    #[test]
    fn test_soma_invariant_validation() {
        // Supervised and Shadow are permitted for Soma gates
        assert!(validate_soma_invariant("soma_gate", AutonomyTier::Shadow).is_ok());
        assert!(validate_soma_invariant("soma_gate", AutonomyTier::Supervised).is_ok());

        // Autonomous is strictly rejected for Soma gates
        assert!(validate_soma_invariant("soma_gate", AutonomyTier::Autonomous).is_err());
        assert!(validate_soma_invariant("soma_prod_deploy", AutonomyTier::Autonomous).is_err());

        // Non-Soma capabilities can be Autonomous
        assert!(validate_soma_invariant("background_ops", AutonomyTier::Autonomous).is_ok());
        assert!(validate_soma_invariant("speculative_sandbox", AutonomyTier::Autonomous).is_ok());
    }
}
