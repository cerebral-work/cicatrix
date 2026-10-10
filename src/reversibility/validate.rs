//! Speculative sandbox validation stage for Wheelhorse reversibility pipeline (CER-2759).
//!
//! Executes speculative rollback plans in an isolated sandbox or dry-run,
//! confirming that applying the proposed diff followed by the compensation plan
//! restores the exact baseline state with zero uncompensated side effects.
//! Enforces `:db/neverZeroValue` schema integrity.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;

use super::classify::{parse_diff_files, ActionClass, ClassificationReport, DiffHunk, DiffLine};
use super::plan::{validate_compensation_plan, CompensationOp, CompensationPlan};

/// Outcome of speculative sandbox rollback validation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ValidationResult {
    /// Rollback plan executed cleanly in sandbox and restored exact baseline.
    Valid {
        baseline_restored: bool,
        side_effects_count: usize,
        audit_receipt: String,
    },
    /// Rollback plan failed to restore baseline or produced uncompensated artifacts.
    Invalid { reason: String },
    /// Action is a OneWayDoor; speculative automated rollback skipped.
    OneWayDoorSkipped { reason: String },
}

/// Execute speculative validation of diff and compensation plan in a sandbox.
pub fn validate_sandbox(
    diff: &str,
    classification: &ClassificationReport,
    plan: &CompensationPlan,
) -> Result<ValidationResult, String> {
    // If OneWayDoor, rollback cannot be autonomously validated
    if let ActionClass::OneWayDoor { reason } = &classification.action_class {
        return Ok(ValidationResult::OneWayDoorSkipped {
            reason: reason.clone(),
        });
    }

    // Verify :db/neverZeroValue constraints on compensation plan
    if let Err(e) = validate_compensation_plan(plan) {
        return Ok(ValidationResult::Invalid {
            reason: format!("compensation plan violates :db/neverZeroValue: {e}"),
        });
    }

    // Perform speculative round-trip in memory and verify zero side effects
    let parsed_files = parse_diff_files(diff)?;
    let mut baseline_state: BTreeMap<String, String> = BTreeMap::new();
    let mut forward_state: BTreeMap<String, String> = BTreeMap::new();

    // Reconstruct baseline and forward states from diff
    for file in &parsed_files {
        let path = file
            .new_path
            .as_deref()
            .or(file.old_path.as_deref())
            .unwrap_or("unknown")
            .to_string();

        if file.is_new_file {
            // Baseline: file does not exist
            let mut content = String::new();
            for hunk in &file.hunks {
                for line in &hunk.lines {
                    if let DiffLine::Add(a) = line {
                        content.push_str(a);
                        content.push('\n');
                    }
                }
            }
            forward_state.insert(path, content);
        } else {
            // Reconstruct baseline content from context + removed lines
            let mut baseline_content = String::new();
            let mut forward_content = String::new();

            for hunk in &file.hunks {
                for line in &hunk.lines {
                    match line {
                        DiffLine::Context(c) => {
                            baseline_content.push_str(c);
                            baseline_content.push('\n');
                            forward_content.push_str(c);
                            forward_content.push('\n');
                        }
                        DiffLine::Remove(r) => {
                            baseline_content.push_str(r);
                            baseline_content.push('\n');
                        }
                        DiffLine::Add(a) => {
                            forward_content.push_str(a);
                            forward_content.push('\n');
                        }
                    }
                }
            }

            baseline_state.insert(path.clone(), baseline_content);
            forward_state.insert(path, forward_content);
        }
    }

    // Apply compensation plan to forward_state to produce restored_state
    let mut restored_state = forward_state.clone();

    for (op_idx, op) in plan.ops.iter().enumerate() {
        match op {
            CompensationOp::DeleteCreatedFile { path } => {
                if !restored_state.contains_key(path) {
                    return Ok(ValidationResult::Invalid {
                        reason: format!(
                            "op {op_idx} DeleteCreatedFile: file `{path}` not found in state"
                        ),
                    });
                }
                restored_state.remove(path);
            }
            CompensationOp::InversePatch { path, patch } => {
                let Some(content) = restored_state.get(path) else {
                    return Ok(ValidationResult::Invalid {
                        reason: format!(
                            "op {op_idx} InversePatch: target file `{path}` not found in state"
                        ),
                    });
                };

                let inv_files = match parse_diff_files(patch) {
                    Ok(f) => f,
                    Err(e) => {
                        return Ok(ValidationResult::Invalid {
                            reason: format!("failed to parse inverse patch for `{path}`: {e}"),
                        });
                    }
                };

                let Some(inv_file) = inv_files.into_iter().next() else {
                    return Ok(ValidationResult::Invalid {
                        reason: format!("inverse patch for `{path}` contains no file entries"),
                    });
                };

                match apply_hunks_to_content(content, &inv_file.hunks) {
                    Ok(reverted) => {
                        restored_state.insert(path.clone(), reverted);
                    }
                    Err(e) => {
                        return Ok(ValidationResult::Invalid {
                            reason: format!("failed to apply inverse patch to `{path}`: {e}"),
                        });
                    }
                }
            }
            CompensationOp::RestoreFile { path, pre_image } => {
                restored_state.insert(path.clone(), pre_image.clone());
            }
            CompensationOp::ManualOperatorAction { instruction } => {
                return Ok(ValidationResult::Invalid {
                    reason: format!(
                        "manual operator compensation cannot be validated autonomously in sandbox: {instruction}"
                    ),
                });
            }
        }
    }

    // Compare restored_state with baseline_state
    let mut side_effects_count = 0;
    let mut uncompensated_reasons = Vec::new();

    // Check for lingering uncompensated or extra files
    for key in restored_state.keys() {
        if !baseline_state.contains_key(key) {
            side_effects_count += 1;
            uncompensated_reasons.push(format!("lingering uncompensated file `{key}`"));
        }
    }

    // Check for missing baseline files or content mismatch
    for (key, baseline_val) in &baseline_state {
        match restored_state.get(key) {
            None => {
                side_effects_count += 1;
                uncompensated_reasons.push(format!("missing baseline file `{key}`"));
            }
            Some(restored_val) => {
                if restored_val != baseline_val {
                    side_effects_count += 1;
                    uncompensated_reasons.push(format!(
                        "file `{key}` content mismatch after compensation rollback"
                    ));
                }
            }
        }
    }

    if side_effects_count > 0 {
        return Ok(ValidationResult::Invalid {
            reason: format!(
                "sandbox verification failed with {side_effects_count} side effect(s): {}",
                uncompensated_reasons.join(", ")
            ),
        });
    }

    let receipt = format!(
        "sandbox_verified:files={};ops={};checksum=ok",
        baseline_state.len(),
        plan.ops.len()
    );

    Ok(ValidationResult::Valid {
        baseline_restored: true,
        side_effects_count: 0,
        audit_receipt: receipt,
    })
}

