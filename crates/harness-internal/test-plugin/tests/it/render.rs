//! A section whose Pack lives in the fixture library renders its tools
//! for the model: the library shapes them by the model's name, the model
//! is shown the names the library lists, a call to a listed name reaches
//! the library's tool of that Pack, and a render the library declines or
//! misuses ends the round as canceled before any request goes out.

use std::num::NonZeroU32;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use harness_gateway_client::TokioTimer;
use runvil::effect::Round;
use runvil::harness::{FindTextPrompt, InferenceBroker, MemoryRecorder, PieceSink};
use runvil::model::{
    Completion, CompletionOptions, CompletionResult, Message, ModelBinding, ModelCatalog, ModelId,
    ModelInfo, ThinkingMode, ToolCall, ToolSchema,
};
use runvil::{BoxFuture, Error, Harness, Host, RunReport, RunSetup};
use serde_json::json;

use crate::support::{copy_fixture, remove, scratch};

/// The name each Host finds its prompt by.
const PROMPT: &str = "prompt.md";

/// What the broker was shown: the wire names each round advertised, and
/// the text of the last message of each round.
#[derive(Default)]
struct Shown {
    tools: Vec<Vec<String>>,
    last: Vec<String>,
}

/// A broker that lists one model, named `model`, calls the tool `wire`
/// in its first round, and answers `done` in every later one. It keeps
/// what each round showed it.
struct ModelBroker {
    model: &'static str,
    wire: &'static str,
    rounds: AtomicUsize,
    shown: Arc<Mutex<Shown>>,
}

impl InferenceBroker for ModelBroker {
    fn models(&self) -> BoxFuture<'static, Result<ModelCatalog, Error>> {
        let model = self.model;
        Box::pin(async move {
            let info = ModelInfo::new(
                ModelId::gateway(model).expect("a valid model id"),
                "A model named for what its render does",
                NonZeroU32::new(8192).expect("a non-zero window"),
                ThinkingMode::Never,
            );
            Ok(ModelCatalog::new([info]).expect("one model is a catalog"))
        })
    }

    fn supports_streaming(&self) -> bool {
        false
    }

    fn chat(
        &self,
        _binding: ModelBinding,
        messages: Vec<Message>,
        tools: Vec<ToolSchema>,
        _options: CompletionOptions,
        _round: Round,
        _on_piece: Option<PieceSink>,
    ) -> BoxFuture<'static, Result<Box<Completion>, Error>> {
        let first = self.rounds.fetch_add(1, Ordering::SeqCst) == 0;
        {
            let mut shown = self.shown.lock().expect("the lock holds");
            shown
                .tools
                .push(tools.iter().map(|tool| tool.name().to_owned()).collect());
            shown.last.push(
                messages
                    .last()
                    .map(|message| message.content().to_owned())
                    .unwrap_or_default(),
            );
        }
        let (model, wire) = (self.model, self.wire);
        Box::pin(async move {
            let result = if first {
                let call = ToolCall::from_parts("call-1", wire, json!({ "a": 1 }))?;
                CompletionResult::ToolCalls(vec![call])
            } else {
                CompletionResult::Text("done".to_owned())
            };
            Completion::from_result(result, model).map(Box::new)
        })
    }
}

/// A prompt that declares the fixture's Pack and runs `lua` in one
/// section.
fn prompt(lua: &str) -> String {
    format!(
        "---\nname: fixture\ndescription: renders the fixture's tools\nrunvil: 0\n\
         uses:\n  - fixture/test\n---\n\n## Only\n\n```lua\n{lua}\n```\n"
    )
}

/// Runs `lua` over the fixture library in `dir`, with a model named
/// `model` whose first round calls `wire`, and returns the run's report
/// and what the broker was shown. The Host and the Harness are gone
/// when this returns, so no library is held.
async fn run_over(
    dir: &Path,
    model: &'static str,
    wire: &'static str,
    lua: &str,
) -> (RunReport, Arc<Mutex<Shown>>) {
    let shown = Arc::new(Mutex::new(Shown::default()));
    let broker = ModelBroker {
        model,
        wire,
        rounds: AtomicUsize::new(0),
        shown: Arc::clone(&shown),
    };
    let host = Host::builder(FindTextPrompt::new(PROMPT, prompt(lua)))
        .with_plugin_dir(dir)
        .build();
    let [report] = host.plugin_reports() else {
        panic!("one report for the directory: {:?}", host.plugin_reports());
    };
    assert!(report.failures().is_empty(), "{:?}", report.failures());
    let parsed = host.parse(PROMPT).expect("the prompt parses");
    let setup = RunSetup::new(Arc::new(MemoryRecorder::new()), broker, TokioTimer);
    let report = Harness::new(Arc::clone(&host), parsed, setup)
        .run()
        .await
        .expect("the recorder takes the run");
    (report, shown)
}

