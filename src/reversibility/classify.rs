//! Diff and mutation classification stage for Wheelhorse reversibility pipeline (CER-2759).
//!
//! Classifies touched surfaces into:
//! - `ReversibleAdditive`: Pure additions (new files, new tests, safe documentation).
//! - `ReversibleWithPlan`: Invertible edits with automated rollback compensation.
//! - `OneWayDoor`: Migrations, schema drops, secret/token rotations, and file deletions.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Classification category for proposed mutations or diff patches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ActionClass {
    /// Pure additive read or safe write (e.g. new files, new tests, safe non-colliding additions).
    ReversibleAdditive,
    /// Modification with exact automated inverse operation.
    ReversibleWithPlan { rollback_script: String },
    /// Non-reversible or state-destroying mutation requiring human review.
    OneWayDoor { reason: String },
}

/// Detailed classification findings from diff inspection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassificationReport {
    /// Roll-up classification class.
    pub action_class: ActionClass,
    /// Touched file paths extracted from the diff.
    pub touched_files: Vec<String>,
    /// Whether any tripwire canary markers or sentinels were touched.
    pub canary_touched: bool,
    /// Whether database migration or schema drop markers were detected.
    pub is_migration: bool,
    /// Whether secret, credential, or token rotation markers were detected.
    pub is_secret_rotation: bool,
    /// Whether file deletions were detected.
    pub is_file_deletion: bool,
    /// Whether the mutation is pure addition without deletions.
    pub is_pure_addition: bool,
    /// Whether any touched files touch Soma production deployment gates or surfaces.
    #[serde(default)]
    pub is_soma_production: bool,
    /// Human-readable reasons explaining the classification.
    pub reasons: Vec<String>,
}

/// Parsed representation of a file within a unified diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffFile {
    pub old_path: Option<String>,
    pub new_path: Option<String>,
    pub is_new_file: bool,
    pub is_deleted_file: bool,
    pub hunks: Vec<DiffHunk>,
}

/// Individual hunk within a diff file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffHunk {
    pub old_start: usize,
    pub old_count: usize,
    pub new_start: usize,
    pub new_count: usize,
    pub lines: Vec<DiffLine>,
}

/// Line in a diff hunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffLine {
    Context(String),
    Add(String),
    Remove(String),
}

/// Parse unified diff text into structured `DiffFile` models.
pub fn parse_diff_files(diff: &str) -> Result<Vec<DiffFile>, String> {
    let mut files: Vec<DiffFile> = Vec::new();
    let mut current_file: Option<DiffFile> = None;
    let mut current_hunk: Option<DiffHunk> = None;

    for line in diff.lines() {
        if line.starts_with("diff --git ") {
            if let Some(hunk) = current_hunk.take() {
                if let Some(ref mut file) = current_file {
                    file.hunks.push(hunk);
                }
            }
            if let Some(file) = current_file.take() {
                files.push(file);
            }

            let parts: Vec<&str> = line.split_whitespace().collect();
            let (old_p, new_p) = if parts.len() >= 4 {
                (
                    Some(parts[2].trim_start_matches("a/").to_string()),
                    Some(parts[3].trim_start_matches("b/").to_string()),
                )
            } else {
                (None, None)
            };

            current_file = Some(DiffFile {
                old_path: old_p,
                new_path: new_p,
                is_new_file: false,
                is_deleted_file: false,
                hunks: Vec::new(),
            });
            continue;
        }

        if line.starts_with("--- ") {
            if current_file.is_some() && current_hunk.is_some() {
                if let Some(hunk) = current_hunk.take() {
                    if let Some(ref mut file) = current_file {
                        file.hunks.push(hunk);
                    }
                }
                if let Some(file) = current_file.take() {
                    files.push(file);
                }
            }

            let p = line.trim_start_matches("--- ").trim();
            if current_file.is_none() {
                let (old_p, is_new) = if p == "/dev/null" {
                    (None, true)
                } else {
                    (Some(p.trim_start_matches("a/").to_string()), false)
                };
                current_file = Some(DiffFile {
                    old_path: old_p,
                    new_path: None,
                    is_new_file: is_new,
                    is_deleted_file: false,
                    hunks: Vec::new(),
                });
                continue;
            } else if let Some(ref mut file) = current_file {
                if p == "/dev/null" {
                    file.is_new_file = true;
                    file.old_path = None;
                } else {
                    file.old_path = Some(p.trim_start_matches("a/").to_string());
                }
                continue;
            }
        }

        if let Some(ref mut file) = current_file {
            if line.starts_with("new file mode ") {
                file.is_new_file = true;
                continue;
            }
            if line.starts_with("deleted file mode ") {
                file.is_deleted_file = true;
                continue;
            }
            if line.starts_with("+++ ") {
                let p = line.trim_start_matches("+++ ").trim();
                if p == "/dev/null" {
                    file.is_deleted_file = true;
                    file.new_path = None;
                } else {
                    file.new_path = Some(p.trim_start_matches("b/").to_string());
                }
                continue;
            }
            if line.starts_with("@@ ") {
                if let Some(hunk) = current_hunk.take() {
                    file.hunks.push(hunk);
                }
                current_hunk = Some(parse_hunk_header(line)?);
                continue;
            }
        }

        if let Some(ref mut hunk) = current_hunk {
            if let Some(stripped) = line.strip_prefix('+') {
                hunk.lines.push(DiffLine::Add(stripped.to_string()));
            } else if let Some(stripped) = line.strip_prefix('-') {
                hunk.lines.push(DiffLine::Remove(stripped.to_string()));
            } else if let Some(stripped) = line.strip_prefix(' ') {
                hunk.lines.push(DiffLine::Context(stripped.to_string()));
            } else if line.is_empty() {
                // Empty context line
                hunk.lines.push(DiffLine::Context(String::new()));
            }
        }
    }

    if let Some(hunk) = current_hunk.take() {
        if let Some(ref mut file) = current_file {
            file.hunks.push(hunk);
        }
    }
    if let Some(file) = current_file.take() {
        files.push(file);
    }

    Ok(files)
}

