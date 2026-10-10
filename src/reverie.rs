//! The reverie bridge (CER-1375, Phase 1): project bug-facts into reverie observations and query
//! "does this diff touch a known-bug surface?" back out. Reverie is the single store; the markdown
//! corpus is the source of truth and the index's content is regenerable from it (design §2).
//!
//! Contract (verified against reveried v0.9.13 @ 127.0.0.1:7437):
//! - `POST /observations` ← `AddObservationParams` (`type`,`title`,`content`,`project`,`topic_key`,
//!   `tags:[{facet,value}]`,`event_id` idempotency,`source`). Required: title + content.
//! - `GET /search?q&type&project&limit&q_mode` → flat JSON array of observations. No tag-filter
//!   param, so file-path matching rides FTS over `content` (which embeds the paths).

use crate::bug_md;
use crate::corpus;
use crate::store::{BugFact, BugStore};
use serde::Serialize;
use std::io;
use std::path::{Path, PathBuf};

pub const PROJECT: &str = "cicatrix";
pub const OBS_TYPE: &str = "bug-fact";
const DEFAULT_URL: &str = "http://127.0.0.1:7437";

/// The canonical (grounded) bug-doc corpus dir. Back-compat shim over
/// [`corpus::resolve_dir`] for the grounded tier (env precedence: `CICATRIX_CORPUS_GROUNDED`
/// → `CICATRIX_CORPUS_RESOLVED` → `CICATRIX_CORPUS` → `docs/bugs/grounded`).
pub fn corpus_dir() -> PathBuf {
    corpus::resolve_dir(corpus::Tier::Grounded)
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Tag {
    pub facet: String,
    pub value: String,
}

/// The `POST /observations` body for one bug-fact. Field names + `type` rename match reverie's
/// `AddObservationParams`.
#[derive(Serialize, Debug, PartialEq)]
pub struct ObservationPayload {
    #[serde(rename = "type")]
    pub type_: String,
    pub title: String,
    pub content: String,
    pub project: String,
    pub topic_key: String,
    pub tags: Vec<Tag>,
    pub event_id: String,
    pub source: String,
}

/// Render a bug-fact as observation content. File paths are embedded verbatim so the FTS index
/// matches a changed-file query (the `/search` tag-filter gap, design §3.2).
pub fn render_content(fact: &BugFact) -> String {
    // The reproducer rides the content (not a tag): it is prose for recall, not a filter handle.
    let reproducer = fact
        .reproducer
        .as_ref()
        .map(|r| format!("reproducer: {r}\n"))
        .unwrap_or_default();
    format!(
        "{slug}\n\nfiles: {files}\nmeta-pattern: {mp}\nfix-commit: {fc}\nregression-test: {rt}\n{reproducer}\n{sym}",
        slug = fact.id,
        files = fact.files.join(", "),
        mp = fact.meta_pattern,
        fc = fact.fix_commit,
        rt = fact.regression_test,
        reproducer = reproducer,
        sym = fact.symptom,
    )
}

/// Project a bug-fact into its reverie observation payload (pure; the one-way markdown→reverie map).
pub fn project(fact: &BugFact) -> ObservationPayload {
    let mut tags: Vec<Tag> = fact
        .files
        .iter()
        .map(|f| Tag {
            facet: "file".into(),
            value: f.clone(),
        })
        .collect();
    tags.push(Tag {
        facet: "meta-pattern".into(),
        value: fact.meta_pattern.clone(),
    });
    tags.push(Tag {
        facet: "fix-commit".into(),
        value: fact.fix_commit.clone(),
    });

    ObservationPayload {
        type_: OBS_TYPE.into(),
        title: fact.id.clone(),
        content: render_content(fact),
        project: PROJECT.into(),
        topic_key: fact.meta_pattern.clone(),
        tags,
        event_id: fact.id.clone(), // stable slug → idempotent re-record (replay dedup)
        source: PROJECT.into(),
    }
}

/// Pull observation titles (== bug slugs) out of a `/search` response. Defensive: the body is a
/// flat array of observations, but tolerate `{results|observations: [...]}` wrappers too.
pub fn extract_slugs(body: &serde_json::Value) -> Vec<String> {
    let arr = body
        .as_array()
        .or_else(|| body.get("results").and_then(|v| v.as_array()))
        .or_else(|| body.get("observations").and_then(|v| v.as_array()));
    let Some(arr) = arr else { return Vec::new() };
    arr.iter()
        .filter_map(|o| o.get("title").and_then(|t| t.as_str()).map(str::to_string))
        .collect()
}

/// HTTP client for reveried. Reverie supplies the *match*; the local corpus supplies the *content*
/// (keeps the markdown canonical). `corpus_dir` is where query results are hydrated from.
#[derive(Clone, Debug)]
pub struct ReverieBridge {
    base_url: String,
    token: Option<String>,
    corpus_dir: PathBuf,
}

/// Standing local credential for the reverie write path: `~/.cicatrix/reverie-token` (0600).
/// Mint with `reveried token mint --sub cicatrix --scope "mcp:read obs:write" --proj cicatrix`
/// (the shape pinned by reveried's `cicatrix_bridge_contract` test). `REVERIE_TOKEN` env wins;
/// the file is the fallback so shells and hooks need no export (CER-1629 fallout).
fn default_token_path() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(|h| PathBuf::from(h).join(".cicatrix/reverie-token"))
}

