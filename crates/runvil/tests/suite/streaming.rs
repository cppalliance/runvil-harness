//! Streaming pieces from a model round to a Host through the Harness.
//!
//! The Harness passes the broker an `on_piece` callback only for a
//! section's own `Chat` rounds, calls the Host's callback for every piece, and keeps the finished reply on the normal recorder path.

use std::num::NonZeroU32;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use harness_gateway_client::{GatewayBroker, GatewayEndpoint, SecretString, TokioTimer};
use runvil::effect::Round;
use runvil::event::ReplyOrigin;
use runvil::harness::{
    Delta, InferenceBroker, MemoryRecorder, PieceSink, Record, RecordKind, RunId, RunMeta,
    RunRecorder,
};
use runvil::ids::RoundId;
use runvil::model::{
    Completion, CompletionOptions, CompletionResult, Message, ModelBinding, ModelCatalog, ModelId,
    ModelInfo, ThinkingMode, ToolSchema,
};
use runvil::{BoxFuture, Error, RunControl, RunSetup};

use crate::support::launch;

/// The one model every fixture broker lists.
const MODEL: &str = "streaming-model";

/// A program that infers once and then chats once: the nested round
/// first, then the section's own round over its message list.
const INFERS_THEN_CHATS: &str = "---\nname: rounds\ndescription: infers then chats\n\
    runvil: 0\n---\n\n\
    ## Only\n\n```lua\n\
    local inferred = models.infer('infer round')\n\
    messages:user('chat round')\n\
    models.loop()\n\
    return inferred .. '|' .. messages[#messages].content\n\
    ```\n";

/// A program that chats twice so each Chat round's pieces can be checked independently.
const CHATS_TWICE: &str = "---\nname: twice\ndescription: chats twice\n\
    runvil: 0\n---\n\n\
    ## Only\n\n```lua\n\
    messages:user('first')\n\
    models.loop()\n\
    messages:user('second')\n\
    models.loop()\n\
    return messages[#messages].content\n\
    ```\n";

/// The main chain starts two section-chat rounds together and joins them.
const PARALLEL_CHATS: &str = "---\nname: parallel\ndescription: parallel chats\n\
    runvil: 0\n---\n\n\
    ## Main\n\n```lua\n\
    local first = tasks.spawn('../First')\n\
    local second = tasks.spawn('../Second')\n\
    tasks.join({ first, second })\n\
    return 'joined'\n```\n\
    ## First\n\n```lua\n\
    messages:user('first')\nmodels.loop()\nreturn 'first done'\n```\n\
    ## Second\n\n```lua\n\
    messages:user('second')\nmodels.loop()\nreturn 'second done'\n```\n";

/// A program that chats once and returns the reply.
const CHATS_ONCE: &str = "---\nname: once\ndescription: chats once\n\
    runvil: 0\n---\n\n\
    ## Only\n\n```lua\n\
    messages:user('hello')\n\
    models.loop()\n\
    return messages[#messages].content\n\
    ```\n";

/// Sends a run message during its first record append.
struct SendsOnFirstAppend {
    inner: MemoryRecorder,
    control: Arc<Mutex<Option<RunControl>>>,
    sent: AtomicBool,
}

impl RunRecorder for SendsOnFirstAppend {
    fn begin_run(&self, meta: RunMeta) -> BoxFuture<'_, Result<RunId, Error>> {
        self.inner.begin_run(meta)
    }

    fn append(&self, run: RunId, record: Record) -> BoxFuture<'_, Result<(), Error>> {
        if !self.sent.swap(true, Ordering::SeqCst) {
            let control = self.control.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(control) = control.as_ref() {
                assert!(control.send("queued before the model answer".to_owned()));
            }
        }
        self.inner.append(run, record)
    }

    fn end_run(
        &self,
        run: RunId,
        result: Result<String, Error>,
    ) -> BoxFuture<'_, Result<(), Error>> {
        self.inner.end_run(run, result)
    }
}

/// A program that chats once with a specific user message and returns the reply.
fn chats_once_with(message: &str) -> String {
    format!(
        "---\nname: once\ndescription: chats once\n\
         runvil: 0\n---\n\n\
         ## Only\n\n```lua\n\
         messages:user('{message}')\n\
         models.loop()\n\
         return messages[#messages].content\n\
         ```\n"
    )
}

/// The model catalog every fixture broker serves.
fn catalog() -> Result<ModelCatalog, Error> {
    ModelCatalog::new([ModelInfo::new(
        ModelId::gateway(MODEL)?,
        "the streaming model",
        NonZeroU32::new(131_072).ok_or_else(|| {
            Error::new(
                runvil::ErrorCode::Internal,
                "131072 is non-zero",
                "an internal constant was invalid",
            )
        })?,
        ThinkingMode::Never,
    )])
}

