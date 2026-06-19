---
project: Reverie
team: Cerebral Work Institute
labels: [cicatrix, memory-surface]
related: [CER-1369]
milestone:
---

# cicatrix → reverie bug-fact bridge (Leg A)

> **Filed 2026-06-18 (Reverie project, Cerebral Work Institute):** Epic **CER-1373** (related CER-1369)
> → **CER-1374** Phase 0 (unblocked) → **CER-1375** Phase 1 (blocked-by CER-1374) →
> **CER-1376** Phase 2+3 (blocked-by CER-1375, related TOD-978). Phases are children of CER-1373.

Epic spec for wiring cicatrix's regression-memory into reverie as the single memory surface.
Design of record: `cicatrix/docs/design/cicatrix-reverie-unsigned-paas-integration.md`.
Decisions (2026-06-18 operator interview): reverie is the only store (janus-datalog dropped);
namespace `project="cicatrix"`; AsOf(commit) preserved via git-ancestry (built in v1); the
networked bridge is gated on the cloud deploy path (CER-1362 → OPS-271), but Phase 0 is unblocked.

## Part 1: Epic — cicatrix bug-fact bridge

cicatrix makes the memory of past bug-fixes queryable at authoring time and is one **content
producer** for the reverie memory surface (CER-1369). Every fixed bug → a reverie observation
(`project=cicatrix`); `cicatrix query <diff>` → a reverie `/search` answering "does this diff touch
a known-bug surface?". The markdown corpus (`docs/bugs/resolved/`) is the source of truth; reverie
is a regenerable one-way projection.

**Scope of the epic:** Phases 0–3 in the design doc (§4). This issue tracks the epic; child issues
carry the phases. **Out of scope:** the reverie cloud deploy itself (Leg B — CER-1362, OPS-271,
reverie-cloud-roadmap R0–R5) and revenant orchestration internals (TOD-978) — this epic consumes
those, it does not build them.

**Acceptance:** Phase 1 demoable (record a bug → query a touching diff → it surfaces) against a
reachable reveried; Phase 3 closes the loop (revenant injects known-bug warnings at agent-spawn).

## Part 2: Phase 0 — cicatrix-side prep (UNBLOCKED)

No reveried required; runs in parallel with the cloud leg.

- `src/bug_md.rs`: parse `docs/bugs/resolved/*.md` per `_SCHEMA.md` → `BugFact`. Pure, no network.
- Projection builder: `BugFact` → reverie observation payload (§3.1 schema:
  `project=cicatrix`, `title=<slug>`, `content=<rendered fact>`, `tags=[file paths, meta-pattern,
  fix-commit]`, `topic_key=<meta-pattern>`). Unit-tested against a fake.
- Drop `JanusStore` framing from `src/store.rs`, README substrate block, CLAUDE.md; make
  `ReverieBridge` the sole planned `BugStore` impl.
- `cicatrix record` writes markdown + builds the fact + re-rolls meta-patterns today; prints the
  would-be observation payload instead of POSTing (no premature "wired" claim).

**Acceptance:** `cargo test` green via `.cicatrix/establish-baseline.sh`; a recorded seed bug yields
the correct observation payload, asserted in a test; both existing seed bugs parse cleanly.

## Part 3: Phase 1 — wire the bridge (GATED: reveried reachable)

Blocked on a reachable reveried — local daemon up, or Leg B R0→R1.

- Implement `ReverieBridge: BugStore` HTTP client: `record` → `POST /observations`; `query` →
  `GET /search` (project=`cicatrix`), endpoint from `REVERIE_URL`, bearer from env.
- `cicatrix query --as-of <commit>`: post-filter results by `fix-commit` git-ancestry
  (`git merge-base --is-ancestor`). Test the §2.1 edge cases: non-ancestor / unborn repo /
  inclusive boundary.
- Verify reverie `/search` can tag-filter precisely for file-path exact-match; if short, file a
  reverie-side issue (do not block v1 on it — fall back to full-text over `content`).

**Acceptance:** record a seed bug → `query` a diff that touches its file → the bug surfaces with its
meta-pattern + regression test; `--as-of` correctly includes/excludes by ancestry; idempotent
re-record updates rather than duplicates.

## Part 4: Phase 2 + Phase 3 — cluster-served & revenant consumption (GATED, future)

Placeholder for the later phases; spec'd at phase start, not before.

- **Phase 2** (gated: Leg B R4 authed ingress + OPS-271): point cicatrix at the cluster reveried
  over authed ingress; `query` works off-laptop through auth.
- **Phase 3** (gated: TOD-978): `cicatrix inject` becomes a live reverie recall — revenant injects
  "known-bug surfaces near your task" into agent context at spawn. Closes the CER-1369 loop.
