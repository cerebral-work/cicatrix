//! Rollback plan generation and compensation operations (CER-2759).
//!
//! Enforces `:db/neverZeroValue` schema integrity constraints:
//! all compensation operations, paths, patches, and instructions must have
//! non-empty, non-zero values.

use serde::{Deserialize, Serialize};

use super::classify::{
    parse_diff_files, ActionClass, ClassificationReport, DiffFile, DiffHunk, DiffLine,
};

/// Atomic inverse compensation operation to restore system baseline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum CompensationOp {
    /// Invert an applied unified diff patch on an existing file.
    InversePatch { path: String, patch: String },
    /// Remove a newly created file created during an additive change.
    DeleteCreatedFile { path: String },
    /// Overwrite file with exact pre-image bytes/content.
    RestoreFile { path: String, pre_image: String },
    /// Manual compensation instruction when an automated inverse is unavailable.
    ManualOperatorAction { instruction: String },
}

/// Structured compensation plan with inverse operations and executable script.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompensationPlan {
    /// Ordered list of compensation operations.
    pub ops: Vec<CompensationOp>,
    /// Turnkey executable rollback script or commands.
    pub rollback_script: String,
    /// Total count of discrete compensation operations.
    pub total_ops: usize,
}

/// Enforce `:db/neverZeroValue` schema integrity on a compensation plan.
pub fn validate_compensation_plan(plan: &CompensationPlan) -> Result<(), String> {
    if plan.ops.is_empty() && plan.rollback_script.trim().is_empty() {
        return Err(
            ":db/neverZeroValue violation: compensation plan cannot have empty ops and empty rollback script"
                .to_string(),
        );
    }

    if plan.rollback_script.trim().is_empty() {
        return Err(
            ":db/neverZeroValue violation: rollback script must not be empty or whitespace"
                .to_string(),
        );
    }

    for (idx, op) in plan.ops.iter().enumerate() {
        match op {
            CompensationOp::InversePatch { path, patch } => {
                if path.trim().is_empty() {
                    return Err(format!(
                        ":db/neverZeroValue violation: op {idx} InversePatch has empty path"
                    ));
                }
                if patch.trim().is_empty() {
                    return Err(format!(
                        ":db/neverZeroValue violation: op {idx} InversePatch has empty patch"
                    ));
                }
            }
            CompensationOp::DeleteCreatedFile { path } => {
                if path.trim().is_empty() {
                    return Err(format!(
                        ":db/neverZeroValue violation: op {idx} DeleteCreatedFile has empty path"
                    ));
                }
            }
            CompensationOp::RestoreFile { path, pre_image } => {
                if path.trim().is_empty() {
                    return Err(format!(
                        ":db/neverZeroValue violation: op {idx} RestoreFile has empty path"
                    ));
                }
                if pre_image.trim().is_empty() {
                    return Err(format!(
                        ":db/neverZeroValue violation: op {idx} RestoreFile has empty pre_image"
                    ));
                }
            }
            CompensationOp::ManualOperatorAction { instruction } => {
                if instruction.trim().is_empty() {
                    return Err(format!(
                        ":db/neverZeroValue violation: op {idx} ManualOperatorAction has empty instruction"
                    ));
                }
            }
        }
    }

    Ok(())
}

