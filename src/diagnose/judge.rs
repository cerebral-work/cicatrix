//! Convergence judge and BugFact document auto-authoring.
//!
//! Evaluates candidate hypotheses, selects the winning root-cause diagnosis,
//! synthesizes the mental-model error, and constructs a schema-compliant
//! markdown document ready for `docs/bugs/observed/BUG_<SLUG>.md`.
//!
//! Clean-room Rust port of the fork-and-judge contract from agent-afk (Apache-2.0).

use std::fs;
use std::path::PathBuf;

use crate::bug_md;
use crate::corpus;
use crate::diagnose::types::{
    ConvergenceReport, DiagnoseError, DiagnoseTarget, ForkOptions, RootCauseHypothesis,
};
use crate::store::BugFact;

/// Convert a test signature or name into an uppercase snake-case bug slug.
pub fn sanitize_slug(name: &str) -> String {
    let stripped = name
        .trim()
        .trim_start_matches("bug:")
        .trim_start_matches("BUG_")
        .replace("::", "_")
        .replace(['-', '.', '/', ' '], "_")
        .to_uppercase();

    let slug: String = stripped
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();

    let clean = slug.trim_matches('_');
    if clean.is_empty() {
        "DIAGNOSED_DEFECT".to_string()
    } else {
        clean.to_string()
    }
}

/// Convert an uppercase slug into a kebab-case identifier.
pub fn slug_to_kebab(slug: &str) -> String {
    slug.to_lowercase().replace('_', "-")
}

/// Judge and converge candidate hypotheses into a final diagnostic verdict.
pub fn judge_and_converge(
    target: &DiagnoseTarget,
    hypotheses: Vec<RootCauseHypothesis>,
    options: &ForkOptions,
) -> Result<ConvergenceReport, DiagnoseError> {
    if hypotheses.is_empty() {
        return Err(DiagnoseError::ConvergenceFailure(
            "no hypotheses supplied for convergence evaluation".to_string(),
        ));
    }

    // Select winning hypothesis with highest confidence
    let mut sorted = hypotheses.clone();
    sorted.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let winner = sorted.first().cloned().ok_or_else(|| {
        DiagnoseError::ConvergenceFailure("failed to select winning hypothesis".to_string())
    })?;

    let rationale = format!(
        "Evaluated {} candidate hypotheses; selected `{}` ({}) with confidence {:.2} based on diagnostic evidence.",
        hypotheses.len(),
        winner.id,
        winner.title,
        winner.confidence
    );

    let raw_slug = sanitize_slug(&target.name);
    let bug_slug = format!("BUG_{raw_slug}");
    let kebab_id = format!("bug:{}", slug_to_kebab(&raw_slug));

    // Determine files list
    let files = if !target.candidate_files.is_empty() {
        target.candidate_files.clone()
    } else {
        let extracted = crate::diagnose::fork::extract_suspected_files(&target.failure_log);
        if !extracted.is_empty() {
            extracted
        } else {
            vec!["src/lib.rs".to_string()]
        }
    };

    let primary_file = files
        .first()
        .cloned()
        .unwrap_or_else(|| "src/lib.rs".to_string());
    let scope_val = primary_file
        .rfind('/')
        .map(|idx| primary_file[..idx].to_string())
        .filter(|s| !s.is_empty());

    let symptom_prose = if !target.failure_log.trim().is_empty() {
        target
            .failure_log
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("Failure observed during execution")
            .trim()
            .to_string()
    } else {
        format!("Regression failure observed in target `{}`", target.name)
    };

    let repro_prose = if let Some(ref cmd) = target.repro_command {
        cmd.clone()
    } else {
        format!("cargo test -j 2 {}", target.name.trim())
    };

    let bug_fact = BugFact {
        id: bug_slug.clone(),
        files: files.clone(),
        symptom: symptom_prose.clone(),
        fix_commit: "UNFIXED — drafted by cicatrix diagnose".to_string(),
        regression_test: winner.suggested_regression_test.clone(),
        meta_pattern: winner.meta_pattern.clone(),
        scope: scope_val.clone(),
        do_not_generalize: false,
        reproducer: target.repro_command.clone(),
        stochastic: None,
        frontier: None,
    };

    // Render markdown conforming strictly to docs/bugs/grounded/_SCHEMA.md
    let files_rendered = files.join(", ");
    let scope_rendered = match &scope_val {
        Some(s) => format!("- **scope:** {s}\n"),
        None => String::new(),
    };
    let repro_field = match &target.repro_command {
        Some(cmd) => format!("- **reproducer:** {cmd}\n"),
        None => String::new(),
    };

    let rendered_markdown = format!(
        "# {bug_slug}\n\n\
         - **id:** {kebab_id}\n\
         - **files:** {files_rendered}\n\
         - **fix-commit:** UNFIXED — drafted by cicatrix diagnose\n\
         - **regression-test:** {regression_test}\n\
         - **meta-pattern:** {meta_pattern}\n\
         {scope_rendered}\
         {repro_field}\
         - **status:** active\n\n\
         > **Observed tier — ungrounded.** Drafted by `cicatrix diagnose`.\n\
         > No fix has landed and no regression test has verified the fix on a clean tree.\n\
         > Move to `docs/bugs/grounded/` once verified.\n\n\
         ## Symptom\n\
         {symptom}\n\n\
         ## Root cause\n\
         The mental-model error: {mental_model}\n\n\
         {mechanism}\n\n\
         ## Reproduction\n\
         ```bash\n\
         {repro}\n\
         ```\n\n\
         ## Resolution\n\
         PENDING: {suggested_fix}\n\n\
         ## Lesson\n\
         Prevent the entire `{meta_pattern}` defect class by verifying boundary invariants \
         and adding regression tests before landing changes in `{primary_file}`.\n",
        bug_slug = bug_slug,
        kebab_id = kebab_id,
        files_rendered = files_rendered,
        regression_test = winner.suggested_regression_test,
        meta_pattern = winner.meta_pattern,
        scope_rendered = scope_rendered,
        repro_field = repro_field,
        symptom = symptom_prose,
        mental_model = winner.mental_model_error,
        mechanism = winner.mechanism,
        repro = repro_prose,
        suggested_fix = winner.suggested_fix,
        primary_file = primary_file,
    );

    // Validate rendered markdown with bug_md parser to guarantee :db/neverZeroValue compliance
    let _parsed = bug_md::parse(&rendered_markdown, Some(&bug_slug))
        .map_err(|e| DiagnoseError::SchemaValidation(e.to_string()))?;

    // Handle persistence to disk if requested
    let mut written_file: Option<String> = None;
    if options.write_file {
        let dest_path: PathBuf = if let Some(ref explicit) = options.output_file {
            explicit.clone()
        } else {
            let base_dir = options
                .target_dir
                .clone()
                .unwrap_or_else(|| corpus::resolve_dir(corpus::Tier::Observed));
            base_dir.join(format!("{bug_slug}.md"))
        };

        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&dest_path, &rendered_markdown)?;
        written_file = Some(dest_path.to_string_lossy().to_string());
    }

    Ok(ConvergenceReport {
        target: target.name.clone(),
        hypotheses,
        winning_hypothesis_id: winner.id,
        convergence_rationale: rationale,
        bug_fact,
        rendered_markdown,
        output_file: written_file,
    })
}
