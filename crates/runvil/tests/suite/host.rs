//! What a Host does around a run: it sends the run's main chain a
//! message that `tasks.receive()` reads, stops an in-flight round or
//! cancels a run through the Harness's control, and receives each piece
//! of a section round through `RunSetup::streaming` under the round id
//! the run records. Two runs that share one broker each receive only
//! their own pieces.

use std::error::Error;
use std::num::NonZeroU32;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Poll, Waker};

use harness_gateway_client::TokioTimer;
use runvil::effect::Round;
use runvil::harness::{
    Delta, InferenceBroker, MemoryRecorder, OfflineBroker, PieceSink, RecordKind,
};
use runvil::model::{
    Completion, CompletionOptions, CompletionResult, Message, ModelBinding, ModelCatalog, ModelId,
    ModelInfo, ThinkingMode, ToolSchema,
};
use runvil::{BoxFuture, Harness, RunSetup};

use crate::support::launch;

/// Reads the operator's next message and returns it.
const RECEIVES: &str = concat!(
    "---\nname: receives\ndescription: Reads the operator's next message\nrunvil: 0\n",
    "uses:\n  - user\n",
    "---\n\n## Only\n\n```lua\n",
    "return tasks.receive()\n",
    "```\n",
);

/// Waits on one model round under a `pcall`, and returns the error kind
/// when the round fails.
const PATIENT: &str = concat!(
    "---\nname: patient\ndescription: Waits on the model\nrunvil: 0\n",
    "---\n\n## Only\n\n```lua\n",
    "local writer = models.get('default')\n",
    "local ok, err = pcall(models.infer, writer, 'Take your time.')\n",
    "return ok and 'answered' or err.kind\n",
    "```\n",
);

/// Runs one section round and returns the model's reply.
const CHATS: &str = concat!(
    "---\nname: chats\ndescription: Says hello to the model\nrunvil: 0\n",
    "---\n\n## Only\n\n```lua\n",
    "messages:user('Hello, desk.')\n",
    "models.loop(models.get('default'))\n",
    "return messages[#messages].content\n",
    "```\n",
);

/// Runs one section round as `CHATS` does, with another message, so that
/// two runs of one broker are told apart by what they send.
const CHATS_AGAIN: &str = concat!(
    "---\nname: chats-again\ndescription: Says goodbye to the model\nrunvil: 0\n",
    "---\n\n## Only\n\n```lua\n",
    "messages:user('Goodbye, desk.')\n",
    "models.loop(models.get('default'))\n",
    "return messages[#messages].content\n",
    "```\n",
);

/// The one stub model the stuck and streaming brokers list.
fn stub_catalog() -> Result<ModelCatalog, Box<dyn Error>> {
    let window = NonZeroU32::new(131_072).ok_or("131072 is non-zero")?;
    let model = ModelInfo::new(
        ModelId::gateway("stub-model")?,
        "stub",
        window,
        ThinkingMode::Never,
    );
    Ok(ModelCatalog::new([model])?)
}

/// Raised once a round reaches the stuck broker, waking whoever waits.
struct Started(Mutex<(bool, Option<Waker>)>);

impl Started {
    fn raise(&self) {
        let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        state.0 = true;
        if let Some(waker) = state.1.take() {
            waker.wake();
        }
    }

    async fn wait(&self) {
        std::future::poll_fn(|cx| {
            let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
            if state.0 {
                return Poll::Ready(());
            }
            state.1 = Some(cx.waker().clone());
            Poll::Pending
        })
        .await;
    }
}

/// Lists the stub model and never answers a round.
struct Stuck {
    catalog: ModelCatalog,
    started: Arc<Started>,
}

impl InferenceBroker for Stuck {
    fn models(&self) -> BoxFuture<'static, Result<ModelCatalog, runvil::Error>> {
        let catalog = self.catalog.clone();
        Box::pin(async move { Ok(catalog) })
    }

    fn supports_streaming(&self) -> bool {
        false
    }

    fn chat(
        &self,
        _binding: ModelBinding,
        _messages: Vec<Message>,
        _tools: Vec<ToolSchema>,
        _options: CompletionOptions,
        _round: Round,
        _on_piece: Option<PieceSink>,
    ) -> BoxFuture<'static, Result<Box<Completion>, runvil::Error>> {
        self.started.raise();
        Box::pin(std::future::pending())
    }
}

/// A Harness running `source` over a stuck broker listing `catalog`,
/// beside the flag its first round raises.
fn stuck_harness(catalog: ModelCatalog, source: &str) -> (Harness, Arc<Started>) {
    let started = Arc::new(Started(Mutex::new((false, None))));
    let broker = Stuck {
        catalog,
        started: Arc::clone(&started),
    };
    let setup = RunSetup::new(MemoryRecorder::new(), broker, TokioTimer);
    (launch(source, setup), started)
}

/// Lists the stub model, says it streams, and replies
/// `You said: <last message>` in two pieces, handing each piece to the
/// callback of a round that has one.
struct Streaming {
    catalog: ModelCatalog,
}

impl InferenceBroker for Streaming {
    fn models(&self) -> BoxFuture<'static, Result<ModelCatalog, runvil::Error>> {
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
    ) -> BoxFuture<'static, Result<Box<Completion>, runvil::Error>> {
        let said = messages
            .last()
            .map(|message| message.content().to_owned())
            .unwrap_or_default();
        Box::pin(async move {
            let mut reply = String::new();
            for piece in ["You said: ".to_owned(), said] {
                reply.push_str(&piece);
                if let Some(on_piece) = &on_piece {
                    on_piece(Delta::Text(piece));
                }
            }
            Completion::from_result(CompletionResult::Text(reply), "stub-model").map(Box::new)
        })
    }
}

