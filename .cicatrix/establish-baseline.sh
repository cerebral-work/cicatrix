#!/usr/bin/env bash
# cicatrix: establish the green-baseline marker — BY CONSTRUCTION, not by fiat.
#
# Runs the test suite and writes .cicatrix/baseline-green ONLY if it is green; removes any
# stale marker on red. The commit-gate (.claude/hooks/commit-gate.sh) refuses to commit
# without this marker, and CLAUDE.md's "the baseline is green at session start" invariant is
# backed by *this run*. Intended to run at session start (wire as a SessionStart hook, opt-in).
#
# Exit 0 = green, marker written. Exit 1 = red, marker absent.
set -euo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$REPO_ROOT"
MARKER="$REPO_ROOT/.cicatrix/baseline-green"
mkdir -p "$REPO_ROOT/.cicatrix"

if ! test_out="$(cargo test --quiet 2>&1)"; then
  rm -f "$MARKER"
  printf '%s\n' "$test_out" >&2
  echo "cicatrix: baseline RED — suite failed; marker NOT written (stale one removed)." >&2
  exit 1
fi

head="$(git rev-parse HEAD 2>/dev/null || echo no-commit)"
stamp="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

# Aggregate across every test binary (cargo prints one `test result:` line per suite).
# tail -1 would under-report; sum passed/failed across all suites instead.
result_lines="$(printf '%s\n' "$test_out" | grep -E 'test result:' || true)"
passed="$(printf '%s\n' "$result_lines" | grep -oE '[0-9]+ passed' | awk '{s+=$1} END{print s+0}')"
failed="$(printf '%s\n' "$result_lines" | grep -oE '[0-9]+ failed' | awk '{s+=$1} END{print s+0}')"
suites="$(printf '%s\n' "$result_lines" | grep -cE 'test result:' || true)"

{
  echo "# cicatrix green-baseline marker — written by .cicatrix/establish-baseline.sh"
  echo "established: $stamp"
  echo "commit: $head"
  echo "suite: ${passed:-0} passed, ${failed:-0} failed across ${suites:-0} test binaries"
} > "$MARKER"

echo "cicatrix: baseline GREEN — marker written to .cicatrix/baseline-green"
