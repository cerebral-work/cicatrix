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

### Fixed
- Stale `docs/bugs/resolved/` → `docs/bugs/grounded/` references in the integration and
  bridge-epic design docs (the corpus was renamed; the docs still named the old tier).
- Integration design §2.1 gains a dated upstream note: janus generalized `AsOf` to a
  version-vector Frontier and shipped branch-Fork-from-Snapshot (PR #119); cicatrix's
  git-ancestry `--as-of` remains the v1 mechanism.
- Drift design §1.1 gains a D0 as-built note (the pre-D0 "prints a path" table no longer
  describes the shipped `drift scan`).

### Added
- Stochastic-failure extension to the bug-doc schema (`docs/bugs/grounded/_SCHEMA.md`), ported
  from `wbrown/janus-datalog`'s occurrence-log genre
  (`docs/bugs/resolved/BUG_WASM_STORAGE_GC_BAD_POINTER_CRASH.md`, finalized at janus `6412d6c2`):
  optional `- **reproducer:**` metadata (the turnkey trigger, parsed onto `BugFact` and carried
  into the reverie projection content), `## Occurrence log` (one numbered row per sighting),
  `## Sanctioned reruns` (rerun governance with a named end condition; absent = nothing
  sanctioned), and Resolution guidance for the fix carrier + closing invariant. Lifecycle:
  observed/ = open (accruing occurrences), grounded/ = resolved (sanction ended).

### Removed
- Agent Jury's `Auto-merge on approval` step, which ran `gh pr merge --squash --delete-branch`
  on an `approved` model verdict (operator ruling 2026-08-19, CER-2077). Merging main is
  operator-gated and the merge-style SOP forbids squash. The jury is advisory: it comments and
  labels, a human merges.

### Fixed
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
- `tests/agent_jury_workflow.rs` — regression suite that extracts the real `run:` blocks from
  `.github/workflows/agent-jury.yml` and executes them against stubbed `curl`/`gh`, so CI shell
  logic is covered by the green-baseline gate instead of being verified by reading run logs.
- `docs/bugs/grounded/BUG_JURY_GUARD_UNREACHABLE_UNDER_SET_E.md` — BugFact for the above.

### Changed
- `README.md` — added a lineage block and a ground-truth pointer row (CANON / CLAUDE / SESSIONS /
  docs/sessions).
- Adopted the `cerebral-work/terrarium` federated-node standard: lineage blocks on the root context
  set, feature-branch → PR → human-merge process (see `CANON.md`).
