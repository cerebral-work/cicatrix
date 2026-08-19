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
