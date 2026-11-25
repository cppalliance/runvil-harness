---
name: Make harness-gateway-client an adapter
overview: Rewrite harness-gateway-client in this repository as a thin adapter over gateway-client, the HTTP client that now lives in its own repository and is pinned by commit. The adapter keeps its name and the signatures Hosts call. It converts the client's plain types to the Engine's through traits of its own, and it keeps the llama.cpp and vLLM metrics parsing and the model catalog rules. The decision pages, the README row, the crate docs, and deny.toml change in the same pull request.
todos:
  - id: harness-adapter
    content: "B2 to B5: rewrite the crate as the adapter, port its tests under the porting rule, pin gateway-client by commit, allow its git source in deny.toml"
    status: pending
  - id: harness-docs
    content: "B7: update the decision pages, the README row, the crate docs, and the crate AGENTS.md"
    status: pending
  - id: harness-pr
    content: "B8 and B9: account for every old test, open the pull request from the fork, wait for CI with the live job, merge fast-forward only"
    status: pending
isProject: false
---

# Make harness-gateway-client an adapter

This is the part of the extraction plan that concerns this repository. The parent plan also covers the new repository `gateway-client`, Workshop, `runvil`, `runvil-gateway`, and `runvil-design`. The client is built and merged before this change, and this change pins it by commit.

## Decisions this plan uses

- **One new crate, `gateway-client`, in the repository `cppalliance/gateway-client`.** It depends on no Runvil crate. The repository name is settled. The crate name is still a proposal, and a rename is cheap while pins are by Git commit.
- **The Engine and the `runvil` facade do not change.** Nothing in the Engine depends on an outside repository, and `cargo xtask api` sees no new re-export.
- **Messages cross by serde.** The client's request function takes `&[M] where M: Serialize + Sync`. The Engine's `Message` already derives `Serialize` in the OpenAI shape, so the adapter passes `&messages` straight in. `InferenceBroker::chat()` keeps its signature.
- **Tools and options are copied through the Engine's public getters.** The client has its own `Tool` and `Options`.
- **The client returns its own `Reply`. The adapter turns it into the Engine's `Completion`** through the Engine's constructors, so the Engine's checks stay in the Engine.
- **The client has its own `Error` with a `kind`.** The adapter maps it to the Engine's `Error` and `ErrorCode`.
- **No `#[non_exhaustive]` in the client, on enums, on `Error`, or on structs.** A wildcard arm in another crate turns a new variant into a run-time case, and the adapter that matches on these enums is pinned and built with the client, so a compile error on change is what we want. It also removes `ErrorCode::Internal` from the mapping and lets the adapter's tests build a client `Error` directly. On a struct with private fields the attribute did nothing. The cost is that adding a variant or a public field is a breaking change for anyone pinned to the crate, and they choose when to take it.
- **`Message` has one public text constructor.** `Message::text(role, content)` and `Message::tool(tool_call_id, content)`, with `Role` public (19 public names).
- **`Tool::new` takes the parameter schema as a JSON object (`Map<String, Value>`), not any `Value`.** The data is a JSON Schema object that tool authors write as JSON (a Plugin hands it across the C contract as text, and Lua's `tools.adhoc` converts a table), so a Rust model of JSON Schema would drop keywords or reimplement the standard. The type only guarantees what the Engine already checks, that it is an object. Confidence: medium.
- **Conversions in the adapter are traits of the adapter's own** (`IntoEngine`, `TryIntoEngine`, `IntoWire`, `TryIntoWire`), not `From` and `Into`. The orphan rule forbids `impl From<A> for B` in a crate that owns neither type, and the client's types and the Engine's types are both foreign to the adapter.
- **`harness-gateway-client` keeps its name and stays in `runvil-harness` as the adapter.** Workshop, the Harness tests, the Harness's product-boundary rules, and Workshop's `harness_pin` check all name that crate, so none of them change. The adapter re-exports `GatewayEndpoint` and `SecretString` from the client, because Workshop names them. `StreamDelta` stays inside the two crates: since the streaming change, the broker maps it to the Harness's `Delta` and no Host names it. The student programs in `runvil-classroom` pin an old Harness commit and keep building on it, and that repository is not part of this run. Optional later rename to `harness-gateway`: cosmetic, about 30 files. Confidence: medium.
- **Discovery is a feature of the client, on by default, reader only.** The Gateway repository keeps the writer, `GatewayInstanceLease`, and the six-field `gateway.json` contract. Workshop keeps the launch lock and shutdown. The reader never deletes the file.
- **Calls are explicit.** `Gateway::find()` reads the discovery file. `Gateway::find_env()` and `Gateway::find_env_named(url_var, key_var)` read environment variables. A known address is `Gateway::new(endpoint)`, plus `with_key(key)` when it needs one. Nothing reads the environment unless the Host asks. Confidence: high.
- **The image-name check moves unchanged.** The reader still requires the process image `runvil-gateway`.
- **Pinned by Git commit, not on crates.io.**

## What stays out

- The `cheapseek` repository: no edits, no writer crate, no contract change, no run-directory override.
- Launching a Gateway, the capability route, vendor-native protocol code, crates.io, and the opaque field's implementation.
- Renames in the client's public API (`Client`, `Chat`, builder style from the report's Appendix A). Stage A and B change where the code lives and which types it uses. They do not redesign the API.
- The transport trait from the report's section 6. `transport.rs` keeps calling `reqwest` and `tokio` directly, as today, and the trait can be added later without touching the rest of this plan.
- A `links` key on the client. Nothing shared crosses repositories yet (Part 10 says when to add it).

