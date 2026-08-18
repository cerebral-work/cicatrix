# BUG_REVERIED_UNBOUNDED_SHUTDOWN_SIGKILL

- **id:** bug:reveried-unbounded-shutdown-sigkill
- **files:** crates/reveried/src/main.rs, crates/reveried/src/dream_scheduler.rs, crates/reverie-dream/src/runner.rs
- **fix-commit:** 49d4fbe (reverie #1192, CER-1628)
- **regression-test:** dream cancel-flag tests (pre-set cancel runs zero phases + reports cancelled; false flag inert) — reverie dream/scheduler suite
- **meta-pattern:** Bound every wait
- **status:** resolved
- **scope:** crates/reveried

## Symptom
Every `systemctl --user restart reveried` issued while a dream cycle was in flight degraded to
`stop-sigterm timed out → SIGKILL`. Observed live 2026-07-18 as a three-SIGKILL journal
signature (05:05, 05:12, 05:22), plus a fourth on a wedged boot — the daemon was SIGKILLed
mid-write on a 5.4 GB SQLite store as the *routine* restart path. HTTP also starved during the
same windows (`/health` and `/search` timing out while a post-`scan` dream phase ran silently
for 9+ minutes).

## Root cause
The implicit tokio `Runtime` drop at the end of the serve arm waits forever for outstanding
`spawn_blocking` work, and a mid-consolidate dream phase on a multi-GB DB runs for minutes
holding the write lock. Nothing in the dream cycle observed cancellation, so SIGTERM had
nothing to cancel — the process could only be killed. **Mental-model error:** treating
shutdown as "the process ends" rather than as a code path with its own budget; an implicit
drop is still an unbounded wait, it just doesn't look like one in the source.

## Reproduction
Start reveried against a large store, let the boot-triggered dream cycle enter a
post-`scan` phase, `systemctl --user stop reveried`, observe `stop-sigterm timed out` after
`TimeoutStopSec` and SIGKILL. (Regression tests exercise the cancel flag directly instead.)

## Resolution
`DreamOptions.cancel` (`Arc<AtomicBool>`) checked at every phase boundary with
`DreamReport.cancelled` marking early stop; `DreamScheduler.request_cancel()` as a terminal
shutdown flag injected into every trigger path; serve arm calls `request_cancel()` then
`rt.shutdown_timeout(15s)` instead of the unbounded implicit drop. Companion fix 7e77b63
(CER-1630) made the open-time `busy_timeout` actually govern boot lock contention (`BEGIN
IMMEDIATE` instead of `DEFERRED` in `migrate()` — the DEFERRED snapshot made the armed busy
handler structurally unreachable on write-lock upgrade).

## Lesson
Every teardown and every wait needs an explicit budget and a cancellation point — including
the implicit ones (runtime drops, in-flight background work). A guard that is armed but
structurally unreachable (CER-1630's busy handler under a stale DEFERRED snapshot) is
indistinguishable from a working guard until the outage: verify guards engage, not that they
exist.
