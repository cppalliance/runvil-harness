//! The adapter's conversions between the client's plain types and the
//! Engine's types.
//!
//! Rust forbids `impl From<A> for B` in a crate that owns neither `A` nor
//! `B` (the orphan rule), and every conversion here pairs a client type with
//! an Engine type. The four traits below are this crate's own, so a local
//! trait can be implemented for foreign types, and a call site reads the
//! same: `error.into_engine()` and `reply.try_into_engine()`. `?` does not
//! convert on its own, so the broker's call sites use `map_err`.

use gateway_client::{
    ClientTiming as WireTiming, Error as WireError, ErrorKind, Options, Outcome, Reply,
    StreamDelta, Tool, Usage as WireUsage,
};
use runvil::harness::Delta;
use runvil::metrics::{CallMetrics, ClientTiming, Usage};
use runvil::model::{
    Completion, CompletionOptions, CompletionResult, RawExchange, ToolCall, ToolSchema,
};
use runvil::{Error, ErrorCode, ErrorFact};
use serde_json::Value;

use crate::fail::decoded;
use crate::metrics::{parse_llama_timings, parse_section, parse_vllm_metrics};

/// A client value and the Engine value it becomes.
pub(crate) trait IntoEngine {
    /// The Engine's type for this client type.
    type Engine;

    /// Converts the client value to the Engine's.
    fn into_engine(self) -> Self::Engine;
}

/// The same, for a conversion that can fail with the Engine's `Error`.
pub(crate) trait TryIntoEngine {
    /// The Engine's type for this client type.
    type Engine;

    /// Converts the client value to the Engine's, or refuses it.
    fn try_into_engine(self) -> Result<Self::Engine, Error>;
}

/// An Engine value and the client value it becomes.
pub(crate) trait IntoWire {
    /// The client's type for this Engine type.
    type Wire;

    /// Converts the Engine value to the client's.
    fn into_wire(self) -> Self::Wire;
}

/// The same, for a conversion that can fail with the Engine's `Error`.
pub(crate) trait TryIntoWire {
    /// The client's type for this Engine type.
    type Wire;

    /// Converts the Engine value to the client's, or refuses it.
    fn try_into_wire(self) -> Result<Self::Wire, Error>;
}

/// The Engine's code for each client error kind. The match has no wildcard
/// arm, so a kind added to the client fails to compile here.
impl IntoEngine for ErrorKind {
    type Engine = ErrorCode;

    fn into_engine(self) -> ErrorCode {
        match self {
            ErrorKind::ContextOverflow => ErrorCode::ContextOverflow,
            ErrorKind::RateLimited => ErrorCode::RateLimited,
            ErrorKind::QuotaExhausted => ErrorCode::QuotaExhausted,
            ErrorKind::Overloaded => ErrorCode::Overloaded,
            ErrorKind::Refused => ErrorCode::Refused,
            ErrorKind::Timeout => ErrorCode::Timeout,
            ErrorKind::Transport => ErrorCode::Transport,
            ErrorKind::ServerError => ErrorCode::ServerError,
            ErrorKind::Rejected => ErrorCode::Rejected,
            ErrorKind::MalformedResponse => ErrorCode::MalformedResponse,
            ErrorKind::EmptyReply => ErrorCode::EmptyReply,
            ErrorKind::Unavailable => ErrorCode::Unavailable,
            ErrorKind::InvalidConfig | ErrorKind::InvalidRequest => ErrorCode::InvalidValue,
        }
    }
}

/// The Engine's error for a client error. The message and the prose are
/// copied as they are, so the wording a model reads is the wording the
/// client wrote, which is today's wording.
impl IntoEngine for WireError {
    type Engine = Error;

    fn into_engine(self) -> Error {
        let mut mapped = Error::new(self.kind.into_engine(), self.message, self.summary);
        if let Some(reason) = self.reason {
            mapped.facts.insert(ErrorFact::Reason, Value::from(reason));
        }
        if let Some(name) = self.name {
            mapped.facts.insert(ErrorFact::Name, Value::from(&*name));
        }
        if let Some(reason) = self.finish_reason {
            mapped
                .facts
                .insert(ErrorFact::FinishReason, Value::from(&*reason));
        }
        if let Some(tokens) = self.prompt_tokens {
            mapped.facts.insert(ErrorFact::Count, Value::from(tokens));
        }
        if let Some(window) = self.context_window {
            mapped.facts.insert(ErrorFact::Limit, Value::from(window));
        }
        mapped
    }
}

/// The Engine accepts only a JSON object as a tool's parameter schema
/// (`tool_schema_new`), so the `else` arm cannot run today. It returns the
/// Engine's own error, with the Engine's own wording and facts, and not an
/// `unwrap` or a silently empty schema.
impl TryIntoWire for &ToolSchema {
    type Wire = Tool;

