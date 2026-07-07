//! Convention-drift scanner — the static-analysis arm.
//!
//! Walks a configured set of repos and, for each, computes a row of presence marks against the
//! house conventions (CLAUDE.md, a CI-target Makefile/justfile, pre-commit, CI workflows, a
//! license, a toolchain pin, signed-release config, a changelog). Pure data model + filesystem
//! heuristics; no clock except `resolve_now`, no network. See CLAUDE.md for the discipline.
//!
//! Invariants this module enforces (so the renderer and CLI can stay dumb):
//! - A per-FILE io error degrades exactly that one marker to `Missing` — never skips the repo.
//! - Only an absent / non-directory repo root yields `Err(RepoSkip)`.
//! - `0` workflows renders the digit `0`, and is NEVER the missing glyph.
//! - rows AND skipped are byte-wise sorted by name exactly once (in `scan`).

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Presence of a convention marker. `glyph` is the rendered cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Present,
    Partial,
    Missing,
}

impl Mark {
    /// Rendered cell. Exhaustive on purpose — adding a variant must force a glyph choice.
    pub fn glyph(self) -> &'static str {
        match self {
            Mark::Present => "\u{2713}", // ✓
            Mark::Partial => "~",
            Mark::Missing => "\u{2717}", // ✗
        }
    }

    /// Strength ordinal for strongest-mark folds (LIC/TOOL across multiple candidate files).
    fn rank(self) -> u8 {
        match self {
            Mark::Present => 2,
            Mark::Partial => 1,
            Mark::Missing => 0,
        }
    }

    /// The stronger of two marks (Present > Partial > Missing).
    fn max(self, other: Mark) -> Mark {
        if self.rank() >= other.rank() {
            self
        } else {
            other
        }
    }
}

/// Inferred (or configured) primary language. Display strings are load-bearing — they appear
/// verbatim in the rendered table and as the config `lang` label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Rust,
    Node,
    Python,
    Terraform,
    Web,
    Unknown,
}

impl std::fmt::Display for Lang {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Lang::Rust => "rust",
            Lang::Node => "node",
            Lang::Python => "python",
            Lang::Terraform => "tf/helm",
            Lang::Web => "web",
            Lang::Unknown => "?",
        };
        f.write_str(s)
    }
}

impl Lang {
    /// Every variant — the one place the variant set is enumerated (used by `from_label` and the
    /// round-trip test). `Display` is the single source of the string spellings.
    const ALL: [Lang; 6] = [
        Lang::Rust,
        Lang::Node,
        Lang::Python,
        Lang::Terraform,
        Lang::Web,
        Lang::Unknown,
    ];

    /// Parse a config `lang` label back into a `Lang` by matching against the `Display` strings —
    /// so there is ONE source of truth for the label↔variant mapping (the `Display` impl), not a
    /// second hand-kept table that could drift from it (cicatrix meta-pattern #2). An unrecognized
    /// label falls back to `Unknown`, so a typo in markers.json degrades to inference-style
    /// behavior rather than crashing the whole scan.
    fn from_label(label: &str) -> Lang {
        Lang::ALL
            .into_iter()
            .find(|l| l.to_string() == label)
            .unwrap_or(Lang::Unknown)
    }
}

/// Count of CI workflow files. Rendered as a decimal; `0` is the digit, not a missing glyph.
pub type CiCount = u32;

/// A civil date `yyyy-mm-dd`.
pub type Date = String;

/// One scanned repo's marker row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoRow {
    pub name: String,
    pub lang: Lang,
    pub claude: Mark,
    pub mk: Mark,
    pub pc: Mark,
    pub ci: CiCount,
    pub lic: Mark,
    pub tool: Mark,
    pub sr: Mark,
    pub chg: Mark,
}

/// A repo that could not be scanned (absent / not a directory).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoSkip {
    pub name: String,
    pub reason: String,
}

/// The full scan result: a generated date, the root the scan was anchored at, the sorted rows,
/// and the sorted skips.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkerTable {
    pub generated: Date,
    pub root: PathBuf,
    pub rows: Vec<RepoRow>,
    pub skipped: Vec<RepoSkip>,
}

// === filesystem primitives =================================================================

/// Classify a single repo-root file by byte length: present(nonempty) / 0-byte / absent.
/// A read error (not "absent") collapses to `Missing` — the per-file io-error policy.
fn mark_file(path: &Path) -> Mark {
    match std::fs::metadata(path) {
        Ok(m) if m.is_file() && m.len() > 0 => Mark::Present,
        Ok(m) if m.is_file() => Mark::Partial, // 0-byte
        Ok(_) => Mark::Missing,                // a dir where a file was expected
        Err(_) => Mark::Missing,               // absent or unreadable → that one marker only
    }
}

