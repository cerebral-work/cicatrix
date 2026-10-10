//! Unified error masking policy (CER-2761, Phase 3.3).
//!
//! Enforces:
//! - Correlation ID generation with the `ref_` prefix.
//! - Redaction of internal paths (`/home/...`, `/tmp/...`, `/rustc/...`, `C:\Users\...`).
//! - Redaction of credentials (API keys, bearer tokens, secrets, passwords, private keys, auth headers).
//! - Stripping of stack traces and backtrace blocks.
//! - Generic error masking for 5xx / internal server errors across CLI, REST HTTP, and MCP JSON-RPC.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static CORRELATION_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Generate a correlation identifier with the `ref_` prefix per EARS spec and Wheelhorse policy.
pub fn generate_correlation_id() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let count = CORRELATION_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("ref_{:x}_{:04x}", now, count & 0xffff)
}

/// Redact credentials, tokens, API keys, and passwords from arbitrary strings.
pub fn sanitize_credentials(input: &str) -> String {
    let mut result = input.to_string();

    // 1. Redact PEM private key blocks
    while let Some(start) = result.find("-----BEGIN ") {
        if let Some(end_marker) = result[start..].find("-----END ") {
            let after_end = start + end_marker + "-----END ".len();
            if let Some(final_dashes) = result[after_end..].find("-----") {
                let end = after_end + final_dashes + 5;
                result.replace_range(start..end, "[REDACTED_PRIVATE_KEY]");
                continue;
            }
        }
        break;
    }

    // 2. Redact URLs with embedded credentials (e.g., http://user:pass@host)
    let url_schemes = ["http://", "https://", "postgres://", "mysql://", "redis://"];
    for scheme in &url_schemes {
        let mut search_idx = 0;
        while let Some(scheme_pos) = result[search_idx..].find(scheme) {
            let abs_scheme_pos = search_idx + scheme_pos + scheme.len();
            if let Some(at_pos) = result[abs_scheme_pos..].find('@') {
                let segment = &result[abs_scheme_pos..abs_scheme_pos + at_pos];
                // Check if segment contains colon and no slashes or whitespace
                if segment.contains(':') && !segment.contains('/') && !segment.contains(' ') {
                    let full_start = abs_scheme_pos;
                    let full_end = abs_scheme_pos + at_pos;
                    result.replace_range(full_start..full_end, "[REDACTED_AUTH]");
                    search_idx = full_start + "[REDACTED_AUTH]".len();
                    continue;
                }
            }
            search_idx = abs_scheme_pos;
        }
    }

    // 3. Redact Bearer tokens
    let bearer_prefixes = ["Bearer ", "bearer "];
    for b_prefix in &bearer_prefixes {
        let mut search_idx = 0;
        while let Some(pos) = result[search_idx..].find(b_prefix) {
            let start = search_idx + pos + b_prefix.len();
            let remainder = &result[start..];
            let end_offset = remainder
                .find(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == ',' || c == ';')
                .unwrap_or(remainder.len());
            let token = &remainder[..end_offset];
            if !token.is_empty() && token != "[REDACTED]" {
                result.replace_range(start..start + end_offset, "[REDACTED]");
                search_idx = start + "[REDACTED]".len();
            } else {
                search_idx = start + end_offset;
            }
        }
    }

    // 4. Redact key-value secrets (e.g. api_key="secret", token=abc1234, password: secret)
    let secret_keys = [
        "authorization",
        "auth_token",
        "access_token",
        "client_secret",
        "private_key",
        "api_key",
        "apikey",
        "api-key",
        "password",
        "passwd",
        "secret",
        "token",
        "auth",
        "key",
    ];

    for key in &secret_keys {
        let mut search_idx = 0;
        while search_idx < result.len() {
            let lower_sub = result[search_idx..].to_ascii_lowercase();
            if let Some(pos) = lower_sub.find(key) {
                let abs_key_pos = search_idx + pos;
                let after_key = abs_key_pos + key.len();

                // Ensure word boundary before key
                let is_word_boundary = if abs_key_pos == 0 {
                    true
                } else {
                    let prev = result[..abs_key_pos].chars().last().unwrap();
                    !prev.is_alphanumeric() && prev != '_' && prev != '-'
                };

                if !is_word_boundary {
                    search_idx = after_key;
                    continue;
                }

                // Check for separator (: or =) with optional spaces
                let remaining = &result[after_key..];
                let mut sep_offset = None;
                for (i, c) in remaining.char_indices() {
                    if c == ':' || c == '=' {
                        sep_offset = Some(after_key + i + 1);
                        break;
                    } else if !c.is_whitespace() && c != '"' && c != '\'' {
                        break;
                    }
                }

                if let Some(val_start_after_sep) = sep_offset {
                    let after_sep = &result[val_start_after_sep..];
                    let mut val_start = val_start_after_sep;
                    let mut quote_char = None;

                    for (i, c) in after_sep.char_indices() {
                        if c == '"' || c == '\'' {
                            quote_char = Some(c);
                            val_start = val_start_after_sep + i + 1;
                            break;
                        } else if !c.is_whitespace() {
                            val_start = val_start_after_sep + i;
                            break;
                        }
                    }

                    if val_start < result.len() {
                        let val_remainder = &result[val_start..];
                        if val_remainder.starts_with("Bearer ")
                            || val_remainder.starts_with("bearer ")
                        {
                            search_idx = val_start + 7;
                            continue;
                        }
                        if val_remainder.starts_with("[REDACTED]") {
                            search_idx = val_start + "[REDACTED]".len();
                            continue;
                        }

                        let val_len = if let Some(q) = quote_char {
                            val_remainder.find(q).unwrap_or(val_remainder.len())
                        } else {
                            val_remainder
                                .find(|c: char| {
                                    c.is_whitespace()
                                        || c == ','
                                        || c == ';'
                                        || c == '}'
                                        || c == ')'
                                })
                                .unwrap_or(val_remainder.len())
                        };

                        let val_slice = &val_remainder[..val_len];
                        if !val_slice.is_empty() && val_slice != "[REDACTED]" {
                            result.replace_range(val_start..val_start + val_len, "[REDACTED]");
                            search_idx = val_start + "[REDACTED]".len();
                            continue;
                        }
                    }
                }

                search_idx = after_key;
            } else {
                break;
            }
        }
    }

    result
}

