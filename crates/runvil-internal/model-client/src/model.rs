//! Prompt-local model bindings: a binding's request settings, the set a
//! prompt fills, and the builders of a round's failure.
//!
//! The caller supplies its role map;
//! [`ModelSet`] holds the five roles filled from it for one prompt, and the
//! prompt's first step checks the requirements it declares against the model
//! that fills each role. `models.use` selects one binding for a section, and
//! a section that never calls it runs on the `default` role. A model-facing
//! section with no model fails with a model-binding failure surfaced through
//! the run error the caller receives.
pub(crate) mod error;
mod options;

pub use error::{
    make_completion_error, make_context_overflow_error, make_empty_reply_error, phrase,
};
pub use options::{CompletionOptions, ModelBinding, ModelInvocation, ModelSet, Temperature};
// `runvil-lua` names the model id through this path; the rest of the catalog
// vocabulary is imported from `runvil-types` directly.
pub use runvil_types::models::ModelId;

#[cfg(test)]
#[path = "model-tests.rs"]
mod tests;
