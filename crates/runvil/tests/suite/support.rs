//! What every suite Host hands a Harness: a Host that finds the one
//! prompt a test runs, with the built-in Packs alone or with the Packs of
//! a test's Rust Plugin beside them.

use std::sync::Arc;

use runvil::harness::FindTextPrompt;
use runvil::plugin::PackRegistry;
use runvil::{Error, Harness, Host, RunSetup};

/// The name every suite Host finds its prompt by.
const PROMPT: &str = "prompt.md";

/// A Harness running `source` with `setup` on a Host that has the
/// built-in Packs alone and finds `source` by its name.
pub(crate) fn launch(source: &str, setup: RunSetup) -> Harness {
    let host = Host::builder(FindTextPrompt::new(PROMPT, source)).build();
    harness_on(host, setup)
}

/// A Harness running `source` with `setup` on a Host whose one Rust
/// Plugin is `install` and that finds `source` by its name.
pub(crate) fn launch_with(
    install: impl FnOnce(&mut PackRegistry) -> Result<(), Error> + Send + 'static,
    source: &str,
    setup: RunSetup,
) -> Harness {
    let host = Host::builder(FindTextPrompt::new(PROMPT, source))
        .with_rust_plugin(install)
        .build();
    harness_on(host, setup)
}

/// A Harness running the prompt `host` finds as [`PROMPT`] with `setup`.
fn harness_on(host: Arc<Host>, setup: RunSetup) -> Harness {
    let prompt = match host.parse(PROMPT) {
        Ok(prompt) => prompt,
        Err(error) => panic!("the suite's prompt parses: {error:?}"),
    };
    Harness::new(host, prompt, setup)
}