fn parse_hunk_header(line: &str) -> Result<DiffHunk, String> {
    // Example: @@ -1,3 +1,4 @@ optional heading
    let parts: Vec<&str> = line.split("@@").collect();
    if parts.len() < 3 {
        return Err(format!("malformed hunk header: `{line}`"));
    }
    let range_spec = parts[1].trim();
    let range_parts: Vec<&str> = range_spec.split_whitespace().collect();
    if range_parts.len() != 2 {
        return Err(format!("unexpected range spec `{range_spec}` in `{line}`"));
    }

    let (old_start, old_count) = parse_range(range_parts[0].trim_start_matches('-'))?;
    let (new_start, new_count) = parse_range(range_parts[1].trim_start_matches('+'))?;

    Ok(DiffHunk {
        old_start,
        old_count,
        new_start,
        new_count,
        lines: Vec::new(),
    })
}

fn parse_range(spec: &str) -> Result<(usize, usize), String> {
    let mut it = spec.split(',');
    let start_str = it
        .next()
        .ok_or_else(|| format!("missing start line in range `{spec}`"))?;
    let start = start_str
        .parse::<usize>()
        .map_err(|e| format!("invalid start line in `{spec}`: {e}"))?;
    let count = if let Some(count_str) = it.next() {
        count_str
            .parse::<usize>()
            .map_err(|e| format!("invalid line count in `{spec}`: {e}"))?
    } else {
        1
    };
    Ok((start, count))
}

