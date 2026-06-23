<!-- lineage
role: session-journal
conforms_to: CANON.md §6; cerebral-work/terrarium SESSIONS.md
defines: Session-Journal
consumes: CANON.md, docs/sessions/ (the fact drop-dir)
-->

# cicatrix — Sessions (append-only handoff journal)

Newest on top. The zero-loss handoff log: each session appends context, decisions, what shipped,
and what's open, so the next session (human or agent) rehydrates fully. Adopted from the terrarium
standard. **Front-load context, one thread to done-done, park-don't-drop.**

This is the **narrative** continuity surface. Atomic, reusable *facts* a session surfaces (gotchas,
config truths, recurring failure classes — the mental-model errors) go in the **drop-dir**,
`docs/sessions/` (schema: `docs/sessions/_SCHEMA.md`).

---

## Session 001 — 2026-06-23 · Adopt terrarium CANON + dropfiles + session surfaces

### Operator (load first)
Christian (`todie`): terse, action-oriented, platform/infra-first; **harsh truth over comfort, no
sycophancy, no AI attribution** (house rule). Thinks in structure; translate intent, don't
transcribe. Drip-feeds context + chases shiny → the agent enforces **loop-discipline: front-load
context, one thread to done-done, park-don't-drop**. Operator approves all irreversible / outward
actions (merge, apply, push-at-scale). Genuine forks → AskUserQuestion.

### Shipped
- Reviewed + squash-merged **PR #4** (static-analysis-on-reverie design doc + README remote fix); 0
  open PRs remained. Local `main` synced (also pulled in the prior CER-1397 P1 corpus work).
- **PR #5** — adopt the `cerebral-work/terrarium` federated-node standard:
  - `CANON.md` — ground-truth charter (what cicatrix is), lineage block; separate from `CLAUDE.md`
    (the agent behavior contract).
  - `SESSIONS.md` (this file) — terrarium append-only handoff journal.
  - `docs/sessions/{grounded,observed}/` + `_SCHEMA.md` — session-fact drop-dir (one file per
    fact), mirroring unsigned-paas (which adopted it from cicatrix's own `docs/bugs/` pattern).
  - `README.md` — lineage block + ground-truth pointer row. `CHANGELOG.md` — Keep-a-Changelog.

### Decisions
- **"dropfiles"** (operator jargon, no literal file in terrarium/janus) → the root-anchored
  canonical context set + lineage (CANON / SESSIONS / README / CHANGELOG), confirmed by interview.
- **Two session surfaces, not one.** First pass conflated them into a single `session/<date>.md`
  dir; corrected after checking unsigned-paas (the reference adopter) to: `SESSIONS.md` (journal) +
  `docs/sessions/` (two-tier fact drop-dir). See `docs/sessions/grounded/FACT_SESSION_DROPDIR_VS_JOURNAL.md`.
- **Full mirror** of unsigned-paas's drop-dir (operator choice) — `docs/sessions/` is distinct from
  `docs/bugs/` (session-facts vs formal code bugs).
- **Scope held** to canon + session surfaces; the full `adopt-standards.md` §1 infra checklist
  (`.claude`/CI/lefthook/Linear, RD-12) is a tracked follow-up, not done here.

### Open threads (park-don't-drop)
- **PR #5** — operator merges (human-merge gate).
- **RD-12 infra adoption** — decide whether cicatrix takes the infra arm (`.claude` permissions +
  terrarium hooks, moon/proto, lefthook, release-please, Linear grooming) or stays docs-only canon.
- **Push up:** cicatrix's `docs/sessions/` two-tier session-fact split is a candidate to push UP to
  terrarium's templates (bidirectional flow).
- Stale remote-only branch `feat/cer-1397-p1-poison-gate` (work landed) — likely deletable.

### How to continue
Operator merges PR #5. Then decide the RD-12 infra-adoption question — the natural next thread if
cicatrix goes beyond canon. Append a new `## Session 002` block **above** Session 001.
