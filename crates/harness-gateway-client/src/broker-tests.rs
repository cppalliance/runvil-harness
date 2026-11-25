//! The Gateway broker against mock gateways: the
//! broker says it streams, `chat` hands each piece to its callback in wire
//! order, a round without a callback answers with the whole reply, a
//! callback that keeps one piece leaves the reply whole, every round is
//! labeled with the model its request named, and `models` lists the
//! catalog the gateway serves under the broker's key. Its `Debug` never
//! prints the bearer key.

use std::num::NonZeroU32;
use std::sync::{Arc, Mutex};

use gateway_client::{GatewayEndpoint, SecretString};
use runvil::effect::Round;
use runvil::event::ReplyOrigin;
use runvil::harness::{Delta, InferenceBroker, PieceSink};
use runvil::ids::RoundId;
use runvil::model::{
    CompletionOptions, CompletionResult, Message, ModelBinding, ModelId, ModelInvocation,
    ThinkingMode,
};
use serde_json::Value;

use super::GatewayBroker;
use crate::test_support::{content_chunk, serve, sse_app, sse_body};

/// The bearer key every mock gateway here is keyed with.
const KEY: &str = "tok";

/// Serves `app` on a loopback port and returns a broker keyed for its
/// `/v1` root.
async fn broker_for(app: axum::Router) -> GatewayBroker {
    GatewayBroker::new(
        GatewayEndpoint::new(&serve(app).await).expect("valid test endpoint"),
        SecretString::new(KEY).expect("non-empty test key"),
    )
}

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

/// A reply that reasons once and then answers in three fragments, closed
/// by a stop finish.
fn three_fragments() -> String {
    sse_body(&[
        serde_json::json!({
            "choices": [{ "index": 0, "delta": { "reasoning_content": "hmm" }, "finish_reason": null }]
        }),
        content_chunk("one "),
        content_chunk("two "),
        content_chunk("three"),
        serde_json::json!({
            "choices": [{ "index": 0, "delta": {}, "finish_reason": "stop" }]
        }),
    ])
}

/// A piece callback that keeps every piece it is handed, beside what it
/// kept.
fn recording() -> (PieceSink, Arc<Mutex<Vec<Delta>>>) {
    let kept: Arc<Mutex<Vec<Delta>>> = Arc::default();
    let sink = Arc::clone(&kept);
    (
        Arc::new(move |piece| sink.lock().unwrap().push(piece)),
        kept,
    )
}

/// The run's first round, dispatched from `origin`.
fn round(origin: ReplyOrigin) -> Round {
    Round::new(RoundId::new(0), origin)
}

fn reply_of(result: &CompletionResult) -> &str {
    match result {
        CompletionResult::Text(text) => text,
        other => panic!("the round replies with text: {other:?}"),
    }
}

#[tokio::test]
async fn chat_hands_each_piece_to_its_callback_in_wire_order() {
    let broker = broker_for(sse_app(three_fragments())).await;
    let (on_piece, kept) = recording();

    assert!(broker.supports_streaming(), "the Gateway broker streams");
    let completion = broker
        .chat(
            binding(),
            vec![Message::user("hi")],
            Vec::new(),
            CompletionOptions::new("m"),
            round(ReplyOrigin::Chat),
            Some(on_piece),
        )
        .await
        .expect("the mock round completes");
    assert_eq!(reply_of(completion.result()), "one two three");
    assert_eq!(
        *kept.lock().unwrap(),
        vec![
            Delta::Reasoning("hmm".to_owned()),
            Delta::Text("one ".to_owned()),
            Delta::Text("two ".to_owned()),
            Delta::Text("three".to_owned()),
        ],
        "each piece, reasoning and text alike, reaches the callback as it arrives, in the stream's order"
    );
}

#[tokio::test]
async fn a_round_without_a_callback_answers_with_the_whole_reply() {
    let broker = broker_for(sse_app(three_fragments())).await;

    let completion = broker
        .chat(
            binding(),
            vec![Message::user("hi")],
            Vec::new(),
            CompletionOptions::new("m"),
            round(ReplyOrigin::Chat),
            None,
        )
        .await
        .expect("the mock round completes");
    assert_eq!(
        reply_of(completion.result()),
        "one two three",
        "a round without a callback streams nowhere, and the completed reply travels in the answer"
    );
}

