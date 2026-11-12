//! Cryptographic human signer verification for earned autonomy promotions (CER-3527).
//!
//! Enforces that tier-raising promotions (mandatory for `Autonomous`, and for any tier when
//! `--require-verified` is set) carry a valid OpenSSH (`SSHSIG`) cryptographic signature over
//! the canonical promotion payload (`cicatrix.autonomy.promote@v1`), verified via
//! `ssh-keygen -Y verify` against an `allowed_signers` file, and signed by a human principal
//! (rejecting automated, bot, and agent identities aligned with `heimdal::validate_human_signer`).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::autonomy::error::AutonomyError;
use crate::autonomy::tier::AutonomyTier;

/// Domain-separated OpenSSH signature namespace for Cicatrix autonomy promotions.
pub const PROMOTE_SIGNATURE_NAMESPACE: &str = "cicatrix.autonomy.promote@v1";

/// Forbidden automated, agent, and bot signer tokens/prefixes (aligned with `heimdal`).
pub const FORBIDDEN_SIGNER_ACTORS: &[&str] = &[
    "volva",
    "völva",
    "automated",
    "automation",
    "auto",
    "agent",
    "subagent",
    "bot",
    "robot",
    "system",
    "system_init",
    "jev",
    "claude",
    "codex",
    "gemini",
    "antigravity",
    "copilot",
    "cursor",
    "devin",
    "llm",
    "ci",
    "cd",
    "github-actions",
    "noreply",
    "no-reply",
    "harness",
    "wheelhorse",
    "daemon",
    "runner",
];

/// Canonical promotion payload signed under [`PROMOTE_SIGNATURE_NAMESPACE`].
///
/// Struct fields are ordered lexicographically so compact `serde_json` output matches
/// RFC 8785 (JSON Canonicalization Scheme) deterministic UTF-8 bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromotionSignPayload {
    pub actor: String,
    pub at: String,
    pub authorized_by: String,
    pub capability: String,
    pub from: AutonomyTier,
    pub reason: String,
    pub to: AutonomyTier,
}

impl PromotionSignPayload {
    /// Deterministic canonical UTF-8 JSON string signed and verified by `ssh-keygen -Y`.
    pub fn canonical_json(&self) -> Result<String, AutonomyError> {
        serde_json::to_string(self).map_err(|e| {
            AutonomyError::Validation(format!(
                "failed to serialize canonical promotion payload: {e}"
            ))
        })
    }

    /// Deterministic canonical UTF-8 JSON bytes signed and verified by `ssh-keygen -Y`.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, AutonomyError> {
        serde_json::to_vec(self).map_err(|e| {
            AutonomyError::Validation(format!(
                "failed to serialize canonical promotion payload: {e}"
            ))
        })
    }
}

/// Caller-supplied signing or detached-signature parameters on a `PromotionRequest`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromotionProof {
    /// Path to SSH key / `.pub` file (or inline `ssh-...` public key) to sign via `ssh-keygen -Y sign`
    /// (compatible with `ssh-agent` and 1Password SSH agent Touch ID).
    #[serde(default)]
    pub sign_key: Option<String>,
    /// Detached OpenSSH signature (file path or inline `-----BEGIN SSH SIGNATURE-----` PEM).
    #[serde(default)]
    pub signature: Option<String>,
    /// Principal identity in `allowed_signers` (defaults to `PromotionRequest::authorized_by`).
    #[serde(default)]
    pub signer_principal: Option<String>,
    /// Explicit `allowed_signers` file path (overrides `CICATRIX_ALLOWED_SIGNERS` and default paths).
    #[serde(default)]
    pub allowed_signers: Option<PathBuf>,
    /// Explicit ISO-8601 timestamp bound into `PromotionSignPayload::at`.
    #[serde(default)]
    pub signed_at: Option<String>,
}

impl PromotionProof {
    /// True when either `--sign-key` or `--signature` was supplied.
    pub fn has_credentials(&self) -> bool {
        self.sign_key.as_ref().is_some_and(|s| !s.trim().is_empty())
            || self
                .signature
                .as_ref()
                .is_some_and(|s| !s.trim().is_empty())
    }
}

/// Verified cryptographic proof returned by [`verify_promotion_signature`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedPromotionProof {
    /// Authenticated human principal matched in `allowed_signers`.
    pub principal: String,
    /// Optional SSH public key fingerprint (e.g. `SHA256:...`) reported by `ssh-keygen -Y verify`.
    pub fingerprint: Option<String>,
    /// Armored `-----BEGIN SSH SIGNATURE-----` PEM block.
    pub signature_pem: String,
}

