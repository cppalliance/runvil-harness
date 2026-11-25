//! The request a round sends, from messages, tools, and options that a real
//! Engine run produced: the OpenAI function-call shape of a replayed tool
//! turn, message order, tool wrapping, and the option fields. Each test
//! sends the round through the broker to a mock gateway and reads back the
//! body the gateway received. A `ToolSchema` and an assistant tool-call
//! message have no public constructor, so [`tool_round`] takes both from a
//! facade `Run` that went through one tool round.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::num::NonZeroU32;
use std::sync::{Arc, Mutex};

use gateway_client::{GatewayEndpoint, SecretString};
use runvil::effect::Round as EffectRound;
use runvil::event::ReplyOrigin;
use runvil::harness::InferenceBroker;
use runvil::ids::RoundId;
use runvil::model::{CompletionOptions, Message, ModelBinding, ModelInvocation, ToolSchema};
use runvil_engine::effect::{Effect, EffectAnswer, IssuedEffect};
use runvil_engine::model::{
    Completion, CompletionResult, ModelId, ModelInfo, ModelRole, ThinkingMode, ToolCall,
};
use runvil_engine::{Prompt, Run, RunParams, RunSeed, Step, Timestamp};
use runvil_engine::{ToolCatalog, ToolId, ToolInfo, ToolOutput};
use serde_json::{Value, json};

use super::GatewayBroker;
use crate::test_support::{content_chunk, serve, sse_body};

/// What a run's second `Chat` effect sent, after one tool round.
struct Round {
    messages: Vec<Message>,
    tools: Vec<ToolSchema>,
    options: CompletionOptions,
}

/// One tool a round offers: its name, description, and parameters schema.
type Offered = (&'static str, &'static str, Value);

/// Drives a facade `Run` through one tool round and returns what its second
/// `Chat` effect sent.
///
/// The section offers every tool in `offered` as `example/test/<name>`, so
/// the model sees it as `example_test_<name>`, and runs `models.loop` over
/// one user message. The section's render is answered with its tools
/// unchanged; the first `Chat` effect with a call to the first tool,
/// `call_1` with `{"value":"one"}`; the tool call with the trusted output
/// `echoed: one`; and the second `Chat` effect, recorded here, with the
/// text `done`.
fn tool_round(offered: &[Offered]) -> Round {
    let mut offers = String::new();
    for (name, _, _) in offered {
        writeln!(offers, "tools.offer('example/test/{name}')").expect("a String takes every write");
    }
    let source = format!(
        "---\nname: round\ndescription: One tool round.\nrunvil: 0\n\
         ---\n\n## Ask\n\n\
         ```lua\n{offers}messages:user('echo once')\n\
         models.loop()\nreturn messages[#messages].content\n```\n"
    );
    let first_wire_name = format!("example_test_{}", offered[0].0);
    let parsed = Prompt::parse(&|_: &str| Ok(source.clone()), "prompt");
    let prompt = parsed.expect("the round prompt parses");
    let tools: Vec<ToolInfo> = offered
        .iter()
        .map(|(name, description, parameters)| {
            let id = ToolId::parse(&format!("example/test/{name}")).expect("a valid tool id");
            ToolInfo::new(id, *description, parameters.clone())
        })
        .collect();
    let model = ModelInfo::new(
        ModelId::gateway("m").expect("a valid model id"),
        "Plays the model from a script",
        NonZeroU32::new(8_192).expect("8192 is non-zero"),
        ThinkingMode::Never,
    );
    let ctx = RunParams::new(RunSeed::from_bytes([7; 32]), Timestamp::UNIX_EPOCH);
    let catalog = ToolCatalog::new(tools).expect("the offered tools are distinct");
    let ctx = ctx
        .with_tools(catalog)
        .with_models(BTreeMap::from([(ModelRole::Default, model)]));
    let mut run = Run::new(Arc::new(prompt), "{}", ctx);
    let mut rounds = Vec::new();
    let result = loop {
        let effects = match run.step() {
            Step::Pending { effects, .. } => effects,
            Step::Done { result, .. } => break result,
        };
        for IssuedEffect { id, effect, .. } in effects {
            let answer = match effect {
                Effect::Chat {
                    binding,
                    messages,
                    tools,
                    ..
                } => {
                    let reply = if rounds.is_empty() {
                        let call = ToolCall::from_parts(
                            "call_1",
                            first_wire_name.as_str(),
                            json!({ "value": "one" }),
                        )
                        .expect("a whole call");
                        CompletionResult::ToolCalls(vec![call])
                    } else {
                        CompletionResult::Text("done".to_owned())
                    };
                    rounds.push(Round {
                        messages,
                        tools,
                        options: binding.completion_options(),
                    });
                    EffectAnswer::Chat(Completion::from_result(reply, "m").map(Box::new))
                }
                Effect::Render { offered, .. } => EffectAnswer::Render(offered),
                Effect::ToolCall { .. } => {
                    EffectAnswer::ToolCall(Ok(ToolOutput::trusted("echoed: one")))
                }
                _ => EffectAnswer::Dropped,
            };
            run.resume(id, answer);
        }
    };
    assert_eq!(
        result,
        Ok("done".to_owned()),
        "the round runs to its terminal reply"
    );
    assert_eq!(rounds.len(), 2, "one tool round, then the terminal round");
    rounds.pop().expect("the terminal round was recorded")
}

