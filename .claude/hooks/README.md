# cicatrix hooks — reasoning-audit arm

Vendored + adapted from [`wbrown/janus-datalog`](https://github.com/wbrown/janus-datalog)
`.claude/hooks/` (2026-06-16). **Inert by default** — nothing here is wired to any live
`settings.json`. Enable per-repo by adding to that repo's `.claude/settings.json` PreToolUse.

| File | Role | Model | Default |
|---|---|---|---|
| `review-edit.sh` | gates each Edit/Write against 8 code failure modes (lib-mod-for-consumer-tests, weakened asserts, workarounds, code/fix without test-first, Write-vs-Edit…) | haiku | opt-in |
| `review-reasoning.sh` | gates the thinking+text chain against 9 inference failure modes (shortcut justification, work-around-not-fix, dismissing evidence, simplify-away-the-bug…) | sonnet (low) | opt-in |
| `lib/review_common.sh` | nonce-stamped, defanged, **fail-closed** verdict channel (anti-forgery) | — | sourced |
| `review-*.system.md`, `review-verdict-contract.md` | reviewer system prompts + verdict JSON contract | — | — |
| `commit-gate.sh` | cicatrix-native: green-baseline + premature-victory + fix-needs-test gate | — | opt-in |
| `validate-bash.sh` | aggressive bash discipline (bans rm/&&/pipes/bare-cat). **OPT-IN:** inert unless `CICATRIX_STRICT_BASH=1` | — | off |

## Enable (example, per repo)
```jsonc
// <repo>/.claude/settings.json
"hooks": { "PreToolUse": [
  { "matcher": "Edit|Write", "hooks": [
    { "type": "command", "command": "$CLAUDE_PROJECT_DIR/.claude/hooks/review-reasoning.sh", "timeout": 60 },
    { "type": "command", "command": "$CLAUDE_PROJECT_DIR/.claude/hooks/review-edit.sh", "timeout": 60 }
  ]}
]}
```
`CLAUDE_HOOKS_ADVISORY=1` routes passes through a manual prompt instead of auto-approving.

## Adaptation notes (vs upstream)
- Upstream targets macOS bash 3.2; here we run WSL bash 5.x (the `.md`-via-`cat` / no-heredoc
  gymnastics are harmless but unnecessary).
- **TODO (cicatrix value-add):** the reviewers currently use *static* failure-mode taxonomies.
  Wire the janus-datalog bug corpus in — "does this diff reintroduce a *known* failure?" — so the
  gate queries real past bugs (`docs/bugs/resolved/`), not just heuristics.
- Per-edit cost: each gated Edit/Write spawns 1–2 `claude -p` calls. Scope deliberately.
