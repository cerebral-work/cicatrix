---
status: in-progress
created: '2026-10-14'
tags: [xtask, gates, ci, tools, toolchain, ears]
priority: high
scope: [xtask, ci]
---

# 0003-cargo-xtask-gates

## Requirements

### Context and user stories

Operator decision, Christian, 2026-10-13: `cargo xtask` is the one build entrypoint for every estate repo, with nothing behind it but cargo and rustup.
This specification adds the gates that `cargo xtask ci` and `cargo xtask install` run.
They sit beside `spec-check`, `.cicatrix/establish-baseline.sh` and the existing CI jobs. Nothing is removed.
The repository had no `rust-toolchain.toml`, and its CI builds with whatever `stable` is that day.
This specification pins Rust 1.99.0, the stable its CI built with on its last green run of `main` (run 38065706318, 2026-10-10).

### Functional requirements

* **XT-1 (Ubiquitous):** The xtask shall provide the commands `tools`, `toolchain-check`, `fmt`, `fmt-check`, `clippy`, `test`, `doc-test`, `deny`, `typos`, `machete`, `ci` and `install`, and keep `spec-check` under its name.
* **XT-2 (Ubiquitous):** The xtask shall keep the version of each tool it runs in one table: cargo-deny 0.19.9, typos-cli 1.49.0, cargo-machete 0.9.1 and cargo-nextest 0.9.104.
* **XT-3 (Event-driven):** When `tools` runs, the xtask shall ask each tool for its version the way the gates run it, with `.tools/bin` first on `PATH`, and shall run `cargo install --locked --version <pin> --root .tools --force <crate>` under Rust 1.94.1 (`rustup run --install 1.94.1`) only for a tool that is missing or at another version.
* **XT-4 (Unwanted behavior):** If a tool still reports another version after its install, then `tools` shall fail and name the tool and both versions.
* **XT-5 (Unwanted behavior):** If `rustc --version` differs from the `channel` in `rust-toolchain.toml`, or a `rust-version` in `Cargo.toml` is above that channel, then `toolchain-check` shall fail and name the values.
* **XT-6 (Ubiquitous):** The xtask shall run each gate from the repository root with `.tools/bin` first on `PATH`, `-j N` where N is `JOBS`, else 2 (the `-j 2` of `.cicatrix/establish-baseline.sh`), and without the variables cargo sets for the xtask binary itself (`CARGO_PKG_*`, `CARGO_MANIFEST_DIR`, `CARGO_MANIFEST_PATH`, `CARGO_CRATE_NAME`, `CARGO_BIN_NAME`, `CARGO_PRIMARY_PACKAGE`).
* **XT-7 (Event-driven):** When `ci` runs, the xtask shall run `tools`, `toolchain-check`, `fmt-check`, `clippy`, `test`, `doc-test`, `deny`, `typos` and `machete` in that order, stop at the first step that fails, print one summary line per step, and exit non-zero if a step failed.
* **XT-8 (Event-driven):** When `install` runs, the xtask shall run `ci`, then `cargo build --release --locked --workspace --exclude xtask --bins -j N`.
* **XT-9 (Event-driven):** When CI runs for a push or a pull request, the job `xtask-ci` shall run `cargo xtask ci` on `ubuntu-latest`, the runner of the existing Rust jobs.
* **XT-10 (Ubiquitous):** The repository root shall carry a `rust-toolchain.toml` that pins Rust 1.99.0 with the `rustfmt` and `clippy` components.
* **XT-11 (State-driven):** While no workspace target is one cargo doc-tests (`cargo metadata --no-deps` reports no `"doctest": true` target; cicatrix and xtask are binaries only), `doc-test` shall print that it has nothing to test and pass, instead of running `cargo test --doc`, which fails with "no library targets found".

### Non-functional constraints

The xtask depends on the standard library only.
The tools build once into `.tools/`, which is gitignored; CI keeps `.tools/` in the rust-cache.

## Design

### Architecture

| Gate | Runs |
|---|---|
| `fmt` | `cargo fmt --all` |
| `fmt-check` | `cargo fmt --all -- --check` |
| `clippy` | `cargo clippy --workspace --all-targets --locked -j N -- -D warnings` |
| `test` | `cargo nextest run --workspace --locked --build-jobs N`; `cargo test --workspace --locked -j N` when nextest is missing |
| `doc-test` | `cargo test --doc --workspace --locked -j N` |
| `deny` | `cargo deny check` |
| `typos` | `typos` |
| `machete` | `cargo machete` |

The code is `xtask/src/gates.rs` (the gates) and `xtask/src/toolchain.rs` (the comparison of XT-5).

### State machines and invariants

`ci` is a fixed sequence. A failed step ends the run; later steps report `not run`, never `ok`.

### Error containment

Each step returns its error as text. `ci` prints it on the step's summary line and exits non-zero.

## Tasks

- [x] 1. Write the clauses XT-1 to XT-10.
- [ ] 2. Write unit tests named for the clauses, red first.
- [ ] 3. Add the gates, `toolchain-check` and `rust-toolchain.toml` (XT-1 to XT-8, XT-10).
- [ ] 4. Add the CI job `xtask-ci` (XT-9).

## Verification

```bash
cargo test -p xtask -j 2
cargo xtask ci
```

## Notes

The pin fixes the Rust that the existing CI jobs build with: `dtolnay/rust-toolchain@stable` installs stable, then rustup follows `rust-toolchain.toml` in the repository. Today that is the same 1.99.0; when stable moves, the pin holds.
The estate default for a repository without a pin is 1.94.1, the `ci-rust` image's Rust. It was the first pin here, and it was replaced: clippy 1.94.1 fails `clippy::overly_complex_bool_expr` (deny by default) on `src/cortex/event.rs` (`is_negative_verdict`), which clippy 1.99.0 does not report, so the existing `fmt + clippy` job would have gone red. Whether that expression is meant as written is a question for its owner, not part of this spec.
