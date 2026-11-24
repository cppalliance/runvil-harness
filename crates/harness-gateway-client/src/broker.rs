//! The Gateway's inference broker: one model round per `Chat` effect on
//! the Gateway client, and the Gateway's model list.
//!
//! The Harness takes only a round's finished reply, so the broker's
//! [`InferenceBroker::chat`] round returns it whole. A Host that shows the
//! reply as it forms attaches an unbounded channel sender with
//! [`GatewayBroker::with_streaming`] before the run; every `Chat`-origin round
//! then sends each [`StreamDelta`] together with its round on that channel
//! as the pieces arrive. Either way the completion names the model the
//! round's options sent it to, so a Host that routes a round to another
//! model sees that model on its reply.

use runvil::event::ReplyOrigin;
use std::fmt;
use tokio::sync::mpsc::UnboundedSender as Sender;

use runvil::effect::Round;
use runvil::harness::InferenceBroker;
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
    sender: Option<ChatStreamSender>,
}

type ChatStreamSender = Sender<(StreamDelta, Round)>;

impl fmt::Debug for GatewayBroker {
    /// The bearer key is never written to logs or `Debug` output.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GatewayBroker")
            .field("client", &self.client)
            .field("api_root", &self.endpoint.url())
            .field("key", &"<redacted>")
            .field("sender", &self.sender)
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
            sender: None,
        }
    }

    /// Attaches an unbounded channel sender that receives streaming deltas.
    ///
    /// Each `Chat`-origin round run through [`InferenceBroker::chat`] sends
    /// every [`StreamDelta`] it produces, together with the round, on this
    /// channel. The sender must be attached before the broker is wrapped
    /// and passed to the Harness, because this call consumes the broker.
    #[must_use]
    pub fn with_streaming(mut self, tx: ChatStreamSender) -> Self {
        self.sender = Some(tx);
        self
    }

    /// One round on the client, handing each live piece to `on_piece`.
    fn round(
        &self,
        messages: Vec<Message>,
        tools: Vec<ToolSchema>,
        options: CompletionOptions,
        on_piece: impl Fn(StreamDelta) + Send + Sync + 'static,
    ) -> BoxFuture<'static, Result<Box<Completion>, Error>> {
        let client = self.client.clone();
        Box::pin(async move {
            // An empty advertisement sends no `tools` field at all, the
            // plain chat-completions shape.
            let tools = (!tools.is_empty()).then_some(tools.as_slice());
            client
                .complete(&messages, tools, &options, on_piece)
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

    fn chat(
        &self,
        _binding: ModelBinding,
        messages: Vec<Message>,
        tools: Vec<ToolSchema>,
        options: CompletionOptions,
        round: Round,
    ) -> BoxFuture<'static, Result<Box<Completion>, Error>> {
        let sender = self
            .sender
            .as_ref()
            .filter(|_| round.origin == ReplyOrigin::Chat)
            .cloned();

        self.round(messages, tools, options, move |piece| {
            if let Some(sender) = &sender {
                let _ = sender.send((piece, round));
            }
        })
    }
}

#[cfg(test)]
#[path = "broker-tests.rs"]
mod tests;
