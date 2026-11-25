//! The Engine's `Error` for each way a round fails: the broker runs a real
//! round against a mock gateway, and the test reads the error the broker
//! returns. The client's tests hold the classifier's own rules. These hold
//! what crosses the adapter: the Engine's code, the prose a model reads, the
//! message that keeps the gateway's text, and the facts.

use std::num::NonZeroU32;

use axum::http::StatusCode;
use axum::routing::post;
use gateway_client::{Gateway, GatewayEndpoint, SecretString};
use runvil::effect::Round;
use runvil::event::ReplyOrigin;
use runvil::harness::InferenceBroker;
use runvil::ids::RoundId;
use runvil::model::{CompletionOptions, Message, ModelBinding, ModelId, ModelInvocation};
use runvil::{Error, ErrorCode, ErrorFact};
use serde_json::{Value, json};

use super::GatewayBroker;
use crate::convert::IntoEngine;
use crate::test_support::{content_chunk, serve, sse_app, sse_body};

/// A binding for the round; the broker runs the round under the effect's
/// frozen options, so the binding's own fields are inert here.
fn binding() -> ModelBinding {
    ModelBinding::new(
        "writer",
        "the round's model",
        ModelId::gateway("m").expect("a literal model name is valid"),
        ModelInvocation::new(),
        NonZeroU32::new(4096).expect("4096 is non-zero"),
    )
}

/// Serves `app` and returns the error a round on it fails with.
async fn round_error(app: axum::Router) -> Error {
    let broker = GatewayBroker::new(
        GatewayEndpoint::new(&serve(app).await).expect("valid test endpoint"),
        SecretString::new("tok").expect("non-empty test key"),
    );
    broker
        .chat(
            binding(),
            vec![Message::user("hi")],
            Vec::new(),
            CompletionOptions::new("m"),
            Round::new(RoundId::new(0), ReplyOrigin::Chat),
            None,
        )
        .await
        .expect_err("the round fails")
}

/// A mock gateway that answers every completion with `status` and `body`.
fn failing_app(status: StatusCode, body: &'static str) -> axum::Router {
    axum::Router::new().route(
        "/v1/chat/completions",
        post(move || async move { (status, body) }),
    )
}

#[tokio::test]
async fn a_401_is_unavailable_with_the_credentials_phrase_and_the_body_in_the_message_only() {
    let error = round_error(failing_app(StatusCode::UNAUTHORIZED, "bad key")).await;
    assert_eq!(error.code, ErrorCode::Unavailable, "{error:?}");
    assert_eq!(
        error.prose,
        "the model backend did not accept the credentials (status 401)"
    );
    assert_eq!(
        error.message,
        "the model backend did not accept the credentials (status 401): bad key"
    );
}

#[tokio::test]
async fn a_429_naming_a_spent_quota_is_quota_exhausted() {
    let error = round_error(failing_app(
        StatusCode::TOO_MANY_REQUESTS,
        "you exceeded your current quota",
    ))
    .await;
    assert_eq!(error.code, ErrorCode::QuotaExhausted, "{error:?}");
    assert_eq!(
        error.prose,
        "the model backend says the usage quota is spent (status 429)"
    );
    assert!(
        error.message.ends_with(": you exceeded your current quota"),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_context_overflow_names_its_counts_as_engine_facts() {
    let error = round_error(failing_app(
        StatusCode::BAD_REQUEST,
        "This model's maximum context length is 4096 tokens. However, your messages \
         resulted in 5120 tokens. Please reduce the length of the messages.",
    ))
    .await;
    assert_eq!(error.code, ErrorCode::ContextOverflow, "{error:?}");
    assert_eq!(
        error.facts.get(&ErrorFact::Count),
        Some(&Value::from(5120_u32)),
        "{error:?}"
    );
    assert_eq!(
        error.facts.get(&ErrorFact::Limit),
        Some(&Value::from(4096_u32)),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_stream_without_the_done_sentinel_is_a_malformed_response() {
    let mut body = sse_body(&[content_chunk("partial")]);
    body.truncate(body.len() - "data: [DONE]\n\n".len());
    let error = round_error(sse_app(body)).await;
    assert_eq!(error.code, ErrorCode::MalformedResponse, "{error:?}");
    assert_eq!(
        error.prose,
        "the model backend sent a reply that could not be understood: \
         completion stream ended without the [DONE] sentinel"
    );
}

#[tokio::test]
async fn an_empty_reply_keeps_its_finish_reason_as_the_engine_fact() {
    let error = round_error(sse_app(sse_body(&[json!({
        "choices": [{ "index": 0, "delta": {}, "finish_reason": "stop" }]
    })])))
    .await;
    assert_eq!(error.code, ErrorCode::EmptyReply, "{error:?}");
    assert_eq!(
        error.facts.get(&ErrorFact::FinishReason),
        Some(&Value::from("stop")),
        "{error:?}"
    );
}

/// The one setup error a Host can meet without a gateway: the client's own
/// environment reader names the variable it did not find.
#[test]
fn a_missing_gateway_url_maps_to_the_reason_and_name_facts() {
    // The names are ones that no machine sets, so the real process
    // environment answers "not set" on every run.
    let wire = Gateway::find_env_named(
        "HARNESS_TEST_UNSET_GATEWAY_URL",
        "HARNESS_TEST_UNSET_GATEWAY_KEY",
    )
    .expect_err("the variable is not set");
    let error = wire.into_engine();
    assert_eq!(error.code, ErrorCode::InvalidValue, "{error:?}");
    assert_eq!(
        error.facts.get(&ErrorFact::Reason),
        Some(&Value::from("missing_env")),
        "{error:?}"
    );
    assert_eq!(
        error.facts.get(&ErrorFact::Name),
        Some(&Value::from("HARNESS_TEST_UNSET_GATEWAY_URL")),
        "{error:?}"
    );
}
