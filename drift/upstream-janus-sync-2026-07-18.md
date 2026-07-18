# Upstream sync report — wbrown/janus-datalog → cicatrix (2026-07-18)

cicatrix vendored the janus-datalog reviewer-hook arm on **2026-06-16**. Upstream HEAD today is
`6d714b3` (pushed 2026-07-18); the hook framework there was consolidated 2026-07-05 (`cd1801b`,
"Adopt the review-hook framework") and hardened since. This report is the file-by-file delta,
with a recommendation per item. **Hooks are operator-immutable (directive 2026-07-04)** — nothing
below was applied; the operator applies by hand.

Reference copy of upstream at the exact sha reviewed: shallow clone in this session's scratchpad
(`…/scratchpad/janus/jd`, transient) — or re-fetch any file with
`gh api repos/wbrown/janus-datalog/contents/<path>?ref=6d714b3`.

## A. Reviewer-hook deltas (vendored 2026-06-16 vs upstream 6d714b3)

| File | Delta | Recommendation |
|---|---|---|
| `review-reasoning.system.md` | **AUTH_LEDGER protocol** added: a mandatory 3-step walk (locate ledger entry → check scope → check for undisclosed new problem) run *before* fail-closed calibration; verified user authorization becomes dispositive and the reviewer may not re-litigate it. Also: corrected-vs-outstanding mistake calibration (a named-and-corrected error in the window is not grounds to block; an uncorrected instance stays live), and "a tool call rejected by another mechanism is not a completed violation." | **Port verbatim** — zero repo-specific content. This mechanizes the note cicatrix CLAUDE.md already carries ("user authorization is primary") and directly fixes the known failure class where the gate blocks work the user already approved. |
| `review-reasoning.sh` | Two changes: (1) removes the double-bounding `tail -75` on the extracted reasoning (upstream 2026-07-01 finding: a settled ruling silently fell out of the reviewer's window); (2) builds `AUTH_LEDGER` via `lib/auth_ledger.jq` over the **full** transcript and appends it to the review prompt as a separate section. | **Port verbatim.** |
| `lib/auth_ledger.jq` | **New file.** Full-transcript scan for the only two genuinely sparse signals regardless of session length: user-typed messages, and `ExitPlanMode`/`AskUserQuestion` tool_results (explicit decisions). Filters `<task-notification>` synthetic user messages and >3000-char blobs. | **Port verbatim.** |
| `review-edit.sh` | `compute_test_evidence`: deterministic test-first ground truth. Extracts top-level func names added by the edit and greps sibling `*_test.go` files on disk, handing the reviewer *verified* evidence instead of relying on a bounded transcript window (fixes false "no test written first" verdicts in long sessions, observed upstream 2026-07-01). Purely additive — never suppresses a real violation. | **Port with Rust adaptation.** The extractor is Go-specific (`func Name(` / `*_test.go`). cicatrix needs: added `fn` names mapped against `#[cfg(test)]` modules in the same file, `tests/*.rs`, and `mod tests` siblings. Mechanism ports; extractor is ~30 lines of awk to rewrite. |
| `validate-bash.sh` | Substantively identical (local copy adds the deliberate `CICATRIX_STRICT_BASH=1` opt-in guard; upstream tweaked one example path). | **No action.** |
| `review-edit.system.md`, `review-verdict-contract.md`, `lib/review_common.sh` | Byte-identical to vendored copies. | **No action.** |

## B. New upstream mechanisms not present in cicatrix

| Item | What it is | Recommendation |
|---|---|---|
| `gate-pretooluse.sh` + `gate-reset.sh` | **Mechanical enforcement of "one mutating action per turn"** — currently a prose-only invariant in cicatrix CLAUDE.md. Edit/Write/NotebookEdit may batch among themselves; Bash must be solo in its turn (any second mutating call is denied via `permissionDecision:"deny"`). Atomic `mkdir` markers under `.claude/.gate-locks/`, no TTL (deliberate: a TTL would re-admit chaining whenever the first call runs long). Reset wired to PostToolBatch + UserPromptSubmit + Stop. | **Operator decision — real friction tradeoff.** Fits the enforced-over-advisory doctrine exactly (this is the trellis for the invariant the repo already states), but it will deny the batched `cmd1; cmd2` style every agent session here currently uses. If adopted, adopt knowingly. |
| `review-commit.sh` | Upstream's LLM commit auditor (premature-victory detection over the reasoning chain, sonnet-judged, nonce-stamped verdict). cicatrix's `commit-gate.sh` is the deterministic sibling (green-baseline marker + victory-phrasing grep + fix-needs-test), adapted from an earlier version of this file. | **Optional.** Complementary, not competing: deterministic gate stays; the LLM arm could be added opt-in like the other reviewers. Low urgency. |
| `.claude/skills/datalog/` (17 KB SKILL.md) + `list-skills.sh` (SessionStart skill inventory) | Upstream's domain-knowledge skill: "reach for it whenever you WRITE a query, not only to inspect" — the query-before-authoring pattern packaged as a skill rather than CLAUDE.md prose. | **Idea to steal, not a port.** The cicatrix analog is a `cicatrix` skill teaching corpus query (`cicatrix query <diff>`) + BUG_*.md authoring against `_SCHEMA.md`. Would become the natural injection surface for CER-1394 (auto-authoring). Park as a ticket. |

## C. Upstream bug corpus

`docs/bugs/resolved/` upstream now holds **96 resolved bugs** (plus 2 active). Naming/format
differ from cicatrix's schema (`BUG-DASH-CASE.md`, no `_SCHEMA.md`, free-form sections). Two uses:
1. **Parser robustness fixture** — 96 real-world variant documents to harden `src/bug_md.rs`
   against (currently tuned to 2 seed bugs of our own schema).
2. **Meta-pattern source** — the `TALE_AND_LESSONS_OF_*` docs (correctness bugs; workarounds &
   invariants) distill that corpus; worth a read pass when regenerating cicatrix meta-patterns.

## D. House findings (this repo, today)

- Local `main` was 5 commits behind; **synced to `e6b48a0`** (CER-1397 two-tier corpus, drift
  scanner D0, CANON adoption, static-analysis design, terrarium federation). Baseline re-established
  **GREEN** on the new tree.
- **Wiring gap:** `.claude/hooks/guard-main-push.sh` (landed in `e6b48a0`, documented in
  `.claude/README.md`) is **not wired** in `.claude/settings.json` — only the SessionStart baseline
  hook is. The guard is inert. Operator to wire (hooks are operator-immutable).
- **Phase-2 blocker worsened:** `reverie.dev.unsigned.gg` no longer resolves/connects at all
  (curl exit 6/timeout on `/health` too — previously `/health` was 200 and data routes 404).
  Consistent with Lyra retirement under OPS-468. CER-1376 remains gated; its eventual target is the
  reveried home on **Cygnus**, wherever Leg B (CER-1362 → OPS-271) lands it. The 2026-06-22
  cutover memory is stale on this point (updated today).
- Ticket state: CER-1374 Done · CER-1375 Done · CER-1397 Done · CER-1373 epic open ·
  CER-1376 gated · CER-1393/1394/1395/1396 (agent-afk ports) in "Ready", unstarted.

## E. Apply path (decisions taken 2026-07-18)

Operator interview outcome: port **all of section A** and **adopt the turn gate as-is**.
Staged, ready-to-place copies (verbatim upstream, plus the Rust-adapted `review-edit.sh` and a
proposed `settings.json` wiring that also fixes the inert `guard-main-push`) live in
`refs/upstream-review-arm-2026-07-18/` (untracked by design — `refs/` is gitignored; re-fetch
any verbatim file from upstream `@6d714b3` if the local copy is gone) — see its `APPLY.md` for
the file map and post-apply checklist. Hooks are operator-immutable: the operator places the files by hand and runs the
hook self-tests (`.claude/hooks/test/run.sh`).
