<!-- lineage
role: canon
conforms_to: cerebral-work/terrarium CANON.md (federated/external-node standard); docs/runbooks/adopt-standards.md
defines: cicatrix, Bug-Fact, Grounded-Corpus, Meta-Pattern, Convention-Drift, Reverie-Bridge, Green-Baseline, Commit-Gate, As-Of-Query
depends_on: wbrown/janus-datalog (method ancestor — agent-discipline layer); cerebral-work/reverie (the store, CER-1369)
consumes: ReverieBridge (src/reverie.rs), the markdown bug corpus (docs/bugs/grounded/), CLAUDE.md (the agent behavior contract)
-->

# cicatrix — CANON

**Status:** **Accepted v1.0** · finalized 2026-06-23 · ground-truth spec. cicatrix is an
**external / federated node** of the `cerebral-work/terrarium` standard (`docs/REGISTRY.md` →
archetypes are adopted, *not* absorbed): it stays its own repo and **consumes** terrarium's
standards without merging in. This file is the *what cicatrix is* charter; **`CLAUDE.md` is the
*how an agent behaves here* contract** — they are deliberately separate. Build is
**canon-anchored**: amend CANON first, then build.

## 0. What cicatrix is

cicatrix is an independent **regression-memory + convention-drift** framework with commit-time
review gates. The premise: an agent (human or LLM) reintroduces a fixed bug because the *memory of
past failures* is not queryable at authoring time. cicatrix makes it queryable, and gates commits
on it. Its method is drawn from **`wbrown/janus-datalog`**'s agent-discipline layer (a corpus of
structured bug-fix docs distilled into meta-patterns, plus fail-closed LLM review hooks), bridged
into **reverie**'s consolidation substrate.

cicatrix is *consolidation applied to defects* — it is not a new memory store. Per the
**2026-06-18 decision, "reverie is the store,"** the janus-datalog datalog sidecar was dropped;
the markdown corpus is the source of truth and each fact is projected one-way into reverie.

## 1. The arms (the constructs)

| Arm | What it is | Surface |
|---|---|---|
| **Bug-memory store** | every fixed bug as a durable structured fact `(bug, file, symptom, root-cause, fix-commit, regression-test, meta-pattern)` | `docs/bugs/grounded/` (truth) → projected to reverie (query index) |
| **Meta-pattern injection** | the corpus rolled up into named recurring rules, injected **upstream** of an edit — a discipline that generates actions, not a final-checkpoint filter | `CLAUDE.md` (machine-managed block, `cicatrix project-meta`) |
| **Convention-drift** | cross-repo repo × convention-marker table vs canonical templates; reverie is one scanned row | `drift/` (see `docs/design/cicatrix-static-analysis-on-reverie.md`) |
| **Reasoning-audit + commit-gate** | fail-closed hooks: block "premature victory," enforce the green baseline, require a regression test per fix | `.claude/hooks/` |

## 2. The store model (decided 2026-06-18: "reverie is the store")

- **Fact store:** **reverie** is the single store. The markdown corpus (`docs/bugs/grounded/`) is
  the source of truth; each bug-fact is projected one-way into a reverie observation
  (`project=cicatrix`) — a regenerable query index, never a second source of truth.
- **Two-tier corpus:** `grounded/` (resolved, projected) vs `observed/` (ungrounded, **not**
  projected until promoted). Only grounded facts reach reverie or the meta-pattern roll-up.
- **Time-travel:** `query --as-of <commit>` preserves janus's `AsOf(commit)` without a second
  store, by filtering on the fix-commit's git-ancestry (`src/gitf.rs`).
- **Transport:** the `ReverieBridge` (`src/reverie.rs`, `BugStore` impl) — `POST /observations`,
  `GET /search`. Convention-drift findings ride the **same** bridge as `type=convention-drift`;
  no new transport, no reverie schema change.

## 3. Invariants (the non-negotiables)

These define cicatrix's correctness; the agent-facing phrasing lives in `CLAUDE.md`.

- **The baseline is green at session start by construction.** Any red test this session was caused
  by this session's work. "Pre-existing failure" is forbidden phrasing.
- **Every bug fix ships a regression test** plus a `docs/bugs/grounded/BUG_*.md` entry.
- **Single source of truth.** The markdown corpus is authoritative; reverie is a regenerable
  projection. Two implementations of one fact drift (meta-pattern #2).
- **Grounded-only projection.** `observed/` never reaches reverie or the meta-pattern block.

## 4. Process (inherited from the terrarium standard)

- Every change = **feature branch off `main`** → **PR** → review → **human-gated merge**. Never
  direct-push to main. The **operator merges**; agents prepare and review.
- Conventional commits, **signed**, **no AI attribution** trailer/footer (house rule).
- RFC-worthy design → **docs-first** (design doc → PR → merge after finalization).
- CI is the validation gate (`fmt + clippy`, baseline gate `cargo test`, cargo-deny, secret scan);
  `.claude/hooks/` enforce the reviewer failure-modes and the commit-gate locally.
- User-facing changes carry a `CHANGELOG.md` `[Unreleased]` entry.

## 5. Federation posture

cicatrix **adopts** the terrarium standards (`cerebral-work/terrarium`,
`docs/runbooks/adopt-standards.md`) as an external node — it stays standalone and reversible. Flow
is **bidirectional**: a discipline cicatrix sharpens (e.g. the grounded/observed two-tier corpus,
the `session/` drop-dir handoff format) is a candidate to push **up** to the terrarium templates so
the whole federation benefits. When you improve a pattern here, ask: *does this belong upstream?*

## 6. Session continuity

Zero-loss handoff lives in **`session/`** — a directory of dated drop files (one per session),
not a monolithic journal. Format and lineage: `session/README.md`. The convention is adopted from
`wbrown/janus-datalog`'s per-session `SESSION_SUMMARY` drops and is cicatrix's federated variant of
terrarium's single `SESSIONS.md`.

## 7. Cross-references

- **Agent behavior contract:** `CLAUDE.md` (invariants as disciplines, meta-patterns, reviewer
  failure modes).
- **Design:** `docs/design/cicatrix-reverie-unsigned-paas-integration.md` (Leg A — the bridge),
  `docs/design/cicatrix-static-analysis-on-reverie.md` (the drift arm),
  `docs/design/cicatrix-bridge-epic.spec.md` (epic skeleton).
- **North star:** CER-1369 (reverie = single holistic memory surface).
- **Standard adopted:** `cerebral-work/terrarium` CANON.md + `docs/runbooks/adopt-standards.md`.
- **Method ancestor:** `wbrown/janus-datalog` (agent-discipline layer; cicatrix keeps the *idea* —
  queryable discipline — while dropping the datalog store, 2026-06-18: reverie is the store).