## Part 5. The adapter: `harness-gateway-client` after the split

The crate keeps its name, its place in `runvil-harness`, and the signatures Hosts call (`GatewayBroker::new`, `provide_gateway_resources`, `TokioTimer`). Only its insides change. The broker's streaming surface is the one the streaming change gave it: `InferenceBroker::chat` takes an `on_piece: Option<PieceSink>` and the broker answers `supports_streaming` with `true`. Because the name stays, Workshop, the Harness tests, and Workshop's `harness_pin` list (`["runvil", "harness-gateway-client", "plugin-mcp"]`) need no edit.

### Layout

```text
crates/harness-gateway-client/
  Cargo.toml
  src/
    lib.rs            CHANGED  the exports below
    broker.rs         CHANGED  round() and models()
    convert.rs        NEW      client types to Engine types
    catalog.rs        CHANGED  catalog_from(): today's model list rules, on Vec<Model>
    metrics.rs        NEW      llama.cpp and vLLM parsing, cut from wire/parse.rs
    fail.rs           NEW      malformed(), with_detail(), decoded(): three helpers copied from failure.rs
    resources.rs      CHANGED  imports and expose_secret()
    timer.rs          MOVED    unchanged
    test_support.rs   NEW      the four mock-gateway helpers the broker tests use
    (broker-tests.rs, catalog-tests.rs, resources-tests.rs move with their files)
```

`broker-tests.rs` uses the client's mock-gateway helpers (`serve`, `sse_app`, `sse_body`, `content_chunk`) from `transport/tests.rs`. The adapter cannot reach a `#[cfg(test)]` item of another crate, so those four functions are copied into `test_support.rs`. The other option is a `test-support` feature on the client, which adds public surface for tests, so I prefer the copy. Confidence: medium.

### Manifests

```toml
# runvil-harness/Cargo.toml, [workspace.dependencies]
# The client, from its own repository, pinned by commit. `version` beside `git` keeps cargo-deny's
# wildcard check quiet, as the entry for runvil-plugin-sys does.
gateway-client = { version = "0.1.0", git = "https://github.com/cppalliance/gateway-client", rev = "<commit pushed in Stage A>" }
```

While developing, append this to the root `Cargo.toml` so the build reads the sibling checkout, and never commit it. Replace it with a pushed `rev` before the final commit.

```toml
# runvil-harness/Cargo.toml, local only
[patch."https://github.com/cppalliance/gateway-client"]
gateway-client = { path = "../gateway-client/crates/gateway-client" }
```

The product-boundary check does not see the crate. It walks only the `crates/` directory of this workspace, and `gateway-client` is not a member.

```toml
# crates/harness-gateway-client/Cargo.toml   CHANGED
[package]
name = "harness-gateway-client"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
publish = false

[dependencies]
runvil.workspace = true
gateway-client.workspace = true
# Derives the wire shapes of llama.cpp timings in metrics.rs.
serde.workspace = true
serde_json.workspace = true
# The sleeps of TokioTimer. Non-test code spawns no task.
tokio = { workspace = true, features = ["time"] }

[dev-dependencies]
axum.workspace = true
# The request tests step a real run to get a system message and an assistant tool-call
# message, which have no public constructor.
runvil-engine.workspace = true
tokio = { workspace = true, features = ["io-util", "test-util"] }

[lints]
workspace = true
```

