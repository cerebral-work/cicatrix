//! Synthetic canary data contracts and validations (CER-2760, Phase 3.2).
//!
//! Provides `TripwireCanary` and `TripwireTouch` data structures complying with
//! `:db/neverZeroValue` schema integrity.

use std::fmt;
use std::io;

use serde::{Deserialize, Serialize};

/// Synthetic regression canary entry registered in the database.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TripwireCanary {
    /// Unique identifier for the canary (e.g., `TRIPWIRE_CANARY_SENTINEL_ALPHA`).
    pub id: String,
    /// Known sentinel marker string embedded in paths or diff content.
    pub sentinel_marker: String,
    /// Targeted sensitive or sentinel file path.
    pub target_path: String,
    /// Human-readable explanation of the tripwire defense purpose.
    pub description: String,
    /// Comma-separated list or collection of roles permitted to touch this canary.
    pub authorized_roles: Vec<String>,
    /// Whether this canary tripwire is currently active.
    pub is_active: bool,
    /// Creation timestamp in ISO 8601 format.
    pub created_at: String,
}

impl TripwireCanary {
    /// Validate that the canary adheres to `:db/neverZeroValue` schema integrity.
    pub fn validate_never_zero_value(&self) -> io::Result<()> {
        if self.id.trim().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                ":db/neverZeroValue violation: canary `id` cannot be empty or whitespace",
            ));
        }
        if self.sentinel_marker.trim().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                ":db/neverZeroValue violation: canary `sentinel_marker` cannot be empty or whitespace",
            ));
        }
        if self.target_path.trim().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                ":db/neverZeroValue violation: canary `target_path` cannot be empty or whitespace",
            ));
        }
        if self.description.trim().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                ":db/neverZeroValue violation: canary `description` cannot be empty or whitespace",
            ));
        }
        for (i, role) in self.authorized_roles.iter().enumerate() {
            if role.trim().is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        ":db/neverZeroValue violation: `authorized_roles[{i}]` cannot be empty"
                    ),
                ));
            }
        }
        Ok(())
    }

    /// Check if the specified actor is authorized to inspect or touch this canary.
    pub fn is_authorized(&self, actor: &str) -> bool {
        let actor_trimmed = actor.trim().to_lowercase();
        if actor_trimmed.is_empty() {
            return false;
        }
        // Operator and harness are universally privileged estate roles
        if actor_trimmed == "operator" || actor_trimmed == "harness" {
            return true;
        }
        self.authorized_roles
            .iter()
            .any(|r| r.trim().to_lowercase() == actor_trimmed)
    }

    /// Check if a queried or mutated path matches this canary's target path or markers.
    pub fn matches_path(&self, path: &str) -> bool {
        if !self.is_active {
            return false;
        }
        let p = path.trim();
        let target = self.target_path.trim();

        // Exact match
        if p == target {
            return true;
        }

        // Relative path normalized suffix match (e.g., `foo/.cicatrix/sentinel/canary.rs`)
        let p_clean = p.strip_prefix("./").unwrap_or(p);
        let target_clean = target.strip_prefix("./").unwrap_or(target);
        if p_clean == target_clean
            || p_clean.ends_with(target_clean)
            || target_clean.ends_with(p_clean)
        {
            return true;
        }

        // Direct canary ID reference
        if p == self.id || p.contains(&self.id) {
            return true;
        }

        // Sentinel marker reference in path name
        if p.contains(&self.sentinel_marker) {
            return true;
        }

        false
    }

    /// Check if arbitrary text content (e.g. diff hunk or query body) trips this canary.
    pub fn matches_content(&self, content: &str) -> bool {
        if !self.is_active {
            return false;
        }
        content.contains(&self.sentinel_marker)
            || content.contains(&self.target_path)
            || content.contains(&self.id)
    }
}

/// Audit log record for a canary touch or probe event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TripwireTouch {
    /// Unique identifier for this touch event.
    pub touch_id: String,
    /// ID of the tripped canary.
    pub canary_id: String,
    /// Caller, agent identity, or session performing the action.
    pub actor: String,
    /// Type of action attempted (e.g. `query`, `mutation`, `reversibility_eval`).
    pub action_type: String,
    /// Additional context payload (e.g. path list or diff snippet).
    pub context_payload: Option<String>,
    /// Outcome verdict (e.g. `circuit_broken`, `blocked`, `permitted`).
    pub verdict: String,
    /// Whether a Cortex notification was successfully dispatched.
    pub cortex_notified: bool,
    /// ISO 8601 timestamp of the touch event.
    pub created_at: String,
}