/// Redact internal host file paths (/home/<user>/..., /tmp/..., C:\Users\<user>\..., /rustc/...).
pub fn sanitize_internal_paths(input: &str) -> String {
    let mut result = input.to_string();

    // 1. Redact /home/<user>/... -> [HOME]/...
    let mut search_idx = 0;
    while let Some(pos) = result[search_idx..].find("/home/") {
        let abs_pos = search_idx + pos;
        let user_start = abs_pos + "/home/".len();
        let remainder = &result[user_start..];
        if let Some(slash_offset) = remainder.find('/') {
            let replace_end = user_start + slash_offset;
            result.replace_range(abs_pos..replace_end, "[HOME]");
            search_idx = abs_pos + "[HOME]".len();
        } else {
            // Reached end of string with just /home/<user>
            let end_offset = remainder
                .find(|c: char| c.is_whitespace() || c == '"' || c == '\'')
                .unwrap_or(remainder.len());
            result.replace_range(abs_pos..user_start + end_offset, "[HOME]");
            search_idx = abs_pos + "[HOME]".len();
        }
    }

    // 2. Redact /tmp/ paths -> [TMP]/...
    search_idx = 0;
    while let Some(pos) = result[search_idx..].find("/tmp/") {
        let abs_pos = search_idx + pos;
        result.replace_range(abs_pos..abs_pos + 5, "[TMP]/");
        search_idx = abs_pos + "[TMP]/".len();
    }

    // 3. Redact /rustc/<hash>/ paths -> [RUSTC]/...
    search_idx = 0;
    while let Some(pos) = result[search_idx..].find("/rustc/") {
        let abs_pos = search_idx + pos;
        let hash_start = abs_pos + "/rustc/".len();
        let remainder = &result[hash_start..];
        if let Some(slash_offset) = remainder.find('/') {
            let replace_end = hash_start + slash_offset;
            result.replace_range(abs_pos..replace_end, "[RUSTC]");
            search_idx = abs_pos + "[RUSTC]".len();
        } else {
            break;
        }
    }

    // 4. Redact Windows C:\Users\<user>\ paths -> [USER_PROFILE]\...
    search_idx = 0;
    let win_user = "c:\\users\\";
    while search_idx < result.len() {
        let lower = result[search_idx..].to_ascii_lowercase();
        if let Some(pos) = lower.find(win_user) {
            let abs_pos = search_idx + pos;
            let user_start = abs_pos + win_user.len();
            let remainder = &result[user_start..];
            if let Some(slash_offset) = remainder.find('\\') {
                let replace_end = user_start + slash_offset;
                result.replace_range(abs_pos..replace_end, "[USER_PROFILE]");
                search_idx = abs_pos + "[USER_PROFILE]".len();
            } else {
                break;
            }
        } else {
            break;
        }
    }

    result
}