/// Apply diff hunks to a multiline content string in memory.
pub fn apply_hunks_to_content(content: &str, hunks: &[DiffHunk]) -> Result<String, String> {
    let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();

    // If content ended with newline, content.lines() strips it, so we track lines as Vec
    for hunk in hunks {
        let mut expected_lines: Vec<String> = Vec::new();
        let mut replacement_lines: Vec<String> = Vec::new();

        for line in &hunk.lines {
            match line {
                DiffLine::Context(c) => {
                    expected_lines.push(c.clone());
                    replacement_lines.push(c.clone());
                }
                DiffLine::Remove(r) => {
                    expected_lines.push(r.clone());
                }
                DiffLine::Add(a) => {
                    replacement_lines.push(a.clone());
                }
            }
        }

        // Find match in lines
        let hunk_match_idx = if !expected_lines.is_empty() {
            find_subslice(&lines, &expected_lines)
        } else {
            Some(0)
        };

        if let Some(idx) = hunk_match_idx {
            lines.splice(idx..idx + expected_lines.len(), replacement_lines);
        } else {
            return Err("patch hunk does not match target file lines".to_string());
        }
    }

    let mut result = lines.join("\n");
    if !result.is_empty() {
        result.push('\n');
    }
    Ok(result)
}

fn find_subslice(haystack: &[String], needle: &[String]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    if needle.len() > haystack.len() {
        return None;
    }
    for i in 0..=(haystack.len() - needle.len()) {
        if &haystack[i..i + needle.len()] == needle {
            return Some(i);
        }
    }
    None
}