#[tokio::test]
async fn tasks_receive_returns_the_message_the_host_sends_byte_for_byte() {
    let typed = "Hello, desk.\r\n\"quoted\" 🦀";
    let setup = RunSetup::new(MemoryRecorder::new(), OfflineBroker, TokioTimer).has_user(true);
    let harness = launch(RECEIVES, setup);
    assert!(
        harness.control().send(typed.to_owned()),
        "a run that has not started accepts the message"
    );

    let report = harness.run().await.expect("the run is reported");
    assert_eq!(report.result, Ok(typed.to_owned()));
    assert!(report.unread.is_empty(), "{:?}", report.unread);
}

#[tokio::test]
async fn stopping_an_in_flight_round_drops_the_call_and_the_prompts_pcall_catches_it() {
    let (harness, round_started) =
        stuck_harness(stub_catalog().expect("the stub catalog builds"), PATIENT);
    let control = harness.control();
    let (report, ()) = tokio::join!(harness.run(), async {
        round_started.wait().await;
        control.stop_round();
    });
    assert_eq!(
        report.expect("the run is reported").result,
        Ok("canceled".to_owned())
    );
}

#[tokio::test]
async fn a_run_canceled_before_it_begins_is_recorded_as_canceled_before_it_starts() {
    let recorder = Arc::new(MemoryRecorder::new());
    let harness = launch(
        PATIENT,
        RunSetup::new(recorder.clone(), OfflineBroker, TokioTimer),
    );
    harness.control().cancel();
    let report = harness.run().await.expect("the run is reported");
    assert!(
        report
            .result
            .as_ref()
            .is_err_and(runvil::Error::is_canceled),
        "the run ends canceled: {:?}",
        report.result
    );
    assert_eq!(
        recorder.result(report.run_id),
        Some(report.result),
        "the recorder holds the canceled run"
    );
    assert!(
        recorder.records(report.run_id).is_empty(),
        "the run ended before it started"
    );
}

/// A setup over `broker` whose streaming callback sends each piece, with
/// its round id, to `window`.
fn streaming_setup(
    recorder: Arc<MemoryRecorder>,
    broker: Arc<Streaming>,
    window: Sender<(u64, Delta)>,
) -> RunSetup {
    RunSetup::new(recorder, broker, TokioTimer).streaming(move |round, piece| {
        // The test holds the receiver until the run ends, so a send fails
        // only once no one is left to read the piece.
        let _ = window.send((round.id.get(), piece));
    })
}

/// A streaming broker listing `catalog`, in the `Arc` that a setup holds.
fn streaming_broker(catalog: ModelCatalog) -> Arc<Streaming> {
    Arc::new(Streaming { catalog })
}

/// The pieces of a `You said: <message>` reply, as the broker hands them.
fn said(message: &str) -> [Delta; 2] {
    [
        Delta::Text("You said: ".to_owned()),
        Delta::Text(message.to_owned()),
    ]
}

#[tokio::test]
async fn a_host_that_sets_streaming_receives_a_section_round_under_the_round_id_the_run_records() {
    let (window, shown) = channel();
    let recorder = Arc::new(MemoryRecorder::new());
    let broker = streaming_broker(stub_catalog().expect("the stub catalog builds"));
    // The setup owns one handle to the recorder, and the test keeps this
    // second one to read the run's records.
    let setup = streaming_setup(recorder.clone(), broker, window);
    let report = launch(CHATS, setup)
        .run()
        .await
        .expect("the run is reported");

    let records = recorder.records(report.run_id);
    let reply = records
        .iter()
        .filter(|record| record.kind == RecordKind::Event)
        .find(|record| record.payload["kind"] == "assistant_reply")
        .expect("the run recorded its reply");
    let round = reply.payload["round"].as_u64().expect("a round id");
    let shown: Vec<(u64, Delta)> = shown.try_iter().collect();
    assert_eq!(
        shown,
        said("Hello, desk.").map(|piece| (round, piece)),
        "each piece reaches the callback under the round the run records"
    );
}

#[tokio::test]
async fn two_runs_that_share_one_broker_each_receive_only_their_own_pieces() {
    let broker = streaming_broker(stub_catalog().expect("the stub catalog builds"));
    let (first_window, first_shown) = channel();
    let (second_window, second_shown) = channel();
    // Both setups hold the one broker, so each takes a handle to it.
    let first = streaming_setup(
        Arc::new(MemoryRecorder::new()),
        Arc::clone(&broker),
        first_window,
    );
    let second = streaming_setup(Arc::new(MemoryRecorder::new()), broker, second_window);

    let (first_report, second_report) = tokio::join!(
        launch(CHATS, first).run(),
        launch(CHATS_AGAIN, second).run()
    );
    assert_eq!(
        first_report.expect("the first run is reported").result,
        Ok("You said: Hello, desk.".to_owned())
    );
    assert_eq!(
        second_report.expect("the second run is reported").result,
        Ok("You said: Goodbye, desk.".to_owned())
    );

    let first_shown: Vec<(u64, Delta)> = first_shown.try_iter().collect();
    let second_shown: Vec<(u64, Delta)> = second_shown.try_iter().collect();
    assert_eq!(
        first_shown,
        said("Hello, desk.").map(|piece| (0, piece)),
        "the first run receives its own pieces, from round 0"
    );
    assert_eq!(
        second_shown,
        said("Goodbye, desk.").map(|piece| (0, piece)),
        "the second run receives its own pieces, from round 0 as well"
    );
}
