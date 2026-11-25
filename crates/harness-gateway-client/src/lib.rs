//! The standard adapter a Host uses to run model rounds and list models
//! through the Runvil Gateway, and to offer the Gateway to the Plugins it
//! loads.
//!
//! [`GatewayBroker`] is the [`runvil::harness::InferenceBroker`] that a Host
//! using the Gateway passes to `RunSetup::new`. It runs every model round on
//! a [`gateway_client::GatewayChat`] under the client's own request limits,
//! and it lists the Gateway's models through [`gateway_client::fetch_models`].
//! It streams: it hands each live piece of a reply, as the Harness's `Delta`,
//! to the round's `on_piece` callback as the piece arrives.
//!
//! The HTTP client, the wire code, and the bearer key's handling on the wire
//! live in `gateway-client`, a repository of its own that this crate depends
//! on at a pinned commit. The client has plain types of its own for a
//! message, a tool, an options value, a reply, and an error. This crate
//! converts between those types and the Engine's, through four traits of its
//! own:
//!
//! - A round's tools and options become the client's, and its messages go
//!   to the client as the Engine writes them, because the Engine's message
//!   type serializes in the chat-completions shape.
//! - The client's reply becomes the Engine's `Completion`, with the metrics
//!   the Engine defines read from the reply's response.
//! - The client's error becomes the Engine's `Error`, with its message and
//!   prose copied as they are and its facts kept.
//! - The client's model list becomes the Engine's `ModelCatalog` under the
//!   Harness's rules: an entry with no context window is skipped, a zero
//!   window is refused, and an entry with a window and no thinking mode is
//!   refused.
//!
//! [`provide_gateway_resources`] offers the Gateway to the Plugins a Host
//! loads through the C contract. It adds the Gateway's web search to the
//! Host's `HostResources` as the entry `runvil/web-search`, which holds
//! the URL of the Gateway's `/tools/web_search` route and the Gateway's
//! key. Every tool call of such a Plugin is lent the entry, and a Plugin
//! that searches the web sends its request to the Gateway itself. The
//! search vendor's credential also stays in the Gateway.
//!
//! [`TokioTimer`] is the stock timer of a Host that runs on tokio. Each of
//! its sleeps is one sleep on tokio's timer. It is in this crate because
//! `runvil` excludes tokio, and this crate depends on tokio for the sleeps.
//!
//! [`GatewayEndpoint`] and [`SecretString`] are the client's types, which
//! this crate re-exports because a Host builds a broker from them.
//!
//! ## Invariants
//!
//! - Every `Completion` and `ToolCall` is built through the Engine's public
//!   validating constructors, so the Engine's model-independent reply
//!   checks run on every decoded turn.
//! - The key is read in one place: `provide_gateway_resources` reads it
//!   with `SecretString::expose_secret` to write the `runvil/web-search`
//!   entry that the Plugin calling the Gateway presents. The broker hands
//!   the key to the client as a `SecretString` and never reads it, and the
//!   broker's `Debug` output redacts it.

mod broker;
mod catalog;
mod convert;
mod fail;
mod metrics;
mod resources;
#[cfg(test)]
mod test_support;
mod timer;

pub use broker::GatewayBroker;
pub use gateway_client::{GatewayEndpoint, SecretString};
pub use resources::provide_gateway_resources;
pub use timer::TokioTimer;
