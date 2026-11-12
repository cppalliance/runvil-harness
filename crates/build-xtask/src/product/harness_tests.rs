//! Harness-family fixtures: outside their family, Harness crates depend on
//! `runvil-engine` and `runvil-plugin` only, and outside crates reach
//! the family only through `runvil` and `harness-gateway-client`.

use super::product_boundary_violations;
use super::test_support::write_crate;

#[test]
fn a_harness_crate_depending_on_the_plugin_contract_passes() {
    let root = tempfile::TempDir::new().expect("tempdir");
    write_crate(
        root.path(),
        "harness-internal/plugins",
        "harness-plugins",
        "[dependencies]\nrunvil-plugin = { path = \"../../runvil-plugin\" }\n",
    );
    write_crate(root.path(), "runvil-plugin", "runvil-plugin", "");
    let violations = product_boundary_violations(root.path());
    assert!(
        violations.is_empty(),
        "runvil-plugin is the second public Engine crate harness crates may name: {violations:?}"
    );
}

#[test]
fn a_harness_crate_build_depending_on_a_build_crate_is_reported() {
    let root = tempfile::TempDir::new().expect("tempdir");
    write_crate(
        root.path(),
        "harness-internal/runner",
        "harness-runner",
        "[build-dependencies]\nbuild-xtask = { path = \"../../build-xtask\" }\n",
    );
    write_crate(root.path(), "build-xtask", "build-xtask", "");
    let violations = product_boundary_violations(root.path());
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert!(
        violations[0].starts_with("harness-runner depends on build-xtask:")
            && violations[0].contains(
                "harness crates must not depend on build-* crates; outside their family they may name only runvil-engine and runvil-plugin"
            ),
        "build-* crates are closed to harness crates in every dependency table: {violations:?}"
    );
}

#[test]
fn a_harness_crate_depending_on_an_unaffiliated_crate_is_reported() {
    let root = tempfile::TempDir::new().expect("tempdir");
    write_crate(
        root.path(),
        "harness-internal/plugins",
        "harness-plugins",
        "[dependencies]\nsome-tool = { path = \"../../some-tool\" }\n",
    );
    write_crate(root.path(), "some-tool", "some-tool", "");
    let violations = product_boundary_violations(root.path());
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert!(
        violations[0].starts_with("harness-plugins depends on some-tool:")
            && violations[0].contains(
                "harness crates must not depend on unaffiliated crates; outside their family they may name only runvil-engine and runvil-plugin"
            ),
        "unaffiliated crates are closed to harness crates: {violations:?}"
    );
}

#[test]
fn an_outside_crate_depending_past_the_harness_facade_is_reported() {
    let root = tempfile::TempDir::new().expect("tempdir");
    write_crate(
        root.path(),
        "outside-tool",
        "outside-tool",
        "[dependencies]\nharness-runner = { path = \"../harness-runner\" }\n",
    );
    write_crate(root.path(), "harness-runner", "harness-runner", "");
    let violations = product_boundary_violations(root.path());
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert!(
        violations[0].starts_with("outside-tool depends on harness-runner:")
            && violations[0].ends_with(
                "outside crates may depend on the harness family only through runvil or harness-gateway-client"
            ),
        "the harness facade rule binds every outside crate: {violations:?}"
    );
}

#[test]
fn a_build_crate_depending_past_the_harness_facade_is_reported() {
    let root = tempfile::TempDir::new().expect("tempdir");
    write_crate(
        root.path(),
        "build-xtask",
        "build-xtask",
        "[dependencies]\nharness-runner = { path = \"../harness-internal/runner\" }\n",
    );
    write_crate(root.path(), "harness-internal/runner", "harness-runner", "");
    let violations = product_boundary_violations(root.path());
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert!(
        violations[0].starts_with("build-xtask depends on harness-runner:")
            && violations[0].ends_with(
                "outside crates may depend on the harness family only through runvil or harness-gateway-client"
            ),
        "container privacy exempts build-* crates, but the harness facade rule does not: {violations:?}"
    );
}

#[test]
fn an_outside_crate_depending_on_the_harness_gateway_client_passes() {
    let root = tempfile::TempDir::new().expect("tempdir");
    write_crate(
        root.path(),
        "outside-tool",
        "outside-tool",
        "[dependencies]\nharness-gateway-client = { path = \"../harness-gateway-client\" }\n",
    );
    write_crate(
        root.path(),
        "harness-gateway-client",
        "harness-gateway-client",
        "",
    );
    let violations = product_boundary_violations(root.path());
    assert!(
        violations.is_empty(),
        "harness-gateway-client is the second harness public crate: {violations:?}"
    );
}

#[test]
fn the_harness_gateway_client_depending_into_the_harness_container_is_reported() {
    let root = tempfile::TempDir::new().expect("tempdir");
    write_crate(
        root.path(),
        "harness-gateway-client",
        "harness-gateway-client",
        "[dependencies]\nharness-runner = { path = \"../harness-internal/runner\" }\n",
    );
    write_crate(root.path(), "harness-internal/runner", "harness-runner", "");
    let violations = product_boundary_violations(root.path());
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert!(
        violations[0].starts_with("harness-gateway-client depends on harness-runner:")
            && violations[0].ends_with(
                "crates/harness-internal is private to its family; only runvil may depend into it"
            ),
        "being public does not admit the gateway client into the harness container: {violations:?}"
    );
}
