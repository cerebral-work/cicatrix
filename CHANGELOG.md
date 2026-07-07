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