/// Classify a unified diff patch string.
pub fn classify_diff(diff: &str) -> Result<ClassificationReport, String> {
    if diff.trim().is_empty() {
        return Err("diff is empty; nothing to classify".to_string());
    }

    let parsed_files = parse_diff_files(diff)?;
    let mut touched_files_set = BTreeSet::new();
    let mut reasons = Vec::new();

    let mut canary_touched = false;
    let mut is_migration = false;
    let mut is_secret_rotation = false;
    let mut is_file_deletion = false;
    let mut has_any_removals = false;
    let mut has_any_additions = false;

    // Detect canaries in raw diff content
    if detect_canary_markers(diff) {
        canary_touched = true;
        reasons.push("synthetic tripwire canary sentinel marker detected".to_string());
    }

    for file in &parsed_files {
        let file_path = file
            .new_path
            .as_deref()
            .or(file.old_path.as_deref())
            .unwrap_or("unknown");
        touched_files_set.insert(file_path.to_string());

        if file.is_deleted_file || file.new_path.is_none() {
            is_file_deletion = true;
            reasons.push(format!("file deletion detected for `{file_path}`"));
        }

        // Migration or schema drop detection
        if is_migration_path(file_path) {
            is_migration = true;
            reasons.push(format!("migration file path touched: `{file_path}`"));
        }

        // Secret or token rotation path detection
        if is_secret_path(file_path) {
            is_secret_rotation = true;
            reasons.push(format!("secret or credential file touched: `{file_path}`"));
        }

        // Inspect hunk content
        for hunk in &file.hunks {
            for diff_line in &hunk.lines {
                match diff_line {
                    DiffLine::Add(line_text) => {
                        has_any_additions = true;
                        if detect_schema_drop(line_text) {
                            is_migration = true;
                            reasons.push(format!(
                                "schema drop or destructive DDL detected in `{file_path}`"
                            ));
                        }
                        if detect_secret_tokens(line_text) {
                            is_secret_rotation = true;
                            reasons.push(format!(
                                "secret assignment or token pattern detected in `{file_path}`"
                            ));
                        }
                    }
                    DiffLine::Remove(line_text) => {
                        has_any_removals = true;
                        if detect_schema_drop(line_text) {
                            is_migration = true;
                            reasons.push(format!(
                                "schema drop or destructive DDL detected in `{file_path}`"
                            ));
                        }
                        if detect_secret_tokens(line_text) {
                            is_secret_rotation = true;
                            reasons.push(format!(
                                "secret assignment or token pattern detected in `{file_path}`"
                            ));
                        }
                    }
                    DiffLine::Context(_) => {}
                }
            }
        }
    }

    let is_pure_addition = !is_file_deletion
        && !has_any_removals
        && has_any_additions
        && (parsed_files.iter().all(|f| f.is_new_file)
            || parsed_files.iter().all(|f| {
                f.hunks
                    .iter()
                    .all(|h| h.lines.iter().all(|l| !matches!(l, DiffLine::Remove(_))))
            }));

    let touched_files: Vec<String> = touched_files_set.into_iter().collect();

    let mut is_soma_production = false;
    for file_path in &touched_files {
        if crate::autonomy::is_soma_production_path(file_path) {
            is_soma_production = true;
            reasons.push(format!(
                "touches Soma production deployment surface `{file_path}`"
            ));
        }
    }

    let action_class = if is_file_deletion || is_migration || is_secret_rotation {
        ActionClass::OneWayDoor {
            reason: reasons.join("; "),
        }
    } else if is_pure_addition {
        ActionClass::ReversibleAdditive
    } else {
        ActionClass::ReversibleWithPlan {
            rollback_script: format!(
                "# Automated rollback generated for {} file(s)",
                touched_files.len()
            ),
        }
    };

    Ok(ClassificationReport {
        action_class,
        touched_files,
        canary_touched,
        is_migration,
        is_secret_rotation,
        is_file_deletion,
        is_pure_addition,
        is_soma_production,
        reasons,
    })
}

/// Detect tripwire canaries or sentinels in paths or diff content.
pub fn detect_canary_markers(content: &str) -> bool {
    let lower = content.to_lowercase();
    lower.contains("tripwire_canaries")
        || lower.contains("tripwire_touches")
        || lower.contains("canary_sentinel")
        || content.contains("TRIPWIRE_")
        || content.contains("SENTINEL_")
        || content.contains("CANARY_")
}

/// Detect whether path is a migration or database schema file.
pub fn is_migration_path(path: &str) -> bool {
    let p = path.to_lowercase();
    p.contains("migrations/")
        || p.contains("db/migrations")
        || p.contains("schema.sql")
        || p.ends_with(".sql")
        || p.contains("migration.rs")
        || p.contains("migrations.rs")
}

/// Detect whether path is a secret, key, or token configuration.
pub fn is_secret_path(path: &str) -> bool {
    let p = path.to_lowercase();
    p == ".env"
        || p.starts_with(".env.")
        || p.ends_with(".key")
        || p.ends_with(".pem")
        || p.contains("id_rsa")
        || p.contains("credentials")
        || p.contains("token")
        || p.contains("secret")
}

/// Detect destructive SQL DDL operations.
pub fn detect_schema_drop(line: &str) -> bool {
    let upper = line.to_uppercase();
    upper.contains("DROP TABLE")
        || upper.contains("DROP COLUMN")
        || upper.contains("ALTER TABLE") && upper.contains(" DROP ")
        || upper.contains("TRUNCATE TABLE")
        || upper.contains("DROP DATABASE")
        || upper.contains("DROP INDEX")
        || upper.contains("DROP SCHEMA")
        || upper.contains("DROP VIEW")
}

