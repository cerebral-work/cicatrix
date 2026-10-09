#!/bin/bash
# review-edit.sh — Supervisory agent that challenges every Edit/Write.
# Evaluates whether the edit is changing the right component, reading the
# recent conversation transcript to understand WHY the agent is making the
# change, not just WHAT it's changing.
#
# cicatrix adaptation of upstream wbrown/janus-datalog @ 6d714b3: the
# deterministic test-evidence pass targets Rust (in-file #[cfg(test)] modules
# and workspace tests/*.rs) instead of Go sibling *_test.go files. Everything
# else is upstream verbatim.
#
# Targets bash 3.2 (macOS /bin/bash). The system prompt lives in
# review-edit.system.md + review-verdict-contract.md and is read with `cat` —
# NOT embedded as a heredoc, which 3.2 mis-parses inside $(...). The verdict
# channel is shared with review-reasoning.sh via lib/review_common.sh.
#
# The --model pin below is deliberate hook configuration, not a cost shortcut.

set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=lib/review_common.sh
source "$SCRIPT_DIR/lib/review_common.sh"

INPUT=$(cat)
TOOL_NAME=$(echo "$INPUT" | jq -r '.tool_name')
FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path // empty')
TRANSCRIPT_PATH=$(echo "$INPUT" | jq -r '.transcript_path // empty')

# Skip review for non-code files (markdown, config, etc.) — allow silently.
case "$FILE_PATH" in
    *.md|*.json|*.yaml|*.yml|*.toml|*.txt|*.lock)
        exit 0
        ;;
esac

# extract_fn_names: read Rust source on stdin, print one fn name per line.
# Matches top-level and impl-block items: optional visibility (pub, pub(crate),
# …) and qualifiers (async/const/unsafe) before `fn name`. Comment lines don't
# match (the anchor requires the qualifiers/fn to open the line). Plain awk —
# no gawk extensions.
extract_fn_names() {
    awk '{
        line = $0
        if (line ~ /#\[cfg\(test\)\]/) { in_test = 1; next }
        if (in_test) next
        if (line ~ /#\[test\]/ || line ~ /#\[tokio::test\]/) { skip_next = 1; next }
        if (line !~ /^[[:space:]]*(pub([(][^)]*[)])?[[:space:]]+)?((async|const|unsafe)[[:space:]]+)*fn[[:space:]]+[A-Za-z_]/) {
            skip_next = 0
            next
        }
        if (skip_next) { skip_next = 0; next }
        sub(/^.*fn[[:space:]]+/, "", line)
        sub(/[^A-Za-z0-9_].*$/, "", line)
        if (line != "") print line
    }'
}

