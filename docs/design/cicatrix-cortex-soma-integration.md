<!-- lineage
role: design
defines: cicatrix consumption seams in soma (known-bug injection) and cortex (learning loop)
consumes: docs/design/cicatrix-reverie-unsigned-paas-integration.md, CER-1827 spike, soma D2 research
-->

# cicatrix × cortex / reverie / soma — integration map

Status: design, 2026-10-06. Supersedes nothing; extends the bridge epic
(`cicatrix-reverie-unsigned-paas-integration.md`) past Leg A into the two live consumers.
Operator decisions this date: foundation-first scope; semantics **both, sequenced** —
soma known-bug injection is Leg 1, cortex learning loop is Leg 2.

## 0 · Foundation (DONE, this date)

Leg A against local reveried v0.23.0 is fully live again:

- Write path: `record` → `POST /observations`, authed by a standing token at
  `~/.cicatrix/reverie-token` (0600), `sub=cicatrix`, `scope="mcp:read obs:write"`,
  `proj=[cicatrix]`, `aud=$REVERIE_PUBLIC_URL`, signed by the daemon's **current**
  keypair. cicatrix reads the file automatically (`ReverieBridge::from_env`);
  `REVERIE_TOKEN` env still wins. Re-mint on key rotation (symptom: `invalid_bearer`
  on `/health` auth counters).
- Corpus: all 7 grounded bug-facts projected, `project=cicatrix type=bug-fact`,
  idempotent on `event_id` (re-record revises, never duplicates).
- Read path: anonymous `GET /search` is open on 0.23.0; `query <files>` works
  without any token.
- Open reverie-side items: CER-1383 (batch `/search` — one round-trip per query,
  not N), CER-1367 (versioned coord/bugs contract).

## 1 · Leg 1 — soma: known-bug warnings at spawn (FIRST consumer)

**Semantic (the cicatrix north-star):** an agent spawned on a task whose file set
touches a known-bug surface gets the meta-pattern + regression guard inline in its
prompt. Nobody runs `cicatrix query` by hand.

**Seam (soma-os, designed in `blackwall/docs/research/d2-run-context-assembly.md`):**
the assembly window in `blackwall/crates/blackwall-cli/src/run.rs` between
`task::substitute_vars` and `execute_run` (the doc's "step 4.5"). D2 spec'd a
generic reverie `<memory>` block; the cicatrix slice is a specialized second block:

```toml
# tasks/<name>/task.toml
[cicatrix]
paths = ["src/auth/", "crates/store/"]   # else: read_paths + write_paths
limit = 3                                 # default 3
```

Assembly issues `cicatrix query <paths...>` (or the equivalent reverie `/search`
with `project=cicatrix type=bug-fact` — cheaper: no subprocess, one HTTP call per
path until CER-1383 lands) and prepends:

```
<known-bugs>
- BUG_... (meta-pattern): <regression guard> — files: ...
</known-bugs>
```

Serve-side mirror: `blackwall-serve/src/server.rs::resolve_prompt` (dispatch path)
must apply the same assembly or k8s-Job-spawned runs diverge from CLI runs
(meta-pattern: *two implementations of one fact drift* — assemble once, share the fn).

**Failure behavior:** memory surface unavailable → run proceeds WITHOUT the block,
warning logged (recall is advisory, never a spawn blocker).

**Acceptance:** a blackwall run whose task paths touch
`crates/reverie-store/src/embed.rs` starts with the `BUG_EMBED_EMPTY_INPUT_400`
warning in its prompt; a run with no touching paths starts clean; reveried down =
run still spawns.

## 2 · Leg 2 — cortex: settle-outcome learning loop (SECOND consumer)

**Semantic (CER-1827 spike / Cortex Spec 0024):** worker corrections become memory.
One settle outcome → one reveried observation (`type=settle-outcome`,
`topic_key=cortex/settle-outcomes/<source>`, with tags `source`, `action_type`,
`guard_tier`, `operator_decision`); worker context assembly prepends top-K for
the event's source; cicatrix ingests and deduplicates events from reverie,
recording negative verdicts into `docs/sessions/observed/` defect facts.

**Seams (cortex repo, as-built survey 2026-10-06):**
- Write: new `cortex-worker::learning` tokio task, outbox consumer, idempotent on
  `(job_id, outcome)`, feature-flagged `learning`. `ContextLogRow`
  (`crates/cortex-worker/src/models.rs:200`) + migration 0006 exist; no writer yet.
- Read: context assembly → `GET /context/smart?project=cortex` + topic search,
  budget ≤2k tokens of worker prompt.
- Existing auth/guard seam to reuse: `crates/cortex-worker/src/guard.rs`
  `external::ReverieGuard` behind the `reverie-guard` cargo feature — same
  reveried address/token plumbing the learning writer needs.

**Explicit non-goals (spike):** no fine-tuning, no automatic guard-policy mutation,
no prompt self-modification. Corrections enter as *context*; policy stays
operator-ratified. Wave propagation (Path 2, multi-event DAG) is M4b+ and stays
dormant — including the open question of whether wave state lives in cortex
Postgres or cicatrix.

## 3 · What this does NOT unlock yet

- **Cloud-served bridge (CER-1376 Phase 2):** still gated on reveried's cluster
  home (Leg B: CER-1362 → OPS-271, Cygnus after Lyra retirement). Off-laptop
  `record`/`query` waits for authed ingress exposing data routes.
- **revenant orchestration (CER-1889):** revenant's own query+injection path is
  the third consumer; the soma leg proves the pattern revenant should copy
  (assembly-window injection, advisory-on-failure).

## 4 · Ticket anchors

| Leg | Ticket | State entering 2026-10-06 |
|---|---|---|
| Foundation (Leg A) | CER-1374 / CER-1375 | Done; write path re-restored this date |
| reverie batch search | CER-1383 | Backlog (M4) |
| reverie contract versioning | CER-1367 | Backlog |
| soma known-bug injection | CER-2611 | Todo (filed this date) |
| cortex learning loop | CER-1827 (spike written) | Design |
| revenant consumer | CER-1889 | Backlog |
| cloud cutover | CER-1376 | Gated (Leg B) |