/// What a [`PieceEmittingBroker`] remembers about a round it served.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Seen {
    round: Round,
    had_sink: bool,
}

/// A broker that emits `pieces` for every `Chat` round and replies
/// `re: <last message>` to every round.
struct PieceEmittingBroker {
    catalog: ModelCatalog,
    pieces: Vec<&'static str>,
    supports_streaming: bool,
    rounds: Arc<Mutex<Vec<Seen>>>,
}

impl PieceEmittingBroker {
    /// A broker that emits `pieces` for Chat rounds and reports that it
    /// streams.
    fn new(catalog: ModelCatalog, pieces: Vec<&'static str>) -> Self {
        Self {
            catalog,
            pieces,
            supports_streaming: true,
            rounds: Arc::default(),
        }
    }

    /// This broker reports that it does not stream.
    fn non_streaming(mut self) -> Self {
        self.supports_streaming = false;
        self
    }

    /// The rounds this broker saw, in dispatch order.
    fn rounds(&self) -> Arc<Mutex<Vec<Seen>>> {
        Arc::clone(&self.rounds)
    }
}

/// Sets an atomic flag when it is dropped, so a test can observe that a
/// broker's round future was dropped.
struct DropFlag(Arc<AtomicBool>);

impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// A handle to a [`CancelingBroker`] that lets the test open its readiness
/// gate and observe whether the broker's round future has been dropped.
struct CancelingBrokerHandle {
    gate: tokio::sync::watch::Sender<bool>,
    readiness: tokio::sync::watch::Receiver<bool>,
    dropped: Arc<AtomicBool>,
}

impl CancelingBrokerHandle {
    /// True once the broker's round future has been dropped.
    fn is_dropped(&self) -> bool {
        self.dropped.load(Ordering::SeqCst)
    }

    /// Opens the readiness gate and reports whether the persistent gate
    /// accepted the update.
    fn release(&self) -> Result<(), tokio::sync::watch::error::SendError<bool>> {
        self.gate.send(true)
    }

    /// True once the test observer sees that the gate was opened.
    fn is_ready(&self) -> bool {
        *self.readiness.borrow()
    }
}

/// A broker that emits one piece and then waits on a controllable gate, so
/// a cancel raised from the Host's sink aborts the round before any more
/// pieces or a reply.
struct CancelingBroker {
    catalog: ModelCatalog,
    pieces: Vec<&'static str>,
    receiver: Arc<Mutex<Option<tokio::sync::watch::Receiver<bool>>>>,
    dropped: Arc<AtomicBool>,
}

impl CancelingBroker {
    /// The first piece this broker emits before it stalls.
    const FIRST_PIECE: &'static str = "first piece";
    /// A second piece the broker would emit if the gate opened before the
    /// round was canceled.
    const SECOND_PIECE: &'static str = "second piece";

    fn new(catalog: ModelCatalog) -> (Self, CancelingBrokerHandle) {
        let (gate, receiver) = tokio::sync::watch::channel(false);
        let dropped = Arc::new(AtomicBool::new(false));
        let broker = Self {
            catalog,
            pieces: vec![Self::FIRST_PIECE, Self::SECOND_PIECE],
            receiver: Arc::new(Mutex::new(Some(receiver.clone()))),
            dropped: Arc::clone(&dropped),
        };
        let handle = CancelingBrokerHandle {
            gate,
            readiness: receiver.clone(),
            dropped,
        };
        (broker, handle)
    }
}

impl InferenceBroker for CancelingBroker {
    fn models(&self) -> BoxFuture<'static, Result<ModelCatalog, Error>> {
        let catalog = self.catalog.clone();
        Box::pin(async move { Ok(catalog) })
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn chat(
        &self,
        _binding: ModelBinding,
        _messages: Vec<Message>,
        _tools: Vec<ToolSchema>,
        _options: CompletionOptions,
        _round: Round,
        on_piece: Option<PieceSink>,
    ) -> BoxFuture<'static, Result<Box<Completion>, Error>> {
        let pieces = self.pieces.clone();
        let receiver = self
            .receiver
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        let dropped = DropFlag(Arc::clone(&self.dropped));
        Box::pin(async move {
            let _guard = dropped;
            if let Some(sink) = on_piece.as_ref() {
                sink(Delta::Text(pieces[0].to_owned()));
            }
            if let Some(mut receiver) = receiver {
                let _ = receiver.changed().await;
            }
            if let Some(sink) = on_piece {
                sink(Delta::Text(pieces[1].to_owned()));
            }
            Completion::from_result(CompletionResult::Text("re: hello".to_owned()), MODEL)
                .map(Box::new)
        })
    }
}

