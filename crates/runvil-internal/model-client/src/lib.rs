//! The Runvil model vocabulary: what a model round exchanges, and how a
//! prompt binds models. No transport.
//!
//! [`client`] holds the chat-completions protocol vocabulary: the wire
//! types that go out of the Engine in a `Chat` effect and come back in its
//! answer ([`client::Message`], [`client::ToolSchema`],
//! [`client::Completion`]), with the validating constructors that run the
//! neutral reply checks on every completion. [`model`] holds the binding
//! vocabulary: [`model::ModelBinding`] with its request settings,
//! [`model::ModelSet`], one prompt's filled roles, and the builders of a
//! round's failure, [`model::make_completion_error`] among them.
//!
//! The model identity and catalog vocabulary
//! ([`ModelId`](runvil_types::models::ModelId),
//! [`ModelCatalog`](runvil_types::models::ModelCatalog),
//! [`ModelInfo`](runvil_types::models::ModelInfo),
//! [`ThinkingMode`](runvil_types::models::ThinkingMode)) lives in
//! `runvil-types`, and a caller builds its catalog there. The metrics
//! vocabulary ([`runvil_types::metrics`], which holds `Usage`,
//! `LlamaTimings`, `VllmMetrics`, `ClientTiming`, and `CallMetrics`) lives
//! there too: [`client::Completion`] holds a round's metrics.
//!
//! This crate contains no HTTP, no wire parsing, no prompt parser, no Lua
//! runtime, and no executor.
//!
//! ## Invariants
//!
//! - May depend on: `runvil-types`. `cargo test -p build-xtask`
//!   enforces the product and container boundaries.

pub mod client;
pub mod detail;
pub mod model;
mod normalize;
#[cfg(test)]
mod test_support;

/// Crate-internal result alias over the failure a model round reports.
type Result<T> = std::result::Result<T, runvil_types::error::Error>;