/// Machine-generate compensation plan for a proposed diff based on classification.
pub fn capture_plan(
    diff: &str,
    classification: &ClassificationReport,
) -> Result<CompensationPlan, String> {
    match &classification.action_class {
        ActionClass::OneWayDoor { reason } => {
            let op = CompensationOp::ManualOperatorAction {
                instruction: format!("Manual operator review and intervention required: {reason}"),
            };
            let rollback_script = format!(
                "# OneWayDoor mutation detected.\n# Automated compensation unavailable.\n# Reason: {reason}\nexit 1"
            );
            let plan = CompensationPlan {
                ops: vec![op],
                rollback_script,
                total_ops: 1,
            };
            validate_compensation_plan(&plan)?;
            Ok(plan)
        }
        ActionClass::ReversibleAdditive => {
            let files = parse_diff_files(diff)?;
            let mut ops = Vec::new();
            let mut script_lines = vec!["#!/bin/sh".to_string(), "set -eu".to_string()];

            for f in files {
                let target_path = f
                    .new_path
                    .as_deref()
                    .or(f.old_path.as_deref())
                    .unwrap_or("unknown")
                    .to_string();

                if f.is_new_file {
                    script_lines.push(format!("rm -f \"{}\"", target_path));
                    ops.push(CompensationOp::DeleteCreatedFile { path: target_path });
                } else if !f.hunks.is_empty() {
                    // File had only additive lines
                    if let Some((path, inv_patch)) = invert_diff_file(&f) {
                        script_lines.push(format!(
                            "cat <<'EOF' | patch -p1\n{}\nEOF",
                            inv_patch.trim_end()
                        ));
                        ops.push(CompensationOp::InversePatch {
                            path,
                            patch: inv_patch,
                        });
                    }
                }
            }

            if ops.is_empty() {
                // Default fallback if no file-level ops were generated
                ops.push(CompensationOp::ManualOperatorAction {
                    instruction: "No files to delete for additive change".to_string(),
                });
                script_lines.push("# No files modified".to_string());
            }

            let rollback_script = script_lines.join("\n");
            let total_ops = ops.len();
            let plan = CompensationPlan {
                ops,
                rollback_script,
                total_ops,
            };
            validate_compensation_plan(&plan)?;
            Ok(plan)
        }
        ActionClass::ReversibleWithPlan { .. } => {
            let files = parse_diff_files(diff)?;
            let mut ops = Vec::new();
            let mut script_lines = vec!["#!/bin/sh".to_string(), "set -eu".to_string()];

            for f in files {
                if f.is_new_file {
                    let target_path = f.new_path.as_deref().unwrap_or("unknown").to_string();
                    script_lines.push(format!("rm -f \"{}\"", target_path));
                    ops.push(CompensationOp::DeleteCreatedFile { path: target_path });
                } else if let Some((path, inv_patch)) = invert_diff_file(&f) {
                    script_lines.push(format!(
                        "cat <<'EOF' | patch -p1\n{}\nEOF",
                        inv_patch.trim_end()
                    ));
                    ops.push(CompensationOp::InversePatch {
                        path,
                        patch: inv_patch,
                    });
                }
            }

            if ops.is_empty() {
                return Err(
                    "failed to generate inverse compensation operations for diff".to_string(),
                );
            }

            let rollback_script = script_lines.join("\n");
            let total_ops = ops.len();
            let plan = CompensationPlan {
                ops,
                rollback_script,
                total_ops,
            };
            validate_compensation_plan(&plan)?;
            Ok(plan)
        }
    }
}

/// Invert an individual diff file into an inverse unified diff patch string.
pub fn invert_diff_file(file: &DiffFile) -> Option<(String, String)> {
    let target_path = file.new_path.as_deref().or(file.old_path.as_deref())?;

    if file.hunks.is_empty() {
        return None;
    }

    let mut out = String::new();
    out.push_str(&format!("diff --git a/{target_path} b/{target_path}\n"));
    out.push_str(&format!("--- a/{target_path}\n"));
    out.push_str(&format!("+++ b/{target_path}\n"));

    for hunk in &file.hunks {
        let inv_hunk = invert_hunk(hunk);
        out.push_str(&format!(
            "@@ -{},{} +{},{} @@\n",
            inv_hunk.old_start, inv_hunk.old_count, inv_hunk.new_start, inv_hunk.new_count
        ));
        for line in &inv_hunk.lines {
            match line {
                DiffLine::Context(c) => {
                    out.push(' ');
                    out.push_str(c);
                    out.push('\n');
                }
                DiffLine::Add(a) => {
                    out.push('+');
                    out.push_str(a);
                    out.push('\n');
                }
                DiffLine::Remove(r) => {
                    out.push('-');
                    out.push_str(r);
                    out.push('\n');
                }
            }
        }
    }

    Some((target_path.to_string(), out))
}

