---
status: complete
created: '2026-11-12'
tags: [autonomy, ssh-signature, human-signer, allowed-signers, security, ears]
priority: high
depends_on: [0001-cicatrix-regression-db-provider]
scope: [core, autonomy, store, mcp, cli]
---

# 0003-verified-autonomy-promotion

## Requirements

### Context and user stories

`cicatrix autonomy promote` previously recorded `--authorized-by` as an unverified plain-text string.
Downstream callers (`vilicus` `train-delivery` and `vespertin` `saga` gate `wheelhorse:reversibility`) consult `cicatrix autonomy check --json` before allowing unattended actions.
To prevent an autonomous agent or automated script from self-promoting capabilities to the `autonomous` tier, promotions to `autonomous` require cryptographic proof from a human operator key listed in an OpenSSH `allowed_signers` file.

### Functional requirements

* **Ubiquitous:** The autonomy promotion subsystem shall canonicalize promotion payloads (`actor`, `at`, `authorized_by`, `capability`, `from`, `reason`, `to`) and verify detached OpenSSH signatures under namespace `cicatrix.autonomy.promote@v1` via `ssh-keygen -Y verify`.
* **Event-driven:** When `--sign-key` is supplied to `cicatrix autonomy promote`, the CLI shall invoke `ssh-keygen -Y sign` before verifying the resulting signature against the resolved `allowed_signers` file.
* **Event-driven:** When `--signature` and `--signed-at` are supplied to `cicatrix autonomy promote`, the subsystem shall verify the detached SSH signature for `--signer-principal` (defaulting to `--authorized-by`) against the resolved `allowed_signers` file.
* **Unwanted behavior:** If a promotion targets the `autonomous` tier (or `--require-verified` is requested) without a valid cryptographic human signature, then the guard shall refuse the promotion fail-closed before writing to the ledger.
* **Unwanted behavior:** If the signer principal or `--authorized-by` matches a bot, agent, CI, or automated role denylist (`[bot]`, `agent`, `claude`, `codex`, `noreply`, `ci`, `automation`, `volva`, `system`, `wheelhorse`, `harness`), then the verifier shall reject the promotion fail-closed.
* **Ubiquitous:** The autonomy ledger and history JSON output shall record `verified_signer`, `signer_fingerprint`, `signature`, and `verified` on every event, migrating pre-existing SQLite databases additively without breaking historical rows.
* **Ubiquitous:** The `cicatrix autonomy check --json` command shall report `verified_promotion`, `verified_signer`, and `signer_fingerprint` for the current tier, and when `--require-verified` is passed on an unverified `autonomous` row, `can_execute_autonomously` and `permitted` shall evaluate to `false`.
* **Ubiquitous:** The `cicatrix autonomy demote` path shall execute immediately without requiring a cryptographic signature so authority is always revocable at once.

### Non-functional constraints

* **Ubiquitous:** The build and test harness shall execute all cargo commands with `-j 2` concurrency.
* **Ubiquitous:** The SQLite schema constraints shall enforce `:db/neverZeroValue` and paired presence across `verified_signer`, `signer_fingerprint`, and `signature`.

## Design

### Architecture

1. `autonomy::signer`: Canonicalizes `PromotionSignPayload`, enforces `validate_human_principal` aligned with `heimdal::validate_human_signer`, resolves `allowed_signers` (`--allowed-signers` -> `CICATRIX_ALLOWED_SIGNERS` -> `~/.config/cicatrix/allowed_signers` -> `~/.ssh/allowed_signers` -> `git config gpg.ssh.allowedSignersFile`), signs via `ssh-keygen -Y sign`, and verifies via `ssh-keygen -Y verify`.
2. `autonomy::guard`: Enforces the estate Soma invariant first, then requires a verified `PromotionProof` whenever `target_tier == Autonomous` or `require_verified` is set.
3. `autonomy::ledger` & `store::sqlite`: Adds additive nullable columns (`verified_signer`, `signer_fingerprint`, `signature`) via `migrations/0006_autonomy_verified_signer.sql` and idempotent `ALTER TABLE` migration for legacy databases, joining `autonomy_state.last_event_id` to `autonomy_ledger` to expose verification provenance on `get_state` and `check`.

## Tasks

- [x] 1. Add additive SQLite schema columns and migration `migrations/0006_autonomy_verified_signer.sql`.
- [x] 2. Implement OpenSSH signature creation, verification, and human principal validation in `src/autonomy/signer.rs`.
- [x] 3. Enforce verified human signer proof in `src/autonomy/guard.rs` and persist proof metadata in `src/autonomy/ledger.rs`.
- [x] 4. Wire CLI flags and JSON fields in `src/main.rs` and MCP handlers in `src/mcp/tools.rs`.
- [x] 5. Add integration and unit tests covering Ed25519 keys, bot denylists, wrong keys, tampered timestamps, legacy row migration, and unsigned demotion.

## Verification

```bash
cargo run -j 2 -p xtask -- spec-check && cargo test -j 2 --all-targets
```

## Notes

Implements CER-3527 (`cicatrix: autonomy promote must prove a human authorized it`).
Principal validation mirrors `vespertin` `saga/crates/heimdal` human-signer denylist invariants.
