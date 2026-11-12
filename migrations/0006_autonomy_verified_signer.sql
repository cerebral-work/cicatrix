-- Schema migration 0006: Cryptographic human signer proof on autonomy promotions (CER-3527).
-- Additive nullable columns for append-only `autonomy_ledger` rows so pre-CER-3527 historical
-- rows remain readable as unverified (`verified_signer IS NULL`) while verified promotions
-- record the authenticated principal, SSH public key fingerprint, and SSHSIG PEM block.
--
-- Note: Applied idempotently via `ensure_column` in `src/store/sqlite.rs` for existing databases.

ALTER TABLE autonomy_ledger ADD COLUMN verified_signer TEXT CHECK (verified_signer IS NULL OR length(trim(verified_signer)) > 0);
ALTER TABLE autonomy_ledger ADD COLUMN signer_fingerprint TEXT CHECK (signer_fingerprint IS NULL OR length(trim(signer_fingerprint)) > 0);
ALTER TABLE autonomy_ledger ADD COLUMN signature TEXT CHECK (signature IS NULL OR length(trim(signature)) > 0);
