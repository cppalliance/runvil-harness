//! `build-xtask` - workspace automation for the Runvil repository.
//!
//! ## Invariants
//!
//! - The tidy-style architecture checks run as tests
//!   (`cargo test -p build-xtask`); `cargo xtask tidy` prints the same
//!   report on demand.
//! - `cargo xtask api` reads rustdoc JSON, so it runs only on the nightly
//!   pinned in `api/toolchain.rs`; its fixtures that build rustdoc JSON are
//!   `#[ignore]`d on stable.

mod api;
mod debug_spelling;
mod doc_hidden;
mod engine_clippy;
mod engine_deps;
mod engine_guards;
mod facade_shape;
mod harness_bans;
mod manifest;
mod no_doctests;
mod product;
mod test_support_leak;
mod tidy;
mod type_size;
mod unsafe_allowlist;

use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let Some(root) = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2) else {
        eprintln!("error: cannot locate workspace root from CARGO_MANIFEST_DIR");
        return ExitCode::FAILURE;
    };
    match args.get(1).map(String::as_str) {
        Some("api") => api::run(root, &args[2..]),
        Some("tidy") => {
            let violations = tidy::all_violations(root);
            if violations.is_empty() {
                println!("tidy: no violations");
                ExitCode::SUCCESS
            } else {
                for violation in &violations {
                    eprintln!("tidy: {violation}");
                }
                ExitCode::FAILURE
            }
        }
        _ => usage(),
    }
}

fn usage() -> ExitCode {
    eprintln!("usage: cargo xtask tidy | cargo +<pinned nightly> xtask api [--check | --bless]");
    ExitCode::from(2)
}