/// The Lua of a section that offers the fixture's `hello`, asks the model
/// once, and returns the last message.
const ASKS: &str = "tools.offer('test/hello')\nmessages:user('go')\nmodels.loop()\n\
                    return messages[#messages].content";

/// The Lua of a section that offers `hello` and returns whether its
/// round ended, and how.
const CATCHES: &str = "tools.offer('test/hello')\nmessages:user('go')\n\
                       local ok, err = pcall(models.loop)\n\
                       return tostring(ok) .. ':' .. err.kind";

/// Runs the section [`ASKS`] for the model `model`, whose round calls
/// `wire`, over a fresh copy of the library, and returns the result, the
/// wire names the first round advertised, and the text the second round
/// began with, which is the call's result.
async fn asks(
    label: &str,
    model: &'static str,
    wire: &'static str,
) -> (String, Vec<String>, String) {
    let dir = scratch(label);
    drop(copy_fixture(&dir, "plugin"));
    let (report, shown) = run_over(&dir, model, wire, ASKS).await;
    let shown = shown.lock().expect("the lock holds");
    let result = report.result.expect("the run completes");
    let advertised = shown.tools[0].clone();
    let called = shown.last[1].clone();
    drop(shown);
    remove(&dir);
    (result, advertised, called)
}

#[tokio::test]
async fn a_model_is_shown_the_names_the_librarys_render_lists_and_a_call_of_one_reaches_the_tool() {
    let (result, advertised, called) = asks(
        "a_model_is_shown_the_names_the_render_lists",
        "shape",
        "shown_hello",
    )
    .await;
    assert_eq!(result, "done");
    assert_eq!(
        advertised,
        ["shown_hello", "test_lite"],
        "hello is renamed, and the tool only the model sees is added under its default wire name"
    );
    assert_eq!(
        called, r#"hello {"a":1}"#,
        "a call of the wire name reaches the library's hello with the model's arguments"
    );
}

#[tokio::test]
async fn a_call_of_the_tool_only_the_model_sees_reaches_the_library() {
    let (result, _advertised, called) = asks(
        "a_call_of_the_tool_only_the_model_sees",
        "shape",
        "test_lite",
    )
    .await;
    assert_eq!(result, "done");
    assert_eq!(
        called, r#"lite {"a":1}"#,
        "the library answers lite by its name, though no catalog lists it"
    );
}

#[tokio::test]
async fn a_model_the_library_leaves_alone_is_shown_the_offered_tool_as_it_was() {
    let (_result, advertised, called) =
        asks("a_model_the_library_leaves_alone", "m", "test_hello").await;
    assert_eq!(advertised, ["test_hello"]);
    assert_eq!(called, r#"hello {"a":1}"#);
}

#[tokio::test]
async fn a_library_that_answers_unsupported_leaves_the_offered_tool_as_it_was() {
    let (_result, advertised, _called) = asks(
        "a_library_that_answers_unsupported",
        "unsupported",
        "test_hello",
    )
    .await;
    assert_eq!(advertised, ["test_hello"]);
}

#[tokio::test]
async fn a_render_the_library_declines_or_misuses_ends_the_round_as_canceled_before_a_request() {
    for model in ["refuse", "misuse"] {
        let dir = scratch(&format!("a_render_that_fails_{model}"));
        drop(copy_fixture(&dir, "plugin"));
        let (report, shown) = run_over(&dir, model, "test_hello", CATCHES).await;
        assert_eq!(report.result, Ok("false:canceled".to_owned()), "{model}");
        assert!(
            shown.lock().expect("the lock holds").tools.is_empty(),
            "{model}: no round went out"
        );
        remove(&dir);
    }
}
