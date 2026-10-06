# FACT_JANUS_NEVERZEROVALUE_TYPE_MISMATCH

- **id:** fact:janus-neverzerovalue-type-mismatch
- **commit:** wbrown/janus-datalog@854bf8ef (PR #118); design record `docs/archive/completed/NEVER_ZERO_VALUE.md`
- **meta-pattern:** Type mismatches kill (seed: `BUG_EMBED_EMPTY_INPUT_400`)
- **status:** observed
- **scope:** docs/bugs/grounded/BUG_EMBED_EMPTY_INPUT_400.md

## Symptom
cicatrix's "type mismatches kill" meta-pattern ("a value crossing a boundary in the wrong shape —
empty vs zero vs null — fails silently downstream") is, in cicatrix's own corpus, one seed bug.
Upstream, the same class recurred structurally enough that janus-datalog added a **schema-level
enforcement** for it.

## Root cause
The mental-model error the pattern names is that a type's zero value reads as "a value was set"
when it often means "nothing was set." janus's store made that explicit: absence is no datom, and
`:db/neverZeroValue` declares per-attribute that the type's zero (`""`, `0`, `0.0`, `false`, nil)
is *not a value* — enforced at write time, with the struct writer treating a zero scalar as
"not set" (janus-datalog@854bf8ef, commit message; corroborating pin at 6d6dfa3e).

## Reproduction
Read janus `docs/archive/completed/NEVER_ZERO_VALUE.md` beside
`docs/bugs/grounded/BUG_EMBED_EMPTY_INPUT_400.md`: the former is the class enforced at the store
boundary; the latter is the class caught downstream as a 400.

## Resolution
None needed in cicatrix — this is corroboration, not a defect. The fact exists so the next
weighing of "is this pattern general enough to keep" has a second, independent instance.
Promotion to grounded = a reviewer confirming the janus citation against the upstream repo.

## Lesson
A meta-pattern that a second project escalates to a schema constraint is a pattern worth
enforcing at a boundary, not just catching downstream: **validate at the seam; choose an explicit
empty representation.** (That is the seed bug's own lesson, now with upstream mass behind it.)