#[tokio::test]
async fn a_callback_that_keeps_one_piece_still_leaves_the_whole_reply() {
    let broker = broker_for(sse_app(three_fragments())).await;
    let kept: Arc<Mutex<Vec<Delta>>> = Arc::default();
    // The callback moves into the round, so the test keeps this second
    // handle to read what the callback kept.
    let sink = Arc::clone(&kept);
    let on_piece: PieceSink = Arc::new(move |piece| {
        let mut pieces = sink.lock().unwrap();
        if pieces.is_empty() {
            pieces.push(piece);
        }
    });

    let completion = broker
        .chat(
            binding(),
            vec![Message::user("hi")],
            Vec::new(),
            CompletionOptions::new("m"),
            round(ReplyOrigin::Chat),
            Some(on_piece),
        )
        .await
        .expect("the mock round completes");
    assert_eq!(
        *kept.lock().unwrap(),
        vec![Delta::Reasoning("hmm".to_owned())],
        "the callback kept the first piece alone"
    );
    assert_eq!(
        reply_of(completion.result()),
        "one two three",
        "the finished reply is whole whatever the callback kept"
    );
}

/// A one-chunk reply whose chunks name `model`, or no model at all.
fn reply_naming(model: Option<&str>) -> String {
    let mut chunks = vec![
        serde_json::json!({
            "choices": [{ "index": 0, "delta": { "content": "ok" }, "finish_reason": null }]
        }),
        serde_json::json!({
            "choices": [{ "index": 0, "delta": {}, "finish_reason": "stop" }]
        }),
    ];
    if let Some(model) = model {
        for chunk in &mut chunks {
            chunk["model"] = Value::from(model);
        }
    }
    sse_body(&chunks)
}

#[tokio::test]
async fn every_round_whole_or_streamed_is_labeled_with_the_model_its_request_named() {
    for body_model in [Some("served-elsewhere"), None] {
        let broker = broker_for(sse_app(reply_naming(body_model))).await;
        let whole = broker
            .chat(
                binding(),
                vec![Message::user("hi")],
                Vec::new(),
                CompletionOptions::new("routed-to"),
                round(ReplyOrigin::Chat),
                None,
            )
            .await
            .expect("the mock round completes");
        let (on_piece, _kept) = recording();
        let streamed = broker
            .chat(
                binding(),
                vec![Message::user("hi")],
                Vec::new(),
                CompletionOptions::new("routed-to"),
                round(ReplyOrigin::Chat),
                Some(on_piece),
            )
            .await
            .expect("the mock round completes");
        for completion in [&whole, &streamed] {
            assert_eq!(
                completion.model(),
                "routed-to",
                "the label is the requested model, whatever the body named ({body_model:?})"
            );
        }
        if body_model.is_none() {
            assert_eq!(
                streamed.metadata_diagnostics(),
                ["completion response named no string `model`; labeled with the requested model"],
                "the diagnostic says where the label came from"
            );
        }
    }
}

#[test]
fn debug_never_leaks_the_bearer_key() {
    let api_root = "http://127.0.0.1:8081/v1";
    let broker = GatewayBroker::new(
        GatewayEndpoint::new(api_root).expect("valid test endpoint"),
        SecretString::new("super-secret-token").expect("non-empty test key"),
    );
    let rendered = format!("{broker:?}");
    assert!(
        !rendered.contains("super-secret-token"),
        "the bearer key must never appear in Debug output, got: {rendered}"
    );
    assert!(
        rendered.contains("<redacted>"),
        "the key field must be redacted, got: {rendered}"
    );
    assert!(
        rendered.contains(api_root),
        "the API root is not a secret and should still appear, got: {rendered}"
    );
}

#[tokio::test]
async fn models_lists_the_catalog_the_gateway_serves_under_the_brokers_key() {
    use axum::Router;
    use axum::http::header::AUTHORIZATION;
    use axum::http::{HeaderMap, StatusCode};
    use axum::routing::get;

    async fn models(headers: HeaderMap) -> Result<axum::Json<Value>, StatusCode> {
        let bearer = headers
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok());
        if bearer != Some("Bearer tok") {
            return Err(StatusCode::UNAUTHORIZED);
        }
        Ok(axum::Json(serde_json::json!({
            "data": [{
                "id": "mock-model",
                "description": "the mock model",
                "context": 131_072,
                "thinking": "switchable",
            }]
        })))
    }
    let broker = broker_for(Router::new().route("/v1/models", get(models))).await;

    let catalog = broker
        .models()
        .await
        .expect("the keyed mock serves its catalog");
    assert_eq!(catalog.models().len(), 1, "the one served model");
    let info = catalog
        .get(&ModelId::gateway("mock-model").expect("a literal model name is valid"))
        .expect("the served model is listed under its gateway id");
    assert_eq!(info.description(), "the mock model");
    assert_eq!(info.context().get(), 131_072);
    assert_eq!(info.thinking(), ThinkingMode::Switchable);
}
