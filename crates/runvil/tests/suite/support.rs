//! What every suite Host hands a Harness: a Host over the suite's Packs
//! that finds the one prompt a test runs.

use std::sync::Arc;

use runvil::harness::{FindTextPrompt, Packs};
use runvil::plugin::PackRegistry;
use runvil::{Harness, Host, RunSetup};

/// The name every suite Host finds its prompt by.
const PROMPT: &str = "prompt.md";

/// A Host's Packs with none but the built-ins.
pub(crate) fn no_packs() -> Packs {
    Packs::new(PackRegistry::new())
}

/// A Harness running `source` with `setup` on a Host over `packs` that
/// finds `source` by its name.
pub(crate) fn launch(packs: Packs, source: &str, setup: RunSetup) -> Harness {
    let host = Arc::new(Host {
        packs,
        prompts: Arc::new(FindTextPrompt::new(PROMPT, source)),
    });
    let prompt = match host.parse(PROMPT) {
        Ok(prompt) => prompt,
        Err(error) => panic!("the suite's prompt parses: {error:?}"),
    };
    Harness::new(host, prompt, setup)
}