/// Read a file to a string, returning `None` on any io error (treated as "not present" by callers).
fn read_text(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

// === per-marker heuristics =================================================================

/// MK: Present iff a Makefile has a top-level `ci:` target OR a justfile has a `ci` recipe;
/// Partial if either file is present but neither declares a ci target; Missing if neither file.
fn scan_mk(root: &Path) -> Mark {
    let makefile = root.join("Makefile");
    let justfile = root.join("justfile");
    let has_makefile = makefile.is_file();
    let has_justfile = justfile.is_file();
    if !has_makefile && !has_justfile {
        return Mark::Missing;
    }
    let makefile_ci = read_text(&makefile)
        .map(|t| t.lines().any(|l| l.starts_with("ci:")))
        .unwrap_or(false);
    let justfile_ci = read_text(&justfile)
        .map(|t| t.lines().any(line_is_justfile_ci))
        .unwrap_or(false);
    if makefile_ci || justfile_ci {
        Mark::Present
    } else {
        Mark::Partial
    }
}

/// Does `line` declare a justfile recipe named EXACTLY `ci`? Recipe headers sit at column 0 (recipe
/// bodies are indented) — so we deliberately do NOT trim leading whitespace, else an indented body
/// line like `    ci-runner build` would false-match and report a `ci` recipe that doesn't exist.
/// The name `ci` must be followed by `:` or its parameters (space/tab); a following `-`/`_`/alnum
/// (`ci-check`, `cicd`) is a DIFFERENT recipe — in a justfile `-` is a name character. This mirrors
/// the Makefile side's strict `starts_with("ci:")`, so the two file types share ONE definition of
/// "has a ci target" rather than drifting apart (cicatrix meta-pattern #2).
fn line_is_justfile_ci(line: &str) -> bool {
    match line.strip_prefix("ci") {
        Some(rest) => matches!(rest.chars().next(), Some(':' | ' ' | '\t')),
        None => false,
    }
}

/// The strongest mark across a set of candidate filenames in `root` (∨ over `Mark::max`): any
/// non-empty candidate → Present, else any present-but-empty → Partial, else Missing. Shared by the
/// multi-name marker columns (LIC, TOOL) so their fold semantics cannot drift apart.
fn strongest_mark(root: &Path, names: &[&str]) -> Mark {
    names
        .iter()
        .map(|name| mark_file(&root.join(name)))
        .fold(Mark::Missing, Mark::max)
}

/// LIC: strongest mark across the candidate license filenames.
fn scan_lic(root: &Path) -> Mark {
    strongest_mark(root, &["LICENSE", "LICENSE.md", "LICENSE.txt", "COPYING"])
}

/// TOOL: strongest mark across the toolchain-pin candidate files.
fn scan_tool(root: &Path) -> Mark {
    strongest_mark(
        root,
        &[
            "rust-toolchain.toml",
            "rust-toolchain",
            ".nvmrc",
            ".python-version",
            ".tool-versions",
            ".ruby-version",
        ],
    )
}

/// Is `path` a YAML workflow file (`.yml`/`.yaml`)? The single definition of "counts as a workflow
/// file", shared by the CI count and the SR scan so they cannot disagree on the extension set.
fn is_yaml(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("yml") | Some("yaml")
    )
}

/// Top-level `.yml`/`.yaml` files in `<root>/.github/workflows/` (nested dirs and non-yaml files
/// excluded; absent/unreadable dir → empty). Enumerated ONCE; both `scan_ci_count` and `scan_sr`
/// consume the result, so the workflows dir is read once per repo, not twice.
fn yaml_workflows(root: &Path) -> Vec<PathBuf> {
    let dir = root.join(".github").join("workflows");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && is_yaml(p))
        .collect()
}

/// Count top-level `.yml`/`.yaml` files in `.github/workflows/`; absent/empty dir → 0. Nested
/// files (subdirectories) and non-yaml files are excluded.
fn scan_ci_count(root: &Path) -> CiCount {
    yaml_workflows(root).len() as CiCount
}

/// SR: signed-release config. Present if any of the strong signals exist; else Partial if a
/// release-named workflow exists; else Missing.
fn scan_sr(root: &Path) -> Mark {
    // strong: dedicated config files
    for name in [
        "release-please-config.json",
        ".release-please-manifest.json",
        "dist-workspace.toml",
    ] {
        if root.join(name).is_file() {
            return Mark::Present;
        }
    }
    // strong: Cargo.toml carries the cargo-dist workspace metadata
    if let Some(cargo) = read_text(&root.join("Cargo.toml")) {
        if cargo.contains("[workspace.metadata.dist]") {
            return Mark::Present;
        }
    }
    // workflow scan: strong substring → Present; release-named filename → Partial.
    let wf_dir = root.join(".github").join("workflows");
    let mut release_named = false;
    if let Ok(entries) = std::fs::read_dir(&wf_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let is_yaml = matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("yml") | Some("yaml")
            );
            if !is_yaml {
                continue;
            }
            if let Some(fname) = path.file_name().and_then(|s| s.to_str()) {
                if fname.to_ascii_lowercase().contains("release") {
                    release_named = true;
                }
            }
            if let Some(text) = read_text(&path) {
                let lower = text.to_ascii_lowercase();
                if lower.contains("release-please") || lower.contains("cargo-dist") {
                    return Mark::Present;
                }
            }
        }
    }
    if release_named {
        Mark::Partial
    } else {
        Mark::Missing
    }
}