/// The `echo` tool with a one-string-field schema.
fn echo() -> Offered {
    (
        "echo",
        "Echo a value.",
        json!({
            "type": "object",
            "properties": { "value": { "type": "string" } },
            "required": ["value"],
        }),
    )
}

/// A mock gateway that records the JSON body of each completion it is sent
/// and answers it with a one-chunk reply.
fn recording_app(sent: Arc<Mutex<Option<Value>>>) -> axum::Router {
    use axum::Router;
    use axum::routing::post;

    Router::new().route(
        "/v1/chat/completions",
        post(move |axum::Json(body): axum::Json<Value>| {
            let sent = Arc::clone(&sent);
            async move {
                *sent.lock().unwrap() = Some(body);
                let reply = sse_body(&[
                    content_chunk("ok"),
                    json!({ "choices": [{ "index": 0, "delta": {}, "finish_reason": "stop" }] }),
                ]);
                (
                    [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                    reply,
                )
            }
        }),
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

/// Sends one round through the broker to a recording mock gateway and returns
/// the JSON body the gateway received.
async fn body_sent(
    messages: Vec<Message>,
    tools: Vec<ToolSchema>,
    options: CompletionOptions,
) -> Value {
    let sent = Arc::new(Mutex::new(None));
    let url = serve(recording_app(Arc::clone(&sent))).await;
    let broker = GatewayBroker::new(
        GatewayEndpoint::new(&url).expect("valid test endpoint"),
        SecretString::new("tok").expect("non-empty test key"),
    );
    broker
        .chat(
            binding(),
            messages,
            tools,
            options,
            EffectRound::new(RoundId::new(0), ReplyOrigin::Chat),
            None,
        )
        .await
        .expect("the mock round completes");
    let body = sent.lock().unwrap().take();
    body.expect("the gateway received the request")
}

/// Checks every `messages[].tool_calls[]` entry against the OpenAI
/// function-call schema a strict endpoint enforces, returning the
/// offending path for the first violation.
fn check_openai_tool_calls(body: &Value) -> Result<(), String> {
    let messages = body
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| "request body had no messages array".to_owned())?;
    for (message_index, message) in messages.iter().enumerate() {
        let Some(calls) = message.get("tool_calls").filter(|calls| !calls.is_null()) else {
            continue;
        };
        let calls = calls.as_array().ok_or_else(|| {
            format!("messages[{message_index}].tool_calls was present but not an array")
        })?;
        for (call_index, call) in calls.iter().enumerate() {
            let path = format!("messages[{message_index}].tool_calls[{call_index}]");
            if !call.is_object() {
                return Err(format!("{path} was not an object"));
            }
            if call.get("type") != Some(&Value::String("function".to_owned())) {
                return Err(format!("{path}.type must be the string \"function\""));
            }
            let id = call
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("{path} had no string id"))?;
            if id.trim().is_empty() {
                return Err(format!("{path}.id was blank"));
            }
            let function = call
                .get("function")
                .filter(|function| function.is_object())
                .ok_or_else(|| format!("{path}.function was not an object"))?;
            let name = function
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("{path}.function had no string name"))?;
            if name.trim().is_empty() {
                return Err(format!("{path}.function.name was blank"));
            }
            let raw = function
                .get("arguments")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    format!("{path}.function.arguments was not a JSON-encoded string")
                })?;
            let decoded = serde_json::from_str::<Value>(raw).map_err(|error| {
                format!("{path}.function.arguments was not valid JSON: {error}")
            })?;
            if !decoded.is_object() {
                return Err(format!(
                    "{path}.function.arguments did not decode to a JSON object"
                ));
            }
        }
    }
    Ok(())
}

