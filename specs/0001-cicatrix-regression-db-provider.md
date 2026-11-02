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

### Upstream Vendoring & Lineage (`janus-datalog-rs` Supersedes `cicatrix`)

> **2026-11-01 Operator Ruling:** Rather than maintaining `cicatrix` or rolling a cleanroom Datalog implementation, we vendor `github.com/wbrown/janus-datalog` (`Apache-2.0`, authored by Wes Brown / `@wbrown`, pinned at commit `69bfc3b256328f20341e7d57649faaa44a366f96`) and maintain idiomatic Rust bindings (`janus-datalog-rs` / `janus-datalog-sys`).

The architecture integrates three upstream lineages with full attribution and provenance:
1. `github.com/wbrown/janus-datalog` (`Apache-2.0`, `@wbrown`, pinned at `69bfc3b256328f20341e7d57649faaa44a366f96`): Vendored upstream Go engine exposed to Rust via `janus-datalog-rs` (`c-archive` FFI), providing recursive rules on a relational-algebra core (semi-naive fixpoint iteration, magic sets, stratified negation), 8 covering indices (including `ATEV` and `TAEV` `History().Since(P)` delta views), CRDT storage (`LWW`, `Add-Wins Set`, `RGA`), `AsOf(elementID)` / `History()` time-travel, and `:db/neverZeroValue` schema integrity constraints.
2. `github.com/autumn-foundation/autumn-harvest` (`MIT OR Apache-2.0`, pinned at `0bad2773659a31c35611b2ed484963e55e88fffa`): Deterministic event-sourced workflow replay, durable signals with deduplication keys, child workflow DAG scheduling, dual-tier SQLite WAL and PostgreSQL storage, and native `#[workflow(mcp)]` exposure.
3. `github.com/wheelhorsedev/nexus`: Reversibility-gated autonomy pipelines (`classify` -> `plan` -> `validate` -> `decide`), synthetic canary tripwires, correlation-masked errors (`ref_` identifiers), and the three-tier earned autonomy trust ladder (`shadow`, `supervised`, `autonomous`).

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
- [x] 3. Implement Phase 1 local embedded SQLite WAL storage engine and schema migrations.
- [x] 4. Implement Phase 1 version-vector temporal frontier and snapshot forking primitives.
- [x] 5. Implement Phase 2 Autumn Harvest deterministic event-sourced workflow runtime.
- [x] 6. Implement Phase 2 durable signal delivery with idempotency deduplication.
- [x] 7. Implement Phase 2 native Model Context Protocol (MCP) provider interface.
- [x] 8. Implement Phase 3 Wheelhorse reversibility gate and canary tripwire defenses.
- [x] 9. Implement Phase 4 earned autonomy trust ladder transitions.
- [ ] 10. Implement Phase 4 central PostgreSQL storage backend.

## Verification

```bash
cargo xtask spec-check -j 2 && cargo test -j 2
```

## Notes

Specification conforms to the Easy Approach to Requirements Syntax (EARS, Mavin et al., Rolls-Royce plc, 2009).
Design incorporates research lineages from `wbrown/janus-datalog`, `autumn-foundation/autumn-harvest`, and `wheelhorsedev/nexus`.
All components maintain strict licensing hygiene and independent cleanroom implementations.
