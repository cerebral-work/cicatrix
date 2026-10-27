# Sovereign Monorepo Consolidation Handoff Notice — Cicatrix

> [!IMPORTANT]
> **Notice of Repository Absorption into `cerebral-work/vespertin`**
> Under **LeanSpec 0017** (`specs/0017-estate-deconvolution-and-vespertin-consolidation.md`) and operator ruling (2026-10-27), `cerebral-work/cicatrix` is scheduled for consolidation into the unified sovereign monorepo:
> **[`cerebral-work/vespertin`](https://github.com/cerebral-work/vespertin)**

## 1. Subsystem Destination

| Source Surface | Target Subsystem in `vespertin` | Subsystem Purpose |
|---|---|---|
| `cerebral-work/cicatrix` | `vespertin/reverie/cicatrix/` | Grounded Bug Store, defect lifecycle tracker (`cicatrix-core`), and CLI interface (`cicatrix-cli`). |

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
