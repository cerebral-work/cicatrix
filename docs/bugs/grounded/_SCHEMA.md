# Bug-doc schema

One file per fixed bug: `BUG_<SHORT_SLUG>.md`. Fixed sections (used by `cicatrix record` to
project a fact `(bug, file, symptom, root-cause, fix-commit, regression-test, meta-pattern)`
into reverie). Keep it narrative — the *mental-model error* is the point.

This is the **grounded** tier (`docs/bugs/grounded/`, renamed from `resolved`): canonical, projected
facts. The sibling **observed** tier (`docs/bugs/observed/`) holds ungrounded bugs that `cicatrix
record` refuses to project until they're promoted to grounded.

```
# BUG_<SLUG>

- **id:** bug:<slug>
- **files:** path/to/file.rs:LINE, ...
- **fix-commit:** <sha or PR#>
- **regression-test:** <test name / path>
- **meta-pattern:** <one of CLAUDE.md's named classes>
- **status:** resolved | active
- **scope:** <optional crate/path glob>           # blast-radius (see below)
- **do-not-generalize:** true                      # optional; omit unless narrow
- **reproducer:** <turnkey command>                # optional; stochastic bugs (see below)

## Symptom
What was observed (the failure, not the cause).

## Root cause
The actual mechanism — and the **mental-model error** that produced it.

## Reproduction
Minimal failing case (ideally the regression test).

## Resolution
What changed, and why it's correct.

## Lesson
The upstream discipline that would have prevented the whole class.
```

## Optional fields

- **Scope** (`- **scope:** <glob>`) — the blast radius for this fact's meta-pattern: a crate or
  path prefix (e.g. `crates/reverie-store`). `cicatrix inject --target <path>` emits only patterns
  whose scope matches the target. **Optional**; when absent the effective scope is the set of parent
  directories of the fact's `files`. Existing seed docs (no `scope`) parse unchanged.
- **do-not-generalize** (`- **do-not-generalize:** true`) — marks a fact too narrow to promote to a
  project-wide rule. Such facts are excluded from the injected / `project-meta` meta-pattern block.
  Accepts `true` / `yes` / `1`; omit the line otherwise.

## Schema integrity (:db/neverZeroValue)

Cicatrix enforces Datomic/Janus schema discipline: absence of an attribute is distinct from setting an explicit empty or zero value.
- **Never zero value:** Empty strings (`""` or `''`), whitespace-only values, empty table cells, and empty list items are forbidden.
- **Optional attributes:** To leave an optional attribute unset (such as `scope`, `reproducer`, or `do-not-generalize`), omit the line completely. Do NOT supply an empty value (for example `- **scope:** ""` or `- **scope:**` causes a parse error).
- **Collections:** Comma-separated list attributes (such as `files`) must contain non-empty paths only (for example `foo.rs, , bar.rs` is rejected).
- **Section prose:** Defined sections (`Symptom`, `Root cause`, `Reproduction`, `Resolution`, `Lesson`) must contain non-empty explanatory content.

## Stochastic failures (the occurrence-log extension)

Ported 2026-10-05 from `wbrown/janus-datalog`
(`docs/bugs/resolved/BUG_WASM_STORAGE_GC_BAD_POINTER_CRASH.md`, finalized at commit `6412d6c2`).
The base schema assumes a deterministic repro + regression test. A **stochastic** bug — flaky,
layout- or timing-sensitive, not reliably reproducible on demand — records differently. Use this
extension when a single reproduction run cannot prove or disprove the bug.

Optional additions to the base doc:

- **`- **reproducer:** <command>`** (metadata) — the *turnkey* trigger, if one exists: the env
  var, flag, or load shape that makes the stochastic failure deterministic (janus's was
  `GOGC=1 …`: constant GC turned a ~25% crash into a 4/4 reproducer). Parsed onto the fact and
  carried into the reverie projection's content. Omit when no turnkey trigger is known.
- **`## Occurrence log`** (section) — one numbered row per sighting, appended as the bug recurs:
  `| n | date | config/context | result |`. Rows carry the *signature* facts (poison values,
  discovery shapes, goroutine/test names) and cross-occurrence pattern notes ("the runs-13–16
  value recurring in a third distinct binary"). The log is what lets a later reader recognize the
  same bug from a single new sighting.
- **`## Sanctioned reruns`** (section) — the rerun-governance policy while the bug is live:
  which exact signature a CI rerun is sanctioned for, under what evidence, and the **end
  condition** ("the sanction ends when the fix carrier lands"). A doc without this section
  sanctions nothing — reruns of a red gate stay fail-closed by default.
- **Resolution** gains, for bugs fixed upstream or by a carrier change: the **fix carrier**
  (the version/pin/CL that carries the fix and how the repo enforces it, e.g. "go.mod pins 1.26.8;
  CI reads the toolchain from go.mod") and the **closing invariant** — one sentence of the form
  *"signature S on carrier ≥ V is a NEW bug, not this one"*, so the resolved doc can never mask a
  recurrence.

**Lifecycle.** An actively-occurring stochastic bug lives in `docs/bugs/observed/`
(`status: active`), accruing occurrence rows; it is *not* projected. On resolution (fix carrier
landed + gate or regression guard in place + sanction ended), promote to `grounded/` with the
closing invariant. This mirrors janus's `docs/bugs/` (open) → `docs/bugs/resolved/` move onto
cicatrix's own tiers: observed = open, grounded = resolved.

