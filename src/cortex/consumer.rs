//! Outbox event stream consumer and defect fact writer (CER-2764, Phase 4.3).
//!
//! Subscribes to Cortex settle outbox payloads.
//! Distinguishes negative verdicts (operator discard, guard deny upheld)
//! and writes them as observed defect facts to `docs/sessions/observed/`
//! while preserving an append-only deduplication ledger in SQLite.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::cortex::error::CortexError;
use crate::cortex::event::SettleOutcomeEvent;
use crate::cortex::store::{is_settle_event_recorded, record_settle_event, RecordedSettleEvent};
use crate::tripwire::guard_check;

/// Status outcome for a consumed settle event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IngestStatus {
    /// Negative verdict ingested and written to `docs/sessions/observed/`.
    IngestedNegative,
    /// Positive or neutral verdict recorded in audit ledger without defect fact.
    IngestedPositive,
    /// Event already processed and recorded; skipped to ensure idempotency.
    DuplicateSkipped,
}

impl IngestStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::IngestedNegative => "ingested_negative",
            Self::IngestedPositive => "ingested_positive",
            Self::DuplicateSkipped => "duplicate_skipped",
        }
    }
}

/// Ingestion receipt for a single settle outcome event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettleIngestResult {
    /// Upstream or generated event ID.
    pub event_id: String,
    /// Underlying job ID.
    pub job_id: String,
    /// Source channel.
    pub source: String,
    /// Ingestion status.
    pub status: IngestStatus,
    /// Whether this is a negative settle verdict.
    pub is_negative: bool,
    /// Linked observed fact ID (if negative).
    pub fact_id: Option<String>,
    /// Path to the generated markdown fact file (if negative).
    pub fact_path: Option<String>,
    /// Informational message or reason.
    pub message: String,
}

/// Consumer for Cortex settle outbox events.
#[derive(Debug, Clone)]
pub struct SettleConsumer {
    observed_dir: PathBuf,
    actor: String,
}

impl Default for SettleConsumer {
    fn default() -> Self {
        Self {
            observed_dir: PathBuf::from("docs/sessions/observed"),
            actor: "cicatrix-cortex-settle-consumer".to_string(),
        }
    }
}

impl SettleConsumer {
    /// Create a new consumer with default paths.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a consumer targeting a specific observed facts directory.
    pub fn with_observed_dir(mut self, path: impl AsRef<Path>) -> Self {
        self.observed_dir = path.as_ref().to_path_buf();
        self
    }

    /// Create a consumer with a custom actor identification.
    pub fn with_actor(mut self, actor: impl Into<String>) -> Self {
        self.actor = actor.into();
        self
    }

    /// Path to the directory where observed defect facts are written.
    pub fn observed_dir(&self) -> &Path {
        &self.observed_dir
    }

