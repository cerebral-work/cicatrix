<!-- lineage
role: changelog
conforms_to: CANON.md §4; cerebral-work/terrarium docs/RELEASE.md
defines: Changelog
consumes: CANON.md
-->

# Changelog

All notable changes to cicatrix are recorded here. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versioning is
[SemVer](https://semver.org/spec/v2.0.0.html). User-facing changes land an `[Unreleased]` entry.

## [Unreleased]

### Added
- Durable signal architecture and human review gates for workflow executions (CER-2757). Implements `Signal<OperatorVerdict>` and `DurableSignal<T>` primitives with idempotency key deduplication. Backs signal deliveries with SQLite `workflow_signal_ledger` table and `autumn-harvest-sqlite` runtime integration. Supports mid-flight workflow parking (`WAITING_SIGNAL(...)`) with zero CPU consumption until operator sign-off. Adds review gate workflow (`review_gate_workflow`) and `--require-review` flag to regression triage. Exposes `cicatrix workflow signal <id> <verdict>` and `cicatrix workflow run review-gate <target_ref>` CLI commands.
- Autumn Harvest durable workflow engine integration for regression triage and convention audit (CER-2756). Implements deterministic event-sourced workflows using embedded SQLite persistence. Adds RegressionTriageWorkflow to ingest test failures, bisect commits with a strict three-step cap, and isolate minimal reproducers with :db/neverZeroValue validation. Adds ConventionAuditWorkflow to scan multi-repo convention drift across estate targets. Enforces 50,000 events and 50 MiB history limits, 10-minute activity timeouts, and exponential retry backoff. Exposes `cicatrix workflow run <triage|audit>`, `cicatrix workflow list`, and `cicatrix workflow status` CLI commands.
- Branch snapshot and forking engine for agent worktrees and DeltaDB virtual threads (CER-2755). Implements `cicatrix branch <fork|drop|settle|list|path>` commands with SQLite `VACUUM INTO ?1` isolation. Provides lock-free snapshot deletion without long-lived trunk write locks, validates `:db/neverZeroValue` schema integrity constraints during settle, and supports isolated speculative branch recording and querying via `--branch` and `CICATRIX_BRANCH`.
- Multi-replica version-vector Frontier engine with Lamport clock causality tracking and causal query filtering (CER-2754). Implements `{ReplicaID -> LamportTimestamp}` vector frontiers (`src/frontier.rs`), Git multi-head causal filtering (`src/gitf.rs`), `--frontier <vector>` CLI evaluation (`src/main.rs`), optional `frontier` attribute parsing in bug-doc markdown (`src/bug_md.rs`), SQLite persistence schema migration (`src/store/sqlite.rs`, `migrations/0001_initial_schema.sql`), and `:db/neverZeroValue` schema integrity validation.
- Embedded SQLite storage engine (`store::SqliteStore`) integrating `autumn-harvest-sqlite` runtime invariants and SQLite WAL mode (CER-2753). Implements local transactional persistence for regression facts, files, and occurrences before asynchronous Reverie projection, initial migration `migrations/0001_initial_schema.sql`, and `:db/neverZeroValue` schema integrity validation.
- EARS specification methodology and headless-v1 schema adopted from cerebral research lane. Added `.ears-spec` root marker, `standards/ears/` syntax guide and cleanroom template, `specs/` catalog with initial specification `specs/0001-cicatrix-regression-db-provider.md`, and `xtask` dual-mode validator with unit tests and repo-level assertion.
- Schema integrity enforcement (`:db/neverZeroValue`) in `src/bug_md.rs` and documented in `docs/bugs/grounded/_SCHEMA.md` (CER-2751). Metadata with empty strings, empty files list items, empty table cells, and missing mandatory sections fail closed with explicit `ParseError`.
- Stochastic failure occurrence log parsing and data structures in `src/store.rs` and `src/bug_md.rs` (CER-2752). Parses markdown tables (`| n | date | config | result |`) into `OccurrenceEntry` and `StochasticSpec`.
- Review reasoning, edit hooks, and deterministic test evidence discovery (CER-2750). Ports upstream Janus review hooks (`.claude/hooks/`), adds full transcript authorization ledger parser (`src/hooks/auth_ledger.rs`), and implements AST-based test reference verifier (`src/hooks/test_evidence.rs`).
- `docs/design/cicatrix-regression-db-provider-roadmap.md` — architectural roadmap and specification for evolving cicatrix into an estate-wide Regression Database Provider across 5 phases (Phase 0 to Phase 4). Integrates temporal frontiers, snapshot database forking, and `:db/neverZeroValue` schema integrity (`wbrown/janus-datalog`), event-sourced durable workflows with `autumn-harvest-sqlite` WAL storage and deduplicated durable signals (`autumn-foundation/autumn-harvest`), and reversibility-gated autonomy, canary tripwires, and earned autonomy trust ladders (`wheelhorsedev/nexus`).
- `docs/bugs/grounded/BUG_YAML_ANCHOR_REPARENTS_SIBLINGS.md` — grounded bug-fact + new
  meta-pattern "A text-anchored edit is not a structural edit" (scripted YAML anchor edit
  silently re-parented siblings; Helm rendered empty strings; all gates passed). First bug
  filed from the estate's infra side. Regression guard in revenant CI. (PR #20)
- `ReverieBridge::from_env` falls back to a standing credential file at
  `~/.cicatrix/reverie-token` (0600) when `REVERIE_TOKEN` is unset — restores the
  `record` write path against auth-hardened reveried (post-#1157, CER-1629) without
  requiring an export in every shell/hook. Token shape (verified against reveried's
  `cicatrix_bridge_contract` test): `reveried token mint --sub cicatrix
  --scope "mcp:read obs:write" --proj cicatrix --aud <REVERIE_PUBLIC_URL>`, signed with
  the daemon's *current* keypair (the June r0 pem no longer matches — key rotation).
- `docs/sessions/observed/FACT_JANUS_NEVERZEROVALUE_TYPE_MISMATCH.md` — observed-tier drop:
  janus's `:db/neverZeroValue` (janus-datalog@854bf8ef) is an upstream instance of cicatrix's
  "type mismatches kill" meta-pattern, corroborating its generality.
- Stochastic-failure extension to the bug-doc schema (`docs/bugs/grounded/_SCHEMA.md`), ported
  from `wbrown/janus-datalog`'s occurrence-log genre
  (`docs/bugs/resolved/BUG_WASM_STORAGE_GC_BAD_POINTER_CRASH.md`, finalized at janus `6412d6c2`):
  optional `- **reproducer:**` metadata (the turnkey trigger, parsed onto `BugFact` and carried
  into the reverie projection content), `## Occurrence log` (one numbered row per sighting),
  `## Sanctioned reruns` (absent = nothing sanctioned, reruns stay fail-closed), and Resolution
  guidance for the fix carrier + closing invariant. Lifecycle: observed/ = open (accruing
  occurrences, unprojected) → grounded/ = resolved (sanction ended).

### Removed
- `.github/workflows/agent-jury.yml` + `tests/agent_jury_workflow.rs` — AI review gate
  removed per operator directive 2026-10-06. Regression CI (fmt/clippy/deny/test/secrets)
  is untouched. (PR #19)
- Agent Jury's `Auto-merge on approval` step, which ran `gh pr merge --squash --delete-branch`
  on an `approved` model verdict (operator ruling 2026-08-19, CER-2077). Merging main is
  operator-gated and the merge-style SOP forbids squash. The jury is advisory: it comments and
  labels, a human merges.

### Fixed
- Stale `docs/bugs/resolved/` → `docs/bugs/grounded/` references in the integration and
  bridge-epic design docs (the corpus was renamed; the docs still named the old tier).
- Integration design §2.1 gains a dated upstream note: janus generalized `AsOf` to a
  version-vector Frontier and shipped branch-Fork-from-Snapshot (PR #119); cicatrix's
  git-ancestry `--as-of` remains the v1 mechanism.
- Drift design §1.1 gains a D0 as-built note (the pre-D0 "prints a path" table no longer
  describes the shipped `drift scan`).
- Agent Jury CI gate no longer fails without a verdict (CER-2077). Under `set -euo pipefail`,
  `jq` exiting 5 on malformed input aborted the review step at the capture assignment, making
  the `review_failed` guards below it unreachable; the `if: always()` post step then died on a
  missing parsed-JSON file. A gateway error or a model answering in prose now produces a
  "Review Failed" comment and label instead of an uninterpretable red gate.

### Added
- `CANON.md` — ground-truth charter (what cicatrix is), with a terrarium-style lineage block;
  separate from `CLAUDE.md` (the agent behavior contract).
- `SESSIONS.md` — terrarium append-only handoff journal (narrative session continuity).
- `docs/sessions/{grounded,observed}/` + `_SCHEMA.md` — session-fact drop-dir (one file per fact,
  two-tier observed→grounded); the session sibling of `docs/bugs/`. Mirrors `unsigned-paas`.
- `CHANGELOG.md` — this file.

### Changed
- `README.md` — added a lineage block and a ground-truth pointer row (CANON / CLAUDE / SESSIONS /
  docs/sessions).
- Adopted the `cerebral-work/terrarium` federated-node standard: lineage blocks on the root context
  set, feature-branch → PR → human-merge process (see `CANON.md`).
