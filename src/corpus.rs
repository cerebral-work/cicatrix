#![allow(dead_code)]
//! Corpus tier abstraction — single source of truth for tier→directory mapping.
//!
//! Tiers:
//! - `grounded`: canonical bug-doc corpus (renamed from `resolved`; legacy alias preserved)
//! - `observed`: new tier for observed-but-unresolved bugs
//!
//! Directory resolution order per tier:
//! Grounded:
//! 1. Tier-specific env var `CICATRIX_CORPUS_GROUNDED`
//! 2. Legacy tier-specific env var `CICATRIX_CORPUS_RESOLVED`
//! 3. Legacy generic env var `CICATRIX_CORPUS`
//! 4. Default path `docs/bugs/grounded`
//!
//! Observed:
//! 1. Tier-specific env var `CICATRIX_CORPUS_OBSERVED`
//! 2. Default path `docs/bugs/observed`

use crate::bug_md;
use crate::store::BugFact;
use std::io;
use std::path::PathBuf;

/// Corpus tier — distinguishes grounded (resolved) bugs from observed (unresolved) ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// Canonical bug-doc corpus. Legacy name: `resolved`.
    Grounded,
    /// Observed-but-unresolved bugs (new tier).
    Observed,
}

impl Tier {
    /// Parse a tier name, accepting both canonical and legacy names.
    /// - `grounded` or `resolved` → `Tier::Grounded`
    /// - `observed` → `Tier::Observed`
    pub fn from_name(name: &str) -> Result<Self, String> {
        match name.to_lowercase().as_str() {
            "grounded" | "resolved" => Ok(Tier::Grounded),
            "observed" => Ok(Tier::Observed),
            _ => Err(format!("unknown corpus tier: {name}")),
        }
    }

    /// Default directory path for this tier (relative to repo root).
    pub fn default_dir(&self) -> &'static str {
        match self {
            Tier::Grounded => "docs/bugs/grounded",
            Tier::Observed => "docs/bugs/observed",
        }
    }

    /// Tier-specific environment variable name.
    pub fn env_var(&self) -> &'static str {
        match self {
            Tier::Grounded => "CICATRIX_CORPUS_GROUNDED",
            Tier::Observed => "CICATRIX_CORPUS_OBSERVED",
        }
    }
}

/// Resolve the directory path for a tier. Checks (in order):
/// 1. Tier-specific env var (`CICATRIX_CORPUS_GROUNDED` or `CICATRIX_CORPUS_OBSERVED`)
/// 2. Legacy tier-specific env var (`CICATRIX_CORPUS_RESOLVED` for grounded)
/// 3. Legacy `CICATRIX_CORPUS` (grounded only)
/// 4. Default path
pub fn resolve_dir(tier: Tier) -> PathBuf {
    match tier {
        Tier::Grounded => {
            if let Ok(dir) = std::env::var("CICATRIX_CORPUS_GROUNDED") {
                return dir.into();
            }
            if let Ok(dir) = std::env::var("CICATRIX_CORPUS_RESOLVED") {
                return dir.into();
            }
            if let Ok(dir) = std::env::var("CICATRIX_CORPUS") {
                return dir.into();
            }
        }
        Tier::Observed => {
            if let Ok(dir) = std::env::var("CICATRIX_CORPUS_OBSERVED") {
                return dir.into();
            }
        }
    }
    tier.default_dir().into()
}

