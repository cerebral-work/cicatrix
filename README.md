# cicatrix

> Persistent, queryable memory of fixed bugs + convention-drift detection, with commit-time review gates.

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
   Stored as markdown (`docs/bugs/resolved/`, human-authorable) *and* projected into a queryable
   temporal store so you can ask: *"does this diff touch a known-bug surface / failure class?"*
2. **Meta-pattern injection** — the corpus rolls up into named recurring rules (`CLAUDE.md`),
   injected **upstream** of an edit (a discipline that generates actions, not a final-checkpoint filter).
3. **Reasoning-audit + commit-gate hooks** (`.claude/hooks/`) — block "premature victory,"
   enforce a green baseline, require a regression test per fix. Plus a **convention-drift scanner**
   (`drift/`) — the static-analysis arm: repo × convention-marker table vs canonical templates.

## Substrate (decided 2026-06-16: "datalog store + reverie bridge")

- **Fact store:** `wbrown/janus-datalog` (Datomic-style EAV + immutable history + time-travel
  `AsOf(commit)`), run as a **sidecar** binary that this Rust crate drives. Lets you query the
  bug graph *as of any past commit*.
- **Bridge:** facts mirror into reverie (dream consolidation already has a `coord/bugs` family);
  cicatrix is "consolidation applied to defects."

## Status — v0 thin slice (stands up the loop, shallow)

| Arm | v0 state |
|---|---|
| Bug-memory (markdown) | ✅ schema + 2 real seed bugs (`docs/bugs/resolved/`) |
| Meta-pattern injection | ✅ `CLAUDE.md` (rolled-up rules) |
| Convention-drift table | ✅ real data from the 2026-06-16 ~/projects topology survey (`drift/`) |
| Commit-gate hook | ✅ minimal `commit-gate.sh` (green-baseline + premature-victory block) |
| janus-datalog store + reverie bridge | ⬜ **next** — `src/store.rs` defines the trait + sidecar contract; v0 uses a local fallback |

## Layout

```
docs/bugs/{resolved,active}/   structured one-file-per-bug memory  (+ _SCHEMA.md)
drift/                         convention-drift scans (repo × marker)
CLAUDE.md                      injected meta-patterns + project contract
.claude/hooks/commit-gate.sh   the audit gate
src/                           Rust crate: CLI (record/query/drift/inject) + store trait
tests/cli.rs                   CLI behavior suite (every verb + the drift-path invariant)
.cicatrix/establish-baseline.sh  runs the suite; writes baseline-green only on green
.cicatrix/baseline-green       session-local marker (gitignored) the commit-gate requires
```

Not yet a git remote — local only pending sign-off.