#[tokio::test]
async fn a_replayed_tool_call_turn_serializes_in_the_openai_function_shape() {
    let round = tool_round(&[echo()]);
    let body = body_sent(round.messages, Vec::new(), round.options).await;
    assert_eq!(check_openai_tool_calls(&body), Ok(()), "{body}");
    let call = &body["messages"][1]["tool_calls"][0];
    assert_eq!(call["id"], "call_1");
    assert_eq!(call["type"], "function");
    assert_eq!(call["function"]["name"], "example_test_echo");
    assert_eq!(call["function"]["arguments"], "{\"value\":\"one\"}");
    assert_eq!(body["messages"][2]["role"], "tool");
    assert_eq!(body["messages"][2]["tool_call_id"], "call_1");
    assert_eq!(body["messages"][2]["content"], "echoed: one");
}

#[tokio::test]
async fn messages_go_out_verbatim_in_order() {
    let messages = vec![
        Message::user("draft text"),
        Message::assistant("done"),
        Message::tool("call_1", "echoed: one"),
    ];
    let body = body_sent(messages, Vec::new(), CompletionOptions::new("m")).await;
    assert_eq!(
        body["messages"],
        json!([
            { "role": "user", "content": "draft text" },
            { "role": "assistant", "content": "done" },
            { "role": "tool", "content": "echoed: one", "tool_call_id": "call_1" },
        ])
    );
}

#[tokio::test]
async fn each_tool_is_wrapped_as_a_function_with_its_schema_in_order() {
    let (_, _, parameters) = echo();
    let round = tool_round(&[
        echo(),
        ("grab", "Grab a value", json!({ "type": "object" })),
    ]);
    let body = body_sent(
        vec![Message::user("hi")],
        round.tools,
        CompletionOptions::new("m"),
    )
    .await;
    assert_eq!(
        body["tools"],
        json!([
            {
                "type": "function",
                "function": {
                    "name": "example_test_echo",
                    "description": "Echo a value.",
                    "parameters": parameters,
                },
            },
            {
                "type": "function",
                "function": {
                    "name": "example_test_grab",
                    "description": "Grab a value",
                    "parameters": { "type": "object" },
                },
            },
        ])
    );
}

#[tokio::test]
async fn unset_options_send_no_option_fields() {
    let body = body_sent(
        vec![Message::user("hi")],
        Vec::new(),
        CompletionOptions::new("analyst"),
    )
    .await;
    assert_eq!(body["model"], "analyst");
    for field in ["temperature", "max_tokens", "chat_template_kwargs"] {
        assert!(body.get(field).is_none(), "{field} is absent: {body}");
    }
}

#[tokio::test]
async fn the_body_always_streams_and_asks_for_usage() {
    let body = body_sent(
        vec![Message::user("hi")],
        Vec::new(),
        CompletionOptions::new("analyst"),
    )
    .await;
    assert_eq!(body["model"], "analyst");
    assert_eq!(body["stream"], true);
    assert_eq!(body["stream_options"]["include_usage"], true);
    assert!(body.get("tools").is_none(), "no tools field without tools");
    assert!(body.get("temperature").is_none());
}

#[tokio::test]
async fn options_and_tools_reach_the_body() {
    let options = CompletionOptions::new("analyst")
        .with_temperature(0.0)
        .expect("0.0 is valid")
        .with_max_tokens(NonZeroU32::new(128).expect("128 is non-zero"))
        .with_thinking(false);
    let round = tool_round(&[("echo", "Echo.", json!({ "type": "object" }))]);
    let body = body_sent(vec![Message::user("hi")], round.tools, options).await;
    assert_eq!(body["temperature"], 0.0);
    assert_eq!(body["max_tokens"], 128);
    assert_eq!(body["chat_template_kwargs"]["enable_thinking"], false);
    assert_eq!(body["tool_choice"], "auto");
    assert_eq!(body["tools"][0]["type"], "function");
    assert_eq!(body["tools"][0]["function"]["name"], "example_test_echo");
}

#[tokio::test]
async fn an_empty_tool_list_sends_no_tools_field() {
    let body = body_sent(
        vec![Message::user("hi")],
        Vec::new(),
        CompletionOptions::new("m"),
    )
    .await;
    assert!(body.get("tools").is_none());
    assert!(body.get("tool_choice").is_none());
}
