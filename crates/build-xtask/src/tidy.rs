//! Tidy-style architecture checks for the Harness family, the sans-I/O
//! Engine (dependency guard, clippy guard, and `test-support` leak guard,
//! run from `engine_guards`), the `runvil-engine` and `runvil` facades'
//! source shape (run from `facade_shape`), the `doc(hidden)` ban over both
//! facades and their containers (run from `doc_hidden`), the doctest ban
//! over every workspace crate (run from `no_doctests`), the `Debug`
//! trait's one spelling over every workspace crate (run from
//! `debug_spelling`), the unsafe allowlist (run from `unsafe_allowlist`),
//! and the type-size limits over every workspace crate (run from
//! `type_size`).
//!
//! Each check returns a list of human-readable violations. The `#[test]`
//! wrappers assert the lists are empty, so `cargo test -p build-xtask`
//! enforces the architecture; `cargo xtask tidy` prints the same report
//! on demand. The lint inheritance check binds every workspace crate. The
//! shared walk's read failures are reported by `marker_violations`, their
//! one owner.

use std::fs;
use std::path::{Path, PathBuf};

/// Marker in a crate's `lib.rs` (or `main.rs`) crate docs. Mandatory for
/// every `harness-*` and `plugin-*` crate (see [`family_requires_marker`]);
/// optional on any other crate.
const INVARIANT_MARKER: &str = "//! ## Invariants";

/// Runs every check and returns all violations.
#[must_use]
pub(crate) fn all_violations(root: &Path) -> Vec<String> {
    let mut violations = marker_violations(root);
    violations.extend(lint_inheritance_violations(root));
    violations.extend(crate::unsafe_allowlist::unsafe_allowlist_violations(root));
    violations.extend(crate::product::product_boundary_violations(root));
    violations.extend(crate::harness_bans::harness_tokio_bans(
        &root.join("crates").join("harness-internal"),
        &root.join("crates").join("runvil"),
    ));
    violations.extend(crate::engine_guards::engine_guard_violations(root));
    violations.extend(crate::facade_shape::facade_shape_violations(root));
    violations.extend(crate::doc_hidden::doc_hidden_violations(root));
    violations.extend(crate::no_doctests::no_doctests_violations(root));
    violations.extend(crate::debug_spelling::debug_spelling_violations(root));
    violations.extend(crate::type_size::type_size_violations(root));
    violations
}

/// Checks that every workspace crate, unread ones included, inherits
/// `[lints] workspace = true` (which includes `unreachable_pub`) and that
/// the workspace root sets it. A walk that found no crate fails: a crate
/// that was never read cannot be shown to inherit.
#[must_use]
fn lint_inheritance_violations(root: &Path) -> Vec<String> {
    let mut violations = Vec::new();
    let root_manifest = root.join("Cargo.toml");
    match fs::read_to_string(&root_manifest)
        .ok()
        .and_then(|text| toml::from_str::<toml::Value>(&text).ok())
    {
        Some(manifest) => {
            let set = manifest
                .get("workspace")
                .and_then(|w| w.get("lints"))
                .and_then(|l| l.get("rust"))
                .and_then(|r| r.get("unreachable_pub"));
            if set.is_none() {
                violations.push(
                    "workspace root does not set `unreachable_pub` in [workspace.lints.rust]"
                        .to_owned(),
                );
            }
        }
        None => violations.push(format!("{}: unparseable manifest", root_manifest.display())),
    }
    let walk = crate::product::workspace_crates(root);
    let dirs: Vec<&PathBuf> = walk
        .crates
        .iter()
        .map(|krate| &krate.dir)
        .chain(&walk.unread)
        .collect();
    if dirs.is_empty() {
        violations.push(format!(
            "{}: the lint inheritance check scanned nothing: no workspace crate was found, \
             so none can be shown to inherit `[lints] workspace = true`",
            root.join("crates").display()
        ));
    }
    for dir in dirs {
        let manifest_path = root.join(dir).join("Cargo.toml");
        let inherits = fs::read_to_string(&manifest_path)
            .ok()
            .and_then(|text| toml::from_str::<toml::Value>(&text).ok())
            .and_then(|manifest| {
                manifest
                    .get("lints")
                    .and_then(|l| l.get("workspace"))
                    .and_then(toml::Value::as_bool)
            })
            .unwrap_or(false);
        if !inherits {
            violations.push(format!(
                "{} does not inherit `[lints] workspace = true`",
                manifest_path.display()
            ));
        }
    }
    violations
}

/// Check that every crate the families bind by name has the marker.
///
/// A manifest the walk could not read, parse, or find a package name in is
/// reported as itself, not skipped: a crate with no readable name cannot be
/// shown exempt from the marker. This check is the one owner of the shared
/// walk's read failures; the product-boundary check shares the walk and
/// leaves them here.
#[must_use]
fn marker_violations(root: &Path) -> Vec<String> {
    let walk = crate::product::workspace_crates(root);
    let mut violations = walk.violations;
    for krate in &walk.crates {
        if family_requires_marker(&krate.package) && !has_marker(&root.join(&krate.dir)) {
            violations.push(format!(
                "{}: src/lib.rs lacks the `{INVARIANT_MARKER}` marker required of every \
                 harness-* and plugin-* crate",
                krate.package
            ));
        }
    }
    violations
}

/// Whether a package name places the crate in a family that must have the
/// marker: `harness-*` and `plugin-*`. The `runvil` facade falls outside
/// both prefixes, so it is exempt.
fn family_requires_marker(name: &str) -> bool {
    ["harness-", "plugin-"]
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

/// Whether a crate's `lib.rs` or `main.rs` crate docs have the marker.
fn has_marker(dir: &Path) -> bool {
    ["src/lib.rs", "src/main.rs"].iter().any(|candidate| {
        fs::read_to_string(dir.join(candidate)).is_ok_and(|text| text.contains(INVARIANT_MARKER))
    })
}

/// A file's path relative to the workspace root with `/` separators, so
/// the allowlist and the violations read the same on every platform.
///
/// A component that is not valid UTF-8 is rendered lossily rather than
/// dropped: dropping it would shorten the path and could make it match an
/// allowlist entry, exempting a file the rule should report.
pub(crate) fn slash_path(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Every `.rs` file under `dir`, recursively.
pub(crate) fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rust_files(dir, &mut files);
    files
}

fn collect_rust_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if entry.file_name() != "target" {
                collect_rust_files(&path, files);
            }
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
}

#[cfg(test)]
#[path = "tidy-tests.rs"]
mod tests;
