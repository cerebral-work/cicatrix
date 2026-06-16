# Bug-doc schema

One file per fixed bug: `BUG_<SHORT_SLUG>.md`. Fixed sections (used by `cicatrix record` to
project a fact `(bug, file, symptom, root-cause, fix-commit, regression-test, meta-pattern)`
into the janus-datalog store). Keep it narrative — the *mental-model error* is the point.

```
# BUG_<SLUG>

- **id:** bug:<slug>
- **files:** path/to/file.rs:LINE, ...
- **fix-commit:** <sha or PR#>
- **regression-test:** <test name / path>
- **meta-pattern:** <one of CLAUDE.md's named classes>
- **status:** resolved | active

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
