---
status: in-progress
created: 2026-10-09
tags: [regression-db, provider, datalog, workflows, reversibility, ears]
priority: high
scope: [core, engine, storage]
---

# 0001-cicatrix-regression-db-provider

## Requirements

### Context and user stories

Software regression tracking across autonomous multi-agent engineering swarms requires active state validation.
Static markdown projections into search endpoints do not enforce schema integrity, temporal causality, or action reversibility.
This specification establishes the architectural requirements for cicatrix as an active regression database provider.
The provider integrates temporal datalog frontiers, deterministic durable workflow replay, and reversibility-gated autonomy.

### Acceptance criteria

* **Ubiquitous:** The cicatrix storage engine shall reject datoms with empty string or zero-valued attributes via neverZeroValue schema integrity constraints.
* **Ubiquitous:** The regression engine shall maintain version-vector temporal frontiers across replica identifiers without mutable state overwrites.
* **Event-driven:** When a query command receives changed file paths, the engine shall match paths against indexed regression surfaces.
* **State-driven:** While evaluating workflow activities, the durable replay engine shall deduplicate signal deliveries using unique idempotency keys.
* **Unwanted behavior:** If an internal server error occurs across provider surfaces, then the engine shall return a generic error message carrying a correlation identifier.
* **Optional feature:** Where an as-of commit or version vector is provided, the query evaluator shall filter regression facts by historical causality.
* **Complex form:** While executing within an earned autonomy tier, when an outward action is proposed, the reversibility gate shall verify the presence of a validated compensation plan before execution.

### Non-functional constraints

* **Ubiquitous:** The build harness shall restrict all cargo invocations to `-j 2` concurrency.
* **Ubiquitous:** The durable workflow runtime shall cap event log history to a maximum of 50,000 events per run.
* **Ubiquitous:** The local runtime shall execute against single-writer SQLite WAL storage without network dependencies.

## Design

### Cleanroom Architecture & Lineage

The system combines three research lineages in cleanroom implementations:
1. `wbrown/janus-datalog`: Version-vector frontiers (`{ReplicaID -> maxLamport}`), snapshot database forking, `:db/neverZeroValue` schema integrity constraints, and longitudinal stochastic occurrence logs.
2. `autumn-foundation/autumn-harvest`: Deterministic event-sourced workflow replay, durable signals with deduplication keys, child workflow DAG scheduling, dual-tier SQLite WAL and PostgreSQL storage, and native `#[workflow(mcp)]` exposure.
3. `wheelhorsedev/nexus`: Reversibility-gated autonomy pipelines (`classify` -> `plan` -> `validate` -> `decide`), synthetic canary tripwires, correlation-masked errors (`ref_` identifiers), and the three-tier earned autonomy trust ladder (`shadow`, `supervised`, `autonomous`).

### Subsystem Boundaries

```text
Clients (CAS, Soma, CLI)
       |
       v
MCP Server & Reversibility Gate (Wheelhorse)
       |
       v
Durable Workflow Replay Engine (Autumn Harvest)
       |
       v
Temporal Frontier & Storage Router (Janus-Datalog)
       |
  +----+----+
  |         |
SQLite   Postgres
(Local)  (Central)
```

### Invariants & Safety Gates

1. Every write operation validates against the `:db/neverZeroValue` invariant.
2. Every outward state mutation verifies a compensation plan before transition to autonomous execution.
3. Synthetic canary records tripwire unapproved queries, halting agent execution immediately.

## Tasks

- [x] 1. Implement Phase 0 test evidence extraction and AuthLedger directive scanning.
- [x] 2. Adopt EARS specification methodology and headless-v1 schema from research lane.
- [ ] 3. Implement Phase 1 local embedded SQLite WAL storage engine and schema migrations.
- [ ] 4. Implement Phase 1 version-vector temporal frontier and snapshot forking primitives.
- [ ] 5. Implement Phase 2 Autumn Harvest deterministic event-sourced workflow runtime.
- [ ] 6. Implement Phase 2 durable signal delivery with idempotency deduplication.
- [ ] 7. Implement Phase 3 Wheelhorse reversibility gate and canary tripwire defenses.
- [ ] 8. Implement Phase 3 earned autonomy trust ladder transitions.
- [ ] 9. Implement Phase 4 central PostgreSQL storage backend and MCP server endpoints.

## Verification

```bash
cargo xtask spec-check -j 2 && cargo test -j 2
```

## Notes

Specification conforms to the Easy Approach to Requirements Syntax (EARS, Mavin et al., Rolls-Royce plc, 2009).
Design incorporates research lineages from `wbrown/janus-datalog`, `autumn-foundation/autumn-harvest`, and `wheelhorsedev/nexus`.
All components maintain strict licensing hygiene and independent cleanroom implementations.
