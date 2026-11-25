//! The stock offline broker: an empty model list, and every round
//! refused.

use runvil_engine::effect::Round;
use runvil_engine::model::{
    Completion, CompletionOptions, Message, ModelBinding, ModelCatalog, ToolSchema,
    make_completion_error, phrase,
};
use runvil_engine::{BoxFuture, Error, ErrorCode};

use crate::performers::InferenceBroker;

/// The broker of a Host that runs prompts without a model.
///
/// It lists an empty catalog. A run over it whose setup selects no model
/// therefore binds none: a prompt that asks no model runs to its end, and
/// a `models.infer` or `models.loop` fails as
/// [`ModelRequired`](ErrorCode::ModelRequired) before a round reaches the
/// broker. A run whose setup selects a valid model id ends as
/// [`RequirementsUnmet`](ErrorCode::RequirementsUnmet), because the list
/// lacks the selection.
///
/// A round that reaches it all the same, from a broker that hands its
/// rounds on or from code outside the Harness, fails as
/// [`Unavailable`](ErrorCode::Unavailable) with that code's fixed phrase.
///
/// Both futures are ready at the first poll, on any executor.
#[derive(Debug)]
pub struct OfflineBroker;

impl InferenceBroker for OfflineBroker {
    fn models(&self) -> BoxFuture<'static, Result<ModelCatalog, Error>> {
        Box::pin(async { Ok(ModelCatalog::empty()) })
    }

    fn supports_streaming(&self) -> bool {
        false
    }

    fn chat(
        &self,
        _binding: ModelBinding,
        _messages: Vec<Message>,
        _tools: Vec<ToolSchema>,
        _options: CompletionOptions,
        _round: Round,
        _on_piece: Option<crate::performers::PieceSink>,
    ) -> BoxFuture<'static, Result<Box<Completion>, Error>> {
        let code = ErrorCode::Unavailable;
        Box::pin(async move { Err(make_completion_error(code, phrase(code))) })
    }
}

#[cfg(test)]
mod tests {
    //! Tests for the stock offline broker: each of its two futures is
    //! ready at the first poll, the list empty and the round refused.

    use std::num::NonZeroU32;

    use futures_util::FutureExt as _;
    use runvil_engine::event::ReplyOrigin;
    use runvil_engine::ids::RoundId;
    use runvil_engine::model::{ModelId, ModelInvocation};

    use super::*;

    #[test]
    fn the_offline_broker_lists_an_empty_catalog_at_the_first_poll() {
        let catalog = OfflineBroker
            .models()
            .now_or_never()
            .expect("the list is ready at the first poll")
            .expect("an empty list is not a failure");
        assert!(catalog.models().is_empty(), "{catalog:?}");
    }

    #[test]
    fn the_offline_broker_refuses_a_round_as_unavailable_at_the_first_poll() {
        let binding = ModelBinding::new(
            "default",
            "the round's model",
            ModelId::gateway("m").expect("a literal model name is valid"),
            ModelInvocation::new(),
            NonZeroU32::new(4096).expect("4096 is non-zero"),
        );
        let options = binding.completion_options();
        let round = Round::new(RoundId::new(0), ReplyOrigin::Chat);
        let error = OfflineBroker
            .chat(
                binding,
                vec![Message::user("hi")],
                Vec::new(),
                options,
                round,
                None,
            )
            .now_or_never()
            .expect("the refusal is ready at the first poll")
            .expect_err("the offline broker refuses every round");
        assert_eq!(error.code, ErrorCode::Unavailable);
        assert_eq!(error.prose, phrase(ErrorCode::Unavailable));
    }
}