/// Strip stack backtraces, panic frames, and compiler internal location dumps.
pub fn strip_stack_traces(input: &str) -> String {
    let mut lines = Vec::new();
    let mut in_backtrace = false;

    for line in input.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("stack backtrace:") {
            in_backtrace = true;
            lines.push("[STACK TRACE STRIPPED]");
            continue;
        }

        if in_backtrace {
            // Stack trace frames typically start with digits and a colon (e.g. "   0: ...") or "at /..."
            let is_frame = trimmed.starts_with("at ")
                || (trimmed
                    .chars()
                    .next()
                    .map(|c| c.is_ascii_digit())
                    .unwrap_or(false)
                    && trimmed.contains(':'));
            let is_note = trimmed.starts_with("note: Some details are omitted")
                || trimmed.starts_with("note: run with `RUST_BACKTRACE=");

            if is_frame || is_note || trimmed.is_empty() {
                continue;
            } else {
                in_backtrace = false;
            }
        }

        lines.push(line);
    }

    lines.join("\n")
}

/// Apply full sanitization suite (stack traces, credentials, internal paths).
pub fn sanitize_all(input: &str) -> String {
    let s1 = strip_stack_traces(input);
    let s2 = sanitize_credentials(&s1);
    sanitize_internal_paths(&s2)
}

/// Standardized masked error representation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaskedError {
    /// Categorical type: "internal_error" (5xx) or "client_error" (4xx).
    pub error_type: String,
    /// Publicly safe error message (masked with correlation ID for internal errors).
    pub message: String,
    /// Correlation identifier for internal error tracking (`ref_...`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
}

impl MaskedError {
    /// Construct a 5xx internal error masked behind a `ref_` correlation ID.
    /// Internal diagnostic details are logged to stderr with the correlation ID,
    /// while the returned error body contains zero file paths, stack traces, or credentials.
    pub fn internal(details: impl AsRef<str>) -> Self {
        let correlation_id = generate_correlation_id();
        let sanitized = sanitize_all(details.as_ref());
        eprintln!("[cicatrix] internal error [{correlation_id}]: {sanitized}");

        Self {
            error_type: "internal_error".to_string(),
            message: format!("internal error (correlation: {correlation_id})"),
            correlation_id: Some(correlation_id),
        }
    }

    /// Construct a client-side error (4xx equivalent) with sanitized diagnostic message.
    pub fn client(message: impl Into<String>) -> Self {
        let msg = sanitize_all(&message.into());
        Self {
            error_type: "client_error".to_string(),
            message: msg,
            correlation_id: None,
        }
    }

    /// Format for CLI standard error reporting.
    pub fn to_cli_string(&self, prefix: &str) -> String {
        format!("{prefix}: {}", self.message)
    }