impl InferenceBroker for PieceEmittingBroker {
    fn models(&self) -> BoxFuture<'static, Result<ModelCatalog, Error>> {
        let catalog = self.catalog.clone();
        Box::pin(async move { Ok(catalog) })
    }

    fn supports_streaming(&self) -> bool {
        self.supports_streaming
    }

    fn chat(
        &self,
        _binding: ModelBinding,
        messages: Vec<Message>,
        _tools: Vec<ToolSchema>,
        _options: CompletionOptions,
        round: Round,
        on_piece: Option<PieceSink>,
    ) -> BoxFuture<'static, Result<Box<Completion>, Error>> {
        self.rounds
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(Seen {
                round,
                had_sink: on_piece.is_some(),
            });
        let pieces = self.pieces.clone();
        let asked = messages
            .last()
            .map(|message| message.content().to_owned())
            .unwrap_or_default();
        let should_stream = self.supports_streaming && on_piece.is_some();
        Box::pin(async move {
            if should_stream {
                for piece in pieces {
                    if let Some(sink) = on_piece.as_ref() {
                        sink(Delta::Text(piece.to_owned()));
                    }
                }
            }
            Completion::from_result(CompletionResult::Text(format!("re: {asked}")), MODEL)
                .map(Box::new)
        })
    }
}

/// A broker that makes two concurrent runs wait on a barrier inside their
/// `Chat` calls before it emits run-specific pieces.
struct OverlappingBroker {
    catalog: ModelCatalog,
    barrier: Arc<tokio::sync::Barrier>,
}

impl OverlappingBroker {
    fn new(catalog: ModelCatalog) -> Self {
        Self {
            catalog,
            barrier: Arc::new(tokio::sync::Barrier::new(2)),
        }
    }
}

impl InferenceBroker for OverlappingBroker {
    fn models(&self) -> BoxFuture<'static, Result<ModelCatalog, Error>> {
        let catalog = self.catalog.clone();
        Box::pin(async move { Ok(catalog) })
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn chat(
        &self,
        _binding: ModelBinding,
        messages: Vec<Message>,
        _tools: Vec<ToolSchema>,
        _options: CompletionOptions,
        _round: Round,
        on_piece: Option<PieceSink>,
    ) -> BoxFuture<'static, Result<Box<Completion>, Error>> {
        let barrier = Arc::clone(&self.barrier);
        let asked = messages
            .last()
            .map(|message| message.content().to_owned())
            .unwrap_or_default();
        Box::pin(async move {
            // Both runs must reach here before either emits a piece.
            barrier.wait().await;
            let pieces = vec![format!("{asked}-A "), format!("{asked}-B ")];
            if let Some(sink) = on_piece {
                for piece in &pieces {
                    sink(Delta::Text(piece.clone()));
                }
            }
            Completion::from_result(CompletionResult::Text(format!("re: {asked}")), MODEL)
                .map(Box::new)
        })
    }
}

#[tokio::test]
async fn a_chat_round_gets_a_sink_and_an_infer_round_gets_none() {
    let broker = PieceEmittingBroker::new(catalog().expect("the catalog builds"), vec!["piece "]);
    let rounds = broker.rounds();
    let setup = RunSetup::new(MemoryRecorder::new(), broker, TokioTimer)
        .selected_model(MODEL)
        .streaming(|_round, _delta| {});
    let report = launch(INFERS_THEN_CHATS, setup)
        .run()
        .await
        .expect("the run is reported");

    assert_eq!(
        report.result,
        Ok("re: infer round|re: chat round".to_owned())
    );
    assert_eq!(
        *rounds.lock().unwrap(),
        [
            Seen {
                round: Round::new(RoundId::new(0), ReplyOrigin::Infer),
                had_sink: false,
            },
            Seen {
                round: Round::new(RoundId::new(1), ReplyOrigin::Chat),
                had_sink: true,
            },
        ]
    );
}

#[tokio::test]
async fn leaving_streaming_unset_gives_every_round_no_sink() {
    let broker = PieceEmittingBroker::new(catalog().expect("the catalog builds"), vec!["piece "]);
    let rounds = broker.rounds();
    let setup = RunSetup::new(MemoryRecorder::new(), broker, TokioTimer).selected_model(MODEL);
    let report = launch(INFERS_THEN_CHATS, setup)
        .run()
        .await
        .expect("the run is reported");

    assert_eq!(
        report.result,
        Ok("re: infer round|re: chat round".to_owned())
    );
    assert!(
        rounds.lock().unwrap().iter().all(|seen| !seen.had_sink),
        "no round receives a sink when streaming is unset"
    );
}