`bytes`, `reqwest`, and `url` are gone from the adapter. They belong to the client now.

```rust
// crates/harness-gateway-client/src/lib.rs   CHANGED (exports)

mod broker;
mod catalog;
mod convert;
mod fail;
mod metrics;
mod resources;
#[cfg(test)]
mod test_support;
mod timer;

pub use broker::GatewayBroker;
pub use gateway_client::{GatewayEndpoint, SecretString};
pub use resources::provide_gateway_resources;
pub use timer::TokioTimer;
```

These are exactly the five names that code outside the crate uses today: `GatewayBroker`, `TokioTimer`, `provide_gateway_resources`, `GatewayEndpoint`, and `SecretString`. `StreamDelta` was a sixth until Workshop moved to the Harness's `Delta`. `fetch_model_catalog` was public and nothing outside called it, so it becomes the private `catalog_from`.

### `convert.rs` (NEW)

The review asked for `From` and `Into`. Rust forbids `impl From<A> for B` in a crate that owns neither `A` nor `B` (the orphan rule, error E0117), and every conversion here pairs a client type with an Engine type, so the adapter owns neither side. The adapter has four traits of its own instead. A local trait may be implemented for foreign types, and the call sites read the same: `error.into_engine()` and `reply.try_into_engine()`. `?` does not convert on its own, so the two call sites in `broker.rs` use `map_err`.

A shared types crate would have allowed real `From` impls. That is part of what the no-shared-crate decision costs.

```rust
//! The adapter's conversions between the client's plain types and the Engine's types.

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
    type Engine;

    fn into_engine(self) -> Self::Engine;
}

/// The same, for a conversion that can fail with the Engine's `Error`.
pub(crate) trait TryIntoEngine {
    type Engine;

    fn try_into_engine(self) -> Result<Self::Engine, Error>;
}

/// An Engine value and the client value it becomes.
pub(crate) trait IntoWire {
    type Wire;

    fn into_wire(self) -> Self::Wire;
}

/// The same, for a conversion that can fail with the Engine's `Error`.
pub(crate) trait TryIntoWire {
    type Wire;

    fn try_into_wire(self) -> Result<Self::Wire, Error>;
}

/// The Engine's code for each client error kind. The match has no wildcard arm, so a kind
/// added to the client fails to compile here.
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

/// The Engine's error for a client error. The message and the prose are copied as they are,
/// so the wording a model reads is the wording the client wrote, which is today's wording.
impl IntoEngine for WireError {
    type Engine = Error;

    fn into_engine(self) -> Error {
        let mut mapped = Error::new(self.kind.into_engine(), self.message, self.summary);
        if let Some(reason) = self.reason {
            mapped.facts.insert(ErrorFact::Reason, Value::from(reason));
        }
        if let Some(name) = self.name {
            mapped.facts.insert(ErrorFact::Name, Value::from(name));
        }
        if let Some(reason) = self.finish_reason {
            mapped.facts.insert(ErrorFact::FinishReason, Value::from(reason));
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

/// The Engine accepts only a JSON object as a tool's parameter schema (`tool_schema_new`), so
/// the `else` arm cannot run today. It returns the Engine's own error, with the Engine's own
/// wording and facts, and not an `unwrap` or a silently empty schema.
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
            error.facts.insert(ErrorFact::Name, Value::from(self.name()));
            error
                .facts
                .insert(ErrorFact::Reason, Value::from("non_object_schema"));
            return Err(error);
        };
        Ok(Tool::new(self.name(), self.description(), parameters.clone()))
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
        Usage::new(self.prompt_tokens, self.completion_tokens, self.total_tokens)
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

/// Builds the Engine's `Completion` from a client `Reply`, through the Engine's own
/// constructors, so the Engine's checks run on every turn. This is the code in
/// `StreamAccumulator::finish` today that built the `Completion`, moved across the boundary.
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
                        ToolCall::from_parts(call.id(), call.name(), arguments)
                            .map_err(decoded)?,
                    );
                }
                CompletionResult::ToolCalls(parts)
            }
        };

        // The model and usage diagnostics came from the client, in today's order. The
        // llama.cpp and vLLM sections come next, as they did in `response_metadata`.
        let mut diagnostics = self.diagnostics().to_vec();
        let llama = parse_section(self.response(), "timings", parse_llama_timings, &mut diagnostics);
        let vllm = parse_section(self.response(), "metrics", parse_vllm_metrics, &mut diagnostics);
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
```

