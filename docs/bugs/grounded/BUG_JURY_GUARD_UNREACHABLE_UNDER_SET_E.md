# BUG_JURY_GUARD_UNREACHABLE_UNDER_SET_E

- **id:** bug:jury-guard-unreachable-under-set-e
- **files:** .github/workflows/agent-jury.yml
- **fix-commit:** CER-2077 (fix/cer-2077-jury-gate-dead-guards)
- **regression-test:** `tests/agent_jury_workflow.rs::review_step_survives_non_json_gateway_body`, `::review_step_survives_non_json_model_content`, `::post_step_reports_when_review_left_no_parsed_json`
- **meta-pattern:** Contracts break silently downstream
- **status:** resolved
- **scope:** .github/workflows

## Symptom

The Agent Jury — the repo's automated merge gate — reported `failure` on PR #13
(run `32124578502`) after 27 seconds, with no verdict, no review comment, and no
label. The only error in the log was:

```
cat: /tmp/jury-parsed.json: No such file or directory
##[error]Process completed with exit code 1
```

The step that was supposed to produce that file had already ended with
`##[error]Process completed with exit code 5`, printing nothing to stderr. Every
review output was empty in the downstream step's env block:

```
VERDICT:
CONFIDENCE:
FINDINGS_COUNT:
REVIEW_FAILED:
```

`REVIEW_FAILED` being empty is the tell: the review step has three guards whose
whole purpose is to set it, and not one of them ran.

## Root cause

The review step runs under `set -euo pipefail` and captures `jq` output into
variables that the *next* line then guards:

```bash
CONTENT=$(echo "$RESPONSE" | jq -r '.choices[0].message.content // empty' 2>/dev/null)

if [ -z "$CONTENT" ]; then      # <-- never reached
  echo "review_failed=true" >> "$GITHUB_OUTPUT"
  exit 0
fi
```

`jq` exits **5** on malformed input. With `pipefail`, the pipeline's status
becomes 5; with `-e`, bash aborts the script *at the assignment*. The `if` below
it is unreachable code in exactly the case it was written for. `2>/dev/null`
made it worse by suppressing jq's own diagnostic, so the step died silently with
a bare numeric status. Confirmed locally:

```
$ echo 'not json' | jq '.' >/dev/null 2>&1; echo $?
5
```

Two assignments had this shape — the gateway-body parse and the model-content
parse — so both a non-JSON HTTP body (proxy/HTML error page returned with 200)
and a model that answers in prose instead of the required JSON object killed the
step before it could report itself as failed.

The second, compounding defect is in the `if: always()` post step. Because it
runs unconditionally, it must tolerate its predecessor not having finished — but
it read the parsed-JSON file with a bare `cat` under `set -e`. So any upstream
breakage (this bug, a timeout, a cancellation) surfaced not as "review failed"
but as a missing-file error and a red gate.

**The mental-model error:** treating `set -e` as a safety net that runs *in
addition to* explicit error handling, when it actually *preempts* it. Writing a
guard directly below a failing command feels like defense in depth; under `-e`
the guard is dead code, and the more carefully the failure path was written, the
more certain it was never to execute. The author checked that the guard existed,
not that it could run.

Net effect: a merge gate that could report `approved` or `failure` but never the
one state it was built to detect — "I could not review this." It failed loudly
enough to look like enforcement and vacuously enough to enforce nothing.

## Reproduction

`tests/agent_jury_workflow.rs` extracts the real step scripts out of
`.github/workflows/agent-jury.yml` and runs them against a stubbed `curl`.

Pre-fix, with the gateway returning a non-JSON body:

```
review step must not abort — it is advisory. stderr:
  left: 5
 right: 0
```

Exit 5 with **empty stderr** is the whole bug in one line: the guard's own error
message never printed, because the guard never ran.

## Resolution

Three changes to `.github/workflows/agent-jury.yml`:

1. Both `jq` capture assignments now end in `|| VAR=""`, so a parse failure
   yields an empty string and falls through to the guard that was always meant
   to handle it. `FINDINGS_COUNT` gained `(.findings // [])` plus a fallback,
   for a model that half-follows the schema after a verdict was already parsed.
2. The post step derives failure from `[ ! -f "$JURY_FILE" ]` as well as from
   `REVIEW_FAILED`, so an aborted predecessor produces the "Review Failed"
   comment and `agent-jury-needs-changes` label rather than a `cat` error.
3. Hardcoded `/tmp/*` paths became `"${RUNNER_TEMP:-/tmp}/..."` — quoted, and
   overridable so the regression tests can run hermetically and in parallel.

Note that the ticket's stated root cause — `runs-on: reverie`, an ARC runner
that does not service this repo — was real but had already been fixed by the
`AGENT_JURY_RUNNER=ubuntu-latest` repo variable. Only the 2026-08-04 run
cancelled; every run since executed and this bash defect is what kept the gate
from working. Fixing the reported cause would have left the gate broken.

## Lesson

**A guard you have not executed is a comment.** Under `set -euo pipefail`, any
`VAR=$(cmd)` or `VAR=$(a | b)` whose failure you intend to handle must opt out
of `-e` explicitly (`|| VAR=""`, or `if ! VAR=$(...)`), or the handler below it
is unreachable. Silencing the command's stderr on top of that removes the last
evidence that it happened.

Two upstream disciplines follow. First: **any step marked `if: always()` has a
contract with the failure case** — it must assume every predecessor output is
empty and every predecessor artifact is missing, because that is precisely when
it earns its keep. Second: **CI shell is production code and needs tests.** This
defect lived in an inline `run:` block that no test in the repo could reach; it
was found only by reading a failed run's log by hand. Extracting the real step
scripts from the workflow and executing them against stubs costs one test file
and turns the gate itself into something the baseline can defend.