    fn try_into_wire(self) -> Result<Tool, Error> {
        let Some(parameters) = self.parameters().as_object() else {
            let message = format!(
                "tool {:?} parameters schema must be a JSON object",
                self.name()
            );
            let mut error = Error::new(
                ErrorCode::ToolSchemaInvalid,
                message,
                ErrorCode::ToolSchemaInvalid.prose(),
            );
            error
                .facts
                .insert(ErrorFact::Name, Value::from(self.name()));
            error
                .facts
                .insert(ErrorFact::Reason, Value::from("non_object_schema"));
            return Err(error);
        };
        Ok(Tool::new(
            self.name(),
            self.description(),
            parameters.clone(),
        ))
    }
}

impl IntoWire for &CompletionOptions {
    type Wire = Options;

    fn into_wire(self) -> Options {
        let mut mapped = Options::new(self.model());
        if let Some(temperature) = self.temperature() {
            mapped = mapped.with_temperature(temperature.get());
        }
        if let Some(max_tokens) = self.max_tokens() {
            mapped = mapped.with_max_tokens(max_tokens);
        }
        if let Some(thinking) = self.thinking() {
            mapped = mapped.with_thinking(thinking);
        }
        mapped
    }
}

impl IntoEngine for &WireUsage {
    type Engine = Usage;

    fn into_engine(self) -> Usage {
        Usage::new(
            self.prompt_tokens,
            self.completion_tokens,
            self.total_tokens,
        )
        .with_cached_tokens(self.cached_tokens)
        .with_reasoning_tokens(self.reasoning_tokens)
    }
}

impl IntoEngine for &WireTiming {
    type Engine = ClientTiming;

    fn into_engine(self) -> ClientTiming {
        ClientTiming::new(self.e2e_ms)
            .with_ttft_ms(self.ttft_ms)
            .with_mean_itl_ms(self.mean_itl_ms)
    }
}

/// One live piece of a reply, as the Harness hands it to a Host's callback.
impl IntoEngine for StreamDelta {
    type Engine = Delta;

    fn into_engine(self) -> Delta {
        match self {
            StreamDelta::Text(text) => Delta::Text(text),
            StreamDelta::Reasoning(text) => Delta::Reasoning(text),
        }
    }
}

/// Builds the Engine's `Completion` from a client `Reply`, through the
/// Engine's own constructors, so the Engine's checks run on every turn.
impl TryIntoEngine for Reply {
    type Engine = Completion;

    fn try_into_engine(self) -> Result<Completion, Error> {
        let result = match self.outcome() {
            Outcome::Text(text) => CompletionResult::Text(text.clone()),
            Outcome::ToolCalls(calls) => {
                let mut parts = Vec::with_capacity(calls.len());
                for call in calls {
                    let arguments = Value::Object(call.arguments().clone());
                    parts.push(
                        ToolCall::from_parts(call.id(), call.name(), arguments).map_err(decoded)?,
                    );
                }
                CompletionResult::ToolCalls(parts)
            }
        };

        // The model and usage diagnostics came from the client, in today's
        // order. The llama.cpp and vLLM sections come next.
        let mut diagnostics = self.diagnostics().to_vec();
        let llama = parse_section(
            self.response(),
            "timings",
            parse_llama_timings,
            &mut diagnostics,
        );
        let vllm = parse_section(
            self.response(),
            "metrics",
            parse_vllm_metrics,
            &mut diagnostics,
        );
        let usage = self.usage().map(IntoEngine::into_engine);
        let timing = self.timing().map(IntoEngine::into_engine);
        let measured = usage.is_some() || llama.is_some() || vllm.is_some() || timing.is_some();
        let metrics = CallMetrics::new()
            .with_usage(usage)
            .with_llama(llama)
            .with_vllm(vllm)
            .with_client(timing);

        let mut completion = Completion::from_result(result, self.model())
            .map_err(decoded)?
            .with_metadata_diagnostics(diagnostics);
        if let Some(reason) = self.finish_reason() {
            completion = completion.with_finish_reason(reason);
        }
        if let Some(reasoning) = self.reasoning() {
            completion = completion.with_reasoning_content(reasoning);
        }
        if measured {
            completion = completion.with_metrics(metrics);
        }
        Ok(completion.with_raw(RawExchange::new(
            self.request().clone(),
            self.response().clone(),
        )))
    }
}

#[cfg(test)]
#[path = "convert-tests.rs"]
mod tests;

#[cfg(test)]
#[path = "convert-tests-reply.rs"]
mod reply_tests;
