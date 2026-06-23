<!-- lineage
role: convention
conforms_to: CANON.md §6
defines: Session-Drop, Drop-Dir-Format
depends_on: wbrown/janus-datalog (docs/archive/<YYYY-MM>/SESSION_SUMMARY_<date>.md — per-session drops)
consumes: CANON.md, CLAUDE.md
-->

# session/ — zero-loss handoff drop dir

One **drop file per session**, so the next session (human or agent) rehydrates fully. This is the
**directory** form of a handoff journal — adopted from `wbrown/janus-datalog`'s per-session
`SESSION_SUMMARY` drops — chosen over a single monolithic `SESSIONS.md` (terrarium's form) so
sessions never contend on one file and each drop is an independent, reviewable artifact.

## Format

- **Filename:** `session/<YYYY-MM-DD>-<kebab-slug>.md` (the date the session started; slug = the
  thread). Multiple drops per day are fine — they sort lexically.
- **Newest is its own file**, not a prepend to an existing one. Never rewrite a prior drop; a drop
  is immutable once the session ends.
- Each drop carries a `lineage` block (`role: session`).

## What a drop captures

`## Objective` · `## Shipped` · `## Decisions` · `## Open threads (park-don't-drop)` ·
`## How to continue`. Surface bad news at the same prominence as good. No AI attribution.

## Skeleton

```markdown
<!-- lineage
role: session
conforms_to: session/README.md
-->

# Session — <date> · <thread>

## Objective
<one or two lines: what this session set out to do>

## Shipped
- <concrete, landed outcomes — PRs, commits, files>

## Decisions
- <decision> — <why>

## Open threads (park-don't-drop)
- <thread> — <next action / blocker>

## How to continue
<the single natural next step for the following session>
```
