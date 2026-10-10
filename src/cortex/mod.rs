//! Cortex Settle Learning Loop Outbox Integration (CER-1827 / CER-2764, Phase 4.3).
//!
//! Integrates Cicatrix to the Cortex settle outbox stream.
//! Distinguishes negative verdicts (operator discard, guard deny upheld)
//! and automatically ingests them as observed defect facts in `docs/sessions/observed/`
//! conforming to `docs/sessions/_SCHEMA.md`.
//!
//! Maintains an immutable append-only deduplication ledger (`cortex_settle_events`)
//! enforcing `:db/neverZeroValue` schema integrity.

pub mod consumer;
pub mod error;
pub mod event;
pub mod store;

pub use consumer::{IngestStatus, SettleConsumer, SettleIngestResult};
pub use error::CortexError;
pub use event::{derive_scope, sanitize_slug, SettleAction, SettleOutcomeEvent};
pub use store::{
    get_settle_event, get_settle_status_summary, is_settle_event_recorded, list_settle_events,
    record_settle_event, RecordedSettleEvent, SettleQueryFilter, SettleStatusSummary, SourceCount,
};