Review points for `convert.rs`:
- `ToolCall::from_parts` and `Completion::from_result` run the Engine's checks a second time. The client's checks are the same four, so a failure here means the two drifted. `decoded` turns it into the `MalformedResponse` it earned, as today.
- The order of calls (`with_metadata_diagnostics`, `with_finish_reason`, `with_reasoning_content`, `with_metrics`, `with_raw`) differs from `finish()` today, but each `with_*` sets one independent field, so the order is not visible.
- `measured` is the test `finish()` runs today. It stays so a reply with nothing measured has no `metrics`.
- The matches on `ErrorKind` and `Outcome` have no wildcard arm. There is no `ErrorCode::Internal` fallback and no "unknown outcome" error. A variant added in the client fails to compile in the adapter, which is where it should be noticed.
- The trait impls are on references (`&WireUsage`, `&WireTiming`, `&ToolSchema`, `&CompletionOptions`) where the caller keeps the value, and on the owned `WireError` and `Reply`, which the adapter consumes.

### `metrics.rs` (NEW: cut and pasted)

Move these items out of `wire/parse.rs` without change: `WireLlamaTimings`, `parse_llama_timings`, `parse_vllm_metrics`, and `parse_section`. They already use only the Engine's `LlamaTimings` and `VllmMetrics` and `serde`. Make the three functions `pub(crate)`. `parse_section` also stays in the client, where `parse_usage` still uses it. The two copies are 14 lines each.

### `fail.rs` (NEW: copied)

```rust
use std::fmt::Display;

use runvil::model::{make_completion_error, phrase};
use runvil::{Error, ErrorCode};

/// A `MalformedResponse` failure: the code's fixed phrase extended with `: ` and `specific`.
pub(crate) fn make_malformed_error(specific: impl Display) -> Error {
    let code = ErrorCode::MalformedResponse;
    make_completion_error(code, format!("{}: {specific}", phrase(code)))
}

/// Appends provider `text`, already bounded and control-escaped, to the message after `: `.
pub(crate) fn with_detail(mut error: Error, text: &str) -> Error {
    error.message.push_str(": ");
    error.message.push_str(text);
    error
}

/// An `InvalidParts` refusal of parts decoded from a reply becomes the `MalformedResponse`
/// the reply earned, keeping its specific. Any other failure passes through.
pub(crate) fn decoded(error: Error) -> Error {
    let code = ErrorCode::InvalidParts;
    if error.code != code {
        return error;
    }
    let prose = error.prose.as_str();
    let specific = prose
        .strip_prefix(phrase(code))
        .and_then(|rest| rest.strip_prefix(": "))
        .unwrap_or(prose);
    make_malformed_error(specific)
}
```

### `catalog.rs` (CHANGED: today's rules on the client's list)

This is the part of `fetch_model_catalog` that built Engine values, with its three rules unchanged: skip an entry with no context window, refuse a zero window, refuse an entry that has a window and no thinking mode. The fetch itself stays in the client.

```rust
use std::num::NonZeroU32;

use gateway_client::{Model, Thinking, escape_controls};
use runvil::Error;
use runvil::model::{ModelCatalog, ModelId, ModelInfo, ThinkingMode};

use crate::convert::IntoEngine;
use crate::fail::{make_malformed_error, with_detail};

/// The largest gateway text kept in a catalog diagnostic, in bytes.
const MAX_CATALOG_ERROR_BODY: usize = 2000;

pub(crate) fn catalog_from(models: Vec<Model>) -> Result<ModelCatalog, Error> {
    let mut infos = Vec::with_capacity(models.len());
    for entry in models {
        // An entry with no context window is not an inference model (the gateway lists its
        // transcription models here too): it gets no model info and must not fail the catalog.
        let Some(context) = entry.context else {
            continue;
        };
        let id = ModelId::gateway(entry.id).map_err(|error| {
            make_malformed_error(format!("model catalog entry has an invalid id: {error}"))
        })?;
        let context = NonZeroU32::new(context).ok_or_else(|| {
            with_detail(
                make_malformed_error("a model declares a zero-token context window"),
                &escape_controls(id.name(), MAX_CATALOG_ERROR_BODY),
            )
        })?;
        let thinking = entry.thinking.map(IntoEngine::into_engine).ok_or_else(|| {
            with_detail(
                make_malformed_error("a model declares a context window but no thinking mode"),
                &escape_controls(id.name(), MAX_CATALOG_ERROR_BODY),
            )
        })?;
        let info = ModelInfo::new(id, entry.description, context, thinking);
        infos.push(match entry.provider {
            Some(provider) => info.with_provider(provider),
            None => info,
        });
    }
    ModelCatalog::new(infos).map_err(|error| {
        with_detail(
            make_malformed_error("gateway returned an inconsistent model catalog"),
            &escape_controls(&error.to_string(), MAX_CATALOG_ERROR_BODY),
        )
    })
}

impl IntoEngine for Thinking {
    type Engine = ThinkingMode;

    fn into_engine(self) -> ThinkingMode {
        match self {
            Thinking::Never => ThinkingMode::Never,
            Thinking::Always => ThinkingMode::Always,
            Thinking::Switchable => ThinkingMode::Switchable,
        }
    }
}
```

