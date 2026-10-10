//! Wheelhorse Reversibility Action Pipeline (CER-2759, Phase 3.1).
//!
//! Provides a 4-stage reversibility action pipeline in Rust:
//! 1. `classify`: Determine whether touched surfaces are reversible (code edits, test additions)
//!    or one-way doors (migrations, schema drops, secret/token rotations, file deletions).
//! 2. `capture_plan`: Machine-generate inverse compensation operations (inverse patches,
//!    file deletions, restorations) enforcing `:db/neverZeroValue` schema integrity.
//! 3. `validate`: Speculatively execute the rollback plan in an isolated sandbox, confirming
//!    exact baseline restoration with zero uncompensated side effects.
//! 4. `decide`: Return `AutoCommit`, `AutoCommitWithNotice`, `RouteToApproval`, or `Block`.

pub mod classify;
pub mod decision;
pub mod plan;
pub mod validate;

pub use classify::{
    classify_diff, is_migration_path, is_secret_path, parse_diff_files, ActionClass,
    ClassificationReport, DiffFile, DiffHunk, DiffLine,
};
pub use decision::{evaluate_verdict, AutonomyTier, ReversibilityVerdict};
pub use plan::{
    capture_plan, invert_diff_file, invert_hunk, validate_compensation_plan, CompensationOp,
    CompensationPlan,
};
pub use validate::{
    apply_hunks_to_content, validate_disk_sandbox, validate_sandbox, ValidationResult,
};

use serde::{Deserialize, Serialize};

/// Comprehensive report produced by the reversibility evaluation pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReversibilityReport {
    /// Action classification result.
    pub action_class: ActionClass,
    /// Machine-generated compensation plan (if applicable).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<CompensationPlan>,
    /// Outcome of speculative sandbox validation.
    pub validation: ValidationResult,
    /// Final reversibility gate verdict.
    pub verdict: ReversibilityVerdict,
    /// Touched file paths extracted from the diff.
    pub touched_files: Vec<String>,
    /// Whether any synthetic canary tripwires were touched.
    pub canary_touched: bool,
    /// Active autonomy capability tier.
    pub tier: AutonomyTier,
    /// Human-readable evaluation summary.
    pub summary: String,
}

/// Orchestrator for the 4-stage reversibility pipeline.
#[derive(Debug, Clone)]
pub struct ReversibilityPipeline {
    pub tier: AutonomyTier,
}

impl ReversibilityPipeline {
    /// Create a new reversibility pipeline at the specified autonomy tier.
    pub fn new(tier: AutonomyTier) -> Self {
        Self { tier }
    }

    /// Execute Stage 1: Classify diff action.
    pub fn classify(&self, diff: &str) -> Result<ClassificationReport, String> {
        classify_diff(diff)
    }

    /// Execute Stage 1 & 2: Classify and capture rollback plan.
    pub fn plan(&self, diff: &str) -> Result<CompensationPlan, String> {
        let classification = self.classify(diff)?;
        capture_plan(diff, &classification)
    }

    /// Execute Stage 1, 2 & 3: Classify, capture plan, and validate in sandbox.
    pub fn validate(&self, diff: &str) -> Result<ValidationResult, String> {
        let classification = self.classify(diff)?;
        let plan = capture_plan(diff, &classification)?;
        validate_sandbox(diff, &classification, &plan)
    }

    /// Execute full 4-stage pipeline: classify -> capture_plan -> validate -> decide.
    pub fn evaluate(&self, diff: &str) -> Result<ReversibilityReport, String> {
        let classification = self.classify(diff)?;
        let plan = capture_plan(diff, &classification)?;
        let validation = validate_sandbox(diff, &classification, &plan)?;
        let verdict = evaluate_verdict(
            &classification.action_class,
            &validation,
            self.tier,
            classification.canary_touched,
        );

        let summary = match &verdict {
            ReversibilityVerdict::AutoCommit => {
                "Change is pure additive and executes autonomously.".to_string()
            }
            ReversibilityVerdict::AutoCommitWithNotice { notice } => {
                format!("Change verified reversible with rollback plan: {notice}")
            }
            ReversibilityVerdict::RouteToApproval { signal_name, .. } => {
                format!(
                    "Change is a OneWayDoor mutation; requires human approval via `{signal_name}`."
                )
            }
            ReversibilityVerdict::Block { violation } => {
                format!("Change rejected by reversibility gate: {violation}")
            }
        };

        Ok(ReversibilityReport {
            action_class: classification.action_class,
            plan: Some(plan),
            validation,
            verdict,
            touched_files: classification.touched_files,
            canary_touched: classification.canary_touched,
            tier: self.tier,
            summary,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipeline_end_to_end_code_edit() {
        let diff = r#"
diff --git a/src/service.rs b/src/service.rs
index 1111111..2222222 100644
--- a/src/service.rs
+++ b/src/service.rs
@@ -10,3 +10,4 @@
 pub fn handle() {
-    let x = 10;
+    let x = 20;
+    println!("{x}");
 }
"#;
        let pipeline = ReversibilityPipeline::new(AutonomyTier::Supervised);
        let report = pipeline.evaluate(diff).unwrap();

        assert!(matches!(
            report.action_class,
            ActionClass::ReversibleWithPlan { .. }
        ));
        assert!(matches!(report.validation, ValidationResult::Valid { .. }));
        assert!(matches!(
            report.verdict,
            ReversibilityVerdict::AutoCommitWithNotice { .. }
        ));
        assert_eq!(report.touched_files, vec!["src/service.rs"]);
        assert!(!report.canary_touched);
    }

    #[test]
    fn test_pipeline_end_to_end_migration_routes_to_approval() {
        let diff = r#"
diff --git a/migrations/0002_drop_column.sql b/migrations/0002_drop_column.sql
new file mode 100644
index 0000000..3333333
--- /dev/null
+++ b/migrations/0002_drop_column.sql
@@ -0,0 +1,2 @@
+ALTER TABLE accounts DROP COLUMN legacy_pin;
"#;
        let pipeline = ReversibilityPipeline::new(AutonomyTier::Autonomous);
        let report = pipeline.evaluate(diff).unwrap();

        assert!(matches!(
            report.action_class,
            ActionClass::OneWayDoor { .. }
        ));
        assert!(matches!(
            report.verdict,
            ReversibilityVerdict::RouteToApproval { .. }
        ));
    }
}
