---
status: complete
created: '2026-10-10'
tags: [diagnose, auto-authoring, bug-facts, hypotheses-fork, agent-afk, ears]
priority: high
scope: [core, diagnose, mcp, cli]
---

# 0002-diagnose-auto-authoring

## Requirements

### Context and user stories

Cicatrix records and queries historical regression facts to prevent recurrent software bugs.
Authoring bug facts currently requires manual extraction of root causes, regression tests, and mental-model errors.
Engineers and autonomous agents require an automated front-end that investigates failures and generates structured facts.
This specification defines the auto-authoring subsystem (`cicatrix diagnose`) that forks root-cause hypotheses and produces schema-compliant bug facts in the observed tier.

### Functional requirements

* **Ubiquitous:** The diagnose subsystem shall emit candidate bug documents conforming to Datomic `:db/neverZeroValue` schema integrity constraints.
* **Ubiquitous:** The diagnose subsystem shall write ungrounded bug facts to the observed corpus tier under `docs/bugs/observed/`.
* **Event-driven:** When a target failure is provided, the subsystem shall fork N parallel root-cause hypotheses across distinct failure categories.
* **State-driven:** While evaluating candidate hypotheses against failure logs, the judge engine shall rank hypotheses by evidence confidence scores.
* **Unwanted behavior:** If a failure log contains empty or zero-value fields, then the parser shall reject the input before hypothesis generation.
* **Optional feature:** Where an explicit output path is specified, the CLI shall write the formatted bug document to the requested location.
* **Complex form:** While operating in offline mode, when mock hypotheses are omitted, the heuristic generator shall synthesize divergent hypotheses deterministically.

### Non-functional constraints

* **Ubiquitous:** The build and test harness shall execute all cargo commands with `-j 2` concurrency.
* **Ubiquitous:** The hypothesis forking engine shall support a concurrency bound between 1 and 5 parallel branches.
* **Ubiquitous:** The generated markdown documents shall parse cleanly through `bug_md::parse_str` without warnings.

## Design

### Architecture

The diagnose subsystem comprises four primary components:
1. `Target Normalizer`: Extracts test signatures, suspected source files, and failure categories from raw compiler logs or test outputs.
2. `Hypothesis Forker`: Spawns N divergent root-cause hypotheses (implementation defects, test specification errors, concurrency/state bounds).
3. `Convergence Judge`: Ranks hypotheses based on diagnostic evidence, synthesizes mental-model errors, and derives the winning diagnosis.
4. `BugFact Builder & Serializer`: Formats the converged diagnosis into a markdown document conforming to `docs/bugs/grounded/_SCHEMA.md`.

```text
Failure Log / Repro
       |
       v
+-------------------+
| Target Normalizer |
+-------------------+
       |
       v
+-------------------+
| Hypothesis Forker | ---> [Hypothesis 1: Impl defect]
|   (N branches)    | ---> [Hypothesis 2: Test defect]
+-------------------+ ---> [Hypothesis 3: State defect]
       |
       v
+-------------------+
| Convergence Judge | (Confidence scoring & evidence synthesis)
+-------------------+
       |
       v
+-------------------+
|  BugFact Builder  | ---> docs/bugs/observed/BUG_<SLUG>.md
+-------------------+
```

### State machines and invariants

* **Corpus Tier Invariant:** Diagnosed bug facts shall enter `docs/bugs/observed/` with `status: active`.
* **Zero Policy Injection Invariant:** Diagnosed facts in the observed tier shall never project into `CLAUDE.md` meta-patterns or reverie active memory.
* **Attribution Notice:** Code ported from `agent-afk` designs shall carry Apache-2.0 design attribution.

### Error containment

All failure log ingestion errors, empty targets, and IO failures shall be encapsulated within `DiagnoseError`.
CLI commands shall return generic error summaries to standard error with exit code 1 or 2.

## Tasks

- [x] 1. Define diagnose data structures and error types in `src/diagnose/types.rs`.
- [x] 2. Implement hypothesis forking and heuristic evidence analysis in `src/diagnose/fork.rs`.
- [x] 3. Implement convergence judge and markdown generation in `src/diagnose/judge.rs`.
- [x] 4. Wire CLI command `cicatrix diagnose` in `src/main.rs`.
- [x] 5. Expose `cicatrix_diagnose` MCP tool in `src/mcp/tools.rs`.
- [x] 6. Write unit and integration tests verifying schema compliance and offline execution.
- [x] 7. Validate full test suite and establish green baseline.

## Verification

```bash
cargo xtask spec-check -j 2 && cargo test -j 2
```

## Notes

Design ported clean-room from `agent-afk` (Griffin Long, Apache-2.0).
Requirements syntax conforms to EARS (Mavin et al., Rolls-Royce plc, IEEE RE 2009).