This is why `escape_controls` is one of the client's 18 public names.

### `broker.rs` (CHANGED: `round` and `models`)

This follows `broker.rs` as it stands after the streaming change (commit `83da52a0`). `GatewayBroker`'s fields, `new`, `Debug`, `supports_streaming` (which answers `true`), and `InferenceBroker::chat` (which passes `on_piece` to `round`) are unchanged. `chat` still takes `Vec<Message>`, `Vec<ToolSchema>`, `CompletionOptions`, and `Option<PieceSink>`. What changes is what `round` and `models` call.

```rust
use gateway_client::{GatewayChat, GatewayEndpoint, SecretString, StreamDelta, Tool, fetch_models};
use runvil::harness::PieceSink;

use crate::catalog::catalog_from;
use crate::convert::{IntoEngine, IntoWire, TryIntoEngine, TryIntoWire};

impl GatewayBroker {
    /// One round on the client. Each live piece goes to `on_piece`, as the Harness's
    /// `Delta`, when the round has one.
    fn round(
        &self,
        messages: Vec<Message>,
        tools: Vec<ToolSchema>,
        options: CompletionOptions,
        on_piece: Option<PieceSink>,
    ) -> BoxFuture<'static, Result<Box<Completion>, Error>> {
        let client = self.client.clone();
        Box::pin(async move {
            let tools = tools
                .iter()
                .map(TryIntoWire::try_into_wire)
                .collect::<Result<Vec<Tool>, Error>>()?;
            // An empty advertisement sends no `tools` field at all, the plain
            // chat-completions shape.
            let tools = (!tools.is_empty()).then_some(tools.as_slice());
            let options = (&options).into_wire();
            let on_delta = move |delta: StreamDelta| {
                if let Some(sink) = on_piece.as_ref() {
                    sink(delta.into_engine());
                }
            };
            let reply = client
                .complete(&messages, tools, &options, on_delta)
                .await
                .map_err(IntoEngine::into_engine)?;
            reply.try_into_engine().map(Box::new)
        })
    }
}

impl InferenceBroker for GatewayBroker {
    fn models(&self) -> BoxFuture<'static, Result<ModelCatalog, Error>> {
        let api_root = self.endpoint.url().to_owned();
        let key = self.key.clone();
        Box::pin(async move {
            let models = fetch_models(&api_root, &key)
                .await
                .map_err(IntoEngine::into_engine)?;
            catalog_from(models)
        })
    }
    // `supports_streaming` and `chat` are unchanged.
}
```

The `match` that today turns a `StreamDelta` into a `Delta` inside the closure becomes `impl IntoEngine for StreamDelta` in `convert.rs`. Neither enum has a wildcard arm in the adapter, so a third kind of piece added to the client fails to compile there.

The line `client.complete(&messages, ...)` is where the serde decision lands. `messages` is a `Vec<runvil::model::Message>`, which derives `Serialize`, so `&messages` coerces to `&[Message]` and the compiler picks `M = runvil::model::Message`. No wrapper type and no conversion of the list.

### `resources.rs` (CHANGED: two lines)

`use crate::config::{GatewayEndpoint, SecretString};` becomes `use gateway_client::{GatewayEndpoint, SecretString};`, and `key.expose()` becomes `key.expose_secret()`. The function's signature, the `runvil/web-search` entry, and the tests are unchanged.

### New tests in the adapter (these cannot live in the client, which cannot see the Engine)

With `non_exhaustive` gone from `Error`, a test can build a client error by struct literal and check the whole mapping, which the earlier draft could only reach through a mock gateway.

