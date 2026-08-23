# cicatrix — agent contract + meta-patterns (injected upstream)

These are **disciplines that generate your actions**, not a checklist to satisfy at the last
checkpoint. Read before editing. Adapted from `wbrown/janus-datalog`'s method.

## Invariants

- **The baseline is green.** The suite passes at session start by construction:
  `.cicatrix/establish-baseline.sh` runs `cargo test` and writes `.cicatrix/baseline-green`
  *only* on green (removing it on red). Run it at session start.
  Therefore any red test during this session **was caused by your work**. "Pre-existing failure"
  is forbidden phrasing — investigate, don't excuse.
- **Every bug fix ships a regression test.** No fix lands without an executable guard, and a
  `docs/bugs/grounded/BUG_*.md` entry (see `_SCHEMA.md`).
- **One mutating action per turn.** Run a single state-changing command, then *read its result
  before acting again*. Don't chain-and-commit on an unread outcome.
- **Banned outright:** `git stash`, `git clean`, `git add -A`, `rm -rf` of work trees, heredocs
  that bury content. These silently destroy work.

## Meta-patterns (rolled up from the bug corpus — query before authoring)

> Regenerated from `docs/bugs/grounded/`. Each is a *class* of past failure to avoid upstream.

- **Type mismatches kill.** A value crossing a boundary in the wrong shape (empty vs zero vs null)
  fails silently downstream. → Validate at the seam; choose an explicit empty representation.
  (seed: `BUG_EMBED_EMPTY_INPUT_400`)
- **Two implementations of one fact drift.** When the same state is written by two code paths,
  one rots restore-blind. → Single source of truth; the read path must see every write path.
  (seed: `BUG_BACKUP_SPLIT_BRAIN`)
- **Edge cases are real cases.** Empty input, first/last element, zero-length batch — these are
  inputs, not exceptions. → Test them explicitly.
- **Correctness before performance.** A fast wrong answer is a bug. → Make it right, then measure.
- **Test structure, not just outcomes.** Many tests through one code path = false confidence.
  → Assert the shape/invariant, not only the happy-path output.

The block below is machine-managed: `cicatrix project-meta` regenerates it from the grounded
corpus (`docs/bugs/grounded/`). Run `cicatrix project-meta` to preview a diff, `--apply` to write.
Do not hand-edit between the markers.

<!-- cicatrix:meta-patterns:start -->
cicatrix meta-patterns (see CLAUDE.md):
- Contracts break silently downstream (seed: BUG_AUTH_HARDENING_STRANDED_CONSUMERS, BUG_JURY_GUARD_UNREACHABLE_UNDER_SET_E)
- Two implementations of one fact drift (seed: BUG_BACKUP_SPLIT_BRAIN)
- Type mismatches kill (seed: BUG_EMBED_EMPTY_INPUT_400)
- Edge cases are real cases (seed: BUG_LCM_TURN_INDEX_COLLISION_500)
- A text-anchored edit is not a structural edit (seed: BUG_MD_PARSER_FENCE_BLINDNESS)
- Bound every wait (seed: BUG_REVERIED_UNBOUNDED_SHUTDOWN_SIGKILL)
<!-- cicatrix:meta-patterns:end -->

## Reviewer failure modes (enforced by `.claude/hooks/`, opt-in)

The vendored supervisors gate code edits against these (full text in
`.claude/hooks/review-*.system.md`). Listed here so they're injected upstream too.

**Edit/diff (review-edit, 8):** modifying a lib to pass a consumer's tests · weakening test
assertions · workarounds over root-cause · changing prod to match tests · temporary tests ·
**production code without tests first** · **bug fix without a failing regression test first** ·
rewriting a whole file with Write instead of Edit.

**Reasoning chain (review-reasoning, 9):** skipping formal reasoning · shortcut justification
("simpler/faster/for now") · inventing unrequested abstractions · **working around instead of
fixing** (incl. routing a conditional around a buggy path) · dismissing evidence
("pre-existing/flaky") · wrong architectural layer · fighting user corrections · circular
reasoning · **simplifying away the bug** (repro that drops the triggering conditions).

> User authorization is primary: an explicit "do it / allow this" overrides the gate.

## How cicatrix uses this file

`cicatrix inject` emits the Meta-patterns section into an agent's context before a task.
`cicatrix record` reads a `BUG_*.md` and projects it as a reverie observation (project=`cicatrix`);
`cicatrix query <changed-files>` asks reverie whether the diff touches a known-bug surface. The
commit-gate hook reads this section to judge "premature victory."
