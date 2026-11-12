//! Family-matrix fixtures: the dependency rules between product families
//! and the classification itself. Container-privacy fixtures sit in
//! `container_tests.rs`.

use super::test_support::{workspace_root, write_crate};
use super::*;

#[test]
fn workspace_respects_the_product_boundary() {
    let violations = product_boundary_violations(&workspace_root());
    assert!(
        violations.is_empty(),
        "product-boundary violations:\n{}",
        violations.join("\n")
    );
}

#[test]
fn an_outside_crate_reaching_past_the_one_public_crate_is_reported() {
    let root = tempfile::TempDir::new().expect("tempdir");
    write_crate(
        root.path(),
        "outside-tool",
        "outside-tool",
        "[dependencies]\nrunvil-lua = { path = \"../runvil-lua\" }\nrunvil-engine = { path = \"../runvil-engine\" }\n",
    );
    write_crate(root.path(), "runvil-lua", "runvil-lua", "");
    write_crate(root.path(), "runvil-engine", "runvil-engine", "");
    let violations = product_boundary_violations(root.path());
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert!(
        violations[0].contains("outside-tool") && violations[0].contains("runvil-lua"),
        "the violation names the crate and the forbidden dep: {violations:?}"
    );
}

#[test]
fn an_outside_crate_depending_on_a_root_engine_crate_other_than_the_facade_is_reported() {
    let root = tempfile::TempDir::new().expect("tempdir");
    write_crate(
        root.path(),
        "outside-tool",
        "outside-tool",
        "[dependencies]\nrunvil-extra = { path = \"../runvil-extra\" }\n\
         runvil-spare = { path = \"../runvil-spare\" }\n",
    );
    for name in ["runvil-extra", "runvil-spare"] {
        write_crate(root.path(), name, name, "");
    }
    let violations = product_boundary_violations(root.path());
    assert_eq!(violations.len(), 2, "{violations:?}");
    assert!(
        violations
            .iter()
            .all(|v| v.starts_with("outside-tool depends on runvil-")
                && v.ends_with("only through runvil-engine or runvil-plugin")),
        "runvil-engine and runvil-plugin are the public crates, wherever the others sit: {violations:?}"
    );
}

#[test]
fn an_outside_crate_depending_on_the_plugin_contract_passes() {
    let root = tempfile::TempDir::new().expect("tempdir");
    write_crate(
        root.path(),
        "outside-tool",
        "outside-tool",
        "[dependencies]\nrunvil-plugin = { path = \"../runvil-plugin\" }\n",
    );
    write_crate(root.path(), "runvil-plugin", "runvil-plugin", "");
    let violations = product_boundary_violations(root.path());
    assert!(violations.is_empty(), "{violations:?}");
}

#[test]
fn dev_build_and_target_dependencies_are_checked() {
    let root = tempfile::TempDir::new().expect("tempdir");
    write_crate(
        root.path(),
        "outside-tool",
        "outside-tool",
        "[dev-dependencies]\nrunvil-prompt = { path = \"../runvil-prompt\" }\n\
         [target.'cfg(windows)'.dependencies]\nharness-runner = { path = \"../harness-runner\" }\n",
    );
    write_crate(root.path(), "runvil-prompt", "runvil-prompt", "");
    write_crate(root.path(), "harness-runner", "harness-runner", "");
    let violations = product_boundary_violations(root.path());
    assert_eq!(
        violations.len(),
        2,
        "the dev-dependency and the target-specific dependency are both reported: {violations:?}"
    );
}

#[test]
fn package_renames_are_resolved_before_classification() {
    let root = tempfile::TempDir::new().expect("tempdir");
    write_crate(
        root.path(),
        "outside-tool",
        "outside-tool",
        "[dependencies]\npf = { package = \"runvil-store\", path = \"../runvil-store\" }\n",
    );
    write_crate(root.path(), "runvil-store", "runvil-store", "");
    let violations = product_boundary_violations(root.path());
    assert_eq!(
        violations.len(),
        1,
        "the renamed dependency on runvil-store is reported: {violations:?}"
    );
}

#[test]
fn an_engine_crate_depending_on_the_harness_facade_is_reported() {
    let root = tempfile::TempDir::new().expect("tempdir");
    write_crate(
        root.path(),
        "runvil-internal/runtime",
        "runvil-runtime",
        "[dependencies]\nrunvil = { path = \"../../runvil\" }\n",
    );
    write_crate(root.path(), "runvil", "runvil", "");
    let violations = product_boundary_violations(root.path());
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert!(
        violations[0].contains("Engine crates must not depend on harness crates"),
        "the violation names the harness ban: {violations:?}"
    );
}