#[tokio::test]
async fn the_host_receives_every_piece_across_rounds() {
    let broker = PieceEmittingBroker::new(
        catalog().expect("the catalog builds"),
        vec!["one ", "two ", "three "],
    );
    let pieces: Arc<Mutex<Vec<(u64, String)>>> = Arc::default();
    let kept = Arc::clone(&pieces);
    let setup = RunSetup::new(MemoryRecorder::new(), broker, TokioTimer)
        .selected_model(MODEL)
        .streaming(move |round, delta| {
            if let Delta::Text(text) = delta {
                kept.lock().unwrap().push((round.id.get(), text));
            }
        });
    let report = launch(CHATS_TWICE, setup)
        .run()
        .await
        .expect("the run is reported");

    assert_eq!(report.result, Ok("re: second".to_owned()));
    assert_eq!(
        *pieces.lock().unwrap(),
        [
            (0, "one ".to_owned()),
            (0, "two ".to_owned()),
            (0, "three ".to_owned()),
            (1, "one ".to_owned()),
            (1, "two ".to_owned()),
            (1, "three ".to_owned()),
        ],
        "the Harness forwards every piece in each Chat round"
    );
}

#[tokio::test]
async fn a_non_streaming_broker_receives_the_sink_but_does_not_call_it() {
    let broker =
        PieceEmittingBroker::new(catalog().expect("the catalog builds"), vec!["one ", "two "])
            .non_streaming();
    let rounds = broker.rounds();
    let sink_called = Arc::new(Mutex::new(false));
    let called = Arc::clone(&sink_called);
    let setup = RunSetup::new(MemoryRecorder::new(), broker, TokioTimer)
        .selected_model(MODEL)
        .streaming(move |_round, _delta| {
            *called.lock().unwrap() = true;
        });
    let report = launch(CHATS_ONCE, setup)
        .run()
        .await
        .expect("the run is reported");

    assert_eq!(report.result, Ok("re: hello".to_owned()));
    assert!(
        !*sink_called.lock().unwrap(),
        "a broker that does not stream never calls the sink"
    );
    assert!(
        rounds.lock().unwrap().iter().all(|seen| seen.had_sink),
        "`supports_streaming` does not gate the sink: the Harness still passes it"
    );
}

#[tokio::test]
async fn two_concurrent_runs_overlap_inside_the_broker_and_keep_their_payloads_separate() {
    let broker = Arc::new(OverlappingBroker::new(
        catalog().expect("the catalog builds"),
    ));

    let first_seen: Arc<Mutex<Vec<(u64, String)>>> = Arc::default();
    let first_seen_ref = Arc::clone(&first_seen);
    let first_setup = RunSetup::new(MemoryRecorder::new(), Arc::clone(&broker), TokioTimer)
        .selected_model(MODEL)
        .streaming(move |round, delta| {
            let Delta::Text(text) = delta else {
                return;
            };
            first_seen_ref.lock().unwrap().push((round.id.get(), text));
        });

    let second_seen: Arc<Mutex<Vec<(u64, String)>>> = Arc::default();
    let second_seen_ref = Arc::clone(&second_seen);
    let second_setup = RunSetup::new(MemoryRecorder::new(), Arc::clone(&broker), TokioTimer)
        .selected_model(MODEL)
        .streaming(move |round, delta| {
            let Delta::Text(text) = delta else {
                return;
            };
            second_seen_ref.lock().unwrap().push((round.id.get(), text));
        });

    let (first_report, second_report) = tokio::join!(
        launch(&chats_once_with("first"), first_setup).run(),
        launch(&chats_once_with("second"), second_setup).run(),
    );
    let first_report = first_report.expect("the first run is reported");
    let second_report = second_report.expect("the second run is reported");

    assert_eq!(first_report.result, Ok("re: first".to_owned()));
    assert_eq!(second_report.result, Ok("re: second".to_owned()));

    assert_eq!(
        *first_seen.lock().unwrap(),
        [(0, "first-A ".to_owned()), (0, "first-B ".to_owned())],
        "the first run captures only its own pieces, on round id 0"
    );
    assert_eq!(
        *second_seen.lock().unwrap(),
        [(0, "second-A ".to_owned()), (0, "second-B ".to_owned())],
        "the second Host captures only its own pieces, on round id 0"
    );
    assert_eq!(
        *first_seen.lock().unwrap(),
        [(0, "first-A ".to_owned()), (0, "first-B ".to_owned())],
        "the first Host's capture remains unchanged after both runs complete"
    );
}

/// Strips run-specific metadata from a record payload before comparing
/// two runs. A replay compares semantics, not timestamps or per-run ids
/// that live outside the record payload.
fn normalize_payload(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (key, value) in map {
                // Run-level identifiers that may differ between two runs
                // even when the payloads describe the same step.
                if key == "started_at" || key == "seed" {
                    continue;
                }
                // Client timing is measured on the transport's own clock,
                // so two runs with the same broker still differ by wall time.
                if key == "metrics" {
                    let metrics = match value {
                        serde_json::Value::Object(metrics) => {
                            let mut metrics = metrics.clone();
                            metrics.remove("client");
                            serde_json::Value::Object(metrics)
                        }
                        other => other.clone(),
                    };
                    out.insert(key.clone(), metrics);
                    continue;
                }
                out.insert(key.clone(), normalize_payload(value));
            }
            serde_json::Value::Object(out)
        }
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.iter().map(normalize_payload).collect())
        }
        other => other.clone(),
    }
}

