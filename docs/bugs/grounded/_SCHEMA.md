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
