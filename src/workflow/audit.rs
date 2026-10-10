//! Convention audit workflow for cicatrix (CER-2756, Phase 2.1).
//!
//! Executes cross-repo marker scans across estate targets and aggregates convention drift reports.

use std::fs;
use std::path::Path;
use std::time::Duration;

use autumn_harvest::policy::RetryPolicy;
use autumn_harvest::prelude::*;
use autumn_harvest_macros::{activity, workflow};
use serde::{Deserialize, Serialize};

/// Input payload for convention audit workflow.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuditInput {
    /// List of repository paths or targets to scan.
    #[serde(default)]
    pub repo_paths: Vec<String>,
    /// Optional specific convention marker to audit for (e.g. `AUTHLOG`, `FIXME`).
    #[serde(default)]
    pub convention_marker: Option<String>,
    /// Optional scan classification labels.
    #[serde(default)]
    pub scan_labels: Vec<String>,
}

/// Input payload for scanning a single target repository.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScanTargetInput {
    /// Target repository path.
    pub repo_path: String,
    /// Optional specific convention marker.
    pub convention_marker: Option<String>,
    /// Classification labels.
    pub scan_labels: Vec<String>,
}

/// Scan result for a single target repository.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TargetScanResult {
    /// Scanned repository or directory path.
    pub repo_path: String,
    /// List of markers or violations identified.
    pub markers_found: Vec<String>,
    /// Whether convention drift was detected.
    pub drift_detected: bool,
    /// Human-readable scan details.
    pub details: String,
}

/// Input for aggregating multiple scan results into a unified report.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AggregateInput {
    /// Collected target scan results.
    pub target_results: Vec<TargetScanResult>,
}

/// Final report produced by the convention audit workflow.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuditReport {
    /// Number of targets scanned.
    pub scanned_targets: usize,
    /// Number of targets exhibiting convention drift.
    pub drift_count: usize,
    /// Detailed results per target.
    pub results: Vec<TargetScanResult>,
    /// Human-readable summary.
    pub summary: String,
}

/// Synchronous runner for scanning a single target for convention markers.
pub fn run_scan_repo_markers(input: ScanTargetInput) -> Result<TargetScanResult, String> {
    let path = Path::new(&input.repo_path);
    if !path.exists() {
        return Ok(TargetScanResult {
            repo_path: input.repo_path,
            markers_found: Vec::new(),
            drift_detected: false,
            details: "Target path does not exist on local filesystem".to_string(),
        });
    }

    let default_markers = ["AUTHLOG", "DRIFT", "FIXME", "TODO"];
    let markers_to_check: Vec<&str> = if let Some(ref m) = input.convention_marker {
        if !m.trim().is_empty() {
            vec![m.trim()]
        } else {
            default_markers.to_vec()
        }
    } else {
        default_markers.to_vec()
    };

    let mut found = Vec::new();

    // Scan immediate entries or recurse 1 level to avoid unbounded traversal
    let mut files_to_scan = Vec::new();
    if path.is_file() {
        files_to_scan.push(path.to_path_buf());
    } else if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file() {
                files_to_scan.push(p);
            } else if p.is_dir() {
                let dir_name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !dir_name.starts_with('.') && dir_name != "target" && dir_name != "node_modules"
                {
                    if let Ok(sub_entries) = fs::read_dir(&p) {
                        for sub_entry in sub_entries.flatten() {
                            let sp = sub_entry.path();
                            if sp.is_file() {
                                files_to_scan.push(sp);
                            }
                        }
                    }
                }
            }
        }
    }

    for file_path in files_to_scan {
        let ext = file_path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();
        if !matches!(
            ext.as_str(),
            "rs" | "md" | "toml" | "json" | "yaml" | "yml" | "txt" | "sh"
        ) {
            continue;
        }

        if let Ok(content) = fs::read_to_string(&file_path) {
            for marker in &markers_to_check {
                if content.contains(marker) {
                    let rel_path = file_path.display().to_string();
                    found.push(format!("{rel_path}:{marker}"));
                }
            }
        }
    }

    let drift_detected = !found.is_empty();
    let details = format!(
        "Scanned `{}`: identified {} convention markers across checked files",
        input.repo_path,
        found.len()
    );

    Ok(TargetScanResult {
        repo_path: input.repo_path,
        markers_found: found,
        drift_detected,
        details,
    })
}