#[tokio::test]
async fn a_run_with_streaming_on_records_the_same_history_as_streaming_off() {
    let off_recorder = Arc::new(MemoryRecorder::new());
    let off_broker =
        PieceEmittingBroker::new(catalog().expect("the catalog builds"), vec!["one ", "two "]);
    let off_setup =
        RunSetup::new(off_recorder.clone(), off_broker, TokioTimer).selected_model(MODEL);
    let off_report = launch(CHATS_ONCE, off_setup)
        .run()
        .await
        .expect("the run is reported");

    let on_recorder = Arc::new(MemoryRecorder::new());
    let on_broker =
        PieceEmittingBroker::new(catalog().expect("the catalog builds"), vec!["one ", "two "]);
    let on_setup = RunSetup::new(on_recorder.clone(), on_broker, TokioTimer)
        .selected_model(MODEL)
        .streaming(|_round, _delta| {});
    let on_report = launch(CHATS_ONCE, on_setup)
        .run()
        .await
        .expect("the run is reported");

    assert_eq!(on_report.result, off_report.result);
    let records_of = |recorder: &MemoryRecorder, run_id| {
        recorder
            .records(run_id)
            .into_iter()
            .map(|record| {
                serde_json::json!({
                    "kind": record.kind.as_str(),
                    "effect_id": record.effect_id,
                    "payload": normalize_payload(&record.payload),
                })
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        records_of(&on_recorder, on_report.run_id),
        records_of(&off_recorder, off_report.run_id),
        "every effect, answer, and event payload is the same with streaming on or off"
    );
}

#[tokio::test]
async fn a_panicking_sink_ends_its_round_as_dropped() {
    let recorder = Arc::new(MemoryRecorder::new());
    let broker =
        PieceEmittingBroker::new(catalog().expect("the catalog builds"), vec!["one ", "two "]);
    let setup = RunSetup::new(recorder.clone(), broker, TokioTimer)
        .selected_model(MODEL)
        .streaming(|_round, _delta| panic!("the sink panics"));
    let report = launch(CHATS_ONCE, setup)
        .run()
        .await
        .expect("the run is reported");

    assert!(
        report
            .result
            .as_ref()
            .is_err_and(runvil::Error::is_canceled),
        "the uncaught canceled error ends the run: {:?}",
        report.result
    );
    let answers: Vec<_> = recorder
        .records(report.run_id)
        .into_iter()
        .filter(|record| record.kind == RecordKind::Answer)
        .map(|record| record.payload)
        .collect();
    assert_eq!(
        answers,
        vec![serde_json::json!("Dropped")],
        "the panicking round is answered Dropped"
    );
}

#[tokio::test]
async fn a_cancel_from_a_no_yield_sink_wins_over_the_ready_reply() {
    let recorder = Arc::new(MemoryRecorder::new());
    let broker = PieceEmittingBroker::new(catalog().expect("the catalog builds"), vec!["one"]);
    let control_slot: Arc<Mutex<Option<runvil::RunControl>>> = Arc::default();
    let control_ref = Arc::clone(&control_slot);
    let setup = RunSetup::new(recorder.clone(), broker, TokioTimer)
        .selected_model(MODEL)
        .streaming(move |_round, _delta| {
            if let Some(control) = control_ref.lock().unwrap().as_ref() {
                control.cancel();
            }
        });
    let harness = launch(CHATS_ONCE, setup);
    *control_slot.lock().unwrap() = Some(harness.control());
    let report = harness.run().await.expect("the run is reported");

    assert!(
        report
            .result
            .as_ref()
            .is_err_and(runvil::Error::is_canceled),
        "the synchronous cancel ends the run canceled: {:?}",
        report.result
    );
    let records = recorder.records(report.run_id);
    assert_eq!(
        records
            .iter()
            .filter(|record| record.kind == RecordKind::Answer)
            .map(|record| record.payload.clone())
            .collect::<Vec<_>>(),
        vec![serde_json::json!("Dropped")],
        "the ready reply is discarded and its round is answered Dropped"
    );
    assert!(
        records.iter().filter(|record| record.kind == RecordKind::Event)
            .all(|record| record.payload.get("kind") != Some(&serde_json::json!("assistant_reply"))),
        "a dropped round produces no assistant_reply event"
    );
}

#[tokio::test]
async fn a_stop_from_a_no_yield_sink_wins_over_the_ready_reply() {
    let recorder = Arc::new(MemoryRecorder::new());
    let broker = PieceEmittingBroker::new(catalog().expect("the catalog builds"), vec!["one"]);
    let control_slot: Arc<Mutex<Option<runvil::RunControl>>> = Arc::default();
    let control_ref = Arc::clone(&control_slot);
    let setup = RunSetup::new(recorder.clone(), broker, TokioTimer)
        .selected_model(MODEL)
        .streaming(move |_round, _delta| {
            if let Some(control) = control_ref.lock().unwrap().as_ref() {
                control.stop_round();
            }
        });
    let harness = launch(CHATS_ONCE, setup);
    *control_slot.lock().unwrap() = Some(harness.control());
    let report = harness.run().await.expect("the run is reported");

    assert!(
        report
            .result
            .as_ref()
            .is_err_and(runvil::Error::is_canceled),
        "the uncaught error from the stopped round ends the run canceled: {:?}",
        report.result
    );
    let records = recorder.records(report.run_id);
    assert_eq!(
        records
            .iter()
            .filter(|record| record.kind == RecordKind::Answer)
            .map(|record| record.payload.clone())
            .collect::<Vec<_>>(),
        vec![serde_json::json!("Dropped")],
        "the ready reply is discarded and its round is answered Dropped"
    );
    assert!(
        records.iter().filter(|record| record.kind == RecordKind::Event)
            .all(|record| record.payload.get("kind") != Some(&serde_json::json!("assistant_reply"))),
        "a dropped round produces no assistant_reply event"
    );
}

#[tokio::test]
async fn a_cancel_during_the_message_answer_drain_returns_the_taken_message_unread() {
    let control_slot: Arc<Mutex<Option<RunControl>>> = Arc::default();
    let recorder = Arc::new(SendsOnFirstAppend {
        inner: MemoryRecorder::new(),
        control: Arc::clone(&control_slot),
        sent: AtomicBool::new(false),
    });
    let broker = PieceEmittingBroker::new(catalog().expect("the catalog builds"), vec!["one"]);
    let control_ref = Arc::clone(&control_slot);
    let setup = RunSetup::new(recorder.clone(), broker, TokioTimer)
        .selected_model(MODEL)
        .streaming(move |_round, _delta| {
            if let Some(control) = control_ref.lock().unwrap().as_ref() {
                control.cancel();
            }
        });
    let harness = launch(CHATS_ONCE, setup);
    *control_slot.lock().unwrap() = Some(harness.control());
    let report = harness.run().await.expect("the run is reported");

    assert!(
        report
            .result
            .as_ref()
            .is_err_and(runvil::Error::is_canceled)
    );
    assert_eq!(
        report.unread,
        ["queued before the model answer"],
        "the message removed from the queue but not delivered remains unread"
    );
    let records = recorder.inner.records(report.run_id);
    assert_eq!(
        records
            .iter()
            .filter(|record| record.kind == RecordKind::Answer)
            .map(|record| record.payload.clone())
            .collect::<Vec<_>>(),
        [serde_json::json!("Dropped")]
    );
    assert!(records.iter().filter(|record| record.kind == RecordKind::Event)
        .all(|record| record.payload.get("kind") != Some(&serde_json::json!("assistant_reply"))));
}

#[tokio::test]
async fn a_reserved_answer_is_accepted_before_the_next_landing_raises_stop() {
    let control_slot: Arc<Mutex<Option<RunControl>>> = Arc::default();
    let recorder = Arc::new(MemoryRecorder::new());
    let broker = PieceEmittingBroker::new(catalog().expect("the catalog builds"), vec!["one"]);
    let control_ref = Arc::clone(&control_slot);
    let pieces = Arc::new(AtomicUsize::new(0));
    let pieces_ref = Arc::clone(&pieces);
    let setup = RunSetup::new(recorder.clone(), broker, TokioTimer)
        .selected_model(MODEL)
        .streaming(move |_round, _delta| {
            if pieces_ref.fetch_add(1, Ordering::SeqCst) == 1
                && let Some(control) = control_ref.lock().unwrap().as_ref()
            {
                control.stop_round();
            }
        });
    let harness = launch(PARALLEL_CHATS, setup);
    *control_slot.lock().unwrap() = Some(harness.control());
    let report = harness.run().await.expect("the run is reported");

    let answers: Vec<_> = recorder
        .records(report.run_id)
        .into_iter()
        .filter(|record| record.kind == RecordKind::Answer)
        .map(|record| record.payload)
        .collect();
    assert_eq!(answers.len(), 2);
    assert!(
        answers[0].get("Chat").is_some(),
        "the reserved first answer is committed"
    );
    assert_eq!(
        answers[1],
        serde_json::json!("Dropped"),
        "the later answer is stopped"
    );
}

#[tokio::test]
async fn a_stop_during_the_message_answer_drain_drops_ready_answers_in_effect_order() {
    let control_slot: Arc<Mutex<Option<RunControl>>> = Arc::default();
    let recorder = Arc::new(SendsOnFirstAppend {
        inner: MemoryRecorder::new(),
        control: Arc::clone(&control_slot),
        sent: AtomicBool::new(false),
    });
    let broker = PieceEmittingBroker::new(catalog().expect("the catalog builds"), vec!["one"]);
    let control_ref = Arc::clone(&control_slot);
    let setup = RunSetup::new(recorder.clone(), broker, TokioTimer)
        .selected_model(MODEL)
        .streaming(move |_round, _delta| {
            if let Some(control) = control_ref.lock().unwrap().as_ref() {
                control.stop_round();
            }
        });
    let harness = launch(PARALLEL_CHATS, setup);
    *control_slot.lock().unwrap() = Some(harness.control());
    let report = harness.run().await.expect("the run is reported");

    assert_eq!(report.result, Ok("joined".to_owned()));
    assert!(
        recorder.sent.load(Ordering::SeqCst),
        "the message arrived before the drain"
    );
    let records = recorder.inner.records(report.run_id);
    let answers: Vec<_> = records
        .iter()
        .filter(|record| record.kind == RecordKind::Answer)
        .collect();
    assert_eq!(answers.len(), 2, "both parallel rounds are answered");
    assert!(
        answers
            .iter()
            .all(|record| record.payload == serde_json::json!("Dropped")),
        "the stop drops every concurrent round: {answers:?}"
    );
    let effect_ids: Vec<_> = answers
        .iter()
        .map(|record| record.effect_id.unwrap())
        .collect();
    assert!(
        effect_ids.windows(2).all(|pair| pair[0] < pair[1]),
        "answers are recorded in effect order: {effect_ids:?}"
    );
    assert!(
        records.iter().filter(|record| record.kind == RecordKind::Event)
            .all(|record| record.payload.get("kind") != Some(&serde_json::json!("assistant_reply"))),
        "dropped rounds produce no assistant_reply event"
    );
}

#[tokio::test]
async fn canceling_from_the_sink_after_the_first_piece_drops_the_round() {
    let recorder = Arc::new(MemoryRecorder::new());
    let (broker, gate) = CancelingBroker::new(catalog().expect("the catalog builds"));
    let delivered: Arc<Mutex<Vec<String>>> = Arc::default();
    let delivered_ref = Arc::clone(&delivered);
    let control_slot: Arc<Mutex<Option<runvil::RunControl>>> = Arc::default();
    let control_ref = Arc::clone(&control_slot);

    let setup = RunSetup::new(recorder.clone(), broker, TokioTimer)
        .selected_model(MODEL)
        .streaming(move |_round, delta| {
            let Delta::Text(text) = delta else {
                return;
            };
            delivered_ref.lock().unwrap().push(text);
            if let Some(control) = control_ref.lock().unwrap().as_ref() {
                control.cancel();
            }
        });
    let harness = launch(CHATS_ONCE, setup);
    *control_slot.lock().unwrap() = Some(harness.control());
    let report = harness.run().await.expect("the run is reported");

    assert!(
        report
            .result
            .as_ref()
            .is_err_and(runvil::Error::is_canceled),
        "the run ends canceled: {:?}",
        report.result
    );
    assert!(
        gate.is_dropped(),
        "the cancel drops the broker's round future"
    );
    assert!(
        gate.release().is_ok(),
        "the retained readiness observer keeps the gate live after cancellation"
    );
    assert!(
        gate.is_ready(),
        "the gate observer sees the released readiness"
    );
    assert_eq!(
        *delivered.lock().unwrap(),
        vec![CancelingBroker::FIRST_PIECE.to_owned()],
        "opening the gate after the broker future is dropped delivers no further pieces"
    );
    let answers: Vec<_> = recorder
        .records(report.run_id)
        .into_iter()
        .filter(|record| record.kind == RecordKind::Answer)
        .map(|record| record.payload)
        .collect();
    assert_eq!(
        answers,
        vec![serde_json::json!("Dropped")],
        "the canceled round is answered Dropped"
    );
    let events: Vec<_> = recorder
        .records(report.run_id)
        .into_iter()
        .filter(|record| record.kind == RecordKind::Event)
        .map(|record| record.payload)
        .collect();
    assert!(
        events
            .iter()
            .all(|event| event.get("kind") != Some(&serde_json::json!("assistant_reply"))),
        "a dropped round produces no assistant_reply event"
    );
}

#[tokio::test]
async fn a_full_host_queue_does_not_stop_harness_delivery_or_durable_reply() {
    let broker = PieceEmittingBroker::new(
        catalog().expect("the catalog builds"),
        vec!["one ", "two ", "three "],
    );
    let (tx, rx) = std::sync::mpsc::sync_channel::<String>(1);
    tx.send("already queued".to_owned())
        .expect("the Host queue accepts its initial item");
    let attempted: Arc<Mutex<Vec<String>>> = Arc::default();
    let attempted_ref = Arc::clone(&attempted);
    let setup = RunSetup::new(MemoryRecorder::new(), broker, TokioTimer)
        .selected_model(MODEL)
        .streaming(move |_round, delta| {
            if let Delta::Text(text) = delta {
                attempted_ref.lock().unwrap().push(text.clone());
                let _ = tx.try_send(text);
            }
        });
    let report = launch(CHATS_ONCE, setup)
        .run()
        .await
        .expect("the run is reported");

    assert_eq!(report.result, Ok("re: hello".to_owned()));
    assert_eq!(
        *attempted.lock().unwrap(),
        ["one ".to_owned(), "two ".to_owned(), "three ".to_owned()],
        "the Harness delivers every piece even when the Host queue is full"
    );
    assert_eq!(rx.try_recv().unwrap(), "already queued");
}

/// Serves `app` on a loopback port and returns its `/v1` root.
async fn serve_gateway(app: axum::Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|error| panic!("the loopback listener binds: {error}"));
    let addr = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("the loopback listener has an address: {error}"));
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .unwrap_or_else(|error| panic!("the mock gateway serves: {error}"));
    });
    format!("http://{addr}/v1")
}

