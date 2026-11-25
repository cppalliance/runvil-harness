//! The loop's one end that no Harness run reaches: a run left with
//! nothing to await, the fault a lost effect would leave, ends its send
//! queue as it ends, so a message the loop accepted and never delivered
//! comes back unread and every later send is refused.

use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use runvil_engine::cancel::CancelHandle;
use runvil_engine::effect::{EffectAnswer, Round, ToolCallOrigin};
use runvil_engine::model::{
    Completion, CompletionOptions, Message, ModelBinding, ModelCatalog, ToolSchema,
};
use runvil_engine::{
    BoxFuture, Error, ErrorCode, Prompt, Run, RunParams, RunSeed, Step, Timestamp,
};
use runvil_engine::{ToolCatalog, ToolId, ToolInfo, ToolOutput};
use runvil_plugin::ToolFiles;
use serde_json::{Value, json};

use super::{Driver, Flights, Kind, Reach, STALLED, drive};
use crate::harness::RunControl;
use crate::performers::{InferenceBroker, Timer, ToolPerformer};
use crate::recorder::{MemoryRecorder, Record, RunId, RunMeta, RunRecorder};
use crate::setup::RunSetup;
use crate::tool_files::{Asked, CallDesk};

/// The fixed seed the run and its recorded meta take.
const SEED: RunSeed = RunSeed::from_bytes([7; 32]);

/// A run of one section that calls the tool `tests/runner/wait`, stepped
/// once with its tool call kept from the loop: the run waits on an effect
/// the loop never sees.
fn lost_run() -> Run {
    let source = "---\nname: lost\ndescription: d\nrunvil: 0\n---\n\n## Only\n\n\
        ```lua\nreturn tools.call('tests/runner/wait')\n```\n";
    let prompt = Prompt::parse(&|_: &str| Ok(source.to_owned()), "prompt");
    let wait = ToolInfo::new(
        ToolId::parse("tests/runner/wait").expect("the wait tool id is valid"),
        "Wait until the test answers.",
        json!({ "type": "object", "properties": {} }),
    );
    let params = RunParams::new(SEED, Timestamp::UNIX_EPOCH)
        .with_tools(ToolCatalog::new([wait]).expect("the one-tool catalog is valid"));
    let mut run = Run::new(Arc::new(prompt.expect("the prompt parses")), "{}", params);
    let Step::Pending { effects, .. } = run.step() else {
        panic!("the run parks on its tool call");
    };
    assert_eq!(effects.len(), 1, "the one tool call the loop never sees");
    run
}

/// Records into `inner`, and sends `text` through `control` as it takes
/// its first record: a Host's message arriving while the loop records.
struct SendsOnFirstAppend {
    inner: MemoryRecorder,
    control: RunControl,
    text: &'static str,
    sent: AtomicBool,
}

impl RunRecorder for SendsOnFirstAppend {
    fn begin_run(&self, meta: RunMeta) -> BoxFuture<'_, Result<RunId, Error>> {
        self.inner.begin_run(meta)
    }

    fn append(&self, run: RunId, record: Record) -> BoxFuture<'_, Result<(), Error>> {
        if !self.sent.swap(true, Ordering::SeqCst) {
            assert!(
                self.control.send(self.text.to_owned()),
                "the live run accepts {}",
                self.text
            );
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

/// A performer for every kind the loop never sees the lost run issue;
/// reaching one is the test's failure.
struct Unused;

impl InferenceBroker for Unused {
    fn models(&self) -> BoxFuture<'static, Result<ModelCatalog, Error>> {
        unreachable!("the effect loop never lists models")
    }

    fn supports_streaming(&self) -> bool {
        unreachable!("the effect loop never lists models")
    }

    fn chat(
        &self,
        _binding: ModelBinding,
        _messages: Vec<Message>,
        _tools: Vec<ToolSchema>,
        _options: CompletionOptions,
        _round: Round,
        _on_piece: Option<crate::performers::PieceSink>,
    ) -> BoxFuture<'static, Result<Box<Completion>, Error>> {
        unreachable!("the lost run issues no Chat effect")
    }
}

impl ToolPerformer for Unused {
    fn call(
        &self,
        _tool: ToolId,
        _alias: String,
        _origin: ToolCallOrigin,
        _args: Value,
        _files: Arc<dyn ToolFiles>,
    ) -> BoxFuture<'static, Result<ToolOutput, Error>> {
        unreachable!("the loop never sees the lost run's tool call")
    }
}