# compute_test_evidence: deterministic ground truth for Rule 6/7 ("write
# tests first"), replacing a blind spot where the reviewer could only see
# whether a test was written recently by scanning a BOUNDED tail of the
# conversation transcript — a long session pushes an earlier test-writing
# turn out of that window, producing a false "no test written" verdict for
# otherwise-correct test-first work (observed and corrected upstream
# 2026-07-01).
#
# Instead of asking the reviewer to remember, check what's always complete
# and current regardless of session length: the tests that actually exist.
# For Rust that is (a) an in-file #[cfg(test)] module referencing the new fn
# (approximated as: the file carries #[cfg(test)] and the fn name occurs at
# least twice — definition plus one reference), and (b) the workspace
# integration tests, <repo-root>/tests/*.rs. This is purely additive to the
# existing transcript-based signal — it never suppresses a real violation,
# it only hands the reviewer verified evidence for cases the transcript
# window would otherwise miss.
compute_test_evidence() {
    local file_path="$1" old_content="$2" new_content="$3"

    case "$file_path" in
        */tests/*|*/benches/*|tests/*|benches/*)
            return 0
            ;;
        *.rs)
            ;;
        *)
            return 0
            ;;
    esac

    local new_funcs old_funcs added_funcs
    new_funcs=$(printf '%s\n' "$new_content" | extract_fn_names | sort -u)
    old_funcs=$(printf '%s\n' "$old_content" | extract_fn_names | sort -u)
    added_funcs=$(comm -23 <(printf '%s\n' "$new_funcs") <(printf '%s\n' "$old_funcs"))

    if [ -z "$added_funcs" ]; then
        return 0
    fi

    local dir root found missing hit tf fn refs
    dir=$(dirname "$file_path")
    root=$(git -C "$dir" rev-parse --show-toplevel 2>/dev/null) || root="$dir"
    found=""
    missing=""
    while IFS= read -r fn; do
        if [ -z "$fn" ]; then
            continue
        fi
        hit=0
        # (a) in-file #[cfg(test)] module: fn name referenced beyond its
        #     definition in a file that carries a test module.
        if printf '%s\n' "$new_content" | grep -q '#\[cfg(test)\]' 2>/dev/null; then
            refs=$(printf '%s\n' "$new_content" | grep -c -- "$fn" 2>/dev/null) || refs=0
            if [ "$refs" -ge 2 ]; then
                hit=1
            fi
        fi
        # (b) workspace integration tests.
        if [ "$hit" = "0" ]; then
            for tf in "$root"/tests/*.rs; do
                if [ -f "$tf" ]; then
                    if grep -q -- "$fn" "$tf" 2>/dev/null; then
                        hit=1
                        break
                    fi
                fi
            done
        fi
        if [ "$hit" = "1" ]; then
            found="$found $fn"
        else
            missing="$missing $fn"
        fi
    done <<< "$added_funcs"

    if [ -n "$found" ]; then
        printf 'DETERMINISTIC TEST-EXISTENCE CHECK (verified against the actual in-file #[cfg(test)] module and %s/tests/*.rs on disk right now, not conversation memory): these newly-added symbols are already referenced by a test:%s. Treat Rule 6/7 as satisfied for them regardless of whether their test-writing turn is visible in the reasoning context below.\n' "$root" "$found"
    fi
    if [ -n "$missing" ]; then
        printf 'DETERMINISTIC TEST-EXISTENCE CHECK found NO test reference (in-file #[cfg(test)] or %s/tests/*.rs) for these newly-added symbols:%s -- Rule 6 applies to them on its own merits.\n' "$root" "$missing"
    fi

    return 0
}

# Build change description based on tool type.
if [ "$TOOL_NAME" = "Edit" ]; then
    OLD_STRING=$(echo "$INPUT" | jq -r '.tool_input.old_string // empty')
    NEW_STRING=$(echo "$INPUT" | jq -r '.tool_input.new_string // empty')
    CHANGE_DESC="TOOL: Edit
FILE: $FILE_PATH
OLD CODE:
$OLD_STRING
NEW CODE:
$NEW_STRING"
    TEST_EVIDENCE=$(compute_test_evidence "$FILE_PATH" "$OLD_STRING" "$NEW_STRING" 2>/dev/null) || TEST_EVIDENCE=""
elif [ "$TOOL_NAME" = "Write" ]; then
    CONTENT=$(echo "$INPUT" | jq -r '.tool_input.content // empty')
    # A Write to a NEW file has no prior contents; don't let cat's failure abort
    # the hook under `set -e`.
    OLD_CONTENTS=$(cat "$FILE_PATH" 2>/dev/null || printf '(new file — no prior contents)')
    CHANGE_DESC="TOOL: Write
FILE: $FILE_PATH
OLD CONTENTS:
$OLD_CONTENTS
CONTENT:
$CONTENT"
    TEST_EVIDENCE=$(compute_test_evidence "$FILE_PATH" "$OLD_CONTENTS" "$CONTENT" 2>/dev/null) || TEST_EVIDENCE=""
else
    exit 0
fi

# Extract recent conversation context (JSONL). We need assistant/user text AND
# tool-use records so Rule 6/7 (test-first) can see whether a test file was
# written before the production edit — the Write/Edit record is a tool_use item,
# not text. Plain-text user messages store .message.content as a STRING,
# structured ones as an ARRAY; branch on type so both flow through. Defanged so
# control-looking tokens are seen as data.
RECENT_CONTEXT=""
if [ -n "$TRANSCRIPT_PATH" ] && [ -f "$TRANSCRIPT_PATH" ]; then
    RECENT_CONTEXT=$(tail -200 "$TRANSCRIPT_PATH" | \
         jq -r 'select(.type == "assistant" or .type == "user" or .type == "human") |
             . as $msg |
             if (.message.content | type) == "string" then
                 "\($msg.type)/text: \(.message.content)"
             else
                 .message.content[]? |
                 if .type == "text" then
                     "\($msg.type)/text: \(.text)"
                 elif .type == "tool_use" then
                     "\($msg.type)/tool: \(.name) file=\(.input.file_path // "")"
                 else
                     empty
                 end
             end' 2>/dev/null | hook_defang) || true
fi

NONCE=$(hook_make_nonce)
FULL_SYSTEM=$(cat "$SCRIPT_DIR/review-edit.system.md" "$SCRIPT_DIR/review-verdict-contract.md" | sed "s/__NONCE__/$NONCE/g")

# Build the full review prompt with context.
REVIEW_PROMPT="PROPOSED CHANGE:
$CHANGE_DESC"

if [ -n "$TEST_EVIDENCE" ]; then
    REVIEW_PROMPT="$REVIEW_PROMPT

---

$TEST_EVIDENCE"
fi

if [ -n "$RECENT_CONTEXT" ]; then
    REVIEW_PROMPT="AGENT'S RECENT REASONING:
$RECENT_CONTEXT

---

$REVIEW_PROMPT"
fi

START_TIME=$(date +%s)
REVIEW_RESULT=$(echo "$REVIEW_PROMPT" | env -u CLAUDECODE -u ANTHROPIC_API_KEY claude -p --model sonnet --system-prompt "$FULL_SYSTEM" --no-session-persistence --tools "" "Review this proposed code change against the failure modes in your instructions. Walk each mode and state whether it applies; your verdict is the conclusion of that walk. Which mode does this change match? If none, state which you checked and why none applies. Note especially whether it modifies the right component." 2>/dev/null) || REVIEW_RESULT=""
END_TIME=$(date +%s)
ELAPSED=$((END_TIME - START_TIME))
LABEL="SUPERVISOR [${ELAPSED}s]"

# Parse the nonce-stamped verdict. No valid verdict (parrot, empty, garbled, or
# the reviewer couldn't run at all) fails closed to a manual prompt — never a
# silent allow.
VERDICT_JSON=$(printf '%s\n' "$REVIEW_RESULT" | hook_extract_verdict_json "$NONCE")
if [ -z "$VERDICT_JSON" ]; then
    DECISION="ask"
    REASON="review inconclusive — reviewer returned no valid verdict; approve manually"
else
    VERDICT=$(printf '%s' "$VERDICT_JSON" | jq -r '.verdict')
    REASON=$(printf '%s' "$VERDICT_JSON" | jq -r '(.reason // "no reason given") | gsub("[\n\r]+"; " ")')
    if [ "$VERDICT" = "block" ]; then
        DECISION="deny"
    else
        # Advisory mode (CLAUDE_HOOKS_ADVISORY=1) routes a pass through a manual
        # prompt; otherwise auto-approve.
        DECISION="allow"
        if [ "${CLAUDE_HOOKS_ADVISORY:-}" = "1" ]; then
            DECISION="ask"
        fi
    fi
fi

hook_log_verdict /tmp/supervisor_log.txt "$ELAPSED" "$DECISION" "$REASON" "$NONCE" "$REVIEW_RESULT"
hook_emit_decision "$DECISION" "$REASON" "$LABEL"
exit 0
