# BUG_AUTH_HARDENING_STRANDED_CONSUMERS

- **id:** bug:auth-hardening-stranded-consumers
- **files:** crates/reverie-store/src/http/auth.rs, cicatrix/src/reverie.rs, ~/.local/bin/mem, ~/.claude/hooks/session-bootstrap.sh
- **fix-commit:** 8ef0b81 (reverie #1187 — the contract-test guard; consumer re-wiring tracked under CER-1629)
- **regression-test:** crates/reveried/tests/cicatrix_bridge_contract.rs (5 tests pinning the consumer wire surface through the real router + auth stack)
- **meta-pattern:** Contracts break silently downstream
- **status:** resolved
- **scope:** crates/reverie-store/src/http

## Symptom
After reverie PR #1157 (tenant-namespace auth hardening) deployed 2026-07-18: every local
raw-HTTP consumer broke at once — `mem raw`/`mem ctx` 401 on every read, the session-bootstrap
hook's context-inject and handoff-recovery curls failed **silently** (`curl -sf … || true`),
the cached hook JWT got 403 (empty `proj` claim default-denied), and cicatrix's
`ReverieBridge` local record/query 401'd, making CER-1375's acceptance round-trip
non-functional. The agent-facing MCP path kept working, which masked the breakage further.

## Root cause
A correct security hardening shipped with no inventory of the contract's existing consumers.
The auth layer's behavior was the *de facto* API for four independent clients, none of which
appeared in the changed repo's tests — so the PR was green while every downstream client
broke. The silent-fallback consumer (`|| true` in session-bootstrap) converted breakage into
invisible feature loss. **Mental-model error:** "the contract is what the server documents"
rather than "the contract is what consumers demonstrably depend on" — and hardening framed as
server-side-only when it was an interface change.

## Reproduction
Anonymous `GET /search` against a post-#1157 JWT-mode reveried → 401 where the 2026-06-22
baseline returned 200; token without a `proj` claim → 403. Pinned permanently by the
regression test's `anonymous_bridge_calls_are_denied_not_dropped` (under JWT mode).

## Resolution
The consumer wire surface was pinned *inside the producer's CI*: reverie #1187 adds
`cicatrix_bridge_contract.rs`, exercising cicatrix's literal payloads and queries through the
real router+auth stack (roundtrip, event_id dedupe, AND-mode path precision, anonymous-401
posture, cross-project denial), with the test header cross-referencing `cicatrix/src/reverie.rs`
so a forced change must land in both. Consumer-side re-wiring (mem CLI bearer support,
session-bootstrap surfacing failures instead of swallowing them, per-consumer `proj`-claim
tokens) tracked under CER-1629; anonymous local reads verified restored 2026-08-16.

## Lesson
A contract change ships with its consumer inventory, and each consumer's dependency belongs in
the producer's CI as an executable pin — drift then fails in the PR that introduces it, not in
a downstream repo days later. Corollary: error-swallowing fallbacks (`|| true`) on
infrastructure calls turn contract breaks into silent data/feature loss; degrade loudly.