```rust
// crates/harness-gateway-client/src/convert.rs, #[cfg(test)]

use gateway_client::{ErrorKind, Message as Wire, Role};
use runvil::model::{Message as Engine, phrase};

use super::*;

/// A client error with only its kind and texts set.
fn wire_error(kind: ErrorKind, message: &str, summary: &str) -> WireError {
    WireError {
        kind,
        message: message.to_owned(),
        summary: summary.to_owned(),
        status: None,
        finish_reason: None,
        prompt_tokens: None,
        context_window: None,
        reason: None,
        name: None,
    }
}

/// The client's phrase table is a copy of the Engine's. It must stay one. The two setup kinds
/// have no phrase in the Engine, because their messages are the setup text itself.
#[test]
fn each_model_kind_has_the_engine_phrase() {
    for kind in ErrorKind::ALL {
        if matches!(kind, ErrorKind::InvalidConfig | ErrorKind::InvalidRequest) {
            continue;
        }
        assert_eq!(kind.phrase(), phrase(kind.into_engine()), "{kind:?}");
    }
}

/// The two message types write the same JSON for the three cases both can build. The other
/// shapes (system, assistant with tool calls) have no public Engine constructor, and the
/// request tests cover them by stepping a real run.
#[test]
fn engine_and_client_messages_write_the_same_json() {
    let pairs = [
        (Engine::user("hi"), Wire::text(Role::User, "hi")),
        (Engine::assistant("ok"), Wire::text(Role::Assistant, "ok")),
        (Engine::tool("call_1", "42"), Wire::tool("call_1", "42")),
    ];
    for (engine, wire) in pairs {
        let engine = serde_json::to_value(&engine).expect("the Engine message serializes");
        let wire = serde_json::to_value(&wire).expect("the client message serializes");
        assert_eq!(engine, wire);
    }
}

/// The context overflow is the one error whose counts the Engine acts on.
#[test]
fn a_context_overflow_keeps_its_counts_as_engine_facts() {
    let mut wire = wire_error(ErrorKind::ContextOverflow, "full message", "the phrase");
    wire.prompt_tokens = Some(9000);
    wire.context_window = Some(8192);
    let error = wire.into_engine();
    assert_eq!(error.code, ErrorCode::ContextOverflow);
    assert_eq!(error.message, "full message");
    assert_eq!(error.prose, "the phrase");
    assert_eq!(error.facts.get(&ErrorFact::Count), Some(&Value::from(9000_u32)));
    assert_eq!(error.facts.get(&ErrorFact::Limit), Some(&Value::from(8192_u32)));
}

/// A setup error keeps the reason and the variable name that the Engine's callers read.
#[test]
fn a_setup_error_keeps_its_reason_and_name_as_engine_facts() {
    let mut wire = wire_error(ErrorKind::InvalidConfig, "missing environment variable: X", "same");
    wire.reason = Some("missing_env");
    wire.name = Some("X".to_owned());
    let error = wire.into_engine();
    assert_eq!(error.code, ErrorCode::InvalidValue);
    assert_eq!(error.facts.get(&ErrorFact::Reason), Some(&Value::from("missing_env")));
    assert_eq!(error.facts.get(&ErrorFact::Name), Some(&Value::from("X")));
}

/// An empty reply keeps the finish reason, as the `FinishReason` fact.
#[test]
fn an_empty_reply_keeps_its_finish_reason_as_an_engine_fact() {
    let mut wire = wire_error(ErrorKind::EmptyReply, "the phrase", "the phrase");
    wire.finish_reason = Some("stop".to_owned());
    let error = wire.into_engine();
    assert_eq!(error.code, ErrorCode::EmptyReply);
    assert_eq!(error.facts.get(&ErrorFact::FinishReason), Some(&Value::from("stop")));
}
```

The non-object branch of `TryIntoWire for &ToolSchema` has no test, because a `ToolSchema` with a non-object schema cannot be built: the Engine refuses it in `tool_schema_new`. Review that branch by reading it.

## Steps

### Step 0, the porting rule

The run order is Stage A, then C, then B, then D (see "The stages").

- Sync every repository in scope (Delivery rule 1) and write down the starting heads.
- Record the old crate's test list (`cargo nextest list -p harness-gateway-client`) after the sync, and save it. The report says 160 test functions, and the crate has since lost `GatewaySearch` (deleted on 2026-11-19) and gained `tests/live.rs`. The saved list is the source of truth.
- **The porting rule.** In a ported test file, the diff may change only imports, type names, accessor names, `error.code == ErrorCode::X` into `error.kind == ErrorKind::X`, and `error.facts[...]` into the matching field. Any change to a string, JSON, or number literal in a test is a finding for the reviewer. `git diff -M` makes this check mechanical.