/// Validate that a signer principal belongs to a human operator and not a bot, agent, or CI identity.
pub fn validate_human_principal(raw: &str) -> Result<String, AutonomyError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(AutonomyError::NonHumanSigner(
            "signer principal cannot be empty".to_string(),
        ));
    }

    let lower = trimmed.to_lowercase();
    if lower.contains("[bot]") || lower.ends_with("-bot") || lower.ends_with("_bot") {
        return Err(AutonomyError::NonHumanSigner(format!(
            "automated or bot principal `{trimmed}` cannot authorize an autonomy promotion"
        )));
    }

    let local = lower.split('@').next().unwrap_or(&lower);
    if local.is_empty() {
        return Err(AutonomyError::NonHumanSigner(format!(
            "invalid signer principal `{trimmed}`"
        )));
    }

    for forbidden in FORBIDDEN_SIGNER_ACTORS {
        if lower == *forbidden
            || local == *forbidden
            || lower.starts_with(&format!("{forbidden}:"))
            || lower.starts_with(&format!("{forbidden}/"))
            || lower.starts_with(&format!("{forbidden}-"))
            || lower.starts_with(&format!("{forbidden}_"))
            || local.starts_with(&format!("{forbidden}:"))
            || local.starts_with(&format!("{forbidden}/"))
            || local.starts_with(&format!("{forbidden}-"))
            || local.starts_with(&format!("{forbidden}_"))
        {
            return Err(AutonomyError::NonHumanSigner(format!(
                "automated actor `{trimmed}` cannot satisfy a human-only autonomy promotion gate"
            )));
        }
    }

    for token in local.split([':', '/', '-', '_', '.', '+', '[', ']', ' ', '\t']) {
        if !token.is_empty() && FORBIDDEN_SIGNER_ACTORS.contains(&token) {
            return Err(AutonomyError::NonHumanSigner(format!(
                "automated actor `{trimmed}` (matched `{token}`) cannot satisfy a human-only autonomy promotion gate"
            )));
        }
    }

    Ok(trimmed.to_string())
}

fn expand_tilde(raw: &str) -> PathBuf {
    let trimmed = raw.trim();
    if let Some(rest) = trimmed.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(trimmed)
}

