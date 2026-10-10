//! Synthetic canary tripwire registry and intrusion guard (CER-2760, Phase 3.2).
//!
//! Provides defense-in-depth against unauthorized agent probing or mutation of
//! sensitive surfaces. Seeds sentinel canaries, audits touch events, breaks circuits
//! fail-closed, and broadcasts alerts over the Cortex agent mesh.

pub mod canary;
pub mod guard;
pub mod notify;

pub use canary::{default_synthetic_canaries, TripwireCanary, TripwireTouch};
pub use guard::{
    current_iso_timestamp, generate_touch_id, guard_check, guard_diff, guard_paths, list_canaries,
    list_touches, record_touch, register_canary, seed_canaries, TripwireIntrusionData,
    TripwireIntrusionError,
};
pub use notify::notify_cortex_intrusion;
