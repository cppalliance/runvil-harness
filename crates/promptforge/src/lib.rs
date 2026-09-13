//! PromptForge integrator-facing facade.
//!
//! One entry point: [`pipeline`] for document prompts (`.md`). This crate
//! re-exports only; it never grows logic or types of its own.
//!
//! Integrators who need substrate types (parser, store, tools, models)
//! depend on those crates directly.

/// Document prompts (`.md`): sections, prose, the built-in tool loop.
pub mod pipeline {
    pub use promptforge_core::execute::run;
    pub use promptforge_core::execute::{RunConfig, RunError};
    pub use promptforge_core::input::{
        INPUT_UNAVAILABLE_FALLBACK, InputBroker, InputError, InputOutcome,
    };
}