/// Create the directory for a tier if it doesn't exist. Returns the resolved path.
pub fn create_dir(tier: Tier) -> io::Result<PathBuf> {
    let dir = resolve_dir(tier);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Read and parse all bug-facts from a tier's directory.
pub fn read_facts(tier: Tier) -> Result<Vec<BugFact>, String> {
    bug_md::parse_dir(&resolve_dir(tier))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Mutex to serialize environment-variable mutating tests.
    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    fn clear_env() {
        std::env::remove_var("CICATRIX_CORPUS");
        std::env::remove_var("CICATRIX_CORPUS_GROUNDED");
        std::env::remove_var("CICATRIX_CORPUS_RESOLVED");
        std::env::remove_var("CICATRIX_CORPUS_OBSERVED");
    }

    #[test]
    fn tier_from_name_accepts_canonical_and_legacy() {
        assert_eq!(Tier::from_name("grounded").unwrap(), Tier::Grounded);
        assert_eq!(Tier::from_name("resolved").unwrap(), Tier::Grounded); // legacy alias
        assert_eq!(Tier::from_name("observed").unwrap(), Tier::Observed);
        // Case-insensitive
        assert_eq!(Tier::from_name("GROUNDED").unwrap(), Tier::Grounded);
        assert_eq!(Tier::from_name("Resolved").unwrap(), Tier::Grounded);
    }

    #[test]
    fn tier_from_name_rejects_unknown() {
        assert!(Tier::from_name("unknown").is_err());
        assert!(Tier::from_name("").is_err());
    }

    #[test]
    fn tier_default_dir_paths() {
        assert_eq!(Tier::Grounded.default_dir(), "docs/bugs/grounded");
        assert_eq!(Tier::Observed.default_dir(), "docs/bugs/observed");
    }

    #[test]
    fn tier_env_var_names() {
        assert_eq!(Tier::Grounded.env_var(), "CICATRIX_CORPUS_GROUNDED");
        assert_eq!(Tier::Observed.env_var(), "CICATRIX_CORPUS_OBSERVED");
    }

    #[test]
    fn resolve_dir_uses_default_when_no_env_set() {
        let _lock = ENV_MUTEX.lock().unwrap();
        clear_env();

        assert_eq!(
            resolve_dir(Tier::Grounded),
            PathBuf::from("docs/bugs/grounded")
        );
        assert_eq!(
            resolve_dir(Tier::Observed),
            PathBuf::from("docs/bugs/observed")
        );
    }

    #[test]
    fn resolve_dir_prefers_tier_specific_env_over_legacy() {
        let _lock = ENV_MUTEX.lock().unwrap();
        clear_env();

        std::env::set_var("CICATRIX_CORPUS_GROUNDED", "/custom/grounded");
        std::env::set_var("CICATRIX_CORPUS_RESOLVED", "/legacy/resolved_env");
        std::env::set_var("CICATRIX_CORPUS", "/legacy/path");

        assert_eq!(
            resolve_dir(Tier::Grounded),
            PathBuf::from("/custom/grounded")
        );

        clear_env();
    }

    #[test]
    fn resolve_dir_prefers_resolved_env_over_legacy_generic() {
        let _lock = ENV_MUTEX.lock().unwrap();
        clear_env();

        std::env::set_var("CICATRIX_CORPUS_RESOLVED", "/legacy/resolved_env");
        std::env::set_var("CICATRIX_CORPUS", "/legacy/path");

        assert_eq!(
            resolve_dir(Tier::Grounded),
            PathBuf::from("/legacy/resolved_env")
        );

        clear_env();
    }

    #[test]
    fn resolve_dir_legacy_aliases_to_grounded() {
        let _lock = ENV_MUTEX.lock().unwrap();
        clear_env();

        std::env::set_var("CICATRIX_CORPUS", "/legacy/corpus");

        assert_eq!(resolve_dir(Tier::Grounded), PathBuf::from("/legacy/corpus"));
        // Observed tier ignores legacy CICATRIX_CORPUS
        assert_eq!(
            resolve_dir(Tier::Observed),
            PathBuf::from("docs/bugs/observed")
        );

        clear_env();
    }

    #[test]
    fn resolve_dir_observed_uses_own_env_only() {
        let _lock = ENV_MUTEX.lock().unwrap();
        clear_env();

        std::env::set_var("CICATRIX_CORPUS_OBSERVED", "/custom/observed");
        std::env::set_var("CICATRIX_CORPUS", "/legacy/path");
        std::env::set_var("CICATRIX_CORPUS_RESOLVED", "/legacy/resolved_env");

        assert_eq!(
            resolve_dir(Tier::Observed),
            PathBuf::from("/custom/observed")
        );

        clear_env();
    }
}
