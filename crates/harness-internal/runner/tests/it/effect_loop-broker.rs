//! The effect loop answers every `Chat` effect through the inference
//! broker: the broker receives each round's `Round`, with origin `Infer`
//! for a nested `models.infer` and `Chat` for a section's own round, and
//! the run resumes with the broker's completion as each round's answer.
//! A setup that streams hands a section's own round a callback that
//! passes each piece, with its round, to the Host's callback in order,
//! and a nested `Infer` round none. A setup without streaming hands
//! every round none. Streaming changes no record of the run.

use std::collections::BTreeMap;
use std::num::NonZeroU32;
use std::sync::{Arc, Mutex};

use harness_runner::effect_loop::drive_run;
use harness_runner::performers::{Delta, InferenceBroker, PieceSink};
use harness_runner::recorder::{RecordKind, RunId, RunRecorder};
use harness_runner::{HostSink, RunSetup};
use runvil_engine::Error;
use runvil_engine::cancel::CancelHandle;
use runvil_engine::effect::Round;
use runvil_engine::event::ReplyOrigin;
use runvil_engine::ids::RoundId;
use runvil_engine::model::{
    Completion, CompletionOptions, CompletionResult, Message, ModelBinding, ModelCatalog, ModelId,
    ModelInfo, ModelRole, ThinkingMode, ToolSchema,
};
use runvil_engine::{BoxFuture, Prompt, Run, RunParams, Timestamp};
use serde_json::{Value, json};

use super::begun_log;
use crate::scripted::{Kept, ScriptedBroker};
use crate::support::{TEST_SEED, Unused};

/// A program that infers once and then chats once: the nested round
/// first, then the section's own round over its message list.
const INFERS_THEN_CHATS: &str = "---\nname: runner-broker\ndescription: infers then chats\n\
    runvil: 0\n---\n\n\
    ## Only\n\n```lua\n\
    local inferred = models.infer('infer round')\n\
    messages:user('chat round')\n\
    models.loop()\n\
    return inferred .. '|' .. messages[#messages].content\n\
    ```\n";

/// A run over [`INFERS_THEN_CHATS`] with the `default` role filled by one
/// current model.
fn infers_then_chats() -> Run {
    let prompt = Prompt::parse(&|_: &str| Ok(INFERS_THEN_CHATS.to_owned()), "prompt");
    let prompt = Arc::new(prompt.expect("the broker fixture parses"));
    let model = ModelInfo::new(
        ModelId::gateway("m").expect("the model id is valid"),
        "The host's current model",
        NonZeroU32::new(131_072).expect("131072 is non-zero"),
        ThinkingMode::Never,
    );
    let ctx = RunParams::new(TEST_SEED, Timestamp::UNIX_EPOCH)
        .with_models(BTreeMap::from([(ModelRole::Default, model)]));
    Run::new(prompt, "{}", ctx)
}

/// One round as the broker saw it: the last message's text and the round.
type Seen = (String, Round);

/// Replies `re: <last message>` to every round and keeps each round it
/// was handed.
#[derive(Default)]
struct RecordingBroker {
    rounds: Arc<Mutex<Vec<Seen>>>,
}

impl InferenceBroker for RecordingBroker {
    fn models(&self) -> BoxFuture<'static, Result<ModelCatalog, Error>> {
        unreachable!("the effect loop never lists models")
    }

    fn supports_streaming(&self) -> bool {
        false
    }

    fn chat(
        &self,
        _binding: ModelBinding,
        messages: Vec<Message>,
        _tools: Vec<ToolSchema>,
        _options: CompletionOptions,
        round: Round,
        _on_piece: Option<PieceSink>,
    ) -> BoxFuture<'static, Result<Box<Completion>, Error>> {
        let asked = messages
            .last()
            .map(|message| message.content().to_owned())
            .unwrap_or_default();
        self.rounds.lock().unwrap().push((asked.clone(), round));
        Box::pin(async move {
            Completion::from_result(CompletionResult::Text(format!("re: {asked}")), "m")
                .map(Box::new)
        })
    }
}

/// A setup recorded through `recorder` whose broker is the recording
/// broker, beside what it saw.
fn recording(recorder: Arc<dyn RunRecorder>) -> (RunSetup, Arc<Mutex<Vec<Seen>>>) {
    let broker = RecordingBroker::default();
    let rounds = Arc::clone(&broker.rounds);
    (RunSetup::new(recorder, broker, Unused), rounds)
}

#[tokio::test]
async fn the_broker_receives_each_rounds_round() {
    let (recorder, run_id) = begun_log().await;
    let (setup, rounds) = recording(recorder);

    drive_run(
        infers_then_chats(),
        &setup,
        Arc::new(Unused),
        run_id,
        CancelHandle::new(),
    )
    .await
    .unwrap();
    assert_eq!(
        *rounds.lock().unwrap(),
        vec![
            (
                "infer round".to_owned(),
                Round::new(RoundId::new(0), ReplyOrigin::Infer)
            ),
            (
                "chat round".to_owned(),
                Round::new(RoundId::new(1), ReplyOrigin::Chat)
            ),
        ],
        "the infer round is round 0 from `Infer`, and the chat round is round 1 from `Chat`"
    );
}

#[tokio::test]
async fn each_round_resumes_the_run_with_the_brokers_completion() {
    let (recorder, run_id) = begun_log().await;
    let (setup, _rounds) = recording(recorder.clone());

    let result = drive_run(
        infers_then_chats(),
        &setup,
        Arc::new(Unused),
        run_id,
        CancelHandle::new(),
    )
    .await;
    assert_eq!(
        result,
        Ok("re: infer round|re: chat round".to_owned()),
        "the program saw each round's reply as the broker wrote it"
    );
    let replies: Vec<Value> = recorder
        .records(run_id)
        .iter()
        .filter(|record| record.kind == RecordKind::Answer)
        .map(|record| record.payload["Chat"]["Ok"]["reply"].clone())
        .collect();
    assert_eq!(
        replies,
        vec![json!("re: infer round"), json!("re: chat round")],
        "each round's answer record holds the broker's completion"
    );
}