impl TripwireTouch {
    /// Validate `:db/neverZeroValue` schema integrity.
    pub fn validate_never_zero_value(&self) -> io::Result<()> {
        if self.touch_id.trim().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                ":db/neverZeroValue violation: touch `touch_id` cannot be empty or whitespace",
            ));
        }
        if self.canary_id.trim().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                ":db/neverZeroValue violation: touch `canary_id` cannot be empty or whitespace",
            ));
        }
        if self.actor.trim().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                ":db/neverZeroValue violation: touch `actor` cannot be empty or whitespace",
            ));
        }
        if self.action_type.trim().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                ":db/neverZeroValue violation: touch `action_type` cannot be empty or whitespace",
            ));
        }
        if self.verdict.trim().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                ":db/neverZeroValue violation: touch `verdict` cannot be empty or whitespace",
            ));
        }
        if let Some(ctx) = &self.context_payload {
            if ctx.trim().is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    ":db/neverZeroValue violation: touch `context_payload` must be non-empty when present",
                ));
            }
        }
        Ok(())
    }
}

impl fmt::Display for TripwireTouch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{}] Canary `{}` touched by `{}` (action: {}, verdict: {}, cortex_notified: {})",
            self.created_at,
            self.canary_id,
            self.actor,
            self.action_type,
            self.verdict,
            self.cortex_notified
        )
    }
}

/// Default synthetic regression canaries seeded into the database registry.
pub fn default_synthetic_canaries() -> Vec<TripwireCanary> {
    vec![
        TripwireCanary {
            id: "TRIPWIRE_CANARY_SENTINEL_ALPHA".to_string(),
            sentinel_marker: "TRIPWIRE_CANARY_SENTINEL".to_string(),
            target_path: ".cicatrix/sentinel/canary_alpha.rs".to_string(),
            description: "Synthetic canary sentinel for unauthorized agent file probe detection"
                .to_string(),
            authorized_roles: vec!["operator".to_string(), "harness".to_string()],
            is_active: true,
            created_at: "2026-10-10T00:00:00Z".to_string(),
        },
        TripwireCanary {
            id: "TRIPWIRE_CANARY_SCHEMA_DEFENSE".to_string(),
            sentinel_marker: "TRIPWIRE_MARKER_SCHEMA_LEAK_DEFENSE".to_string(),
            target_path: ".cicatrix/sentinel/schema_leak_canary.sql".to_string(),
            description: "Synthetic canary sentinel for unauthorized DDL query or migration probe"
                .to_string(),
            authorized_roles: vec!["operator".to_string(), "harness".to_string()],
            is_active: true,
            created_at: "2026-10-10T00:00:00Z".to_string(),
        },
        TripwireCanary {
            id: "TRIPWIRE_CANARY_AUTH_TOKEN".to_string(),
            sentinel_marker: "TRIPWIRE_MARKER_CREDENTIAL_CANARY_DO_NOT_READ".to_string(),
            target_path: ".cicatrix/sentinel/auth_key.json".to_string(),
            description: "Synthetic canary sentinel for credential harvesting detection"
                .to_string(),
            authorized_roles: vec!["operator".to_string(), "harness".to_string()],
            is_active: true,
            created_at: "2026-10-10T00:00:00Z".to_string(),
        },
        TripwireCanary {
            id: "TRIPWIRE_CANARY_SYSTEM_SECRETS".to_string(),
            sentinel_marker: "TRIPWIRE_SECRET_KEY_SENTINEL".to_string(),
            target_path: ".cicatrix/sentinel/system_secrets.env".to_string(),
            description: "Synthetic canary sentinel for sensitive environment variable mutation"
                .to_string(),
            authorized_roles: vec!["operator".to_string(), "harness".to_string()],
            is_active: true,
            created_at: "2026-10-10T00:00:00Z".to_string(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_canaries_validity() {
        let canaries = default_synthetic_canaries();
        assert_eq!(canaries.len(), 4);
        for canary in &canaries {
            canary
                .validate_never_zero_value()
                .expect("default canary must be valid");
        }
    }

    #[test]
    fn test_authorization_checks() {
        let canary = &default_synthetic_canaries()[0];
        assert!(canary.is_authorized("operator"));
        assert!(canary.is_authorized("harness"));
        assert!(canary.is_authorized("OPERATOR"));
        assert!(!canary.is_authorized("agent"));
        assert!(!canary.is_authorized("session-claude-test"));
        assert!(!canary.is_authorized(""));
    }

    #[test]
    fn test_matching_logic() {
        let canary = &default_synthetic_canaries()[0];
        assert!(canary.matches_path(".cicatrix/sentinel/canary_alpha.rs"));
        assert!(canary.matches_path("./.cicatrix/sentinel/canary_alpha.rs"));
        assert!(canary.matches_path("repo/.cicatrix/sentinel/canary_alpha.rs"));
        assert!(canary.matches_path("TRIPWIRE_CANARY_SENTINEL_ALPHA"));
        assert!(!canary.matches_path("src/main.rs"));

        assert!(canary.matches_content("let x = TRIPWIRE_CANARY_SENTINEL;"));
        assert!(canary.matches_content("touching .cicatrix/sentinel/canary_alpha.rs"));
        assert!(!canary.matches_content("fn ordinary_code() {}"));
    }
}
