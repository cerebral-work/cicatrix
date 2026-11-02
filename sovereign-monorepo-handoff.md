# Sovereign Monorepo Consolidation Handoff Notice — Cicatrix

> [!IMPORTANT]
> **Notice of Deprecation & Supersession by `janus-datalog-rs` (`wbrown/janus-datalog` Rust Bindings)**
> Per operator ruling (2026-11-01), rather than maintaining `cicatrix` or rolling a cleanroom Datalog implementation, `cerebral-work/cicatrix` is superseded by **`janus-datalog-rs`** — idiomatic Rust bindings (`c-archive` FFI) over vendored upstream **[`github.com/wbrown/janus-datalog`](https://github.com/wbrown/janus-datalog)** (`Apache-2.0`, authored by Wes Brown / `@wbrown`, pinned at `69bfc3b256328f20341e7d57649faaa44a366f96`) inside **[`cerebral-work/vespertin`](https://github.com/cerebral-work/vespertin)**.

## 1. Subsystem Destination

| Source Surface | Target Subsystem in `vespertin` | Subsystem Purpose |
|---|---|---|
| `github.com/wbrown/janus-datalog` | `vespertin/vendor/janus-datalog-rs/` | Rust FFI bindings (`c-archive` / `bindgen`) over upstream `wbrown/janus-datalog` (`Apache-2.0`, `@wbrown`): recursive rules on a relational-algebra core (semi-naive fixpoint, magic sets, stratified negation), 8 covering indices (`ATEV`, `TAEV` `History().Since(P)`), CRDT storage, `AsOf()`/`History()`, and `:db/neverZeroValue`. |
| `cerebral-work/cicatrix` (`docs/bugs/grounded/`) | `vespertin/reverie/` | Grounded bug-fact corpus projected into Reverie + queried via `janus-datalog-rs` (`cicatrix` Rust crate retired). |

## 2. Consolidation Rules & Guidelines

1. **Zero Code Loss**: All defect records, bug schema validators, core libraries, tests, and historical git lineage are preserved during migration into `vespertin/reverie/cicatrix/`.
2. **Clean Vendoring Rules**: Upstream vendor dependencies retain original crate names, git provenance, authorship, and root dual licenses under `vespertin/vendor/`.
3. **Orchestration Standard**: Build, test, and release orchestration moves from `.moon/` / `Justfile` to `tracel-xtask` (`cargo xtask` with mandatory `-j 2` host throttling).
4. **Slack Transport Unification**: Slack integrations unify onto Slack App `revenant` (`A0C6JAFBQHY`) with the Oni Mask avatar (`:japanese_ogre:`).
5. **Post-Migration Archive**: Upon verification of `vespertin` CI and deployment checks, this repository will be transitioned to read-only archived state on GitHub (`gh repo archive`).

## 3. Governance and Inquiries

- **Lead Coordinator Lane**: `revenant` (`cerebral-work/revenant`, `estate/w53:pE`).
- **Partner Lane**: `vilicus` (`cerebral-work/vilicus`).
- **Specification Reference**: [LeanSpec 0017](file:///home/ctodie/projects/platform/runtime/revenant/specs/0017-estate-deconvolution-and-vespertin-consolidation.md).