/// The `Infer` round of [`INFERS_THEN_CHATS`], its first.
fn infer_round() -> Round {
    Round::new(RoundId::new(0), ReplyOrigin::Infer)
}

/// The `Chat` round of [`INFERS_THEN_CHATS`], its second.
fn chat_round() -> Round {
    Round::new(RoundId::new(1), ReplyOrigin::Chat)
}

/// The pieces the streaming brokers here emit to each round that has a
/// callback: reasoning, then two fragments of text.
fn pieces() -> Vec<Delta> {
    vec![
        Delta::Reasoning("hmm".to_owned()),
        Delta::Text("one ".to_owned()),
        Delta::Text("two".to_owned()),
    ]
}

/// A Host callback that keeps every round and piece it is handed, beside
/// what it kept.
fn collecting() -> (
    impl Fn(Round, Delta) + Send + Sync + 'static,
    Kept<(Round, Delta)>,
) {
    let kept = Kept::default();
    // The callback moves into a setup, so the test keeps this second
    // handle to read what the callback collected.
    let sink = Arc::clone(&kept);
    (
        move |round, piece| sink.lock().unwrap().push((round, piece)),
        kept,
    )
}

/// A Host callback that keeps the first piece it is handed and ignores
/// the rest, beside what it kept.
fn keeping_one() -> (
    impl Fn(Round, Delta) + Send + Sync + 'static,
    Kept<(Round, Delta)>,
) {
    let kept = Kept::default();
    // The callback moves into a setup, so the test keeps this second
    // handle to read what the callback kept.
    let sink = Arc::clone(&kept);
    (
        move |round, piece| {
            let mut first = sink.lock().unwrap();
            if first.is_empty() {
                first.push((round, piece));
            }
        },
        kept,
    )
}

/// Drives [`INFERS_THEN_CHATS`] over `setup` as the run `run_id`, which
/// the setup's recorder has begun.
async fn drive_it(setup: &RunSetup, run_id: RunId) {
    drive_run(
        infers_then_chats(),
        setup,
        Arc::new(Unused),
        run_id,
        CancelHandle::new(),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn the_hosts_callback_gets_each_piece_in_order_under_the_chat_round() {
    let (recorder, run_id) = begun_log().await;
    let broker = ScriptedBroker::replying().emitting(pieces());
    let (sink, collected) = collecting();
    let setup = RunSetup::new(recorder, broker, Unused).streaming(sink);

    drive_it(&setup, run_id).await;
    assert_eq!(
        *collected.lock().unwrap(),
        pieces()
            .into_iter()
            .map(|piece| (chat_round(), piece))
            .collect::<Vec<_>>(),
        "every piece reaches the Host's callback in order, each with the chat round's id"
    );
}

#[tokio::test]
async fn an_infer_round_is_handed_no_callback_and_a_chat_round_one() {
    let (recorder, run_id) = begun_log().await;
    let broker = ScriptedBroker::replying().emitting(pieces());
    let handed = broker.handed();
    let setup = RunSetup::new(recorder, broker, Unused).streaming(|_round, _piece| {});

    drive_it(&setup, run_id).await;
    assert_eq!(
        *handed.lock().unwrap(),
        vec![(infer_round(), false), (chat_round(), true)],
        "only the section's own round is handed a callback"
    );
}

#[tokio::test]
async fn a_setup_without_streaming_hands_every_round_no_callback() {
    let (recorder, run_id) = begun_log().await;
    let broker = ScriptedBroker::replying().emitting(pieces());
    let handed = broker.handed();
    let setup = RunSetup::new(recorder, broker, Unused);

    drive_it(&setup, run_id).await;
    assert_eq!(
        *handed.lock().unwrap(),
        vec![(infer_round(), false), (chat_round(), false)],
        "a setup that leaves streaming unset hands no round a callback"
    );
}

/// The kind and payload of every record of one run of
/// [`INFERS_THEN_CHATS`] over a broker that emits [`pieces`], with
/// `streaming` as the setup's callback.
async fn recorded(streaming: Option<HostSink>) -> Vec<(RecordKind, Value)> {
    let (recorder, run_id) = begun_log().await;
    let broker = ScriptedBroker::replying().emitting(pieces());
    // The setup owns one handle to the recorder, and the test keeps this
    // second one to read the run's records after the drive.
    let mut setup = RunSetup::new(Arc::clone(&recorder), broker, Unused);
    setup.streaming = streaming;

    drive_it(&setup, run_id).await;
    recorder
        .records(run_id)
        .into_iter()
        .map(|record| (record.kind, record.payload))
        .collect()
}

#[tokio::test]
async fn the_kind_and_payload_of_every_record_are_equal_across_three_runs() {
    let off = recorded(None).await;

    let (all, collected) = collecting();
    let streamed = recorded(Some(Arc::new(all))).await;
    assert_eq!(
        collected.lock().unwrap().len(),
        pieces().len(),
        "the streaming run's callback saw every piece"
    );

    let (one, kept) = keeping_one();
    let keeps_one = recorded(Some(Arc::new(one))).await;
    assert_eq!(
        kept.lock().unwrap().len(),
        1,
        "the third run's callback kept one piece"
    );

    assert!(!off.is_empty(), "the runs recorded something to compare");
    assert_eq!(streamed, off, "streaming changes no record");
    assert_eq!(
        keeps_one, off,
        "a callback that keeps one piece changes no record"
    );
}