/// Renders `events` as SSE `data:` lines closed by the `[DONE]` sentinel.
fn sse_body(events: &[serde_json::Value]) -> String {
    let mut body = String::new();
    for event in events {
        body.push_str("data: ");
        body.push_str(&event.to_string());
        body.push_str("\n\n");
    }
    body.push_str("data: [DONE]\n\n");
    body
}

/// A content fragment for a mock gateway SSE stream.
fn content_chunk(text: &str) -> serde_json::Value {
    serde_json::json!({
        "model": MODEL,
        "choices": [{ "index": 0, "delta": { "content": text }, "finish_reason": null }]
    })
}

/// A mock gateway that serves a model catalog and streams one completion
/// body for every chat request.
fn gateway_app(chat_body: String) -> axum::Router {
    use axum::Router;
    use axum::routing::{get, post};

    let models = serde_json::json!({
        "data": [{
            "id": MODEL,
            "object": "model",
            "context": 131_072,
            "thinking": "never"
        }]
    });

    Router::new()
        .route(
            "/v1/models",
            get(move || {
                let models = models.clone();
                async move { axum::Json(models) }
            }),
        )
        .route(
            "/v1/chat/completions",
            post(move || {
                let body = chat_body.clone();
                async move {
                    (
                        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                        body,
                    )
                }
            }),
        )
}

