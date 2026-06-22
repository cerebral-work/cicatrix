# BUG_BACKUP_SPLIT_BRAIN

- **id:** bug:backup-split-brain
- **files:** ~/.config/systemd/user/engram-backup.service, crates/reveried/src/backup.rs, scripts/engram-backup
- **fix-commit:** (ops fix 2026-06-15 — repoint systemd unit at scripts/engram-backup)
- **regression-test:** engram-backup-check staleness alarm (newest backup < 9h)
- **meta-pattern:** Two implementations of one fact drift
- **status:** resolved

## Symptom
202 backup files / 17.6 GB accumulated in `~/.engram/` root with no retention; meanwhile the
supported restore path (`engram-restore --list`, reading `~/.engram/backups/`) saw **none** of
them. Backups existed but were invisible to restore.

## Root cause
Two code paths wrote "the backup": the systemd timer ran the binary `engram backup` (dumps
unpruned `engram.db.<ts>.bak` into the wrong directory, no retention), while the well-engineered
`scripts/engram-backup` (online-backup API + integrity check + prune, writes restore-visible
`backups/engram-<ts>.db`) was orphaned. The mental-model error: assuming "a backup ran" implies
"a *restorable* backup exists." The read path (restore) never saw the active write path.

## Reproduction
Trigger the systemd backup; run `engram-restore --list` → empty, despite fresh `.bak` files in root.

## Resolution
Repoint the systemd unit at `scripts/engram-backup` (restore-visible path + RETAIN=14); add an
hourly staleness alarm writing a `BACKUP_STALE` sentinel if newest backup > 9h.

## Lesson
Single source of truth for any stateful fact. If two paths can write it, the read path must see
every write path — otherwise one silently rots restore-blind.
