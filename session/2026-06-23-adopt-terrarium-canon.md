<!-- lineage
role: session
conforms_to: session/README.md
-->

# Session — 2026-06-23 · Adopt terrarium CANON + dropfiles + session/ drop dir

## Objective
Bring cicatrix onto the `cerebral-work/terrarium` standard as a federated/external node: adopt a
root **CANON.md** + the canonical **dropfiles** (root context set carrying lineage blocks), and the
**`session/` drop-dir** handoff format from `cicatrix`/`wbrown/janus-datalog` (per-session drop
files) instead of terrarium's single `SESSIONS.md`.

## Shipped
- Reviewed + squash-merged **PR #4** (static-analysis-on-reverie design doc + README remote fix);
  0 open PRs remained. Local `main` synced (also pulled in the previously-merged CER-1397 P1 corpus
  work, `7935834`).
- **CANON.md** (new) — ground-truth "what cicatrix is" charter with a terrarium-style `lineage`
  block; deliberately separate from `CLAUDE.md` (the agent behavior contract). Captures the arms,
  the "reverie is the store" model, invariants, the inherited feature-branch→PR→human-merge process,
  and the federation posture.
- **session/** (new) — `session/README.md` defines the drop-dir format (janus-derived, dated
  drop-per-session, immutable); this file is the inaugural drop.
- **README.md** — added a `lineage` block + pointers to CANON.md and session/.
- **CHANGELOG.md** (new) — Keep-a-Changelog form with `[Unreleased]` per terrarium `docs/RELEASE.md`.

## Decisions
- "dropfiles" (operator jargon, no literal file in terrarium/janus) resolved via `AskUserQuestion`
  to **the root-anchored canonical context set + lineage**: CANON.md + session/ + README + CHANGELOG,
  each with a `<!-- lineage -->` block.
- **Scope held to the three named artifacts** — did *not* run the full `adopt-standards.md` §1
  checklist (.claude permissions/hooks, moon/lefthook/release-please CI, Linear grooming). That is
  the larger RD-12 infra adoption; tracked as a follow-up, not done here.
- CANON ≠ CLAUDE: CANON is the charter; CLAUDE is the behavior contract. No duplication.

## Open threads (park-don't-drop)
- **PR for this branch** (`design/adopt-terrarium-canon`) — operator merges (human-merge gate).
- **Full terrarium adoption (RD-12 scope)** — `.claude/settings.json` permissions + terrarium hook
  set (`guard-main-push`, `ci-gate`, `lint-on-edit`), moon/proto, lefthook, release-please +
  cliff.toml, Linear grooming. Decide whether cicatrix takes the infra arm or stays docs-only canon.
- **Bidirectional push-up:** the `session/` drop-dir format is a candidate to push UP to terrarium's
  templates (it improves on the monolithic `SESSIONS.md`). File against `cerebral-work/terrarium`.
- Stale remote-only branch `feat/cer-1397-p1-poison-gate` (work landed via the P1 merge) — likely
  deletable.

## How to continue
Open a PR for `design/adopt-terrarium-canon`, let the operator merge. Then decide the RD-12
infra-adoption question (above) — that's the natural next thread if cicatrix goes beyond canon.
