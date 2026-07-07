# FACT_SESSION_DROPDIR_VS_JOURNAL

- **id:** fact:session-dropdir-vs-journal
- **files:** SESSIONS.md, docs/sessions/_SCHEMA.md, CANON.md:§6
- **commit:** PR#5
- **gate:** federation reconciliation (compare against the reference adopter before shipping)
- **meta-pattern:** Two implementations of one fact drift
- **status:** grounded
- **scope:** SESSIONS.md, docs/sessions/**

## Symptom
The first adoption pass created a single `session/<YYYY-MM-DD>-<slug>.md` directory — one markdown
file per session — and called it the whole "session/ drop dir." It matched neither surface the
canonical adopter (`unsigned-gg/unsigned-paas`) actually uses.

## Root cause
**Mental-model error: collapsing two distinct surfaces into one.** "Session continuity" is *two*
things, not one:
1. a **narrative handoff journal** (append-only, newest-on-top) — the continuity story; and
2. a **fact drop-dir** (two-tier `observed/`+`grounded/`, one file per *fact*) — atomic, reusable
   mental-model errors.
The terrarium standard supplies (1) as `SESSIONS.md`; the cicatrix/janus drop-dir pattern supplies
(2). Reading "session/ drop dir format from cicatrix/janus-datalog" as a single dir of per-session
files conflated the journal *into* the drop-dir and lost the per-fact, two-tier shape that makes the
drop-dir queryable. (cicatrix already owns this pattern via `docs/bugs/` — it should not have been
re-derived differently three directories over.)

## Reproduction
Diff the first pass against the reference: `unsigned-paas` ships `SESSIONS.md` **and**
`docs/sessions/{observed,grounded}/ + _SCHEMA.md`; the first cicatrix pass shipped only
`session/<date>.md`. Two surfaces vs one ⇒ drift.

## Resolution
Split into the two canonical surfaces: `SESSIONS.md` (journal) + `docs/sessions/` (fact drop-dir,
full mirror of unsigned-paas, which itself adopted cicatrix's `docs/bugs/` pattern). Deleted the
divergent `session/` dir.

## Lesson
When adopting a federated standard, **diff against the reference implementation before shipping** —
don't re-derive a shared pattern from its name. A pattern that already exists in this repo
(`docs/bugs/`) is the strongest hint of its canonical shape; reuse it, don't reinvent it. (This is
exactly cicatrix meta-pattern #2: two implementations of one fact drift.)