fn token_from_file(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn resolve_token(env_token: Option<String>, file_token: Option<String>) -> Option<String> {
    env_token
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or(file_token)
}

impl ReverieBridge {
    /// `REVERIE_URL` (default `http://127.0.0.1:7437`), bearer from `REVERIE_TOKEN` if set,
    /// else `~/.cicatrix/reverie-token`; corpus from the grounded tier (default
    /// `docs/bugs/grounded`).
    pub fn from_env() -> Self {
        let base_url = std::env::var("REVERIE_URL")
            .unwrap_or_else(|_| DEFAULT_URL.to_string())
            .trim_end_matches('/')
            .to_string();
        let token = resolve_token(
            std::env::var("REVERIE_TOKEN").ok(),
            default_token_path().as_deref().and_then(token_from_file),
        );
        Self {
            base_url,
            token,
            corpus_dir: corpus_dir(),
        }
    }

    fn auth(&self, req: ureq::Request) -> ureq::Request {
        match &self.token {
            Some(t) => req.set("Authorization", &format!("Bearer {t}")),
            None => req,
        }
    }
}

fn to_io(e: ureq::Error) -> io::Error {
    io::Error::other(e.to_string())
}

impl BugStore for ReverieBridge {
    fn record(&mut self, fact: &BugFact) -> io::Result<()> {
        let payload = project(fact);
        let url = format!("{}/observations", self.base_url);
        self.auth(ureq::post(&url))
            .send_json(&payload)
            .map_err(to_io)?;
        Ok(())
    }

    fn touches_known_bug(&self, changed_files: &[String]) -> io::Result<Vec<BugFact>> {
        if changed_files.is_empty() {
            return Ok(Vec::new());
        }
        // One query per changed file in default (AND) mode: every path token must appear in the
        // observation, so a full path matches its own bug but not a sibling file in the same dir.
        // (verified: `q_mode=or` on a slash-path returns nothing; AND on the full path is precise.)
        let url = format!("{}/search", self.base_url);
        let mut slugs: std::collections::HashSet<String> = std::collections::HashSet::new();
        for file in changed_files {
            let resp = self
                .auth(ureq::get(&url))
                .query("q", file)
                .query("project", PROJECT)
                .query("type", OBS_TYPE)
                .query("limit", "50")
                .call()
                .map_err(to_io)?;
            let body: serde_json::Value = resp.into_json()?; // ureq's into_json yields io::Error
            slugs.extend(extract_slugs(&body));
        }

        // Hydrate matched slugs to full facts from the canonical corpus.
        let facts = bug_md::parse_dir(&self.corpus_dir)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Ok(facts
            .into_iter()
            .filter(|f| slugs.contains(&f.id))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> BugFact {
        BugFact {
            id: "BUG_SAMPLE".into(),
            files: vec!["src/a.rs:12".into(), "src/b.rs".into()],
            symptom: "It broke.".into(),
            fix_commit: "#42 (CER-1)".into(),
            regression_test: "sample guard".into(),
            meta_pattern: "Type mismatches kill".into(),
            scope: None,
            do_not_generalize: false,
            reproducer: None,
            stochastic: None,
            frontier: None,
        }
    }

    #[test]
    fn projection_shape_and_idempotency_key() {
        let p = project(&sample());
        assert_eq!(p.type_, "bug-fact");
        assert_eq!(p.title, "BUG_SAMPLE");
        assert_eq!(p.project, "cicatrix");
        assert_eq!(p.event_id, "BUG_SAMPLE"); // slug == idempotency key
        assert_eq!(p.topic_key, "Type mismatches kill");
    }

    #[test]
    fn content_embeds_every_file_path_for_fts() {
        let c = render_content(&sample());
        assert!(
            c.contains("src/a.rs:12") && c.contains("src/b.rs"),
            "paths missing: {c}"
        );
    }

    /// The reproducer is recall prose: present in content when the fact carries one, and the
    /// rendered line is absent entirely (not blank-valued) when it doesn't.
    #[test]
    fn content_carries_reproducer_only_when_present() {
        let mut with = sample();
        with.reproducer = Some("GOGC=1 cargo test -p store".into());
        let c = render_content(&with);
        assert!(
            c.contains("reproducer: GOGC=1 cargo test -p store"),
            "reproducer missing: {c}"
        );
        let without = render_content(&sample());
        assert!(
            !without.contains("reproducer:"),
            "reproducer line leaked: {without}"
        );
    }

    #[test]
    fn tags_cover_files_metapattern_and_fixcommit() {
        let p = project(&sample());
        assert!(p.tags.contains(&Tag {
            facet: "file".into(),
            value: "src/a.rs:12".into()
        }));
        assert!(p.tags.contains(&Tag {
            facet: "file".into(),
            value: "src/b.rs".into()
        }));
        assert!(p.tags.contains(&Tag {
            facet: "meta-pattern".into(),
            value: "Type mismatches kill".into()
        }));
        assert!(p.tags.contains(&Tag {
            facet: "fix-commit".into(),
            value: "#42 (CER-1)".into()
        }));
    }

    #[test]
    fn payload_serializes_type_field_as_type() {
        let v = serde_json::to_value(project(&sample())).unwrap();
        assert_eq!(v["type"], "bug-fact"); // renamed from type_
        assert_eq!(v["event_id"], "BUG_SAMPLE");
        assert_eq!(v["tags"][0]["facet"], "file");
    }

    #[test]
    fn extract_slugs_handles_array_and_wrappers_and_garbage() {
        let arr = serde_json::json!([{"title": "BUG_A"}, {"title": "BUG_B"}, {"nope": 1}]);
        assert_eq!(extract_slugs(&arr), vec!["BUG_A", "BUG_B"]);
        let wrapped = serde_json::json!({"results": [{"title": "BUG_C"}]});
        assert_eq!(extract_slugs(&wrapped), vec!["BUG_C"]);
        assert!(extract_slugs(&serde_json::json!({"unexpected": true})).is_empty());
    }

    // --- token resolution: REVERIE_TOKEN env wins; ~/.cicatrix/reverie-token is the
    // standing local credential (post-#1157 reveried auths writes; CER-1629) ---

    fn tmp_token_file(name: &str, contents: Option<&str>) -> PathBuf {
        let p = std::env::temp_dir().join(format!("cicatrix-test-{name}-{}", std::process::id()));
        match contents {
            Some(c) => std::fs::write(&p, c).unwrap(),
            None => {
                let _ = std::fs::remove_file(&p);
            }
        }
        p
    }

    #[test]
    fn token_file_read_trims_whitespace_and_newline() {
        let p = tmp_token_file("trim", Some("  tok-abc\n"));
        assert_eq!(token_from_file(&p).as_deref(), Some("tok-abc"));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn token_file_missing_or_blank_is_none() {
        let missing = tmp_token_file("missing", None);
        assert_eq!(token_from_file(&missing), None);
        let blank = tmp_token_file("blank", Some("  \n"));
        assert_eq!(token_from_file(&blank), None);
        let _ = std::fs::remove_file(&blank);
    }

    #[test]
    fn env_token_wins_over_file_token() {
        assert_eq!(
            resolve_token(Some(" env-tok ".into()), Some("file-tok".into())),
            Some("env-tok".into())
        );
    }

    #[test]
    fn file_token_used_when_env_absent_or_empty() {
        assert_eq!(
            resolve_token(None, Some("file-tok".into())),
            Some("file-tok".into())
        );
        assert_eq!(
            resolve_token(Some(String::new()), Some("file-tok".into())),
            Some("file-tok".into())
        );
        assert_eq!(resolve_token(None, None), None);
    }
}
