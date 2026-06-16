# Convention-drift scan — ~/projects — 2026-06-16

Static-analysis arm. Every repo vs the 4 canonical templates (`template-{rust,node,python,terraform}`).
Source: topology survey (6-agent fan-out, 2026-06-16). This is the seed corpus the scanner will
regenerate on a schedule.

Legend: ✓ present · ✗ missing · ~ partial.
Columns: CLAUDE=CLAUDE.md · MK=Makefile/justfile w/ ci target · PC=pre-commit · CI=#workflows ·
LIC=LICENSE · TOOL=toolchain pin · SR=signed-release config · CHG=CHANGELOG.

## Your repos (todie / cerebral-work / unsigned-gg) — prioritized

| Repo | lang | CLAUDE | MK | PC | CI | LIC | TOOL | SR | CHG |
|---|---|---|---|---|---|---|---|---|---|
| reverie | rust | ✓ | ✓ | ✓ | 6 | ✓ | ✓ | ✗ | ✓ |
| engram-rs | rust | ✓ | ✓ | ✓ | 2 | ✓ | ✓ | ✓ | ✗ |
| pact | rust | ✓ | ✓ | ✓ | 7 | ✓ | ✗ | ✗ | ✓ |
| reach | rust | ✓ | ✓ | ✓ | 5 | ✗ | ✗ | ✓ | ✓ |
| revenant | rust | ✓ | ✓ | ✓ | 4 | ✓ | ✗ | ✓ | ✓ |
| linearctl | node | ✓ | ✗ | ✗ | 4 | ✓ | ✗ | ✓ | ✓ |
| unsigned-paas | tf/helm | ✓ | ✓ | ✓ | 16 | ✗ | ✗ | ✓ | ✓ |
| unsigned-gg | web | ✓ | ✗ | ✗ | 1 | ✗ | ✗ | ✗ | ✗ |
| cortex | ? | ✓ | ✗ | ✗ | 1 | ✓ | ✗ | ✗ | ✗ |
| Signal-Desktop | node | ✗ | ✗ | ✗ | 11 | ✓ | ✓ | ✗ | ✗ |
| cerebral-work (container) | ? | ✗ | ✗ | ✗ | 0 | ✗ | ✗ | ✗ | ✗ |

## Ranked normalization actions

1. **cerebral-work** — complete void; clarify intent (container for `reverie-slack-app/`) before backfilling.
2. **Signal-Desktop** — add CLAUDE.md + release-please (from template-node).
3. **unsigned-gg / unsigned-paas** — add LICENSE (ownership marker).
4. **pact / reach / revenant** — add `rust-toolchain.toml` pin (from template-rust); reach also LICENSE.
5. **reverie** — add release-please-config.json (signed-release).
6. **engram-rs** — add CHANGELOG.md.
7. **linearctl** — add `.nvmrc`.

Third-party clones (heretic, rina, cortex, rtk, claude-hud/relay/esp, beepboopd, dreamcode, memd,
todie-daemon*) — **defer**; drift-from-upstream tracking only, not normalization targets.

> Full per-repo inventory + dedup candidates: see the topology survey synthesis (parked).