    /// Ingest a single parsed `SettleOutcomeEvent`.
    pub fn ingest_event(
        &self,
        conn: &Connection,
        event: &SettleOutcomeEvent,
    ) -> Result<SettleIngestResult, CortexError> {
        // 1. Enforce schema integrity on event attributes
        event
            .validate_integrity()
            .map_err(CortexError::Validation)?;

        // 2. Fail-closed canary tripwire guard check
        for file in &event.files {
            guard_check(
                conn,
                file,
                Some(&event.proposal_summary),
                &self.actor,
                "ingest_cortex_settle",
            )?;
        }
        guard_check(
            conn,
            &event.source,
            Some(&event.proposal_summary),
            &self.actor,
            "ingest_cortex_settle",
        )?;

        // 3. Deduplication check
        let is_dup =
            is_settle_event_recorded(conn, &event.id, &event.job_id, &event.settle_action)?;

        if is_dup {
            return Ok(SettleIngestResult {
                event_id: event.id.clone(),
                job_id: event.job_id.clone(),
                source: event.source.clone(),
                status: IngestStatus::DuplicateSkipped,
                is_negative: event.is_negative_verdict(),
                fact_id: None,
                fact_path: None,
                message: "Event already recorded in settle ledger (duplicate skipped)".to_string(),
            });
        }

        // 4. Negative verdict handling
        let is_negative = event.is_negative_verdict();
        let (fact_id, fact_path) = if is_negative {
            let (slug, markdown) = event.to_session_fact_markdown();
            fs::create_dir_all(&self.observed_dir)?;

            let filename = format!("FACT_{}.md", slug.to_uppercase().replace('-', "_"));
            let full_path = self.observed_dir.join(&filename);
            fs::write(&full_path, markdown.as_bytes())?;

            let fact_id = format!("fact:{slug}");
            let fact_path_str = full_path.to_string_lossy().to_string();
            (Some(fact_id), Some(fact_path_str))
        } else {
            (None, None)
        };

        // 5. Append to audit ledger
        let recorded = RecordedSettleEvent {
            id: event.id.clone(),
            event_id: event.id.clone(),
            job_id: event.job_id.clone(),
            source: event.source.clone(),
            action_type: event.action_type.clone(),
            guard_tier: event.guard_tier.clone(),
            guard_decision: event.guard_decision.clone(),
            operator_decision: event.operator_decision.clone(),
            settle_action: event.settle_action.clone(),
            proposal_summary: event.proposal_summary.clone(),
            rejection_reason: event.rejection_reason.clone(),
            edit_delta: event.edit_delta.clone(),
            files: event.files.clone(),
            meta_pattern: event.meta_pattern.clone(),
            is_negative,
            fact_id: fact_id.clone(),
            fact_path: fact_path.clone(),
            created_at: event.created_at.clone(),
        };

        record_settle_event(conn, &recorded)?;

        let status = if is_negative {
            IngestStatus::IngestedNegative
        } else {
            IngestStatus::IngestedPositive
        };

        let message = if is_negative {
            format!(
                "Negative settle verdict ingested; observed defect fact written to `{}`",
                fact_path.as_deref().unwrap_or("")
            )
        } else {
            "Settle outcome recorded in audit ledger (non-negative verdict)".to_string()
        };

        Ok(SettleIngestResult {
            event_id: event.id.clone(),
            job_id: event.job_id.clone(),
            source: event.source.clone(),
            status,
            is_negative,
            fact_id,
            fact_path,
            message,
        })
    }

    /// Ingest a JSON value containing either a single settle event object or an array of events.
    pub fn ingest_json_value(
        &self,
        conn: &Connection,
        val: &serde_json::Value,
    ) -> Result<Vec<SettleIngestResult>, CortexError> {
        let mut results = Vec::new();

        if let Some(arr) = val.as_array() {
            for item in arr {
                let event =
                    SettleOutcomeEvent::parse_from_value(item).map_err(CortexError::Validation)?;
                let res = self.ingest_event(conn, &event)?;
                results.push(res);
            }
        } else {
            let event =
                SettleOutcomeEvent::parse_from_value(val).map_err(CortexError::Validation)?;
            let res = self.ingest_event(conn, &event)?;
            results.push(res);
        }

        Ok(results)
    }

    /// Ingest raw string payload supporting single JSON, JSON array, or newline-delimited JSON (NDJSON).
    pub fn ingest_str(
        &self,
        conn: &Connection,
        raw: &str,
    ) -> Result<Vec<SettleIngestResult>, CortexError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Ok(Vec::new());
        }

        // Try parsing as single JSON object or array
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
            return self.ingest_json_value(conn, &val);
        }

        // Try parsing line-by-line NDJSON
        let mut results = Vec::new();
        for line in trimmed.lines() {
            let line_trimmed = line.trim();
            if line_trimmed.is_empty() {
                continue;
            }
            let val: serde_json::Value = serde_json::from_str(line_trimmed)?;
            let event =
                SettleOutcomeEvent::parse_from_value(&val).map_err(CortexError::Validation)?;
            let res = self.ingest_event(conn, &event)?;
            results.push(res);
        }

        Ok(results)
    }

    /// Read and ingest settle events from an open reader.
    pub fn ingest_reader<R: Read>(
        &self,
        conn: &Connection,
        mut reader: R,
    ) -> Result<Vec<SettleIngestResult>, CortexError> {
        let mut content = String::new();
        reader.read_to_string(&mut content)?;
        self.ingest_str(conn, &content)
    }

    /// Ingest settle events from a file on disk.
    pub fn ingest_file(
        &self,
        conn: &Connection,
        path: impl AsRef<Path>,
    ) -> Result<Vec<SettleIngestResult>, CortexError> {
        let content = fs::read_to_string(path)?;
        self.ingest_str(conn, &content)
    }
}