    /// Format as JSON Value for `--json` CLI output or REST responses.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_else(|_| {
            serde_json::json!({
                "error_type": self.error_type,
                "message": self.message,
                "correlation_id": self.correlation_id,
            })
        })
    }

    /// Convert to HTTP status code, serialized JSON response body, and optional X-Correlation-Id header.
    pub fn to_http_response(&self) -> (u16, String, Option<String>) {
        if self.error_type == "internal_error" {
            let ref_id = self
                .correlation_id
                .clone()
                .unwrap_or_else(generate_correlation_id);
            let body = serde_json::json!({
                "error": "internal_server_error",
                "message": format!("internal server error (correlation: {ref_id})"),
                "correlation_id": ref_id,
            })
            .to_string();
            (500, body, Some(ref_id))
        } else {
            let body = serde_json::json!({
                "error": "bad_request",
                "message": self.message,
            })
            .to_string();
            (400, body, None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_correlation_id_uniqueness_and_prefix() {
        let id1 = generate_correlation_id();
        let id2 = generate_correlation_id();
        assert!(id1.starts_with("ref_"));
        assert!(id2.starts_with("ref_"));
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_credential_sanitization() {
        let input = "Failed to connect with Authorization: Bearer secret_token_123456 and api_key=\"key_xyz987\"";
        let sanitized = sanitize_credentials(input);
        assert!(!sanitized.contains("secret_token_123456"));
        assert!(!sanitized.contains("key_xyz987"));
        assert!(sanitized.contains("Bearer [REDACTED]"));
        assert!(sanitized.contains("api_key=\"[REDACTED]\""));

        let db_url = "postgres://admin:supersecretpwd@db.internal:5432/regress";
        let sanitized_url = sanitize_credentials(db_url);
        assert!(!sanitized_url.contains("supersecretpwd"));
        assert!(sanitized_url.contains("postgres://[REDACTED_AUTH]@db.internal:5432/regress"));

        let pem =
            "-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEA...\n-----END RSA PRIVATE KEY-----";
        let sanitized_pem = sanitize_credentials(pem);
        assert_eq!(sanitized_pem, "[REDACTED_PRIVATE_KEY]");
    }

    #[test]
    fn test_internal_path_sanitization() {
        let input = "File error at /home/ctodie/projects/cicatrix/src/main.rs:42 and temp /tmp/worktree_123/db.sqlite";
        let sanitized = sanitize_internal_paths(input);
        assert!(!sanitized.contains("/home/ctodie/"));
        assert!(!sanitized.contains("/tmp/"));
        assert!(sanitized.contains("[HOME]/projects/cicatrix/src/main.rs:42"));
        assert!(sanitized.contains("[TMP]/worktree_123/db.sqlite"));
    }

    #[test]
    fn test_stack_trace_stripping() {
        let trace = "fatal crash occurred!\nstack backtrace:\n   0: rust_begin_unwind\n   1: core::panicking::panic_fmt\n   2: cicatrix::main\nnote: run with `RUST_BACKTRACE=1` environment variable\nresuming execution.";
        let stripped = strip_stack_traces(trace);
        assert!(!stripped.contains("rust_begin_unwind"));
        assert!(!stripped.contains("core::panicking"));
        assert!(stripped.contains("[STACK TRACE STRIPPED]"));
        assert!(stripped.contains("fatal crash occurred!"));
        assert!(stripped.contains("resuming execution."));
    }

    #[test]
    fn test_masked_error_internal() {
        let err = MaskedError::internal(
            "database failure at /home/ctodie/db.sqlite with token=secret123",
        );
        assert_eq!(err.error_type, "internal_error");
        assert!(err.correlation_id.is_some());
        let ref_id = err.correlation_id.as_ref().unwrap();
        assert!(ref_id.starts_with("ref_"));
        assert_eq!(
            err.message,
            format!("internal error (correlation: {ref_id})")
        );

        // External representations guarantee zero leaks
        let cli_msg = err.to_cli_string("cicatrix query");
        assert_eq!(
            cli_msg,
            format!("cicatrix query: internal error (correlation: {ref_id})")
        );
        assert!(!cli_msg.contains("/home/"));
        assert!(!cli_msg.contains("secret123"));

        let (status, http_body, header_id) = err.to_http_response();
        assert_eq!(status, 500);
        assert_eq!(header_id.as_deref(), Some(ref_id.as_str()));
        assert!(!http_body.contains("/home/"));
        assert!(!http_body.contains("secret123"));
        assert!(http_body.contains(ref_id));
    }

    #[test]
    fn test_masked_error_client_sanitized() {
        let err = MaskedError::client("Invalid path /home/ctodie/bad_file.txt with key=secret");
        assert_eq!(err.error_type, "client_error");
        assert!(err.correlation_id.is_none());
        assert!(!err.message.contains("/home/ctodie/"));
        assert!(!err.message.contains("secret"));
        assert!(err.message.contains("[HOME]/bad_file.txt"));
    }
}