fn git_allowed_signers_file() -> Option<PathBuf> {
    let out = Command::new("git")
        .args(["config", "--get", "gpg.ssh.allowedSignersFile"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let val = String::from_utf8_lossy(&out.stdout);
    let trimmed = val.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(expand_tilde(trimmed))
    }
}

/// Resolve the `allowed_signers` file path using the precedence chain:
/// 1. Explicit `--allowed-signers <path>` (fails closed if missing)
/// 2. `CICATRIX_ALLOWED_SIGNERS` environment variable (fails closed if missing)
/// 3. `~/.config/cicatrix/allowed_signers` (if present)
/// 4. `~/.ssh/allowed_signers` (if present)
/// 5. `git config --get gpg.ssh.allowedSignersFile` (if present)
pub fn resolve_allowed_signers_path(explicit: Option<&Path>) -> Result<PathBuf, AutonomyError> {
    if let Some(path) = explicit {
        if path.as_os_str().is_empty() {
            return Err(AutonomyError::SignatureVerification(
                "explicit --allowed-signers path cannot be empty".to_string(),
            ));
        }
        if !path.is_file() {
            return Err(AutonomyError::SignatureVerification(format!(
                "allowed_signers file `{}` does not exist",
                path.display()
            )));
        }
        return Ok(path.to_path_buf());
    }

    if let Ok(env_val) = std::env::var("CICATRIX_ALLOWED_SIGNERS") {
        let trimmed = env_val.trim();
        if !trimmed.is_empty() {
            let p = expand_tilde(trimmed);
            if !p.is_file() {
                return Err(AutonomyError::SignatureVerification(format!(
                    "CICATRIX_ALLOWED_SIGNERS file `{}` does not exist",
                    p.display()
                )));
            }
            return Ok(p);
        }
    }

    if let Ok(home) = std::env::var("HOME") {
        let home_dir = PathBuf::from(&home);
        let cicatrix_default = home_dir.join(".config/cicatrix/allowed_signers");
        if cicatrix_default.is_file() {
            return Ok(cicatrix_default);
        }
        let ssh_default = home_dir.join(".ssh/allowed_signers");
        if ssh_default.is_file() {
            return Ok(ssh_default);
        }
    }

    if let Some(git_cfg) = git_allowed_signers_file() {
        if git_cfg.is_file() {
            return Ok(git_cfg);
        }
    }

    Err(AutonomyError::SignatureVerification(
        "no allowed_signers file found; configure --allowed-signers <path>, CICATRIX_ALLOWED_SIGNERS, ~/.config/cicatrix/allowed_signers, or ~/.ssh/allowed_signers".to_string(),
    ))
}

fn looks_like_inline_ssh_pubkey(s: &str) -> bool {
    let t = s.trim_start();
    t.starts_with("ssh-ed25519 ")
        || t.starts_with("ssh-rsa ")
        || t.starts_with("ecdsa-sha2-")
        || t.starts_with("sk-ssh-ed25519@openssh.com ")
        || t.starts_with("sk-ecdsa-sha2-nistp256@openssh.com ")
}

/// Sign `payload` via `ssh-keygen -Y sign -q -n cicatrix.autonomy.promote@v1 -f <key>`.
///
/// Accepts either a filesystem key/pubkey path or an inline OpenSSH public key string
/// (written to an ephemeral temp `.pub` file so `ssh-agent` / 1Password SSH agent can sign).
pub fn sign_promotion_payload(
    payload: &PromotionSignPayload,
    sign_key: &str,
) -> Result<String, AutonomyError> {
    let trimmed_key = sign_key.trim();
    if trimmed_key.is_empty() {
        return Err(AutonomyError::SignatureVerification(
            "--sign-key cannot be empty".to_string(),
        ));
    }

    let payload_bytes = payload.canonical_bytes()?;
    let tmp_dir = tempfile::tempdir().map_err(|e| {
        AutonomyError::SignatureVerification(format!("failed to create temp dir for signing: {e}"))
    })?;

    let key_path = if looks_like_inline_ssh_pubkey(trimmed_key) {
        let pub_path = tmp_dir.path().join("signer.pub");
        let mut content = trimmed_key.to_string();
        if !content.ends_with('\n') {
            content.push('\n');
        }
        std::fs::write(&pub_path, content).map_err(|e| {
            AutonomyError::SignatureVerification(format!(
                "failed to stage inline signing public key: {e}"
            ))
        })?;
        pub_path
    } else {
        let expanded = expand_tilde(trimmed_key);
        if !expanded.exists() {
            return Err(AutonomyError::SignatureVerification(format!(
                "signing key `{}` does not exist",
                expanded.display()
            )));
        }
        expanded
    };

    let mut child = Command::new("ssh-keygen")
        .args(["-Y", "sign", "-q", "-n", PROMOTE_SIGNATURE_NAMESPACE, "-f"])
        .arg(&key_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            AutonomyError::SignatureVerification(format!(
                "failed to invoke ssh-keygen -Y sign: {e}"
            ))
        })?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(&payload_bytes).map_err(|e| {
            AutonomyError::SignatureVerification(format!(
                "failed to feed payload to ssh-keygen -Y sign: {e}"
            ))
        })?;
    }

    let out = child.wait_with_output().map_err(|e| {
        AutonomyError::SignatureVerification(format!("ssh-keygen -Y sign wait failed: {e}"))
    })?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(AutonomyError::SignatureVerification(format!(
            "ssh-keygen -Y sign failed: {}",
            if stderr.is_empty() {
                format!("exit status {}", out.status)
            } else {
                stderr
            }
        )));
    }

    let pem = String::from_utf8_lossy(&out.stdout).trim().to_string();
    validate_ssh_signature_armor(&pem)?;
    Ok(pem)
}

/// Load a detached SSH signature from either an inline PEM string or a file path.
pub fn load_detached_signature(sig_or_path: &str) -> Result<String, AutonomyError> {
    let trimmed = sig_or_path.trim();
    if trimmed.is_empty() {
        return Err(AutonomyError::SignatureVerification(
            "--signature cannot be empty".to_string(),
        ));
    }
    let pem = if trimmed.starts_with("-----BEGIN SSH SIGNATURE-----") {
        trimmed.to_string()
    } else {
        let path = expand_tilde(trimmed);
        std::fs::read_to_string(&path)
            .map_err(|e| {
                AutonomyError::SignatureVerification(format!(
                    "failed to read signature file `{}`: {e}",
                    path.display()
                ))
            })?
            .trim()
            .to_string()
    };
    validate_ssh_signature_armor(&pem)?;
    Ok(pem)
}

