//! Tests for the `Completion` that a client `Reply` becomes: the backend
//! sections and the client's clock merge into one `CallMetrics`, and the
//! request and reassembled response bodies travel as the raw exchange. Each
//! test runs a real round on a mock gateway, because a `Reply` is built only
//! by the client.

use gateway_client::{GatewayChat, GatewayEndpoint, Options, SecretString, StreamDelta};
use runvil::metrics::{LlamaTimings, VllmMetrics};
use runvil::model::Message;
use serde_json::json;

use super::*;
use crate::test_support::{content_chunk, serve, sse_app, sse_body};

/// A keyed client pointed at a mock gateway that answers every completion
/// with the given SSE body.
async fn sse_client(body: String) -> GatewayChat {
    GatewayChat::new(
        GatewayEndpoint::new(&serve(sse_app(body)).await).expect("valid test endpoint"),
        SecretString::new("tok").expect("non-empty test key"),
    )
}

fn openai_options() -> Options {
    Options::new("m")
}

/// Runs one round on a mock gateway that streams `payloads`, and converts the
/// reply the client returns.
async fn round_of(payloads: &[Value]) -> Completion {
    sse_client(sse_body(payloads))
        .await
        .complete(&[Message::user("hi")], None, &openai_options(), |_| {})
        .await
        .expect("a complete turn finishes")
        .try_into_engine()
        .expect("the reply converts")
}

fn text_and_stop() -> [Value; 2] {
    [
        json!({
            "model": "qwen3-30b",
            "choices": [{ "index": 0, "delta": { "content": "Hello!" }, "finish_reason": null }]
        }),
        json!({
            "model": "qwen3-30b",
            "choices": [{ "index": 0, "delta": {}, "finish_reason": "stop" }]
        }),
    ]
}

/// The client timing a round measured. Its figures come from a real clock, so a test reads
/// them back and checks the rest of the metrics around them.
fn measured_client_timing(completion: &Completion) -> ClientTiming {
    completion
        .metrics()
        .and_then(|metrics| metrics.client.clone())
        .expect("the client's clock is measured on every round")
}

#[tokio::test]
async fn streamed_text_usage_timings_and_client_timing_accumulate() {
    // The llama.cpp streamed shape: content fragments, a finish chunk, and
    // the include_usage summary chunk holding usage plus timings. The
    // accumulated completion must match the buffered equivalent while the
    // deltas reach the callback in order, and the client's own clock must
    // populate ClientTiming.
    let client = sse_client(sse_body(&[
        content_chunk("Hel"),
        content_chunk("lo!"),
        serde_json::json!({
            "model": "qwen3-30b",
            "choices": [{ "index": 0, "delta": {}, "finish_reason": "stop" }]
        }),
        serde_json::json!({
            "model": "qwen3-30b",
            "choices": [],
            "usage": { "completion_tokens": 3, "prompt_tokens": 7, "total_tokens": 10 },
            "timings": {
                "prompt_n": 7, "prompt_ms": 12.5, "prompt_per_second": 560.0,
                "predicted_n": 3, "predicted_ms": 30.5, "predicted_per_second": 98.5
            }
        }),
    ]))
    .await;
    let seen = std::sync::Mutex::new(Vec::new());
    let completion = client
        .complete(&[Message::user("hi")], None, &openai_options(), |delta| {
            seen.lock().expect("delta log").push(delta);
        })
        .await
        .expect("a streamed text turn completes")
        .try_into_engine()
        .expect("the reply converts");
    match completion.result() {
        CompletionResult::Text(text) => assert_eq!(text, "Hello!"),
        other => panic!("expected text, got {other:?}"),
    }
    assert_eq!(
        *seen.lock().expect("delta log"),
        vec![
            StreamDelta::Text("Hel".to_owned()),
            StreamDelta::Text("lo!".to_owned()),
        ],
        "each content fragment reaches the callback live, in order"
    );
    assert_eq!(completion.finish_reason(), Some("stop"));
    assert_eq!(
        completion.model(),
        "m",
        "the label is the requested model, not the body's `qwen3-30b`"
    );
    let metrics = completion.metrics().expect("the round reported metrics");
    let usage = metrics.usage.as_ref().expect("usage from the final chunk");
    assert_eq!(usage.total_tokens, 10);
    let timings = metrics
        .llama
        .as_ref()
        .expect("timings from the final chunk");
    assert_eq!(timings.predicted_n, 3);
    assert_eq!(metrics.vllm, None, "llama.cpp reports no vLLM section");
    let timing = metrics
        .client
        .as_ref()
        .expect("the streaming transport measures its own clock");
    assert!(
        timing.ttft_ms.is_some_and(|ttft| ttft >= 0.0),
        "TTFT is measured once the first delta arrives: {timing:?}"
    );
    assert!(
        timing.mean_itl_ms.is_some_and(|itl| itl >= 0.0),
        "mean ITL is measured with two delta chunks: {timing:?}"
    );
    assert!(timing.e2e_ms >= 0.0);
    let raw = completion
        .raw()
        .expect("the HTTP path attaches the raw exchange for debug capture");
    assert_eq!(raw.request()["messages"][0]["content"], "hi");
    assert_eq!(raw.request()["stream"], true);
    assert_eq!(
        raw.response()["choices"][0]["message"]["content"],
        "Hello!",
        "the response is the reassembled buffered body"
    );
}