impl Timer for Unused {
    fn sleep(&self, _duration: Duration) -> BoxFuture<'static, ()> {
        unreachable!("the lost run issues no Timer effect")
    }
}

#[test]
fn a_stop_leaves_a_surviving_tool_call_in_flight() {
    let source = "---\nname: survivor\ndescription: d\nrunvil: 0\n---\n\n## Only\n\n\
        ```lua\nreturn tools.call('tests/runner/wait')\n```\n";
    let prompt = Prompt::parse(&|_: &str| Ok(source.to_owned()), "prompt");
    let wait = ToolInfo::new(
        ToolId::parse("tests/runner/wait").expect("the wait tool id is valid"),
        "Wait until the test answers.",
        json!({ "type": "object", "properties": {} }),
    )
    .survives_stop(true);
    let params = RunParams::new(SEED, Timestamp::UNIX_EPOCH)
        .with_tools(ToolCatalog::new([wait]).expect("the one-tool catalog is valid"));
    let mut run = Run::new(Arc::new(prompt.expect("the prompt parses")), "{}", params);
    let Step::Pending { mut effects, .. } = run.step() else {
        panic!("the run issues its surviving tool call");
    };
    assert_eq!(effects.len(), 1);
    let issued = effects.pop().unwrap();
    let runvil_engine::effect::Effect::ToolCall { survives_stop, .. } = issued.effect else {
        panic!("the effect is a tool call");
    };
    assert!(survives_stop);

    let asked = Arc::new(Asked::default());
    let mut flights = Flights::new(NonZeroUsize::MIN);
    flights.queue(
        issued.id,
        issued.provenance,
        survives_stop,
        CallDesk::open(issued.id, &asked),
        Box::pin(async { EffectAnswer::ToolCall(Ok(ToolOutput::trusted("completed"))) }),
    );

    flights.landed_now();
    assert!(flights.abort(Reach::AllButSurvivors).is_empty());
    assert_eq!(flights.len(), 1, "the reserved answer remains in flight");
    let (id, _provenance, answer) = flights
        .take_landed()
        .expect("the stop-surviving answer remains reserved");
    assert_eq!(id, issued.id);
    let EffectAnswer::ToolCall(Ok(output)) = answer else {
        panic!("the reserved answer is the completed tool call");
    };
    assert_eq!(output.text(), "completed");
    assert!(flights.take_landed().is_none(), "the answer is taken once");
    assert_eq!(flights.len(), 0);
    assert!(
        flights.abort(Reach::All).is_empty(),
        "a call whose answer was taken is no longer owed"
    );
}

struct DropProbe(Arc<std::sync::atomic::AtomicUsize>);