/// Synchronous runner for aggregating target scan results into an audit report.
pub fn run_aggregate_drift_report(input: AggregateInput) -> Result<AuditReport, String> {
    let scanned_targets = input.target_results.len();
    let drift_count = input
        .target_results
        .iter()
        .filter(|r| r.drift_detected)
        .count();

    let summary = format!(
        "Convention audit complete: scanned {scanned_targets} target(s); detected drift in {drift_count} target(s)."
    );

    Ok(AuditReport {
        scanned_targets,
        drift_count,
        results: input.target_results,
        summary,
    })
}

/// Scan target repo markers activity declaration.
#[activity(
    start_to_close = "600s",
    retry = RetryPolicy::exponential(3, Duration::from_secs(1))
)]
pub async fn scan_repo_markers(
    _ctx: &ActivityContext,
    input: ScanTargetInput,
) -> Result<TargetScanResult, String> {
    run_scan_repo_markers(input)
}

/// Aggregate drift report activity declaration.
#[activity(
    start_to_close = "600s",
    retry = RetryPolicy::exponential(3, Duration::from_secs(1))
)]
pub async fn aggregate_drift_report(
    _ctx: &ActivityContext,
    input: AggregateInput,
) -> Result<AuditReport, String> {
    run_aggregate_drift_report(input)
}

/// Convention audit workflow orchestrating cross-repo marker scans and drift aggregation.
#[workflow(mcp)]
pub async fn audit_workflow(
    ctx: &WorkflowContext,
    input: AuditInput,
) -> Result<AuditReport, String> {
    let targets = if input.repo_paths.is_empty() {
        vec![".".to_string()]
    } else {
        input.repo_paths.clone()
    };

    let mut target_results = Vec::new();
    for target in targets {
        let scan_input = ScanTargetInput {
            repo_path: target,
            convention_marker: input.convention_marker.clone(),
            scan_labels: input.scan_labels.clone(),
        };

        let result: TargetScanResult = ctx
            .execute_activity(&scan_repo_markers_info(), scan_input)
            .await
            .map_err(|e| e.to_string())?;

        target_results.push(result);
    }

    let agg_input = AggregateInput { target_results };

    let report: AuditReport = ctx
        .execute_activity(&aggregate_drift_report_info(), agg_input)
        .await
        .map_err(|e| e.to_string())?;

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_scan_repo_markers_nonexistent() {
        let input = ScanTargetInput {
            repo_path: "/path/to/nonexistent/repo_dir_12345".to_string(),
            convention_marker: None,
            scan_labels: vec![],
        };

        let result = run_scan_repo_markers(input).expect("scan should complete without error");
        assert!(!result.drift_detected);
        assert!(result.markers_found.is_empty());
        assert!(result.details.contains("does not exist"));
    }

    #[test]
    fn test_scan_repo_markers_detects_drift() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let file_path = temp_dir.path().join("service.rs");
        fs::write(
            &file_path,
            "// AUTHLOG: critical auth failure marker\nfn main() { println!(\"ok\"); }\n// FIXME: temporary workaround\n",
        )
        .expect("write test file");

        let input = ScanTargetInput {
            repo_path: temp_dir.path().display().to_string(),
            convention_marker: None,
            scan_labels: vec![],
        };

        let result = run_scan_repo_markers(input).expect("scan should succeed");
        assert!(result.drift_detected);
        assert_eq!(result.markers_found.len(), 2);
        assert!(result.markers_found.iter().any(|m| m.contains("AUTHLOG")));
        assert!(result.markers_found.iter().any(|m| m.contains("FIXME")));
    }

    #[test]
    fn test_aggregate_drift_report() {
        let target_results = vec![
            TargetScanResult {
                repo_path: "repo_a".to_string(),
                markers_found: vec!["file.rs:TODO".to_string()],
                drift_detected: true,
                details: "1 marker".to_string(),
            },
            TargetScanResult {
                repo_path: "repo_b".to_string(),
                markers_found: vec![],
                drift_detected: false,
                details: "clean".to_string(),
            },
        ];

        let input = AggregateInput { target_results };
        let report = run_aggregate_drift_report(input).expect("aggregation should succeed");
        assert_eq!(report.scanned_targets, 2);
        assert_eq!(report.drift_count, 1);
        assert!(report.summary.contains("scanned 2 target(s)"));
        assert!(report.summary.contains("detected drift in 1 target(s)"));
    }
}