#[tokio::test]
async fn finish_merges_every_reported_section_and_the_client_clock_into_one_call_metrics() {
    let [text, stop] = text_and_stop();
    let summary = json!({
        "model": "qwen3-30b",
        "choices": [],
        "usage": {
            "prompt_tokens": 20,
            "completion_tokens": 5,
            "total_tokens": 25,
            "prompt_tokens_details": { "cached_tokens": 16 },
            "completion_tokens_details": { "reasoning_tokens": 2 }
        },
        "timings": {
            "prompt_n": 20, "prompt_ms": 12.5, "prompt_per_second": 1600.0,
            "predicted_n": 5, "predicted_ms": 30.5, "predicted_per_second": 98.5
        },
        "metrics": { "time_to_first_token_ms": 8.5 }
    });
    let completion = round_of(&[text, stop, summary]).await;
    assert_eq!(
        completion.metrics(),
        Some(
            &CallMetrics::new()
                .with_usage(Some(
                    Usage::new(20, 5, 25)
                        .with_cached_tokens(Some(16))
                        .with_reasoning_tokens(Some(2))
                ))
                .with_llama(Some(LlamaTimings::new(20, 12.5, 1600.0, 5, 30.5, 98.5)))
                .with_vllm(Some(
                    VllmMetrics::new().with_time_to_first_token_ms(Some(8.5))
                ))
                .with_client(Some(measured_client_timing(&completion)))
        )
    );
}

#[tokio::test]
async fn a_stream_with_no_usage_chunk_yields_no_usage() {
    let completion = round_of(&text_and_stop()).await;
    let metrics = completion
        .metrics()
        .expect("the client clock alone still makes metrics");
    assert_eq!(metrics.usage, None);
    assert_eq!(metrics.llama, None);
    assert_eq!(metrics.vllm, None);
    assert_eq!(metrics.client, Some(measured_client_timing(&completion)));
}

#[tokio::test]
async fn a_malformed_usage_section_degrades_and_leaves_the_other_sections() {
    let [text, stop] = text_and_stop();
    let summary = json!({
        "choices": [],
        "usage": "broken",
        "timings": {
            "prompt_n": 7, "prompt_ms": 12.5, "prompt_per_second": 560.0,
            "predicted_n": 3, "predicted_ms": 30.5, "predicted_per_second": 98.5
        }
    });
    let completion = round_of(&[text, stop, summary]).await;
    let metrics = completion.metrics().expect("the timings were measured");
    assert_eq!(metrics.usage, None, "a malformed usage degrades");
    assert!(metrics.llama.is_some(), "its sibling section survives");
    assert_eq!(completion.metadata_diagnostics().len(), 1);
}
