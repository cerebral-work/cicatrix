//! Earned Autonomy Trust Ladder & Append-Only Promotion Ledger (CER-2762, Phase 4.1).
//!
//! Implements a 3-tier capability trust ladder:
//! - `Shadow`: Dry-run and speculative sandbox execution only.
//! - `Supervised`: Human approval required for one-way doors / production gates; notice for reversible changes.
//! - `Autonomous`: Auto-commits reversible additive changes, captures rollback notices for reversible modifications.
//!
//! Enforces the estate invariant:
//! Soma production gates strictly require human operator verdicts.
//! Earned autonomy applies only to background operational tasks and speculative sandboxes.

pub mod error;
pub mod guard;
pub mod ledger;
pub mod soma;
pub mod tier;

pub use error::AutonomyError;
pub use guard::{
    check_capability_autonomy, demote_capability, promote_capability, seed_default_capabilities,
    AutonomyCheckResult, DemotionRequest, PromotionRequest,
};
pub use ledger::{
    generate_event_id, get_state, get_tier, list_events, list_states, record_event,
    AutonomyActionType, AutonomyEvent, AutonomyState,
};
pub use soma::{is_soma_production_capability, is_soma_production_path, validate_soma_invariant};
pub use tier::AutonomyTier;
