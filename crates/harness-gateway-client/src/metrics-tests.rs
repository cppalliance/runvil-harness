//! Tests for the lenient parse of the llama.cpp and vLLM sections and its
//! degrade policy. The client's tests hold the `usage` section and the model
//! name.

use runvil::metrics::{CallMetrics, LlamaTimings, VllmMetrics};

use super::*;

/// What the adapter reads out of a response body: the two backend sections,
/// and a diagnostic for each present section that did not parse.
struct ResponseMetadata {
    metrics: CallMetrics,
    diagnostics: Vec<String>,
}

/// Reads both sections the way `try_into_engine` does.
fn response_metadata(body: &Value) -> ResponseMetadata {
    let mut diagnostics = Vec::new();
    let metrics = CallMetrics::new()
        .with_llama(parse_section(
            body,
            "timings",
            parse_llama_timings,
            &mut diagnostics,
        ))
        .with_vllm(parse_section(
            body,
            "metrics",
            parse_vllm_metrics,
            &mut diagnostics,
        ));
    ResponseMetadata {
        metrics,
        diagnostics,
    }
}

/// Parses the metadata and counts its diagnostics, so the tests can pin
/// both halves of the degrade policy: malformed sections report, and
/// well-formed or absent sections stay silent.
fn with_warn_count(body: &Value) -> (ResponseMetadata, usize) {
    let metadata = response_metadata(body);
    let warnings = metadata.diagnostics.len();
    (metadata, warnings)
}

/// One assistant text choice, shared by the metadata fixture bodies.
fn reply_choice() -> Value {
    serde_json::json!([{
        "index": 0,
        "message": { "role": "assistant", "content": "hi" },
        "finish_reason": "stop"
    }])
}

#[test]
fn llama_body_parses_model_usage_and_timings() {
    let body = serde_json::json!({
        "id": "chatcmpl-llama",
        "object": "chat.completion",
        "created": 1_726_000_000_u64,
        "model": "qwen3-30b",
        "choices": reply_choice(),
        "usage": { "completion_tokens": 3, "prompt_tokens": 7, "total_tokens": 10 },
        "timings": {
            "prompt_n": 7,
            "prompt_ms": 12.5,
            "prompt_per_token_ms": 1.75,
            "prompt_per_second": 560.0,
            "predicted_n": 3,
            "predicted_ms": 30.5,
            "predicted_per_token_ms": 10.25,
            "predicted_per_second": 98.5,
            "draft_n": 4,
            "draft_n_accepted": 2
        }
    });

    let (metadata, warnings) = with_warn_count(&body);
    assert_eq!(warnings, 0, "a well-formed body must not warn");
    assert_eq!(
        metadata.metrics.llama,
        Some(
            LlamaTimings::new(7, 12.5, 560.0, 3, 30.5, 98.5)
                .with_draft_n(4)
                .with_draft_n_accepted(2)
        )
    );
    assert_eq!(metadata.metrics.vllm, None);
}

#[test]
fn llama_timings_without_draft_counters_default_to_zero() {
    // Without a configured draft model llama.cpp omits the draft
    // counters entirely; zero drafted tokens is the truthful reading,
    // so the common non-speculative body must not degrade to None.
    let body = serde_json::json!({
        "model": "qwen3-30b",
        "choices": reply_choice(),
        "timings": {
            "prompt_n": 7,
            "prompt_ms": 12.5,
            "prompt_per_second": 560.0,
            "predicted_n": 3,
            "predicted_ms": 30.5,
            "predicted_per_second": 98.5
        }
    });

    let (metadata, warnings) = with_warn_count(&body);
    assert_eq!(warnings, 0);
    let timings = metadata.metrics.llama.unwrap();
    assert_eq!(timings.draft_n, 0);
    assert_eq!(timings.draft_n_accepted, 0);
}

#[test]
fn vllm_body_parses_metrics_and_cached_tokens() {
    let body = serde_json::json!({
        "id": "chatcmpl-vllm",
        "object": "chat.completion",
        "model": "meta-llama/Llama-3.1-8B-Instruct",
        "choices": reply_choice(),
        "usage": {
            "prompt_tokens": 20,
            "completion_tokens": 5,
            "total_tokens": 25,
            "prompt_tokens_details": { "cached_tokens": 16 }
        },
        "metrics": {
            "time_to_first_token_ms": 8.5,
            "generation_time_ms": 22.5,
            "queue_time_ms": 1.5,
            "mean_itl_ms": 7.5,
            "tokens_per_second": 133.5
        }
    });

    let (metadata, warnings) = with_warn_count(&body);
    assert_eq!(warnings, 0, "a well-formed body must not warn");
    assert_eq!(metadata.metrics.llama, None);
    assert_eq!(
        metadata.metrics.vllm,
        Some(
            VllmMetrics::new()
                .with_time_to_first_token_ms(Some(8.5))
                .with_generation_time_ms(Some(22.5))
                .with_queue_time_ms(Some(1.5))
                .with_mean_itl_ms(Some(7.5))
                .with_tokens_per_second(Some(133.5))
        )
    );
}

#[test]
fn vllm_metrics_omit_what_was_not_measured() {
    let body = serde_json::json!({
        "model": "m",
        "choices": reply_choice(),
        "metrics": { "time_to_first_token_ms": 8.5 }
    });

    let (metadata, warnings) = with_warn_count(&body);
    assert_eq!(warnings, 0);
    assert_eq!(
        metadata.metrics.vllm,
        Some(VllmMetrics::new().with_time_to_first_token_ms(Some(8.5))),
        "fields vLLM did not measure stay None inside a parsed section"
    );
}

#[test]
fn absent_and_null_backend_sections_are_none_without_warning() {
    let bare = serde_json::json!({ "model": "m", "choices": reply_choice() });
    let with_nulls = serde_json::json!({
        "model": "m",
        "choices": reply_choice(),
        "timings": null,
        "metrics": null
    });

    for body in [bare, with_nulls] {
        let (metadata, warnings) = with_warn_count(&body);
        assert_eq!(warnings, 0, "absence is normal, never a warning: {body}");
        assert_eq!(metadata.metrics.llama, None);
        assert_eq!(metadata.metrics.vllm, None);
    }
}

#[test]
fn a_malformed_backend_section_degrades_with_a_warning_and_leaves_its_sibling() {
    let body = serde_json::json!({
        "model": "m",
        "choices": reply_choice(),
        "timings": { "prompt_n": 7 },
        "metrics": { "time_to_first_token_ms": 8.5 }
    });

    let (metadata, warnings) = with_warn_count(&body);
    assert_eq!(warnings, 1, "only the broken section warns");
    assert!(
        metadata.diagnostics[0].starts_with("malformed `timings` in completion response ignored"),
        "the diagnostic names the section: {:?}",
        metadata.diagnostics
    );
    assert_eq!(metadata.metrics.llama, None);
    assert!(
        metadata.metrics.vllm.is_some(),
        "a malformed sibling section must not take the other down with it"
    );
}