// === language inference ====================================================================

/// Fixed-order first-match language inference (used when config supplies no explicit label).
/// Cargo.toml wins over package.json; index.html + package.json resolves to Node (manifest wins).
pub fn infer_lang(root: &Path) -> Lang {
    if root.join("Cargo.toml").is_file() {
        return Lang::Rust;
    }
    if root.join("package.json").is_file() {
        return Lang::Node;
    }
    if root.join("pyproject.toml").is_file()
        || root.join("setup.py").is_file()
        || root.join("requirements.txt").is_file()
    {
        return Lang::Python;
    }
    if has_tf_file(root) || root.join("terraform").is_dir() || root.join("Chart.yaml").is_file() {
        return Lang::Terraform;
    }
    if root.join("index.html").is_file() || root.join("public").is_dir() {
        return Lang::Web;
    }
    Lang::Unknown
}

/// Any top-level `*.tf` file present?
fn has_tf_file(root: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(root) else {
        return false;
    };
    entries.flatten().any(|e| {
        let p = e.path();
        p.is_file() && p.extension().and_then(|x| x.to_str()) == Some("tf")
    })
}

// === repo scan =============================================================================

/// Scan a single repo root. Returns a fully-populated row, or a skip if the root is absent or is
/// not a directory. The returned `name` is the path's basename; callers (scan_config) may override
/// it with the authoritative config name.
pub fn scan_repo(root: &Path) -> Result<RepoRow, RepoSkip> {
    let name = root
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    if !root.is_dir() {
        // Reason is deliberately PATH-FREE: the row's `name` already identifies the repo, and the
        // scanned path has been tilde-EXPANDED to an absolute `$HOME/...` — embedding it would leak
        // the operator's home directory into the committed, shared `convention-drift-<date>.md`
        // (machine-specific noise + a privacy leak). The portable identifier is the config name.
        return Err(RepoSkip {
            name,
            reason: "absent or not a directory".to_string(),
        });
    }
    Ok(RepoRow {
        name,
        lang: infer_lang(root),
        claude: mark_file(&root.join("CLAUDE.md")),
        mk: scan_mk(root),
        pc: mark_file(&root.join(".pre-commit-config.yaml")),
        ci: scan_ci_count(root),
        lic: scan_lic(root),
        tool: scan_tool(root),
        sr: scan_sr(root),
        chg: mark_file(&root.join("CHANGELOG.md")),
    })
}

// === date ==================================================================================

/// Resolve the "now" date. `CICATRIX_DRIFT_NOW`, if set, is returned VERBATIM (for tests and
/// reproducible regeneration). Otherwise compute the UTC civil date from the system clock with
/// pure std arithmetic — no chrono, no time crate.
pub fn resolve_now() -> Date {
    if let Ok(forced) = std::env::var("CICATRIX_DRIFT_NOW") {
        return forced;
    }
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    civil_from_unix_secs(secs)
}

/// Convert seconds-since-epoch (UTC) to a `yyyy-mm-dd` civil date. Uses Howard Hinnant's
/// `civil_from_days` algorithm (public-domain), which handles leap years exactly.
fn civil_from_unix_secs(secs: u64) -> Date {
    let days = (secs / 86_400) as i64; // days since 1970-01-01
    civil_from_days(days)
}

/// days since 1970-01-01 → `yyyy-mm-dd`. From Hinnant, "chrono-Compatible Low-Level Date
/// Algorithms" — valid across the proleptic Gregorian calendar.
fn civil_from_days(z: i64) -> Date {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

// === config (P2) ===========================================================================

/// One configured repo entry. `lang` is an optional explicit label (one of the `Lang::Display`
/// strings); absent → inference. `path` is where to scan; `name` is the authoritative display
/// name (used for both rows and skips, so it matches markers.json exactly).
#[derive(Debug, Clone, Deserialize)]
pub struct RepoEntry {
    pub name: String,
    pub path: String,
    #[serde(default)]
    pub lang: Option<String>,
}

/// The full markers.json config: a root the table is anchored at, and the repo list. `root` is a
/// display string (e.g. `~/projects`) — it is NOT tilde-expanded; per-repo `path` is what's scanned.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub root: String,
    pub repos: Vec<RepoEntry>,
}

impl Config {
    /// Parse a Config from JSON text. A malformed/ill-shaped document is a hard error (the seam):
    /// the loader returns `Err`, never panics.
    pub fn from_json(text: &str) -> Result<Config, String> {
        serde_json::from_str(text).map_err(|e| e.to_string())
    }

