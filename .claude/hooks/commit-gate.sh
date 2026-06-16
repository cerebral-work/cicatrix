#!/usr/bin/env bash
# cicatrix commit-gate (v0). Adapted from wbrown/janus-datalog review-commit.sh.
# Blocks a commit on: (1) no green-baseline marker, (2) premature-victory phrasing in the
# commit message, (3) a bug fix with no accompanying regression test + BUG_*.md entry.
# Wire as a PreToolUse Bash hook (matches `git commit *`) or a git pre-commit hook.
# v0 is heuristic + local; the janus-datalog "does this reintroduce a known failure?" query
# lands when src/store.rs is wired.
set -euo pipefail
REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || echo .)"
MSG="${1:-$(git -C "$REPO_ROOT" log -1 --format=%B 2>/dev/null || echo '')}"

block() { echo "cicatrix: [BLOCKED] $1" >&2; exit 2; }

# (1) green baseline must have been recorded this session
[ -f "$REPO_ROOT/.cicatrix/baseline-green" ] || \
  block "no .cicatrix/baseline-green marker — establish a green baseline before committing."

# (2) premature-victory phrasing (claiming done with hedges/known-unsolved)
if printf '%s' "$MSG" | grep -qiE 'should (now )?work|probably fix|TODO: (real|proper) fix|hack(y)? fix|temporar(y|ily)|will fix later|i think this|hopefully'; then
  block "premature-victory phrasing in commit message — prove it, don't hope it."
fi

# (3) if staged diff touches src/ but adds no test and no bug doc, warn-block on fixes
staged="$(git -C "$REPO_ROOT" diff --cached --name-only 2>/dev/null || true)"
if printf '%s' "$MSG" | grep -qiE '^fix(\(|:)' && \
   printf '%s' "$staged" | grep -qE '^src/' && \
   ! printf '%s' "$staged" | grep -qiE 'test|docs/bugs/'; then
  block "fix commit with no regression test or docs/bugs/ entry — every fix ships a guard."
fi

echo "cicatrix: commit-gate passed"