/// Invert a single diff hunk: swap old and new ranges, and swap Add and Remove lines.
pub fn invert_hunk(hunk: &DiffHunk) -> DiffHunk {
    let mut inv_lines = Vec::with_capacity(hunk.lines.len());
    for line in &hunk.lines {
        match line {
            DiffLine::Context(c) => inv_lines.push(DiffLine::Context(c.clone())),
            DiffLine::Add(a) => inv_lines.push(DiffLine::Remove(a.clone())),
            DiffLine::Remove(r) => inv_lines.push(DiffLine::Add(r.clone())),
        }
    }

    DiffHunk {
        old_start: hunk.new_start,
        old_count: hunk.new_count,
        new_start: hunk.old_start,
        new_count: hunk.old_count,
        lines: inv_lines,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reversibility::classify::classify_diff;

    #[test]
    fn test_capture_plan_for_code_edit() {
        let diff = r#"
diff --git a/src/main.rs b/src/main.rs
index 1111111..2222222 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -5,3 +5,4 @@
 fn run() {
-    println!("version 1");
+    println!("version 2");
+    println!("extra banner");
 }
"#;
        let classification = classify_diff(diff).unwrap();
        let plan = capture_plan(diff, &classification).unwrap();

        assert_eq!(plan.total_ops, 1);
        match &plan.ops[0] {
            CompensationOp::InversePatch { path, patch } => {
                assert_eq!(path, "src/main.rs");
                assert!(patch.contains("diff --git a/src/main.rs b/src/main.rs"));
                assert!(patch.contains("--- a/src/main.rs"));
                assert!(patch.contains("+++ b/src/main.rs"));
                assert!(patch.contains("@@ -5,4 +5,3 @@"));
                assert!(patch.contains("-    println!(\"version 2\");"));
                assert!(patch.contains("-    println!(\"extra banner\");"));
                assert!(patch.contains("+    println!(\"version 1\");"));
            }
            other => panic!("expected InversePatch op, got {other:?}"),
        }
        assert!(plan.rollback_script.contains("patch -p1"));
    }

    #[test]
    fn test_capture_plan_for_additive_new_file() {
        let diff = r#"
diff --git a/tests/test_feature.rs b/tests/test_feature.rs
new file mode 100644
index 0000000..3333333
--- /dev/null
+++ b/tests/test_feature.rs
@@ -0,0 +1,3 @@
+#[test]
+fn test_it() {}
"#;
        let classification = classify_diff(diff).unwrap();
        let plan = capture_plan(diff, &classification).unwrap();

        assert_eq!(plan.total_ops, 1);
        match &plan.ops[0] {
            CompensationOp::DeleteCreatedFile { path } => {
                assert_eq!(path, "tests/test_feature.rs");
            }
            other => panic!("expected DeleteCreatedFile op, got {other:?}"),
        }
        assert!(plan
            .rollback_script
            .contains("rm -f \"tests/test_feature.rs\""));
    }

    #[test]
    fn test_never_zero_value_rejects_empty_fields() {
        let bad_plan = CompensationPlan {
            ops: vec![CompensationOp::InversePatch {
                path: "".to_string(),
                patch: "valid patch".to_string(),
            }],
            rollback_script: "rm file".to_string(),
            total_ops: 1,
        };
        assert!(validate_compensation_plan(&bad_plan).is_err());

        let bad_patch_plan = CompensationPlan {
            ops: vec![CompensationOp::InversePatch {
                path: "src/file.rs".to_string(),
                patch: "   ".to_string(),
            }],
            rollback_script: "rm file".to_string(),
            total_ops: 1,
        };
        assert!(validate_compensation_plan(&bad_patch_plan).is_err());

        let bad_script_plan = CompensationPlan {
            ops: vec![CompensationOp::DeleteCreatedFile {
                path: "src/file.rs".to_string(),
            }],
            rollback_script: "".to_string(),
            total_ops: 1,
        };
        assert!(validate_compensation_plan(&bad_script_plan).is_err());
    }
}
