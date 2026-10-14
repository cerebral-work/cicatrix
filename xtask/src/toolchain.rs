//! `cargo xtask toolchain-check` (spec 0003, XT-5).
//!
//! Fails unless the active `rustc` equals the `channel` in `rust-toolchain.toml`, and a
//! `rust-version` in `Cargo.toml`, when there is one, is not above that channel. The comparison
//! is ported from `cerebral-work/rust-monorepo-template:xtask/src/toolchain.rs` (`verify`,
//! `parse_version`, `toml_string`), with `String` errors because this xtask uses std only.

use std::fs;
use std::path::Path;
use std::process::Command;

/// Read the pins, ask `rustc --version`, compare them, and print the result.
pub fn check(root: &Path) -> Result<(), String> {
    let toolchain = fs::read_to_string(root.join("rust-toolchain.toml"))
        .map_err(|e| format!("cannot read rust-toolchain.toml: {e}"))?;
    let manifest = fs::read_to_string(root.join("Cargo.toml"))
        .map_err(|e| format!("cannot read Cargo.toml: {e}"))?;
    let channel = toml_string(&toolchain, Some("toolchain"), "channel")
        .ok_or_else(|| "no `channel` in rust-toolchain.toml".to_string())?;
    let rust_version = toml_string(&manifest, Some("workspace.package"), "rust-version")
        .or_else(|| toml_string(&manifest, Some("package"), "rust-version"));
    let out = Command::new("rustc")
        .arg("--version")
        .current_dir(root)
        .output()
        .map_err(|e| format!("cannot run `rustc --version`: {e}"))?;
    if !out.status.success() {
        return Err("`rustc --version` failed".to_string());
    }
    let rustc = String::from_utf8_lossy(&out.stdout).trim().to_string();
    verify(&channel, &rustc, rust_version.as_deref())?;
    println!(
        "toolchain-check: ok ({rustc}; rust-version {})",
        rust_version.as_deref().unwrap_or("not set")
    );
    Ok(())
}

/// Compare the pinned channel, the `rustc --version` line and the MSRV, if any (XT-5).
pub fn verify(channel: &str, rustc_line: &str, rust_version: Option<&str>) -> Result<(), String> {
    let _ = (channel, rustc_line, rust_version);
    Err(String::new())
}

/// Parse `1.85` or `1.94.1` into a comparable triple. A missing patch is 0.
pub fn parse_version(text: &str) -> Result<(u64, u64, u64), String> {
    let _ = text;
    Err(String::new())
}

/// Read `key = "value"` from the given `[section]` (or the whole file if `None`).
/// A minimal line scanner: enough for `rust-toolchain.toml` and `Cargo.toml`.
pub fn toml_string(text: &str, section: Option<&str>, key: &str) -> Option<String> {
    let _ = (text, section, key);
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xt5_matching_version_passes() {
        assert_eq!(
            verify("1.94.1", "rustc 1.94.1 (e408947bf 2026-03-25)", None),
            Ok(())
        );
        assert_eq!(
            verify(
                "1.94.1",
                "rustc 1.94.1 (e408947bf 2026-03-25)",
                Some("1.85")
            ),
            Ok(())
        );
    }

    #[test]
    fn xt5_different_rustc_fails_and_names_both() {
        let err = verify("1.94.1", "rustc 1.99.0 (b940084d7 2026-09-28)", None)
            .err()
            .unwrap_or_default();
        assert!(err.contains("1.94.1"), "{err}");
        assert!(err.contains("1.99.0"), "{err}");
        assert!(verify("1.94.1", "rustc 1.94.0 (abc 2026-03-01)", None).is_err());
    }

    #[test]
    fn xt5_msrv_above_channel_fails() {
        assert!(verify("1.94.1", "rustc 1.94.1 (abc 2026-03-25)", Some("1.95")).is_err());
        assert!(verify("1.94.1", "rustc 1.94.1 (abc 2026-03-25)", Some("1.94.2")).is_err());
    }

    #[test]
    fn xt5_bad_input_fails() {
        assert!(verify("1.94.1", "", None).is_err());
        assert!(verify("1.94.1", "rustc 1.94.1 (x)", Some("one.two")).is_err());
        assert!(verify("1.94.1", "rustc 1.94.1 (x)", Some("1.2.3.4")).is_err());
    }

    #[test]
    fn xt5_version_order() {
        assert!(parse_version("1.9") < parse_version("1.10"));
        assert_eq!(parse_version("1.85"), Ok((1, 85, 0)));
        assert_eq!(parse_version("1.94.1"), Ok((1, 94, 1)));
    }

    #[test]
    fn xt5_reads_sections() {
        let toolchain = "[toolchain]\nchannel = \"1.94.1\" # pin\ncomponents = []\n";
        assert_eq!(
            toml_string(toolchain, Some("toolchain"), "channel").as_deref(),
            Some("1.94.1")
        );
        let manifest =
            "[package]\nname = \"cicatrix\"\nrust-version = \"1.85\"\n[workspace]\nmembers = []\n";
        assert_eq!(
            toml_string(manifest, Some("package"), "rust-version").as_deref(),
            Some("1.85")
        );
        assert_eq!(
            toml_string(manifest, Some("workspace.package"), "rust-version"),
            None
        );
    }

    #[test]
    fn xt5_check_fails_on_a_wrong_pin() {
        let dir = std::env::temp_dir().join(format!("cicatrix-xtask-tc-{}", std::process::id()));
        let written = fs::create_dir_all(&dir)
            .and_then(|()| {
                fs::write(
                    dir.join("rust-toolchain.toml"),
                    "[toolchain]\nchannel = \"0.0.1\"\n",
                )
            })
            .and_then(|()| fs::write(dir.join("Cargo.toml"), "[package]\nname = \"x\"\n"));
        assert!(written.is_ok(), "{written:?}");
        // The real rustc is never 0.0.1, so this must fail.
        let result = check(&dir);
        let _ = fs::remove_dir_all(&dir);
        assert!(result.is_err());
    }

    #[test]
    fn xt10_the_repository_pins_rust_1_94_1_with_rustfmt_and_clippy() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let toolchain = fs::read_to_string(root.join("rust-toolchain.toml")).unwrap_or_default();
        assert_eq!(
            toml_string(&toolchain, Some("toolchain"), "channel").as_deref(),
            Some("1.94.1")
        );
        let components = toolchain
            .lines()
            .find(|line| line.trim_start().starts_with("components"))
            .unwrap_or_default();
        assert!(components.contains("\"rustfmt\""), "{components}");
        assert!(components.contains("\"clippy\""), "{components}");
    }
}
