---
type: "guide"
status: "draft"
owner: "cicatrix"
approved_by: ""
last_reviewed: "2026-10-09"
sources: []
---

# EARS syntax guide for agentic specifications

The Easy Approach to Requirements Syntax, known as EARS, is a notation for requirements in natural language. Alistair Mavin and colleagues developed it at Rolls-Royce plc. They first published it at the IEEE International Requirements Engineering Conference in 2009.

## Purpose

Unconstrained natural language causes ambiguity in software development. A language model invents a missing requirement when the text gives it no structure. EARS restricts the grammar to a small set of sentence patterns. Each pattern maps to a state machine, a unit test, or a runtime assertion.

EARS defines five basic patterns. An author combines the building blocks of those patterns to write a compound requirement. This guide calls the common combination the complex form. Every normative clause uses the keyword `shall`.

## Before you start

Read the specification procedure in `specs/README.md`. Copy the template in `spec-template.md`. Write the acceptance criteria of the specification with the patterns below.

## The five basic patterns

### 1. Ubiquitous

Use the ubiquitous pattern for a continuous invariant. The requirement applies at all times. It has no precondition.

```text
The <system> shall <action>.
```

Cicatrix example:
`The cicatrix binary shall format all diagnostic messages to stderr.`

### 2. Event-driven

Use the event-driven pattern when a trigger starts an action.

```text
When <trigger>, the <system> shall <action>.
```

Cicatrix example:
`When a query command receives changed files, the regression engine shall match paths against known bug surfaces.`

### 3. State-driven

Use the state-driven pattern while an operational state remains active.

```text
While <state>, the <system> shall <action>.
```

Cicatrix example:
`While offline mode is active, the reverie client shall return cached observations without initiating network requests.`

### 4. Unwanted behavior

Use the unwanted behavior pattern for error detection, an edge case, or failure containment.

```text
If <condition>, then the <system> shall <action>.
```

Cicatrix example:
`If an observed bug markdown path is supplied to record, then the command shall exit with a non-zero status code.`

### 5. Optional feature

Use the optional feature pattern when the behavior depends on a feature that the build includes.

```text
Where <feature>, the <system> shall <action>.
```

Cicatrix example:
`Where an as-of commit is provided, the query command shall filter results by git ancestry.`

## The complex form

Combine a state precondition and a trigger when the behavior needs both. Keep the clauses in this order.

```text
While <state>, when <trigger>, the <system> shall <action>.
```

Cicatrix example:
`While evaluating regression tests, when a test execution exceeds the timeout bound, the runner shall mark the result as failed.`

The source notation also permits an optional precondition in the event-driven pattern and in the unwanted behavior pattern. This estate keeps those two patterns in their short form.

## Negative rules

Observe these rules in every requirement clause.

1. Do not use an ambiguous modal. Never write `should`, `could`, `may`, or `might`. Write `shall` for a mandatory requirement. For an optional item, use the optional feature pattern with `shall`.
2. Do not use a vague qualifier. Never write `fast`, `robust`, `scalable`, or `easy`. State an exact numerical limit, a latency bound, or a memory threshold.
3. Keep each clause atomic. State one action in one clause. Do not join two operational requirements with `and`. Split a procedure of several steps into separate clauses.
4. Write every requirement in original phrasing. Do not copy vendor documentation or a schema string.

## Check the result

Run `cargo xtask spec-check -j 2`. The validator requires at least one clause with `shall`. It reports an ambiguous modal. It reports a clause that starts with a word other than The, When, While, If, or Where.

Each pattern also informs an automated test:

* Ubiquitous: a Clippy rule, a compiler invariant, or a static analysis check.
* Event-driven: a unit test that sends the trigger and asserts the postcondition.
* State-driven: a property-based test that verifies the state invariant holds.
* Unwanted behavior: a negative test that injects the fault and asserts the error.
* Optional feature: conditional compilation, or a configuration test matrix.
* Complex form: a state machine simulation test.

## Source

Mavin, A., Wilkinson, P., Harwood, A., and Novak, M. Easy Approach to Requirements Syntax (EARS). IEEE International Requirements Engineering Conference, 2009.
