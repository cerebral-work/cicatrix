//! Cortex security notification integration for tripwire intrusions (CER-2760, Phase 3.2).
//!
//! Broadcasts fail-closed security alerts over the Cortex agent mesh when an
//! unauthorized canary touch occurs.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// Send security notification via cortex-msg CLI over the agent mesh.
///
/// Sends a high-priority security alert to the `coordinator` role.
/// If `CICATRIX_TRIPWIRE_NOTIFY_OUTPUT` is set, writes the alert to the specified file
/// for test assertions.
/// Returns `true` if notification was successfully delivered or recorded.
pub fn notify_cortex_intrusion(
    canary_id: &str,
    target_path: &str,
    actor: &str,
    action: &str,
    context: Option<&str>,
) -> bool {
    let subject = format!("security:canary-tripwire:{canary_id}");
    let mut body = format!(
        "CRITICAL SECURITY ALERT: Canary Tripwire Touched!\n\
         Canary ID: {canary_id}\n\
         Target Path: {target_path}\n\
         Actor: {actor}\n\
         Action: {action}\n\
         Verdict: CIRCUIT_BROKEN (Fail-Closed)\n"
    );
    if let Some(ctx) = context {
        body.push_str(&format!("Context: {ctx}\n"));
    }

    // Check for test mock output path
    if let Ok(out_path) = std::env::var("CICATRIX_TRIPWIRE_NOTIFY_OUTPUT") {
        let trimmed = out_path.trim();
        if !trimmed.is_empty() {
            if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(trimmed) {
                let _ = writeln!(f, "--- NOTIFICATION ---");
                let _ = writeln!(f, "Subject: {subject}");
                let _ = writeln!(f, "{body}");
                return true;
            }
        }
    }

    if let Ok(mock) = std::env::var("CICATRIX_TRIPWIRE_MOCK_NOTIFY") {
        if mock == "1" || mock == "true" {
            return true;
        }
    }

    // Try `cortex-msg` on PATH, then check ~/.claude/bin/cortex-msg
    let binary = if which_exists("cortex-msg") {
        "cortex-msg".to_string()
    } else {
        let home = std::env::var("HOME").unwrap_or_default();
        let fallback = format!("{home}/.claude/bin/cortex-msg");
        if Path::new(&fallback).exists() {
            fallback
        } else {
            eprintln!("[cicatrix-tripwire] cortex-msg binary not found; mesh notification skipped");
            return false;
        }
    };

    match Command::new(&binary)
        .args(["send", "coordinator", &subject, "--body", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(mut child) => {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(body.as_bytes());
            }
            match child.wait() {
                Ok(status) => status.success(),
                Err(e) => {
                    eprintln!("[cicatrix-tripwire] error waiting for cortex-msg child: {e}");
                    false
                }
            }
        }
        Err(e) => {
            eprintln!("[cicatrix-tripwire] failed to spawn cortex-msg (`{binary}`): {e}");
            false
        }
    }
}

fn which_exists(binary: &str) -> bool {
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in path_var.split(':') {
            let p = Path::new(dir).join(binary);
            if p.is_file() {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_notify_mock_file_recording() {
        let tmp = NamedTempFile::new().expect("temp file");
        let path_str = tmp.path().to_str().unwrap().to_string();
        std::env::set_var("CICATRIX_TRIPWIRE_NOTIFY_OUTPUT", &path_str);

        let res = notify_cortex_intrusion(
            "TRIPWIRE_CANARY_SENTINEL_ALPHA",
            ".cicatrix/sentinel/canary_alpha.rs",
            "agent-worker",
            "query",
            Some("attempted file read"),
        );
        std::env::remove_var("CICATRIX_TRIPWIRE_NOTIFY_OUTPUT");

        assert!(res);
        let contents = std::fs::read_to_string(tmp.path()).expect("read temp file");
        assert!(contents.contains("CRITICAL SECURITY ALERT: Canary Tripwire Touched!"));
        assert!(contents.contains("TRIPWIRE_CANARY_SENTINEL_ALPHA"));
        assert!(contents.contains("agent-worker"));
    }
}
