# BUG_LCM_TURN_INDEX_COLLISION_500

- **id:** bug:lcm-turn-index-collision-500
- **files:** crates/reverie-store/src/backends/engram_compat.rs, crates/reverie-store/src/http/lcm_turns.rs
- **fix-commit:** e397c36 (reverie #1205, CER-1643); observability follow-up 0f5f6d9 (#1218, CER-1631/1643)
- **regression-test:** store-level index-collision → `InsertOutcome::Conflict{TurnIndexOccupied}` test + HTTP-level 409-labeling test (reverie #1205)
- **meta-pattern:** Edge cases are real cases
- **status:** resolved
- **scope:** crates/reverie-store

## Symptom
`POST /v1/turns` failed continuously with `lcm_insert_turn failed err=Query returned no rows`
(HTTP 500) on the live daemon 2026-07-18 — session turn capture silently dropping for the
active paas/reverie/blackwall sessions while `/health` reported `db_healthy:true` throughout.
First live read misdiagnosed it as schema-migration drift from the day's wedged-boot deploys;
the actual mechanism was narrower.

## Root cause
`lcm_insert_turn`'s conflict fallback handled exactly one conflict kind — the
`(session_id, content_hash)` dedupe case. A conflict on `UNIQUE(session_id, turn_index)` with
*different* content fell through to a bare `query_row` → "Query returned no rows" → anyhow 500.
The capture hook is fire-and-forget and caches per-session turn-index counters in `/tmp`; the
day's wedged-boot outages drifted those counters, producing a continuous stream of distinct
index collisions. **Mental-model error:** "the" unique constraint — writing the fallback for
the conflict you expect and letting every other constraint on the same table share its error
path; plus trusting client-side counters to stay aligned with server state across outages.

## Reproduction
Insert a turn at `(session_id, turn_index=N)`, then insert different content at the same
`(session_id, N)`: pre-fix the second insert 500s with "Query returned no rows"; post-fix it
returns 409 `error=turn_index_conflict / reason=turn_index_occupied`. (Both covered by the
regression tests in #1205.)

## Resolution
Fallback lookup made `.optional()`; on miss the occupying row is resolved by
`(session_id, turn_index)` (also `.optional()`-guarded, with a distinguishable
invariant-violation message if both lookups miss) and surfaced as
`InsertOutcome::Conflict{TurnIndexOccupied}` → HTTP 409 with an accurate label (the previous
hardcoded `event_id_conflict` label mislabeled non-event_id conflicts). #1218 then put
turn-write failure telemetry on `/health` — closing the "db_healthy:true while every turn
write fails" monitoring hole.

## Lesson
Every UNIQUE constraint on a table is its own edge case: a conflict handler that names one
constraint must either enumerate the others or fail with a message that identifies which
invariant fired — never share a bare not-found path. And a write path whose failures don't
degrade any health signal is unmonitored, whatever the dashboard says.