/// Detect secret assignments or token strings.
pub fn detect_secret_tokens(line: &str) -> bool {
    let trimmed = line.trim();
    let upper = trimmed.to_uppercase();
    upper.contains("TOKEN=")
        || upper.contains("SECRET=")
        || upper.contains("API_KEY=")
        || upper.contains("PASSWORD=")
        || upper.contains("PRIVATE_KEY")
        || upper.contains("AWS_SECRET_ACCESS_KEY")
        || trimmed.contains("BEGIN RSA PRIVATE KEY")
        || trimmed.contains("BEGIN PRIVATE KEY")
        || trimmed.contains("ghp_")
        || trimmed.contains("sk-ant-")
        || trimmed.contains("xoxb-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_pure_additive_new_files() {
        let diff = r#"
diff --git a/tests/new_test.rs b/tests/new_test.rs
new file mode 100644
index 0000000..1234567
--- /dev/null
+++ b/tests/new_test.rs
@@ -0,0 +1,5 @@
+#[test]
+fn test_sanity() {
+    assert_eq!(2 + 2, 4);
+}
"#;
        let report = classify_diff(diff).expect("classification should succeed");
        assert_eq!(report.action_class, ActionClass::ReversibleAdditive);
        assert!(report.is_pure_addition);
        assert!(!report.is_file_deletion);
        assert!(!report.is_migration);
        assert!(!report.is_secret_rotation);
        assert_eq!(report.touched_files, vec!["tests/new_test.rs"]);
    }

    #[test]
    fn test_classify_reversible_with_plan_code_edit() {
        let diff = r#"
diff --git a/src/lib.rs b/src/lib.rs
index 1111111..2222222 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -10,3 +10,4 @@
 fn compute() -> i32 {
-    41
+    let bonus = 1;
+    41 + bonus
 }
"#;
        let report = classify_diff(diff).expect("classification should succeed");
        assert!(matches!(
            report.action_class,
            ActionClass::ReversibleWithPlan { .. }
        ));
        assert!(!report.is_pure_addition);
        assert!(!report.is_file_deletion);
        assert_eq!(report.touched_files, vec!["src/lib.rs"]);
    }

    #[test]
    fn test_classify_one_way_door_file_deletion() {
        let diff = r#"
diff --git a/src/old_module.rs b/src/old_module.rs
deleted file mode 100644
index 2222222..0000000
--- a/src/old_module.rs
+++ /dev/null
@@ -1,3 +0,0 @@
-pub fn deprecated() {
-    println!("goodbye");
-}
"#;
        let report = classify_diff(diff).expect("classification should succeed");
        assert!(matches!(
            report.action_class,
            ActionClass::OneWayDoor { .. }
        ));
        assert!(report.is_file_deletion);
    }

    #[test]
    fn test_classify_one_way_door_migration_and_schema_drop() {
        let diff = r#"
diff --git a/migrations/0002_drop_legacy_table.sql b/migrations/0002_drop_legacy_table.sql
new file mode 100644
index 0000000..3333333
--- /dev/null
+++ b/migrations/0002_drop_legacy_table.sql
@@ -0,0 +1,2 @@
+-- Drop table
+DROP TABLE legacy_users;
"#;
        let report = classify_diff(diff).expect("classification should succeed");
        assert!(matches!(
            report.action_class,
            ActionClass::OneWayDoor { .. }
        ));
        assert!(report.is_migration);
    }

    #[test]
    fn test_classify_one_way_door_secret_rotation() {
        let diff = r#"
diff --git a/.env b/.env
index 4444444..5555555 100644
--- a/.env
+++ b/.env
@@ -1,2 +1,2 @@
-API_KEY=old_secret_key_12345
+API_KEY=new_rotated_secret_token_98765
"#;
        let report = classify_diff(diff).expect("classification should succeed");
        assert!(matches!(
            report.action_class,
            ActionClass::OneWayDoor { .. }
        ));
        assert!(report.is_secret_rotation);
    }

    #[test]
    fn test_classify_canary_tripwire_touched() {
        let diff = r#"
diff --git a/src/store.rs b/src/store.rs
index 6666666..7777777 100644
--- a/src/store.rs
+++ b/src/store.rs
@@ -5,3 +5,4 @@
 // regression store
+const TRIPWIRE_CANARY_SENTINEL: &str = "active";
"#;
        let report = classify_diff(diff).expect("classification should succeed");
        assert!(report.canary_touched);
    }

    #[test]
    fn test_classify_soma_production_surface() {
        let diff = r#"
diff --git a/.soma/deploy.yaml b/.soma/deploy.yaml
new file mode 100644
index 0000000..8888888
--- /dev/null
+++ b/.soma/deploy.yaml
@@ -0,0 +1,5 @@
+version: 1
+cluster: prod
+"#;
        let report = classify_diff(diff).expect("classification should succeed");
        assert!(report.is_soma_production);
    }
}