#[tokio::test]
async fn a_gateway_stream_returns_the_full_reply_to_the_recorder_after_the_host_stops_consuming() {
    let chat_body = sse_body(&[
        content_chunk("first "),
        content_chunk("second "),
        content_chunk("third"),
        serde_json::json!({
            "choices": [{ "index": 0, "delta": {}, "finish_reason": "stop" }]
        }),
    ]);
    let base = serve_gateway(gateway_app(chat_body)).await;
    let endpoint = GatewayEndpoint::new(&base).expect("valid test endpoint");
    let broker = GatewayBroker::new(
        endpoint,
        SecretString::new("tok").expect("non-empty test key"),
    );
    let recorder = Arc::new(MemoryRecorder::new());
    let preview: Arc<Mutex<Vec<String>>> = Arc::default();
    let preview_ref = Arc::clone(&preview);

    let setup = RunSetup::new(recorder.clone(), broker, TokioTimer)
        .selected_model(MODEL)
        .streaming(move |_round, delta| {
            let Delta::Text(text) = delta else {
                return;
            };
            let mut preview = preview_ref.lock().unwrap();
            if preview.is_empty() {
                preview.push(text);
            }
        });
    let report = launch(CHATS_ONCE, setup)
        .run()
        .await
        .expect("the run is reported");

    assert_eq!(report.result, Ok("first second third".to_owned()));
    assert_eq!(
        *preview.lock().unwrap(),
        vec!["first ".to_owned()],
        "the Host retains only the prefix it wants even as the Harness forwards every piece"
    );
    let records = recorder.records(report.run_id);
    let reply = records
        .iter()
        .filter(|record| record.kind == RecordKind::Event)
        .find(|record| record.payload["kind"] == "assistant_reply")
        .expect("the run recorded its reply");
    assert_eq!(
        reply.payload["text"],
        serde_json::json!("first second third"),
        "the recorder holds the full reply, not just the preview"
    );
}