### Stage B, in `runvil-harness`

0. **B0, sync.** `git fetch upstream` and rebase local `master` onto `upstream/master`. The Harness has 7 new upstream commits (the live tests of the shipped prompts and the CI job that runs them) and 2 local commits that upstream lacks. Rebase, check that `broker.rs` still matches Part 5 (the new upstream commits touch `tests/live.rs` and `ci.yml`, not `broker.rs`), and force-push the fork's `master` with lease.
1. **B1, dependency.** Add `gateway-client` to `[workspace.dependencies]` with the `rev` of X2 (the merged id of gateway-client pull request 2, or X1 if Stage C stalls), and append the temporary `[patch]` of Part 5 so the build reads the sibling checkout.
2. **B2, adapter.** Rewrite `harness-gateway-client` as Part 5 describes. Delete from it the files that now live in the client (`wire.rs`, `wire/`, `transport.rs`, `config.rs`, `failure.rs`, and their tests) and add `convert.rs`, `catalog.rs`, `metrics.rs`, `fail.rs`, and `test_support.rs`. Edit `broker.rs`, `resources.rs`, `lib.rs`, and `Cargo.toml`.
3. **B3, tests.** Port `broker-tests.rs`, `catalog-tests.rs`, and `resources-tests.rs` under the porting rule, and add the adapter tests of Part 5.
4. **B4, compile and test.** `cargo clippy --workspace --all-targets --all-features` with warnings denied, `cargo nextest run --locked --workspace --all-features`, `cargo test -p build-xtask` (the product-boundary check must pass unchanged), and the two `cargo doc` runs. Record each fix in the commit message.
5. **B5, pin and policy.** Remove the `[patch]` and set `rev` to a pushed commit. Add `https://github.com/cppalliance/gateway-client` to `allow-git` in [deny.toml](runvil-harness/deny.toml) and fix its comment, which today says "The one git source" and names only `runvil-plugin-api`. Run `cargo deny check` and `cargo audit`, and commit the updated `Cargo.lock`.
6. **B6, Workshop check.** Build Workshop against the local Harness with a path override in a scratch copy. It compiles with no source change, because the five names it uses still come from `harness-gateway-client`.
7. **B7, documents.** Update what a Host or a reader sees, in the same change (the Harness's own rule): the decision pages, the README row, and the crate's `//!` docs. Part 11 lists each edit.
8. **Test accounting.** Every test name in the list saved at Step 0 appears in the new repository or in the adapter. A name that does not is listed in the commit message with the reason it went. The live test `a_host_sends_the_shipped_hello_prompt_through_a_real_gateway` in `crates/harness-gateway-client/tests/live.rs` stays in the adapter, and CI's live job runs it against a real Gateway, which makes it an end-to-end check that the rewritten broker behaves as before.
9. **B9, pull request.** Push `master` to the fork, open the pull request from `vinniefalco:master` against `cppalliance/runvil-harness`, wait for CI (all jobs, the live job included), and merge fast-forward only. The merged id is H.

One Harness commit, "Make harness-gateway-client an adapter over gateway-client", holds B2 to B5. Deleting the old code and adding the adapter is one change, and it builds green. The documents of B7 can be a second commit in the same pull request.

## Part 8. Audit list

What a reviewer checks, and how:

- **Nothing in the client names Runvil.** The client builds in a repository that holds no Runvil crate, so a dependency on one cannot resolve. Check that its manifests have no `path` or `git` entry for a Runvil crate. No guard test is needed.
- **The Harness's own checks still pass.** `cargo test -p build-xtask` runs unchanged, because `gateway-client` is not a workspace member and the product-boundary matrix does not reach it ([product.rs](runvil-harness/crates/build-xtask/src/product.rs)).
- **Behavior is preserved.** The saved test list is accounted for name by name (Stage B step 8), and the test diff shows no changed literal (the porting rule).
- **No `non_exhaustive` and no `Internal` fallback.** Search the new repository for `non_exhaustive`, and the adapter for `ErrorCode::Internal`. Neither should appear.
- **Conversions are the adapter's own traits.** The compiler enforces this: a `From` impl between two foreign types does not compile.
- **The request bytes are the same.** `request-tests.rs` is ported with its JSON expectations unchanged. The adapter's broker tests send a real stepped run's messages to a mock gateway and read back the body. The system message and the assistant tool-call message have no public constructor, which is why the adapter keeps `runvil-engine` as a dev-dependency.
- **The error text is the same.** The client's classifier tests keep their expected strings. The adapter adds tests that run a round against the mock gateway for these cases and assert the Engine `Error`: a 401 (`Unavailable`, the credentials phrase), a 429 with quota text (`QuotaExhausted`), a context overflow with token counts (`Count` and `Limit` facts), a stream without `[DONE]` (`MalformedResponse`), an empty reply with a finish reason (`FinishReason` fact), and a missing environment variable (`Reason` and `Name` facts).
- **The phrase table has not drifted.** `each_model_kind_has_the_engine_phrase` compares all 12 phrases with the Engine's, visiting every kind through `ErrorKind::ALL`.
- **The error mapping keeps every fact.** `a_context_overflow_keeps_its_counts_as_engine_facts`, `a_setup_error_keeps_its_reason_and_name_as_engine_facts`, and `an_empty_reply_keeps_its_finish_reason_as_an_engine_fact`.
- **Every public item is documented.** `missing_docs` and `cargo doc` with `-D warnings` fail the build otherwise.
- **The two message types write the same JSON.** `engine_and_client_messages_write_the_same_json`.
- **The key is read in two places.** Search for `expose_secret(` in both crates. The client's `transport.rs` and `models.rs` (the `Authorization` header) and the adapter's `resources.rs` (the Plugin entry) are the only callers. The adapter's `models()` passes `&SecretString`, so it does not read the key.
- **The `unsafe` is the same.** Stage C moves 255 lines and 7 `unsafe` blocks. Diff `sys/` against the Gateway's copy, which must show only paths.

### runvil-harness: the documents (step B7)

These go in the same pull request as the adapter. The Harness's [AGENTS.md](runvil-harness/AGENTS.md) says a change to what a Host sees updates its decision entry in the same change, and that an entry is free of file paths, internal names, counts, test names, dates, and history. The edits below therefore name crates and behavior only.

- **`decisions/harness-and-host.md`, the entry "`harness-gateway-client` is a public Harness crate beside `runvil`".** The crate holds `GatewayBroker`, `provide_gateway_resources`, and `TokioTimer`, and re-exports `GatewayEndpoint` and `SecretString`. The HTTP client, the wire code, and the fetch of the model list are in `gateway-client`, a repository of its own, which this crate depends on at a pinned commit. An outside crate may depend on `runvil` and `harness-gateway-client`. A program that wants only a client, without the Harness, depends on `gateway-client`. `GatewayChat` and `fetch_model_catalog` leave the sentence, because the crate no longer holds them.
- **The same page, the entry on `provide_gateway_resources`.** "The function is in `harness-gateway-client` because that crate alone reads the key out of the client's secret value" becomes: the function is here because this is the one place that reads the key out of the client's secret value, through the client's `expose_secret`.
- **`decisions/models-and-conversations.md`, the entry "The Engine defines the model vocabulary, and `harness-gateway-client` sends the HTTP requests".** The Engine defines the model vocabulary. `gateway-client` has its own plain types for a message, a tool, a reply, and an error, and sends the HTTP requests. `harness-gateway-client` converts between those types and the Engine's, and runs the Engine's checks on every reply. "The wire functions are public so that another broker can reuse them" becomes: any program, another broker included, can use `gateway-client` for the wire.
- **`README.md`, the row for `harness-gateway-client` in the crate table.** It describes the adapter, and one added sentence links `gateway-client`: https://github.com/cppalliance/gateway-client.
- **The `//!` docs of `crates/harness-gateway-client/src/lib.rs`.** They describe `GatewayChat`, the wire functions, and their invariants, which moved. Rewrite them for what the adapter holds. Its invariants are that every `Completion` and `ToolCall` is built through the Engine's constructors, and that the key is read in one place. The check is `node --test tools/docs-claims.mjs`, which enforces the defined terms in every `## Invariants` crate doc.
- **`crates/harness-gateway-client/AGENTS.md`.** Its one rule, that `build_request_body` is the only place a request body is shaped, moves to the client's `AGENTS.md`. The adapter's file gets a rule that fits it: conversions between client and Engine types live in `convert.rs`, and no code here shapes a request body.
