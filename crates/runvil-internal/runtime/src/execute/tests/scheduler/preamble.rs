//! The preamble on the scheduler: the Lua between the frontmatter and the
//! first heading, which runs as the first top-level section under the
//! empty name. Each submodule holds one topic and reaches the scenario
//! below through `use super::*`.

use super::*;

/// A run of `source` for a scheduler preamble test: under the test model,
/// which fills all five roles.
fn preamble_scenario(source: &str) -> Scenario {
    Scenario::new(source).bound()
}

mod control_flow;
mod entry;
mod returns_and_prose;
