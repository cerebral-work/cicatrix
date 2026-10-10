---
type: "guide"
status: "draft"
owner: "cicatrix"
approved_by: ""
last_reviewed: "2026-10-09"
sources: []
---

# Specification template

This page holds the template for a new specification in the `specs/` directory. Copy the fenced block below into the new file. Replace each brace placeholder.

## Purpose

The template fixes the section order that `cargo xtask spec-check` enforces. The order is Requirements, Design, Tasks, Verification, Notes. The frontmatter in the block is the frontmatter of a specification. It is not the frontmatter of this page.

## Before you start

Read `specs/README.md` for the numbering rule and the registry table. Read [the EARS syntax guide](ears-syntax-guide.md) for the requirement patterns.

## The template

````text
---
status: planned
created: '{date}'
tags: []
priority: medium
scope: []
---

# {number}-{slug}

## Requirements

### Context and user stories

Describe the user problem and technical context.
Keep descriptions concise and factual.

### Functional requirements

Specify functional requirements using Easy Approach to Requirements Syntax (EARS) clauses.
Every normative requirement clause must include the keyword `shall`.

* **Ubiquitous:** The `{system}` shall `{action}`.
* **Event-driven:** When `{trigger}`, the `{system}` shall `{action}`.
* **State-driven:** While `{state}`, the `{system}` shall `{action}`.
* **Unwanted behavior:** If `{error_condition}`, then the `{system}` shall `{action}`.
* **Optional feature:** Where `{feature_flag}`, the `{system}` shall `{action}`.
* **Complex form:** While `{state}`, when `{trigger}`, the `{system}` shall `{action}`.

### Non-functional constraints

State operational boundaries, resource caps, and latency targets.
State memory limits and CPU constraints directly.

## Design

### Architecture

Describe component boundaries, modules, and interfaces.
Document interaction diagrams where components exchange messages.

### State machines and invariants

List state transitions and data invariants.
Ensure every state transition handles failure cases.

### Error containment

Define failure isolation zones and recovery policies.
Do not let errors escape subsystem boundaries without structured translation.

## Tasks

- [ ] 1. Define data structures and type signatures.
- [ ] 2. Implement domain logic and state transitions.
- [ ] 3. Write unit tests for normal and edge cases.
- [ ] 4. Connect component interfaces and verify build.
- [ ] 5. Run automated verification gates.

## Verification

```bash
cargo xtask spec-check -j 2 && cargo test -j 2
```

Document manual verification steps if automated checks do not cover external integrations.

## Notes

Record design trade-offs, rejected alternatives, and reference citations.
Requirements syntax conforms to EARS (Mavin et al., Rolls-Royce plc, IEEE RE 2009).
````

## Check the result

Run `cargo xtask spec-check -j 2`. Add one row for the new specification to the table in `specs/README.md`.
