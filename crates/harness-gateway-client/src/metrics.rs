//! The backend timing sections of a reply that the Engine's metrics
//! vocabulary names: llama.cpp's `timings` and vLLM's `metrics`.
//!
//! The client keeps the whole reassembled response in its reply. This
//! module reads the two extension sections out of that body, because their
//! canonical types are the Engine's. The `usage` section and the model name
//! stay with the client, which reports its own diagnostics for them.

use runvil::metrics::{LlamaTimings, VllmMetrics};
use serde::Deserialize;
use serde_json::Value;

/// Parses one top-level metadata section leniently.
///
/// Absent or JSON-null is `None` silently - a frontier body has no `timings`
/// and that is not a defect. A present section that fails `parse` degrades to
/// `None` with a diagnostic naming the section and the parse failure.
pub(crate) fn parse_section<T>(
    body: &Value,
    key: &str,
    parse: impl FnOnce(&Value) -> Result<T, serde_json::Error>,
    diagnostics: &mut Vec<String>,
) -> Option<T> {
    match body.get(key) {
        None | Some(Value::Null) => None,
        Some(value) => match parse(value) {
            Ok(parsed) => Some(parsed),
            Err(error) => {
                diagnostics.push(format!(
                    "malformed `{key}` in completion response ignored: {error}"
                ));
                None
            }
        },
    }
}

/// The wire shape of llama.cpp's top-level `timings` extension.
///
/// The draft counters appear only when a speculative-decoding draft model
/// ran, so an absent counter means zero drafted tokens, not an unknown; the
/// per-token rates the server also sends are derivable and ignored.
#[derive(Deserialize)]
struct WireLlamaTimings {
    prompt_n: u32,
    prompt_ms: f64,
    prompt_per_second: f64,
    predicted_n: u32,
    predicted_ms: f64,
    predicted_per_second: f64,
    #[serde(default)]
    draft_n: u32,
    #[serde(default)]
    draft_n_accepted: u32,
}

/// Parses llama.cpp's `timings` object into the canonical [`LlamaTimings`].
pub(crate) fn parse_llama_timings(value: &Value) -> Result<LlamaTimings, serde_json::Error> {
    let wire = WireLlamaTimings::deserialize(value)?;
    Ok(LlamaTimings::new(
        wire.prompt_n,
        wire.prompt_ms,
        wire.prompt_per_second,
        wire.predicted_n,
        wire.predicted_ms,
        wire.predicted_per_second,
    )
    .with_draft_n(wire.draft_n)
    .with_draft_n_accepted(wire.draft_n_accepted))
}

/// Parses vLLM's `metrics` object into the canonical [`VllmMetrics`].
///
/// The canonical type is its own wire shape: every field is optional because
/// vLLM omits what it did not measure, and unknown keys are ignored.
pub(crate) fn parse_vllm_metrics(value: &Value) -> Result<VllmMetrics, serde_json::Error> {
    VllmMetrics::deserialize(value)
}

#[cfg(test)]
#[path = "metrics-tests.rs"]
mod tests;