impl Drop for DropProbe {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "The test checks teardown and subsequent landing together."
)]
async fn dropping_withdrawn_effects_does_not_poll_another_live_future() {
    let source = "---\nname: withdrawals\ndescription: d\nrunvil: 0\n---\n\n\
        ## Main\n\n```lua\n\
        local first = tasks.spawn('../First')\n\
        local second = tasks.spawn('../Second')\n\
        tasks.join({ first, second })\n```\n\
        ## First\n\n```lua\nreturn tools.call('tests/runner/wait')\n```\n\
        ## Second\n\n```lua\nreturn tools.call('tests/runner/wait')\n```\n";
    let prompt = Prompt::parse(&|_: &str| Ok(source.to_owned()), "prompt");
    let wait = ToolInfo::new(
        ToolId::parse("tests/runner/wait").expect("the wait tool id is valid"),
        "Wait until the test answers.",
        json!({ "type": "object", "properties": {} }),
    );
    let params = RunParams::new(SEED, Timestamp::UNIX_EPOCH)
        .with_tools(ToolCatalog::new([wait]).expect("the one-tool catalog is valid"));
    let mut run = Run::new(Arc::new(prompt.expect("the prompt parses")), "{}", params);
    let Step::Pending { effects, .. } = run.step() else {
        panic!("both child tool calls are issued");
    };
    assert_eq!(effects.len(), 2, "the two calls are concurrently in flight");
    let mut effects = effects.into_iter();
    let withdrawn = effects.next().expect("the withdrawn effect exists");
    let live = effects.next().expect("the live effect exists");

    let control = RunControl::new(CancelHandle::new());
    let recorder = Arc::new(MemoryRecorder::new());
    let run_id = recorder
        .begin_run(RunMeta {
            seed: SEED,
            flags: 0,
            started_at: Timestamp::UNIX_EPOCH,
        })
        .await
        .expect("the recorder begins the test run");
    let asked = Arc::new(Asked::default());
    let mut flights = Flights::new(NonZeroUsize::MIN);
    let dropped = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let drop_probe = DropProbe(Arc::clone(&dropped));
    let mut withdrawn_performer: BoxFuture<'static, EffectAnswer> = Box::pin(async move {
        let _drop_probe = drop_probe;
        std::future::pending::<EffectAnswer>().await
    });
    assert!(
        withdrawn_performer
            .as_mut()
            .poll(&mut std::task::Context::from_waker(std::task::Waker::noop()))
            .is_pending()
    );
    flights.start(
        withdrawn.id,
        withdrawn.provenance,
        Kind::ToolCall,
        withdrawn_performer,
    );
    let polled = Arc::new(AtomicBool::new(false));
    let polled_ref = Arc::clone(&polled);
    let cancel_control = control.clone();
    flights.start(
        live.id,
        live.provenance,
        Kind::ToolCall,
        Box::pin(async move {
            polled_ref.store(true, Ordering::SeqCst);
            cancel_control.cancel();
            EffectAnswer::ToolCall(Ok(ToolOutput::trusted("completed")))
        }),
    );
    let mut driver = Driver {
        run,
        broker: Arc::new(Unused),
        tool: Arc::new(Unused),
        timer: Arc::new(Unused),
        recorder,
        run_id,
        flights,
        asked,
        cancel: control.cancel_handle().clone(),
        stop: Arc::clone(control.stop_signal()),
        sends: Arc::clone(control.send_queue()),
        unread: Vec::new(),
    };

    driver
        .drop_in_flight(Reach::Withdrawn(&[withdrawn.id]))
        .await
        .unwrap();
    assert_eq!(
        dropped.load(Ordering::SeqCst),
        1,
        "the withdrawn performer is destroyed synchronously"
    );
    assert!(
        !polled.load(Ordering::SeqCst),
        "the unchecked drain never polls a live effect"
    );
    assert_eq!(
        driver.flights.len(),
        1,
        "the unrelated effect remains in flight"
    );

    driver.flights.landed_now();
    assert!(polled.load(Ordering::SeqCst));
    assert_eq!(
        driver.flights.len(),
        1,
        "the unrelated answer is reserved, not retired by abort"
    );
    let (landed_id, _, answer) = driver
        .flights
        .take_landed()
        .expect("the unrelated effect lands once on a later poll");
    assert_eq!(landed_id, live.id);
    assert!(matches!(answer, EffectAnswer::ToolCall(Ok(_))));
    assert!(driver.flights.take_landed().is_none());
    assert_eq!(driver.flights.len(), 0);
}

#[tokio::test]
async fn a_run_left_with_nothing_to_await_reports_the_message_it_never_delivered_and_refuses_later_ones()
 {
    let control = RunControl::new(CancelHandle::new());
    let recorder = Arc::new(SendsOnFirstAppend {
        inner: MemoryRecorder::new(),
        control: control.clone(),
        text: "second",
        sent: AtomicBool::new(false),
    });
    let meta = RunMeta {
        seed: SEED,
        flags: 0,
        started_at: Timestamp::UNIX_EPOCH,
    };
    let run_id = recorder.begin_run(meta).await.unwrap();
    assert!(control.send("first".to_owned()));

    // The loop delivers `first` before its first step, and `second`
    // arrives as the loop records that delivery. The step then finds
    // nothing to await, so the loop never delivers `second`.
    let (result, unread) = drive(
        lost_run(),
        &RunSetup::new(recorder.clone(), Unused, Unused),
        Arc::new(Unused),
        run_id,
        &control,
    )
    .await;

    let error = result.as_ref().expect_err("a stall ends the run");
    assert_eq!(error.code, ErrorCode::Internal);
    assert_eq!(error.located_message(), STALLED);
    assert_eq!(recorder.inner.result(run_id), Some(result.clone()));
    assert_eq!(
        unread,
        ["second"],
        "the message the loop accepted and never delivered"
    );
    assert!(
        !control.send("late".to_owned()),
        "the ended run refuses the message"
    );
}
