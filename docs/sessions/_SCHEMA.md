# Session-fact drop-dir schema

One file per fact: `FACT_<SHORT_SLUG>.md`. This is the **session** sibling of `docs/bugs/` — same
two-tier drop-dir pattern (cicatrix is its origin; `wbrown/janus-datalog` the method ancestor),
but for facts a *session* surfaces that aren't formal code bugs: gotchas, non-obvious config
truths, process/mental-model errors, recurring failure classes. Keep it narrative — the
**mental-model error** is the point, not the symptom.

Two tiers:

- **observed/** (`docs/sessions/observed/`) — ungrounded drops. Cheap to write; the inbox.
- **grounded/** (`docs/sessions/grounded/`) — canonical facts, promoted from observed once verified
  (reproduced, fixed, or confirmed against reality). Only grounded facts are safe to cite as canon.

```
# FACT_<SLUG>

- **id:** fact:<slug>
- **files:** path/to/file:LINE, ...          # the surface the fact lives on (omit if process-only)
- **commit:** <sha or PR#>                    # where it was established/fixed (omit if observed-only)
- **gate:** <CI gate / hook>                  # the check that catches regressions (omit if none yet)
- **meta-pattern:** <one of CLAUDE.md's named classes, or a new one>
- **status:** grounded | observed
- **scope:** <optional path glob>             # blast-radius; absent ⇒ parent dirs of `files`
- **do-not-generalize:** true                 # optional; omit unless too narrow to promote

## Symptom
What was observed (the failure / surprising behaviour, not the cause).

## Root cause
The actual mechanism — and the **mental-model error** that produced it.

## Reproduction
Minimal case (ideally a command, render, or the gate that shows it).

## Resolution
What changed, and why it's correct.

## Lesson
The upstream discipline that would have prevented the whole class.
```

## Tiers + promotion
A drop starts in `observed/`. Promote to `grounded/` only when verified — the fix landed and a gate
guards it, or the truth is confirmed against reality. Mirrors `docs/bugs/` promotion exactly.

## Relationship to docs/bugs/
`docs/bugs/` = formal code defects with regression tests, projected to reverie by `cicatrix record`.
`docs/sessions/` = broader session-surfaced facts (process, config, mental-model). A session-fact
that turns out to be a reproducible code bug graduates to a `docs/bugs/` entry.
