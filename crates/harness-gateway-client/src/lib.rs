//! The standard client a Host uses to run model rounds and list models
//! through the Runvil Gateway, and to offer the Gateway to the Plugins it
//! loads.
//!
//! [`GatewayBroker`] is the [`runvil::harness::InferenceBroker`] that a Host using
//! the Gateway passes to `RunSetup::new`. It runs every model round on a
//! [`GatewayChat`] under the client's own request limits, and it lists the
//! Gateway's models through [`fetch_model_catalog`]. When the Harness
//! passes a piece callback, the broker forwards each live piece to it as
//! it arrives.
//!
//! [`GatewayChat`] is the HTTP client that sends a `Chat` effect's round
//! to one Gateway URL. It presents the Gateway's shared bearer key when
//! built with one. Every request streams: the Gateway's
//! `/chat/completions` endpoint answers with server-sent events (SSE).
//! [`GatewayChat::complete`] sends the request body and reads the stream
//! within the client's byte cap and per-receive timeout. It calls the
//! caller's delta callback as each piece arrives and returns the one
//! completion the round produced. [`fetch_model_catalog`] fetches the
//! Gateway's model list as a typed `ModelCatalog`. The client holds no
//! credential except the Gateway's shared key. The model vendor's
//! credential stays in the Gateway.
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
//! `runvil` excludes tokio, and this crate depends on tokio for its
//! per-receive timeout.
//!
//! Under the client sits the OpenAI chat-completions wire code. It turns a
//! round into a request body, a streamed reply into a
//! [`Completion`](runvil::model::Completion), and a failed response
//! into the [`Error`](runvil::Error) the round fails with.
//!
//! - [`build_request_body`] builds the JSON body that every round sends.
//! - [`read_completion_stream`] reads a streamed reply from a
//!   [`ChunkSource`] the caller supplies, up to its `[DONE]` sentinel and
//!   within a byte cap. It forwards each live delta, assembles the stream
//!   into one completion, and checks that turn against one strict set of
//!   rules.
//! - [`read_body_capped`] reads, within a byte cap, a body that the caller
//!   decodes whole.
//! - [`escape_controls`] cuts a backend error body to a length limit and
//!   escapes its control characters.
//! - [`make_classify_http_failure_error`] turns a failure status and its body into an
//!   error of the matching model code. [`make_classify_stream_error`] does
//!   the same for an error envelope that arrives inside the stream.
//!
//! The client, or another broker, owns the connection and supplies the
//! wire code's chunks and clock.
//!
//! ## Invariants
//!
//! - Every `Completion` and `ToolCall` is built through the Engine's public
//!   validating constructors, so the Engine's model-independent reply
//!   checks run on every decoded turn.
//! - A Gateway bearer key is never written to logs, `Debug`, `Display`, or
//!   error text.
//! - The key leaves the crate in two places only: the `Authorization`
//!   header of a request to the Gateway, and the `runvil/web-search` entry
//!   that `provide_gateway_resources` offers, which holds it as plain text
//!   for the Plugin that calls the Gateway.
//! - A backend error body is bounded and control-escaped before it is
//!   kept, and only in the error's message, never in its prose.
//! - A client sends requests without a key only when the caller builds it
//!   with `GatewayChat::keyless`, which does not check the endpoint's
//!   address, or when `GatewayChat::from_env` finds no key for a loopback
//!   URL. A client built with `GatewayChat::disabled` also holds no key,
//!   but it sends no requests.

mod broker;
mod catalog;
mod config;
mod failure;
mod resources;
mod timer;
mod transport;
mod wire;

pub use broker::GatewayBroker;
pub use catalog::fetch_model_catalog;
pub use config::GatewayEndpoint;
pub use config::SecretString;
pub use resources::provide_gateway_resources;
pub use timer::TokioTimer;
pub use transport::GatewayChat;
pub use wire::classify::make_classify_http_failure_error;
pub use wire::classify::make_classify_stream_error;
pub use wire::delta::StreamDelta;
pub use wire::read::ChunkSource;
pub use wire::read::read_body_capped;
pub use wire::read::read_completion_stream;
pub use wire::request::build_request_body;
pub use wire::stream::escape_controls;
