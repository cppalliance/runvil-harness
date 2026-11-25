//! The Gateway's inference broker: one model round per `Chat` effect on
//! the Gateway client, and the Gateway's model list.
//!
//! The Harness takes only a round's finished reply, so the broker's
//! `InferenceBroker` round returns it whole. When the Harness passes a
//! piece callback, the broker forwards each live piece to it as it
//! arrives, so no fragment ever reaches the Host's recorder. Either way
//! the completion names the model the round's options sent it to, so a
//! Host that routes a round to another model sees that model on its reply.

use std::fmt;

use runvil::effect::Round;
use runvil::harness::{Delta, InferenceBroker, PieceSink};
use runvil::model::{
    Completion, CompletionOptions, Message, ModelBinding, ModelCatalog, ToolSchema,
};
use runvil::{BoxFuture, Error};

use crate::catalog::fetch_model_catalog;
use crate::config::{GatewayEndpoint, SecretString};
use crate::transport::GatewayChat;
use crate::wire::delta::StreamDelta;

/// A model broker that sends the Harness's model rounds to the Runvil
/// Gateway and lists the Gateway's models.
///
/// A Host passes this broker to the Harness as its [`InferenceBroker`].
/// The broker performs each `Chat` effect from the Engine as one model
/// round on a [`GatewayChat`]. It lists the Gateway's models through
/// [`fetch_model_catalog`]. Both kinds of request go to the same API root
/// and authenticate with the same bearer key.
///
/// Every round uses the client's own request limits: a 120 second wait for
/// each receive and a 16 MiB cap on each reply.
///
/// The broker's futures, [`models`](InferenceBroker::models) included, need
/// a tokio runtime with its reactor and timer. A Host that uses this broker
/// must therefore await `Harness::run` inside such a runtime. The Harness's
/// own futures run on any executor.
#[derive(Clone)]
pub struct GatewayBroker {
    client: GatewayChat,
    endpoint: GatewayEndpoint,
    key: SecretString,
}

impl fmt::Debug for GatewayBroker {
    /// The bearer key is never written to logs or `Debug` output.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GatewayBroker")
            .field("client", &self.client)
            .field("api_root", &self.endpoint.url())
            .field("key", &"<redacted>")
            .finish()
    }
}

impl GatewayBroker {
    /// Creates a broker that reaches the Gateway at `endpoint` with the
    /// bearer key `key`.
    ///
    /// `endpoint` is the Gateway's OpenAI-compatible API root, the `/v1`
    /// path. Model rounds and model-list requests both go to it.
    #[must_use]
    pub fn new(endpoint: GatewayEndpoint, key: SecretString) -> GatewayBroker {
        let client = GatewayChat::new(endpoint.clone(), key.clone());
        GatewayBroker {
            client,
            endpoint,
            key,
        }
    }

    /// One round on the client, handing each live piece to `on_piece`.
    fn round(
        &self,
        messages: Vec<Message>,
        tools: Vec<ToolSchema>,
        options: CompletionOptions,
        on_piece: Option<PieceSink>,
    ) -> BoxFuture<'static, Result<Box<Completion>, Error>> {
        let client = self.client.clone();
        Box::pin(async move {
            // An empty advertisement sends no `tools` field at all, the
            // plain chat-completions shape.
            let tools = (!tools.is_empty()).then_some(tools.as_slice());
            let on_delta = move |delta: StreamDelta| {
                if let Some(sink) = on_piece.as_ref() {
                    sink(match delta {
                        StreamDelta::Text(text) => Delta::Text(text),
                        StreamDelta::Reasoning(text) => Delta::Reasoning(text),
                    });
                }
            };
            client
                .complete(&messages, tools, &options, on_delta)
                .await
                .map(Box::new)
        })
    }
}

impl InferenceBroker for GatewayBroker {
    fn models(&self) -> BoxFuture<'static, Result<ModelCatalog, Error>> {
        let api_root = self.endpoint.url().to_owned();
        let key = self.key.clone();
        Box::pin(async move { fetch_model_catalog(&api_root, key.expose()).await })
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn chat(
        &self,
        _binding: ModelBinding,
        messages: Vec<Message>,
        tools: Vec<ToolSchema>,
        options: CompletionOptions,
        _round: Round,
        on_piece: Option<PieceSink>,
    ) -> BoxFuture<'static, Result<Box<Completion>, Error>> {
        self.round(messages, tools, options, on_piece)
    }
}

#[cfg(test)]
#[path = "broker-tests.rs"]
mod tests;