    /// Load + parse markers.json from a path. Missing/unreadable file → hard error.
    pub fn load(path: &Path) -> Result<Config, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Config::from_json(&text)
    }
}

// === assembly ==============================================================================

/// Build a `MarkerTable` from `(name, path, explicit-lang)` entries. Each entry is scanned; Ok
/// rows and Err skips are partitioned, the authoritative config `name` overrides the basename on
/// BOTH sides, an explicit lang label overrides inference, then rows AND skipped are byte-wise
/// sorted by name exactly once. This is the single production assembly path (scan_config delegates).
pub fn scan(
    generated: Date,
    root: PathBuf,
    entries: &[(String, PathBuf, Option<Lang>)],
) -> MarkerTable {
    let mut rows = Vec::new();
    let mut skipped = Vec::new();
    for (name, path, explicit_lang) in entries {
        match scan_repo(path) {
            Ok(mut row) => {
                row.name = name.clone();
                if let Some(lang) = explicit_lang {
                    row.lang = *lang;
                }
                rows.push(row);
            }
            Err(mut skip) => {
                skip.name = name.clone();
                skipped.push(skip);
            }
        }
    }
    rows.sort_by(|a, b| a.name.cmp(&b.name));
    skipped.sort_by(|a, b| a.name.cmp(&b.name));
    MarkerTable {
        generated,
        root,
        rows,
        skipped,
    }
}

/// Expand a leading `~` / `~/` in a scanned repo path to `$HOME` for filesystem access; any other
/// path passes through unchanged. This is applied ONLY to the per-repo `path` (what we `stat`), not
/// to the display `root` — so the header keeps the portable `~/repos` string while the scan reaches
/// the real directory. If `$HOME` is unset, the `~` is left intact: the path then fails to resolve
/// and is reported in `skipped` (never silently dropped). Rust's `Path` does not expand `~` itself,
/// so this seam is what lets a `~/...` config entry resolve to a real directory instead of skipping.
pub fn expand_home(path: &str) -> PathBuf {
    let home = || std::env::var("HOME").ok();
    if path == "~" {
        if let Some(h) = home() {
            return PathBuf::from(h);
        }
    } else if let Some(rest) = path.strip_prefix("~/") {
        if let Some(h) = home() {
            return PathBuf::from(h).join(rest);
        }
    }
    PathBuf::from(path)
}