fn validate_ssh_signature_armor(pem: &str) -> Result<(), AutonomyError> {
    let trimmed = pem.trim();
    if !trimmed.starts_with("-----BEGIN SSH SIGNATURE-----")
        || !trimmed.ends_with("-----END SSH SIGNATURE-----")
    {
        return Err(AutonomyError::SignatureVerification(
            "invalid SSH signature armor: expected `-----BEGIN SSH SIGNATURE-----` PEM block"
                .to_string(),
        ));
    }
    Ok(())
}

fn extract_fingerprint(verify_stdout: &str) -> Option<String> {
    verify_stdout
        .split_whitespace()
        .find(|tok| tok.starts_with("SHA256:") || tok.starts_with("MD5:"))
        .map(str::to_string)
}

/// Verify an OpenSSH `SSHSIG` signature over `payload` against `allowed_signers` for `principal`.
pub fn verify_promotion_signature(
    payload: &PromotionSignPayload,
    signature_pem: &str,
    principal: &str,
    allowed_signers: &Path,
) -> Result<VerifiedPromotionProof, AutonomyError> {
    let clean_principal = validate_human_principal(principal)?;
    validate_ssh_signature_armor(signature_pem)?;

    if !allowed_signers.is_file() {
        return Err(AutonomyError::SignatureVerification(format!(
            "allowed_signers file `{}` does not exist",
            allowed_signers.display()
        )));
    }

    let payload_bytes = payload.canonical_bytes()?;
    let tmp_dir = tempfile::tempdir().map_err(|e| {
        AutonomyError::SignatureVerification(format!(
            "failed to create temp dir for signature verification: {e}"
        ))
    })?;
    let sig_path = tmp_dir.path().join("promotion.sig");
    let mut sig_content = signature_pem.trim().to_string();
    sig_content.push('\n');
    std::fs::write(&sig_path, &sig_content).map_err(|e| {
        AutonomyError::SignatureVerification(format!("failed to stage signature file: {e}"))
    })?;

    let mut child = Command::new("ssh-keygen")
        .args([
            "-Y",
            "verify",
            "-f",
            &allowed_signers.to_string_lossy(),
            "-I",
            &clean_principal,
            "-n",
            PROMOTE_SIGNATURE_NAMESPACE,
            "-s",
            &sig_path.to_string_lossy(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            AutonomyError::SignatureVerification(format!(
                "failed to invoke ssh-keygen -Y verify: {e}"
            ))
        })?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(&payload_bytes).map_err(|e| {
            AutonomyError::SignatureVerification(format!(
                "failed to feed payload to ssh-keygen -Y verify: {e}"
            ))
        })?;
    }

    let out = child.wait_with_output().map_err(|e| {
        AutonomyError::SignatureVerification(format!("ssh-keygen -Y verify wait failed: {e}"))
    })?;

    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();

    let expected_marker = format!("Good \"{PROMOTE_SIGNATURE_NAMESPACE}\" signature");
    if !out.status.success() || !stdout.contains(&expected_marker) {
        let detail = if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else {
            format!("exit status {}", out.status)
        };
        return Err(AutonomyError::SignatureVerification(format!(
            "SSH signature verification failed for principal `{clean_principal}` against `{}`: {detail}",
            allowed_signers.display()
        )));
    }

    Ok(VerifiedPromotionProof {
        principal: clean_principal,
        fingerprint: extract_fingerprint(&stdout),
        signature_pem: signature_pem.trim().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn generate_ed25519_key(dir: &Path, name: &str, comment: &str) -> (PathBuf, String) {
        let key_path = dir.join(name);
        let status = Command::new("ssh-keygen")
            .args(["-q", "-t", "ed25519", "-N", "", "-C", comment, "-f"])
            .arg(&key_path)
            .status()
            .expect("ssh-keygen available");
        assert!(status.success(), "ssh-keygen key generation must succeed");
        let pubkey = std::fs::read_to_string(dir.join(format!("{name}.pub")))
            .expect("read generated pubkey");
        (key_path, pubkey)
    }

    fn sample_payload(authorized_by: &str) -> PromotionSignPayload {
        PromotionSignPayload {
            actor: "vilicus-lane".to_string(),
            at: "2026-11-12T17:00:00.000Z".to_string(),
            authorized_by: authorized_by.to_string(),
            capability: "train-delivery".to_string(),
            from: AutonomyTier::Supervised,
            reason: "20 consecutive green shadow/supervised runs".to_string(),
            to: AutonomyTier::Autonomous,
        }
    }

    #[test]
    fn test_validate_human_principal_accepts_humans_and_rejects_bots() {
        assert_eq!(
            validate_human_principal("chris@todie.io").unwrap(),
            "chris@todie.io"
        );
        assert_eq!(validate_human_principal("ctodie").unwrap(), "ctodie");
        assert_eq!(
            validate_human_principal("ahackley@cerebral.work").unwrap(),
            "ahackley@cerebral.work"
        );

        for bot in [
            "",
            "   ",
            "volva",
            "völva",
            "volva:lane-1",
            "claude-bot@cerebral.work",
            "github-actions[bot]",
            "dependabot[bot]",
            "agent",
            "agent-claude",
            "claude",
            "codex",
            "noreply@github.com",
            "no-reply@cerebral.work",
            "ci",
            "ci-runner@cerebral.work",
            "automation",
            "automated",
            "system",
            "system_init",
            "wheelhorse",
            "harness",
        ] {
            assert!(
                matches!(
                    validate_human_principal(bot),
                    Err(AutonomyError::NonHumanSigner(_))
                ),
                "expected `{bot}` to be rejected as non-human"
            );
        }
    }

    #[test]
    fn test_canonical_payload_field_ordering() {
        let p = sample_payload("chris@todie.io");
        let json = p.canonical_json().unwrap();
        assert_eq!(
            json,
            r#"{"actor":"vilicus-lane","at":"2026-11-12T17:00:00.000Z","authorized_by":"chris@todie.io","capability":"train-delivery","from":"supervised","reason":"20 consecutive green shadow/supervised runs","to":"autonomous"}"#
        );
    }

    #[test]
    fn test_sign_and_verify_roundtrip_and_tamper_rejection() {
        let tmp = tempfile::tempdir().unwrap();
        let (chris_key, chris_pub) =
            generate_ed25519_key(tmp.path(), "chris_ed25519", "chris@todie.io");
        let (rogue_key, _) = generate_ed25519_key(tmp.path(), "rogue_ed25519", "rogue@todie.io");

        let allowed_signers = tmp.path().join("allowed_signers");
        std::fs::write(&allowed_signers, format!("chris@todie.io {chris_pub}")).unwrap();

        let payload = sample_payload("chris@todie.io");
        let sig_pem = sign_promotion_payload(&payload, chris_key.to_str().unwrap()).unwrap();

        // 1. Valid signature with matching allowed_signers succeeds and extracts SHA256 fingerprint.
        let verified =
            verify_promotion_signature(&payload, &sig_pem, "chris@todie.io", &allowed_signers)
                .expect("valid human signature must verify");
        assert_eq!(verified.principal, "chris@todie.io");
        assert!(
            verified
                .fingerprint
                .as_deref()
                .is_some_and(|fp| fp.starts_with("SHA256:")),
            "expected SHA256 fingerprint, got {:?}",
            verified.fingerprint
        );

        // 2. Tampered payload is rejected.
        let mut tampered = payload.clone();
        tampered.actor = "other-lane".to_string();
        assert!(matches!(
            verify_promotion_signature(&tampered, &sig_pem, "chris@todie.io", &allowed_signers),
            Err(AutonomyError::SignatureVerification(_))
        ));

        // 3. Signature from an untrusted key (not in allowed_signers) is rejected.
        let rogue_sig = sign_promotion_payload(&payload, rogue_key.to_str().unwrap()).unwrap();
        assert!(matches!(
            verify_promotion_signature(&payload, &rogue_sig, "chris@todie.io", &allowed_signers),
            Err(AutonomyError::SignatureVerification(_))
        ));

        // 4. Bot principal is rejected even if its key were placed in allowed_signers.
        let (bot_key, bot_pub) =
            generate_ed25519_key(tmp.path(), "bot_ed25519", "claude-bot@cerebral.work");
        let bot_allowed = tmp.path().join("bot_allowed_signers");
        std::fs::write(&bot_allowed, format!("claude-bot@cerebral.work {bot_pub}")).unwrap();
        let bot_payload = sample_payload("claude-bot@cerebral.work");
        let bot_sig = sign_promotion_payload(&bot_payload, bot_key.to_str().unwrap()).unwrap();
        assert!(matches!(
            verify_promotion_signature(
                &bot_payload,
                &bot_sig,
                "claude-bot@cerebral.work",
                &bot_allowed
            ),
            Err(AutonomyError::NonHumanSigner(_))
        ));
    }
}