#[test]
fn an_outside_crate_depending_on_the_facade_passes() {
    let root = tempfile::TempDir::new().expect("tempdir");
    write_crate(
        root.path(),
        "outside-tool",
        "outside-tool",
        "[dependencies]\nrunvil-engine = { path = \"../runvil-engine\" }\n",
    );
    write_crate(root.path(), "runvil-engine", "runvil-engine", "");
    let violations = product_boundary_violations(root.path());
    assert!(
        violations.is_empty(),
        "the Engine facade is a legal dependency for outside crates: {violations:?}"
    );
}

#[test]
fn an_unparseable_manifest_is_reported() {
    let root = tempfile::TempDir::new().expect("tempdir");
    let dir = root.path().join("crates").join("broken");
    std::fs::create_dir_all(&dir).expect("the crate directory creates");
    std::fs::write(dir.join("Cargo.toml"), "not [valid toml").expect("the manifest writes");
    let violations = workspace_crates(root.path()).violations;
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert!(
        violations[0].contains("unparseable manifest"),
        "the violation reports the parse failure: {violations:?}"
    );
    assert!(
        product_boundary_violations(root.path()).is_empty(),
        "the read failure belongs to the marker check, not this one"
    );
}

#[test]
fn a_harness_crate_depending_on_the_engine_facade_passes() {
    let root = tempfile::TempDir::new().expect("tempdir");
    write_crate(
        root.path(),
        "harness-internal/runner",
        "harness-runner",
        "[dependencies]\nrunvil-engine = { path = \"../../runvil-engine\" }\n",
    );
    write_crate(root.path(), "runvil-engine", "runvil-engine", "");
    let violations = product_boundary_violations(root.path());
    assert!(
        violations.is_empty(),
        "the Engine facade is legal for harness crates: {violations:?}"
    );
}

#[test]
fn a_harness_crate_depending_on_a_private_product_crate_is_reported() {
    let root = tempfile::TempDir::new().expect("tempdir");
    write_crate(
        root.path(),
        "harness-internal/plugins",
        "harness-plugins",
        "[dependencies]\nrunvil-runtime = { path = \"../../runvil-runtime\" }\n",
    );
    write_crate(root.path(), "runvil-runtime", "runvil-runtime", "");
    let violations = product_boundary_violations(root.path());
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert!(
        violations[0].starts_with("harness-plugins depends on runvil-runtime:")
            && violations[0].contains(
                "outside crates may depend on the Engine family only through runvil-engine or runvil-plugin"
            ),
        "the violation names the harness crate and the facade rule: {violations:?}"
    );
}

#[test]
fn a_harness_crate_reaching_past_the_engine_facade_is_reported() {
    let root = tempfile::TempDir::new().expect("tempdir");
    write_crate(
        root.path(),
        "harness-internal/runner",
        "harness-runner",
        "[dependencies]\nrunvil-lua = { path = \"../../runvil-lua\" }\n",
    );
    write_crate(root.path(), "runvil-lua", "runvil-lua", "");
    let violations = product_boundary_violations(root.path());
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert!(
        violations[0].starts_with("harness-runner depends on runvil-lua:"),
        "the single-public-crate rule binds harness crates: {violations:?}"
    );
}

#[test]
fn an_outside_crate_depending_on_the_harness_facade_passes() {
    let root = tempfile::TempDir::new().expect("tempdir");
    write_crate(
        root.path(),
        "outside-tool",
        "outside-tool",
        "[dependencies]\nrunvil = { path = \"../runvil\" }\n",
    );
    write_crate(root.path(), "runvil", "runvil", "");
    let violations = product_boundary_violations(root.path());
    assert!(
        violations.is_empty(),
        "runvil is the harness public crate for outside crates: {violations:?}"
    );
}

#[test]
fn the_engine_facade_depending_on_the_harness_facade_is_reported() {
    let root = tempfile::TempDir::new().expect("tempdir");
    write_crate(root.path(), "runvil", "runvil", "");
    write_crate(
        root.path(),
        "runvil-engine",
        "runvil-engine",
        "[dependencies]\nrunvil = { path = \"../runvil\" }\n",
    );
    let violations = product_boundary_violations(root.path());
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert!(
        violations[0].starts_with("runvil-engine depends on runvil:"),
        "the Engine facade depending on the harness facade is reported: {violations:?}"
    );
}

#[test]
fn family_classification_follows_the_naming_rules() {
    assert_eq!(family("runvil-runtime"), Family::Engine);
    assert_eq!(family("build-xtask"), Family::Build);
    assert_eq!(family("serde"), Family::Unaffiliated);
}

#[test]
fn the_runvil_package_belongs_to_the_harness_family() {
    assert_eq!(family("runvil"), Family::Harness);
    assert_eq!(family("harness-runner"), Family::Harness);
    assert_eq!(family("harness"), Family::Unaffiliated);
    assert_eq!(family("harnesses"), Family::Unaffiliated);
}

#[test]
fn only_a_prefixed_runvil_package_belongs_to_the_engine_family() {
    assert_eq!(family("runvil-engine"), Family::Engine);
    assert_eq!(family("runvilr"), Family::Unaffiliated);
}