/// Orchestrate a full scan from a parsed `Config`. Enumerates configured repos in order, scans
/// each at its (tilde-expanded) `path`, carries the explicit lang label (or falls back to inference
/// inside `scan`), roots the table at the config `root`, dates it with `resolve_now()`, and returns
/// the sorted table. A configured-but-absent/unreadable repo lands in `skipped` (scan continues,
/// exit SUCCESS) — never silently dropped.
pub fn scan_config(config: &Config) -> MarkerTable {
    let entries: Vec<(String, PathBuf, Option<Lang>)> = config
        .repos
        .iter()
        .map(|r| {
            let lang = r.lang.as_deref().map(Lang::from_label);
            (r.name.clone(), expand_home(&r.path), lang)
        })
        .collect();
    scan(resolve_now(), PathBuf::from(&config.root), &entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Mutex;

    // Serialize env-mutating tests (resolve_now branches). Mirrors src/corpus.rs ~106-114.
    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    fn clear_env() {
        std::env::remove_var("CICATRIX_DRIFT_NOW");
    }

    /// Fresh empty temp dir, unique per test name + pid. Caller fills it with marker files.
    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cicatrix_drift_{tag}_{}", std::process::id()));
        fs::remove_dir_all(&d).ok();
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn write(dir: &Path, name: &str, body: &str) {
        let p = dir.join(name);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(p, body).unwrap();
    }

    fn touch(dir: &Path, name: &str) {
        write(dir, name, "");
    }

    // --- Mark glyphs ----------------------------------------------------------------------

    #[test]
    fn mark_glyphs_are_exact() {
        assert_eq!(Mark::Present.glyph(), "\u{2713}"); // ✓
        assert_eq!(Mark::Partial.glyph(), "~");
        assert_eq!(Mark::Missing.glyph(), "\u{2717}"); // ✗
    }

    #[test]
    fn lang_display_strings_are_exact() {
        assert_eq!(Lang::Rust.to_string(), "rust");
        assert_eq!(Lang::Node.to_string(), "node");
        assert_eq!(Lang::Python.to_string(), "python");
        assert_eq!(Lang::Terraform.to_string(), "tf/helm");
        assert_eq!(Lang::Web.to_string(), "web");
        assert_eq!(Lang::Unknown.to_string(), "?");
    }

    // --- CLAUDE / PC / CHG (the simple byte-length-tier markers) --------------------------

    #[test]
    fn claude_pc_chg_mark_boundaries() {
        // present
        let d = tmp("simple_present");
        write(&d, "CLAUDE.md", "x");
        write(&d, ".pre-commit-config.yaml", "repos: []");
        write(&d, "CHANGELOG.md", "# changes");
        let r = scan_repo(&d).unwrap();
        assert_eq!(r.claude, Mark::Present);
        assert_eq!(r.pc, Mark::Present);
        assert_eq!(r.chg, Mark::Present);

        // 0-byte → Partial
        let d = tmp("simple_partial");
        touch(&d, "CLAUDE.md");
        touch(&d, ".pre-commit-config.yaml");
        touch(&d, "CHANGELOG.md");
        let r = scan_repo(&d).unwrap();
        assert_eq!(r.claude, Mark::Partial);
        assert_eq!(r.pc, Mark::Partial);
        assert_eq!(r.chg, Mark::Partial);

        // absent → Missing
        let d = tmp("simple_missing");
        let r = scan_repo(&d).unwrap();
        assert_eq!(r.claude, Mark::Missing);
        assert_eq!(r.pc, Mark::Missing);
        assert_eq!(r.chg, Mark::Missing);
    }

    // --- MK -------------------------------------------------------------------------------

    #[test]
    fn mk_makefile_ci_target_is_present() {
        let d = tmp("mk_makefile_ci");
        write(&d, "Makefile", "all:\n\techo hi\nci:\n\tcargo test\n");
        assert_eq!(scan_repo(&d).unwrap().mk, Mark::Present);
    }

    #[test]
    fn mk_justfile_ci_recipe_is_present() {
        let d = tmp("mk_justfile_ci");
        // indented + word-boundary `ci` recipe
        write(
            &d,
            "justfile",
            "default:\n    echo hi\nci:\n    cargo test\n",
        );
        assert_eq!(scan_repo(&d).unwrap().mk, Mark::Present);
    }

    #[test]
    fn mk_present_but_no_ci_target_is_partial() {
        let d = tmp("mk_partial");
        write(&d, "Makefile", "build:\n\tcargo build\n");
        assert_eq!(scan_repo(&d).unwrap().mk, Mark::Partial);
    }

    #[test]
    fn mk_absent_is_missing() {
        let d = tmp("mk_missing");
        assert_eq!(scan_repo(&d).unwrap().mk, Mark::Missing);
    }

    #[test]
    fn mk_justfile_word_boundary_rejects_cicd() {
        // `cicd:` must NOT be read as a `ci` recipe (next char is alphanumeric).
        let d = tmp("mk_cicd");
        write(&d, "justfile", "cicd:\n    echo no\n");
        assert_eq!(scan_repo(&d).unwrap().mk, Mark::Partial);
    }

    #[test]
    fn mk_justfile_indented_body_line_is_not_a_ci_recipe() {
        // Regression: an indented recipe BODY line starting with `ci` (e.g. invoking `ci-runner`)
        // must NOT be read as a `ci` recipe header — only a column-0 recipe named `ci` counts.
        let d = tmp("mk_just_body");
        write(&d, "justfile", "build:\n    ci-runner build\n");
        assert_eq!(scan_repo(&d).unwrap().mk, Mark::Partial);
    }

    #[test]
    fn mk_ci_check_is_partial_consistently_across_file_types() {
        // Regression (two-implementations-of-one-fact): `ci-check` is not a bare `ci` target, so
        // Makefile and justfile must BOTH report Partial — never diverge by file type.
        let dm = tmp("mk_cicheck_make");
        write(&dm, "Makefile", "ci-check:\n\tcargo test\n");
        assert_eq!(
            scan_repo(&dm).unwrap().mk,
            Mark::Partial,
            "Makefile ci-check"
        );
        let dj = tmp("mk_cicheck_just");
        write(&dj, "justfile", "ci-check:\n    cargo test\n");
        assert_eq!(
            scan_repo(&dj).unwrap().mk,
            Mark::Partial,
            "justfile ci-check"
        );
    }

    #[test]
    fn lang_label_round_trips_every_variant() {
        // from_label is derived from Display (single source); guard the round-trip so a future
        // Display change or new variant can't silently break config `lang` parsing (meta-pattern #2).
        for v in Lang::ALL {
            assert_eq!(
                Lang::from_label(&v.to_string()),
                v,
                "round-trip failed for {v:?}"
            );
        }
        assert_eq!(Lang::from_label("kotlin"), Lang::Unknown); // unknown degrades, never panics
    }

    // --- LIC (strongest-mark across candidates) -------------------------------------------

    #[test]
    fn lic_populated_license_md_beats_empty_copying() {
        let d = tmp("lic_strongest");
        write(&d, "LICENSE.md", "MIT");
        touch(&d, "COPYING"); // 0-byte
        assert_eq!(scan_repo(&d).unwrap().lic, Mark::Present);
    }

    #[test]
    fn lic_only_empty_is_partial() {
        let d = tmp("lic_partial");
        touch(&d, "LICENSE");
        assert_eq!(scan_repo(&d).unwrap().lic, Mark::Partial);
    }

    #[test]
    fn lic_absent_is_missing() {
        let d = tmp("lic_missing");
        assert_eq!(scan_repo(&d).unwrap().lic, Mark::Missing);
    }

    // --- TOOL -----------------------------------------------------------------------------

    #[test]
    fn tool_pin_present_partial_missing() {
        let d = tmp("tool_present");
        write(&d, ".nvmrc", "20");
        assert_eq!(scan_repo(&d).unwrap().tool, Mark::Present);

        let d = tmp("tool_partial");
        touch(&d, "rust-toolchain.toml");
        assert_eq!(scan_repo(&d).unwrap().tool, Mark::Partial);

        let d = tmp("tool_missing");
        assert_eq!(scan_repo(&d).unwrap().tool, Mark::Missing);
    }

    // --- SR -------------------------------------------------------------------------------

    #[test]
    fn sr_release_please_config_is_present() {
        let d = tmp("sr_rp_config");
        write(&d, "release-please-config.json", "{}");
        assert_eq!(scan_repo(&d).unwrap().sr, Mark::Present);
    }

    #[test]
    fn sr_cargo_dist_metadata_is_present() {
        let d = tmp("sr_cargo_dist");
        write(&d, "Cargo.toml", "[workspace.metadata.dist]\nx = 1\n");
        assert_eq!(scan_repo(&d).unwrap().sr, Mark::Present);
    }

    #[test]
    fn sr_workflow_substring_is_present() {
        let d = tmp("sr_wf_substring");
        write(
            &d,
            ".github/workflows/ship.yml",
            "jobs:\n  rp:\n    uses: googleapis/release-please-action\n",
        );
        assert_eq!(scan_repo(&d).unwrap().sr, Mark::Present);
    }

    #[test]
    fn sr_release_named_workflow_only_is_partial() {
        let d = tmp("sr_release_named");
        write(
            &d,
            ".github/workflows/release.yml",
            "jobs:\n  x:\n    runs-on: a\n",
        );
        assert_eq!(scan_repo(&d).unwrap().sr, Mark::Partial);
    }

    #[test]
    fn sr_nothing_is_missing() {
        let d = tmp("sr_missing");
        write(
            &d,
            ".github/workflows/test.yml",
            "jobs:\n  x:\n    runs-on: a\n",
        );
        assert_eq!(scan_repo(&d).unwrap().sr, Mark::Missing);
    }

    // --- CI count -------------------------------------------------------------------------

    #[test]
    fn ci_absent_dir_is_zero() {
        let d = tmp("ci_no_dir");
        assert_eq!(scan_repo(&d).unwrap().ci, 0);
    }

    #[test]
    fn ci_empty_dir_is_zero() {
        let d = tmp("ci_empty_dir");
        fs::create_dir_all(d.join(".github/workflows")).unwrap();
        assert_eq!(scan_repo(&d).unwrap().ci, 0);
    }

    #[test]
    fn ci_counts_yml_and_yaml_toplevel_only() {
        let d = tmp("ci_count");
        write(&d, ".github/workflows/a.yml", "x");
        write(&d, ".github/workflows/b.yaml", "x");
        write(&d, ".github/workflows/notes.md", "x"); // non-yaml excluded
        write(&d, ".github/workflows/nested/c.yml", "x"); // nested excluded
        assert_eq!(scan_repo(&d).unwrap().ci, 2);
    }

    // --- lang inference -------------------------------------------------------------------

    #[test]
    fn lang_unknown_when_no_signal() {
        let d = tmp("lang_unknown");
        assert_eq!(infer_lang(&d), Lang::Unknown);
        assert_eq!(scan_repo(&d).unwrap().lang, Lang::Unknown);
    }

    #[test]
    fn lang_positive_each() {
        let d = tmp("lang_rust");
        write(&d, "Cargo.toml", "[package]\n");
        assert_eq!(infer_lang(&d), Lang::Rust);

        let d = tmp("lang_node");
        write(&d, "package.json", "{}");
        assert_eq!(infer_lang(&d), Lang::Node);

        let d = tmp("lang_python");
        write(&d, "pyproject.toml", "[project]\n");
        assert_eq!(infer_lang(&d), Lang::Python);

        let d = tmp("lang_tf");
        write(&d, "main.tf", "resource {}");
        assert_eq!(infer_lang(&d), Lang::Terraform);

        let d = tmp("lang_web");
        write(&d, "index.html", "<html>");
        assert_eq!(infer_lang(&d), Lang::Web);
    }

    #[test]
    fn lang_cargo_beats_package_json() {
        let d = tmp("lang_tiebreak_rust");
        write(&d, "Cargo.toml", "[package]\n");
        write(&d, "package.json", "{}");
        assert_eq!(infer_lang(&d), Lang::Rust);
    }

    #[test]
    fn lang_index_html_plus_package_json_is_node() {
        let d = tmp("lang_tiebreak_node");
        write(&d, "index.html", "<html>");
        write(&d, "package.json", "{}");
        assert_eq!(infer_lang(&d), Lang::Node);
    }

    // --- empty + unreadable repo ----------------------------------------------------------

    #[test]
    fn empty_repo_emits_one_all_missing_row() {
        let d = tmp("empty_repo");
        let r = scan_repo(&d).unwrap();
        assert_eq!(r.lang, Lang::Unknown);
        assert_eq!(r.ci, 0);
        for m in [r.claude, r.mk, r.pc, r.lic, r.tool, r.sr, r.chg] {
            assert_eq!(m, Mark::Missing);
        }
        // and it is EMITTED as a row (not dropped) when run through the assembly:
        let table = scan(
            "2026-01-01".into(),
            PathBuf::from("/root"),
            &[("empty".into(), d.clone(), None)],
        );
        assert_eq!(table.rows.len(), 1);
        assert_eq!(table.skipped.len(), 0);
        assert_eq!(table.rows[0].name, "empty");
    }

    #[test]
    fn unreadable_repo_is_skipped_not_dropped() {
        // Construct the bad entry as a FILE where a directory is expected (portable across OSes).
        let d = tmp("skip_parent");
        write(&d, "not-a-dir", "i am a file");
        let bad = d.join("not-a-dir");
        let table = scan(
            "2026-01-01".into(),
            PathBuf::from("/root"),
            &[("ghost".into(), bad, None)],
        );
        assert_eq!(table.rows.len(), 0);
        assert_eq!(table.skipped.len(), 1, "absent repo must NOT be dropped");
        assert_eq!(table.skipped[0].name, "ghost");
    }

    // --- byte-wise sort -------------------------------------------------------------------

    #[test]
    fn rows_and_skipped_sort_byte_wise() {
        // Capital 'Zeta' (0x5A) sorts BEFORE lowercase 'alpha' (0x61) under ASCII byte ordering.
        let good = tmp("sort_good");
        let entries = vec![
            ("alpha".into(), good.clone(), None),
            ("Zeta".into(), good.clone(), None),
            ("mike".into(), good.clone(), None),
        ];
        // skips, as files-where-dirs-expected, in a scrambled order:
        let sp = tmp("sort_skip");
        write(&sp, "f", "x");
        let f = sp.join("f");
        let skip_entries = vec![
            ("Yankee".into(), f.clone(), None),
            ("bravo".into(), f.clone(), None),
        ];
        let mut all = entries;
        all.extend(skip_entries);
        let table = scan("2026-01-01".into(), PathBuf::from("/root"), &all);
        let row_names: Vec<&str> = table.rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(row_names, vec!["Zeta", "alpha", "mike"]);
        let skip_names: Vec<&str> = table.skipped.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(skip_names, vec!["Yankee", "bravo"]);
    }

    // --- resolve_now (both branches) ------------------------------------------------------

    #[test]
    fn resolve_now_override_is_verbatim() {
        let _g = ENV_MUTEX.lock().unwrap();
        clear_env();
        std::env::set_var("CICATRIX_DRIFT_NOW", "1999-12-31");
        assert_eq!(resolve_now(), "1999-12-31");
        clear_env();
    }

    #[test]
    fn resolve_now_default_matches_civil_date_shape() {
        let _g = ENV_MUTEX.lock().unwrap();
        clear_env();
        let now = resolve_now();
        // /\d{4}-\d{2}-\d{2}/
        let bytes = now.as_bytes();
        assert_eq!(now.len(), 10, "expected yyyy-mm-dd, got {now}");
        assert!(bytes[..4].iter().all(u8::is_ascii_digit));
        assert_eq!(bytes[4], b'-');
        assert!(bytes[5..7].iter().all(u8::is_ascii_digit));
        assert_eq!(bytes[7], b'-');
        assert!(bytes[8..10].iter().all(u8::is_ascii_digit));
    }

    /// Civil-date math is correctness-critical and the shape regex would pass a WRONG date —
    /// pin known vectors (epoch, a leap day, a post-leap date).
    #[test]
    fn civil_date_known_vectors() {
        assert_eq!(civil_from_unix_secs(0), "1970-01-01");
        // 2000-02-29 is a leap day (div-by-400). 2000-02-29T00:00:00Z = 951_782_400.
        assert_eq!(civil_from_unix_secs(951_782_400), "2000-02-29");
        // 2000-03-01 the next day.
        assert_eq!(civil_from_unix_secs(951_868_800), "2000-03-01");
        // 2026-06-16T00:00:00Z = 1_781_568_000.
        assert_eq!(civil_from_unix_secs(1_781_568_000), "2026-06-16");
    }

    // --- config (P2) ----------------------------------------------------------------------

    #[test]
    fn config_parses_repos_and_optional_lang() {
        let json = r#"{
            "root": "~/projects",
            "repos": [
                {"name": "reverie", "path": "~/projects/reverie", "lang": "rust"},
                {"name": "cortex", "path": "~/projects/cortex"}
            ]
        }"#;
        let cfg = Config::from_json(json).unwrap();
        assert_eq!(cfg.root, "~/projects");
        assert_eq!(cfg.repos.len(), 2);
        assert_eq!(cfg.repos[0].name, "reverie");
        assert_eq!(cfg.repos[0].lang.as_deref(), Some("rust"));
        assert_eq!(cfg.repos[1].name, "cortex");
        assert_eq!(cfg.repos[1].lang, None);
    }

    #[test]
    fn malformed_config_errors_not_panics() {
        assert!(Config::from_json("{ this is not json").is_err());
        // wrong shape (missing required fields) also errors
        assert!(Config::from_json(r#"{"root": "x"}"#).is_err());
    }

    #[test]
    fn config_load_missing_file_errors() {
        let missing = std::env::temp_dir().join("cicatrix_no_such_markers_xyzzy.json");
        std::fs::remove_file(&missing).ok();
        assert!(Config::load(&missing).is_err());
    }

    #[test]
    fn scan_config_enumerates_partitions_and_skips_by_name() {
        // one real (empty) repo, one absent path → skip; assert partition + names + sort.
        let real = tmp("cfg_real");
        let cfg = Config {
            root: "~/projects".into(),
            repos: vec![
                RepoEntry {
                    name: "zeta-real".into(),
                    path: real.to_string_lossy().into_owned(),
                    lang: Some("rust".into()),
                },
                RepoEntry {
                    name: "absent-one".into(),
                    path: "/nonexistent/path/cicatrix_absent_xyzzy".into(),
                    lang: None,
                },
            ],
        };
        let table = scan_config(&cfg);
        assert_eq!(table.root, PathBuf::from("~/projects"));
        assert_eq!(table.rows.len(), 1, "the real repo is a row");
        assert_eq!(table.rows[0].name, "zeta-real");
        assert_eq!(table.rows[0].lang, Lang::Rust, "explicit label honored");
        assert_eq!(table.skipped.len(), 1, "absent repo skipped, NOT dropped");
        assert_eq!(table.skipped[0].name, "absent-one");
        // The skip reason must be PORTABLE: no filesystem path (which, after tilde-expansion, would
        // be the operator's absolute `$HOME/...`) may leak into the committed report. A `/` in the
        // reason means a path slipped through — the leak this guards.
        assert!(
            !table.skipped[0].reason.contains('/'),
            "skip reason leaked a path (not portable): {}",
            table.skipped[0].reason
        );
    }

    // --- tilde/home expansion -------------------------------------------------------------
    // Regression guard: a `~`-prefixed repo path must be stat-able. Rust's Path does not expand
    // `~`, so without `expand_home` every configured repo (markers.json uses `~/repos/...`) skips
    // and the table renders `_No repos found._`. Env-mutating → serialized on ENV_MUTEX; HOME is
    // saved and restored so the override never leaks to other tests.

    #[test]
    fn expand_home_resolves_leading_tilde_only() {
        let _lock = ENV_MUTEX.lock().unwrap();
        let saved = std::env::var("HOME").ok();
        std::env::set_var("HOME", "/home/zz");
        assert_eq!(expand_home("~"), PathBuf::from("/home/zz"));
        assert_eq!(
            expand_home("~/repos/reverie"),
            PathBuf::from("/home/zz/repos/reverie")
        );
        // No leading tilde → unchanged; an interior `~` is NOT a home reference.
        assert_eq!(expand_home("/abs/path"), PathBuf::from("/abs/path"));
        assert_eq!(expand_home("rel/~/x"), PathBuf::from("rel/~/x"));
        match saved {
            Some(h) => std::env::set_var("HOME", h),
            None => std::env::remove_var("HOME"),
        }
    }

    #[test]
    fn scan_config_expands_tilde_path_into_a_row_not_a_skip() {
        let _lock = ENV_MUTEX.lock().unwrap();
        let saved = std::env::var("HOME").ok();
        // A fake HOME holding one real (empty) repo dir; the config addresses it via `~/myrepo`.
        let home = tmp("fakehome");
        fs::create_dir_all(home.join("myrepo")).unwrap();
        std::env::set_var("HOME", &home);
        let cfg = Config {
            root: "~/".into(),
            repos: vec![RepoEntry {
                name: "myrepo".into(),
                path: "~/myrepo".into(), // must expand under HOME, else it would skip
                lang: None,
            }],
        };
        let table = scan_config(&cfg);
        // Restore HOME before asserting, so a failed assert can't leak the override.
        match saved {
            Some(h) => std::env::set_var("HOME", h),
            None => std::env::remove_var("HOME"),
        }
        assert_eq!(table.rows.len(), 1, "tilde path must expand+scan to a row");
        assert_eq!(table.rows[0].name, "myrepo");
        assert!(
            table.skipped.is_empty(),
            "tilde path must NOT be skipped: {:?}",
            table.skipped
        );
    }
}
