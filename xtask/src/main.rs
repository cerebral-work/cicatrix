//! Repo automation. Run with `cargo xtask <command>`.
//!
//! Commands:
//! - `spec-check`: validate `specs/` (see `spec_check.rs`).
//! - `tools`, `toolchain-check`, `fmt`, `fmt-check`, `clippy`, `test`, `doc-test`, `deny`,
//!   `typos`, `machete`, `ci` and `install`: the gates of spec 0003 (see `gates.rs`).

mod gates;
mod spec_check;
mod toolchain;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// The repo root: the parent of the `xtask` directory.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

fn main() -> ExitCode {
    let arg = std::env::args().nth(1);
    let root = repo_root();
    match arg.as_deref() {
        Some("spec-check") => spec_check::run(&root),
        other => match other.and_then(gates::Gate::parse) {
            Some(gate) => gates::run(&root, gate),
            None => {
                if let Some(name) = other {
                    eprintln!("xtask: unknown command {name:?}");
                }
                eprintln!("{}", usage());
                ExitCode::from(2)
            }
        },
    }
}

/// The usage line: `spec-check`, then the gates of spec 0003.
fn usage() -> String {
    let mut names = vec!["spec-check"];
    names.extend(gates::Gate::ALL.iter().map(|g| g.name()));
    format!("usage: cargo xtask <{}>", names.join("|"))
}
