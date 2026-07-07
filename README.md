<!-- lineage
role: readme
conforms_to: CANON.md
defines: cicatrix (orientation surface)
consumes: CANON.md, CLAUDE.md, SESSIONS.md, docs/sessions/
-->

# cicatrix

> Persistent, queryable memory of fixed bugs + convention-drift detection, with commit-time review gates.

**Ground truth:** [`CANON.md`](CANON.md) (what cicatrix is) · [`CLAUDE.md`](CLAUDE.md) (how an agent
behaves here) · [`SESSIONS.md`](SESSIONS.md) (append-only handoff journal) ·
[`docs/sessions/`](docs/sessions/) (session-fact drop-dir).

An independent **regression-memory + convention-drift** framework. Its approach is drawn from
**Wes Brown's [`wbrown/janus-datalog`](https://github.com/wbrown/janus-datalog)** — specifically that
project's agent-discipline layer: a corpus of structured bug-fix docs distilled into meta-patterns,
plus fail-closed LLM review hooks — bridged into [reverie](https://github.com/cerebral-work/reverie)'s
consolidation substrate.

The premise: an agent (human or LLM) reintroduces a fixed bug because the *memory of past failures*
isn't queryable at authoring time. cicatrix makes it queryable, and gates commits on it.

## Three arms

1. **Bug-memory store** — every fixed bug is a durable structured fact
   `(bug, file, symptom, root-cause, fix-commit, regression-test, meta-pattern)`.
   Stored as markdown (`docs/bugs/grounded/`, human-authorable) *and* projected into a queryable
   temporal store so you can ask: *"does this diff touch a known-bug surface / failure class?"*
2. **Meta-pattern injection** — the corpus rolls up into named recurring rules (`CLAUDE.md`),
   injected **upstream** of an edit (a discipline that generates actions, not a final-checkpoint filter).
3. **Reasoning-audit + commit-gate hooks** (`.claude/hooks/`) — block "premature victory,"
   enforce a green baseline, require a regression test per fix. Plus a **convention-drift scanner**
   (`drift/`) — the static-analysis arm: repo × convention-marker table vs canonical templates.

## Substrate (decided 2026-06-18: "reverie is the store")

- **Fact store:** **reverie** is the single store (the janus-datalog sidecar was dropped for v1).
  The markdown corpus (`docs/bugs/grounded/`) is the source of truth; each bug-fact is projected
  one-way into a reverie observation (`project=cicatrix`) — a regenerable query index.
- **Time-travel:** `query --as-of <commit>` preserves janus's `AsOf(commit)` without a second
  store, by filtering on the fix-commit's git-ancestry.
- **Why reverie:** it's the holistic memory surface (CER-1369); cicatrix is "consolidation applied
  to defects." Full design: `docs/design/cicatrix-reverie-unsigned-paas-integration.md`.

## Status — v0 thin slice (stands up the loop, shallow)

| Arm | v0 state |
|---|---|
| Bug-memory (markdown) | ✅ schema + 2 real seed bugs (`docs/bugs/grounded/`) |
| Meta-pattern injection | ✅ `CLAUDE.md` (rolled-up rules) |
| Convention-drift table | ✅ real data from the 2026-06-16 ~/projects topology survey (`drift/`) |
| Commit-gate hook | ✅ minimal `commit-gate.sh` (green-baseline + premature-victory block) |
| reverie bridge (`record`/`query`) | ✅ Phase 0+1 — `ReverieBridge` projects facts → `POST /observations` and queries `/search`; `--as-of` git-ancestry filter (CER-1374/1375, against local reveried) |
| cloud-served + revenant consumption | ⬜ **next** — Phase 2/3, gated on the reverie cloud deploy (CER-1362 → OPS-271) and revenant (TOD-978) |

## Layout

```
docs/bugs/grounded/            grounded (resolved) one-file-per-bug memory  (+ _SCHEMA.md)
docs/bugs/observed/            observed-but-ungrounded bugs (NOT projected until promoted)
drift/                         convention-drift scans (repo × marker)
CLAUDE.md                      injected meta-patterns + project contract
.claude/hooks/commit-gate.sh   the audit gate
src/                           Rust crate: CLI (record/query/drift/inject/project-meta) + store trait
tests/cli.rs                   CLI behavior suite (every verb + the drift-path invariant)
.cicatrix/establish-baseline.sh  runs the suite; writes baseline-green only on green
.cicatrix/baseline-green       session-local marker (gitignored) the commit-gate requires
```

Remote: `cerebral-work/cicatrix` (private).