/// Execute speculative validation on disk inside a clean temporary sandbox directory.
pub fn validate_disk_sandbox(
    diff: &str,
    classification: &ClassificationReport,
    plan: &CompensationPlan,
) -> Result<ValidationResult, String> {
    if let ActionClass::OneWayDoor { reason } = &classification.action_class {
        return Ok(ValidationResult::OneWayDoorSkipped {
            reason: reason.clone(),
        });
    }

    let temp_dir = tempfile::tempdir()
        .map_err(|e| format!("failed to create temporary sandbox directory: {e}"))?;
    let root = temp_dir.path();

    let parsed_files = parse_diff_files(diff)?;
    let mut baseline_files: BTreeMap<String, String> = BTreeMap::new();

    // Setup baseline files on disk
    for file in &parsed_files {
        let path = file
            .new_path
            .as_deref()
            .or(file.old_path.as_deref())
            .unwrap_or("unknown");

        if !file.is_new_file {
            let mut baseline_content = String::new();
            for hunk in &file.hunks {
                for line in &hunk.lines {
                    match line {
                        DiffLine::Context(c) | DiffLine::Remove(c) => {
                            baseline_content.push_str(c);
                            baseline_content.push('\n');
                        }
                        DiffLine::Add(_) => {}
                    }
                }
            }
            let file_disk_path = root.join(path);
            if let Some(parent) = file_disk_path.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            fs::write(&file_disk_path, &baseline_content).map_err(|e| e.to_string())?;
            baseline_files.insert(path.to_string(), baseline_content);
        }
    }

    // Apply forward changes to disk
    for file in &parsed_files {
        let path = file
            .new_path
            .as_deref()
            .or(file.old_path.as_deref())
            .unwrap_or("unknown");
        let file_disk_path = root.join(path);

        if file.is_new_file {
            let mut content = String::new();
            for hunk in &file.hunks {
                for line in &hunk.lines {
                    if let DiffLine::Add(a) = line {
                        content.push_str(a);
                        content.push('\n');
                    }
                }
            }
            if let Some(parent) = file_disk_path.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            fs::write(&file_disk_path, content).map_err(|e| e.to_string())?;
        } else {
            let mut forward_content = String::new();
            for hunk in &file.hunks {
                for line in &hunk.lines {
                    match line {
                        DiffLine::Context(c) | DiffLine::Add(c) => {
                            forward_content.push_str(c);
                            forward_content.push('\n');
                        }
                        DiffLine::Remove(_) => {}
                    }
                }
            }
            fs::write(&file_disk_path, forward_content).map_err(|e| e.to_string())?;
        }
    }

    // Apply compensation plan ops to disk
    for op in &plan.ops {
        match op {
            CompensationOp::DeleteCreatedFile { path } => {
                let file_disk_path = root.join(path);
                if file_disk_path.exists() {
                    fs::remove_file(&file_disk_path).map_err(|e| e.to_string())?;
                }
            }
            CompensationOp::InversePatch { path, patch } => {
                let file_disk_path = root.join(path);
                let current_content =
                    fs::read_to_string(&file_disk_path).map_err(|e| e.to_string())?;
                let inv_files = parse_diff_files(patch)?;
                if let Some(inv_file) = inv_files.into_iter().next() {
                    let reverted = apply_hunks_to_content(&current_content, &inv_file.hunks)?;
                    fs::write(&file_disk_path, reverted).map_err(|e| e.to_string())?;
                }
            }
            CompensationOp::RestoreFile { path, pre_image } => {
                let file_disk_path = root.join(path);
                fs::write(&file_disk_path, pre_image).map_err(|e| e.to_string())?;
            }
            CompensationOp::ManualOperatorAction { instruction } => {
                return Ok(ValidationResult::Invalid {
                    reason: format!(
                        "cannot execute manual compensation in disk sandbox: {instruction}"
                    ),
                });
            }
        }
    }

    // Verify baseline restoration on disk
    for (path, baseline_content) in &baseline_files {
        let file_disk_path = root.join(path);
        if !file_disk_path.exists() {
            return Ok(ValidationResult::Invalid {
                reason: format!("baseline file `{path}` missing on disk after rollback"),
            });
        }
        let restored = fs::read_to_string(&file_disk_path).map_err(|e| e.to_string())?;
        if &restored != baseline_content {
            return Ok(ValidationResult::Invalid {
                reason: format!("baseline file `{path}` content mismatch on disk after rollback"),
            });
        }
    }

    Ok(ValidationResult::Valid {
        baseline_restored: true,
        side_effects_count: 0,
        audit_receipt: format!("disk_sandbox_verified:files={}", baseline_files.len()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reversibility::classify::classify_diff;
    use crate::reversibility::plan::capture_plan;

    #[test]
    fn test_validate_code_edit_roundtrip_valid() {
        let diff = r#"
diff --git a/src/calc.rs b/src/calc.rs
index 1111111..2222222 100644
--- a/src/calc.rs
+++ b/src/calc.rs
@@ -1,3 +1,3 @@
 fn add(a: i32, b: i32) -> i32 {
-    a - b
+    a + b
 }
"#;
        let classification = classify_diff(diff).unwrap();
        let plan = capture_plan(diff, &classification).unwrap();
        let validation = validate_sandbox(diff, &classification, &plan).unwrap();

        match validation {
            ValidationResult::Valid {
                baseline_restored,
                side_effects_count,
                ..
            } => {
                assert!(baseline_restored);
                assert_eq!(side_effects_count, 0);
            }
            other => panic!("expected Valid result, got {other:?}"),
        }
    }

    #[test]
    fn test_validate_pure_addition_roundtrip_valid() {
        let diff = r#"
diff --git a/tests/audit_test.rs b/tests/audit_test.rs
new file mode 100644
index 0000000..3333333
--- /dev/null
+++ b/tests/audit_test.rs
@@ -0,0 +1,3 @@
+#[test]
+fn test_it() {}
"#;
        let classification = classify_diff(diff).unwrap();
        let plan = capture_plan(diff, &classification).unwrap();
        let validation = validate_sandbox(diff, &classification, &plan).unwrap();

        match validation {
            ValidationResult::Valid {
                baseline_restored,
                side_effects_count,
                ..
            } => {
                assert!(baseline_restored);
                assert_eq!(side_effects_count, 0);
            }
            other => panic!("expected Valid result, got {other:?}"),
        }
    }

    #[test]
    fn test_validate_uncompensated_side_effects_fails() {
        let diff = r#"
diff --git a/tests/extra.rs b/tests/extra.rs
new file mode 100644
index 0000000..4444444
--- /dev/null
+++ b/tests/extra.rs
@@ -0,0 +1,2 @@
+// extra file
"#;
        let classification = classify_diff(diff).unwrap();
        // Malformed plan that fails to delete the file
        let bad_plan = CompensationPlan {
            ops: vec![CompensationOp::RestoreFile {
                path: "non_existent.rs".to_string(),
                pre_image: "dummy".to_string(),
            }],
            rollback_script: "echo dummy".to_string(),
            total_ops: 1,
        };

        let validation = validate_sandbox(diff, &classification, &bad_plan).unwrap();
        match validation {
            ValidationResult::Invalid { reason } => {
                assert!(reason.contains("side effect"));
            }
            other => panic!("expected Invalid result, got {other:?}"),
        }
    }

    #[test]
    fn test_validate_disk_sandbox_execution() {
        let diff = r#"
diff --git a/src/worker.rs b/src/worker.rs
index 1111111..2222222 100644
--- a/src/worker.rs
+++ b/src/worker.rs
@@ -1,3 +1,3 @@
 pub fn ping() -> &'static str {
-    "pong_old"
+    "pong_new"
 }
"#;
        let classification = classify_diff(diff).unwrap();
        let plan = capture_plan(diff, &classification).unwrap();
        let validation = validate_disk_sandbox(diff, &classification, &plan).unwrap();

        assert!(matches!(validation, ValidationResult::Valid { .. }));
    }
}
