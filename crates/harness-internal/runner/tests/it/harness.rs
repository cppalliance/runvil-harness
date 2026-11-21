//! The per-run Harness: `run` begins the run's record, resolves the run's
//! model through the setup's broker, prepares the parsed prompt, and
//! drives the run as one future; the record opens
//! with the run's own start, numbered from zero; a prompt reads the
//! setup's selection through a role's handle, and the first listed model
//! when the setup selects none; `models.get` refuses a
//! Gateway model id; a run that fails after it
//! starts is a recorded failed run, never an error of `run`; a Vfs effect's
//! answer record directly follows its effect record; two runs in flight at
//! once share the prompt one parse returned; and one run completes
//! under a thread-parking executor with no runtime behind it. How a stop and a
//! cancel reach the effects in flight sits in the `stop` child module, how
//! a stop reaches the tool calls waiting for a slot and how a task's early
//! end drops its stop-surviving call while the run goes on in the
//! `stop_cap` child module, how what a stop reaches depends on when the
//! loop sees it in the `stop_timing` child module, the messages a Host
//! sends the main chain in the `inbox` child module, a run whose prompt
//! executes a prompt it lists in the `execute` child module, the file
//! operations a tool call asks for through its files in the `files` child
//! module, the order of one reply's calls when a tool changes files in
//! the `files_order` child module, one path used by two parties of a run
//! in the `files_shared` child module, the turns the loop gives its
//! executor while a call asks for files in the `files_turns` child
//! module, and the runs that end before they start and the one error
//! `run` returns in the `errors` child module.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::task::{Context, Poll, Wake, Waker};
use std::thread::{self, Thread};
use std::time::Duration;

use harness_runner::performers::{InferenceBroker, Timer};
use harness_runner::recorder::{MemoryRecorder, Record, RecordKind, RunId, RunMeta, RunRecorder};
use harness_runner::{Harness, Host, Packs, RunControl, RunReport, RunSetup};
use runvil_engine::{BoxFuture, Error, ErrorCode};
use runvil_plugin::PackRegistry;
use serde_json::{Value, json};

use crate::scripted::{Listing, MODEL, ScriptedBroker};
use crate::support::{PendingTimer, host_over, parsed};

#[path = "harness-errors.rs"]
mod errors;
#[path = "harness-execute.rs"]
mod execute;
#[path = "harness-files.rs"]
mod files;
#[path = "harness-files-order.rs"]
mod files_order;
#[path = "harness-files-shared.rs"]
mod files_shared;
#[path = "harness-files-turns.rs"]
mod files_turns;
#[path = "harness-inbox.rs"]
mod inbox;
#[path = "harness-stop.rs"]
mod stop;
#[path = "harness-stop-cap.rs"]
mod stop_cap;
#[path = "harness-stop-timing.rs"]
mod stop_timing;

/// How long a test waits for a run before it fails.
const PATIENCE: Duration = Duration::from_secs(10);

/// The setup of a run recorded through `recorder` and served by `broker`,
/// with [`MODEL`] selected, the empty object `{}` as its arguments, and
/// an empty memory filesystem for its tool calls.
pub(crate) fn setup(
    recorder: Arc<dyn RunRecorder>,
    broker: impl InferenceBroker + 'static,
) -> RunSetup {
    RunSetup {
        selected_model: Some(MODEL.to_owned()),
        ..RunSetup::new(recorder, Arc::new(broker))
    }
}

/// A Harness running `source` with `setup` on a Host over `packs` that
/// sleeps through `timer`.
pub(crate) fn launch(
    packs: Packs,
    timer: impl Timer + 'static,
    source: &str,
    setup: RunSetup,
) -> Harness {
    Harness::new(host_over(packs, timer), parsed(source), setup)
}

/// A Host's Packs with none but the built-ins.
pub(crate) fn no_packs() -> Packs {
    Packs::new(PackRegistry::new())
}

/// A Harness running `source` over `recorder` and `broker` with no Packs
/// and a timer no test here reaches.
fn plain_harness(recorder: &Arc<MemoryRecorder>, broker: ScriptedBroker, source: &str) -> Harness {
    launch(
        no_packs(),
        PendingTimer::default(),
        source,
        setup(recorder.clone(), broker),
    )
}

/// Yields until `condition` holds, or fails naming `what`. Every
/// performer here is in memory, so the run beside the caller reaches each
/// state within a few polls.
pub(crate) async fn until(what: &str, condition: impl Fn() -> bool) {
    for _ in 0..10_000 {
        if condition() {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("{what}");
}

/// Runs `harness` beside `steer`, which is handed the run's control, and
/// returns the run's result.
pub(crate) async fn run_beside<F>(
    harness: Harness,
    steer: impl FnOnce(RunControl) -> F,
) -> Result<RunReport, Error>
where
    F: Future<Output = ()>,
{
    let control = harness.control();
    let (result, ()) = tokio::time::timeout(PATIENCE, async {
        tokio::join!(harness.run(), steer(control))
    })
    .await
    .expect("the run ends in time");
    result
}

/// The answer payloads of the effects whose record payload is keyed by
/// `kind` (`Chat`, `ToolCall`, `Timer`, `Vfs`) and whose body passes
/// `filter`, in effect order.
fn answers_where(records: &[Record], kind: &str, filter: impl Fn(&Value) -> bool) -> Vec<Value> {
    records
        .iter()
        .filter(|record| record.kind == RecordKind::Effect)
        .filter_map(|effect| {
            let body = effect.payload.get(kind)?;
            filter(body).then_some(effect.effect_id)
        })
        .map(|id| {
            records
                .iter()
                .find(|record| record.kind == RecordKind::Answer && record.effect_id == id)
                .expect("every effect is answered")
                .payload
                .clone()
        })
        .collect()
}

/// The answer payloads of every effect of `kind`, in effect order.
fn answers_to(records: &[Record], kind: &str) -> Vec<Value> {
    answers_where(records, kind, |_| true)
}

/// Whether a `ToolCall` effect body calls `tool`.
fn calls(tool: &'static str) -> impl Fn(&Value) -> bool {
    move |body| body["tool"] == tool
}

/// The run a test here drives: the first one a fresh recorder begins.
const FIRST_RUN: RunId = RunId::from_raw(1);

/// Whether `record` is an event of `kind`.
fn is_event(record: &Record, kind: &str) -> bool {
    record.kind == RecordKind::Event && record.payload["kind"] == kind
}

/// How many times `records` show the chain of the task `task` parked on
/// its empty inbox.
fn inbox_waits(records: &[Record], task: &str) -> usize {
    records
        .iter()
        .filter(|record| record.task_id == task && is_event(record, "inbox_waiting"))
        .count()
}

/// How many times `records` show the main chain parked on its empty
/// inbox.
fn main_waits(records: &[Record]) -> usize {
    inbox_waits(records, "0")
}

/// Whether `record` is the effect record of a `store.write`. The loop
/// appends an effect's record before it performs the effect, so a
/// recorder that acts on this record acts before the write runs.
fn is_store_write(record: &Record) -> bool {
    record.kind == RecordKind::Effect && record.payload["Vfs"]["op"].get("Write").is_some()
}

/// What a [`HookedRecorder`] runs on each record the loop appends, beside
/// the run's control.
type Hook = dyn Fn(&Record, &RunControl) + Send + Sync;

/// A recorder that keeps every record in `records` and, once the test
/// puts the run's control in `control`, hands `hook` each record beside
/// that control as the loop appends it.
struct HookedRecorder {
    records: Arc<MemoryRecorder>,
    control: Arc<OnceLock<RunControl>>,
    hook: Box<Hook>,
}

impl RunRecorder for HookedRecorder {
    fn begin_run(&self, meta: RunMeta) -> BoxFuture<'_, Result<RunId, Error>> {
        self.records.begin_run(meta)
    }

    fn append(&self, run: RunId, record: Record) -> BoxFuture<'_, Result<(), Error>> {
        if let Some(control) = self.control.get() {
            (self.hook)(&record, control);
        }
        self.records.append(run, record)
    }

    fn end_run(
        &self,
        run: RunId,
        result: Result<String, Error>,
    ) -> BoxFuture<'_, Result<(), Error>> {
        self.records.end_run(run, result)
    }
}

/// A recorder over `records` that runs `hook` on each record the loop
/// appends, beside the run's control, once the test puts the control in
/// the returned slot.
fn hooked_recorder(
    records: &Arc<MemoryRecorder>,
    hook: impl Fn(&Record, &RunControl) + Send + Sync + 'static,
) -> (Arc<HookedRecorder>, Arc<OnceLock<RunControl>>) {
    let slot = Arc::new(OnceLock::new());
    let recorder = HookedRecorder {
        records: Arc::clone(records),
        control: Arc::clone(&slot),
        hook: Box::new(hook),
    };
    (Arc::new(recorder), slot)
}

/// A prompt whose one section writes `paper.md`, writes `report.md` from
/// what it reads back, and returns the report.
const FILES: &str = "---\nname: files\ndescription: d\nrunvil: 0\n---\n\n\
    ## Only\n\n```lua\n\
    store.write('paper.md', '# Paper')\n\
    store.write('report.md', 'seen: ' .. store.read('paper.md'))\n\
    return store.read('report.md')\n```\n";

/// A prompt whose one section writes a store file and then raises.
const RAISES: &str = "---\nname: raises\ndescription: d\nrunvil: 0\n---\n\n\
    ## Only\n\n```lua\nstore.write('a.md', '1')\nerror('the section gives up')\n```\n";

/// A prompt that writes two store files and reads them back.
const WRITES_TWICE: &str = "---\nname: writes\ndescription: d\nrunvil: 0\n---\n\n\
    ## Only\n\n```lua\n\
    store.write('a.md', '1')\nstore.write('b.md', '2')\n\
    return store.read('a.md') .. store.read('b.md')\n```\n";

/// A prompt that writes a store file, infers once through its one role,
/// and returns the reply beside the file.
const INFERS_AND_STORES: &str = "---\nname: infers\ndescription: d\nrunvil: 0\n\
    ---\n\n\
    ## Only\n\n```lua\n\
    store.write('note.md', 'kept')\n\
    return models.infer('asked') .. '|' .. store.read('note.md')\n```\n";

/// A prompt that returns the model id of the `default` role's handle.
const READS_THE_SELECTION: &str = "---\nname: reads-selection\ndescription: d\nrunvil: 0\n\
    ---\n\n\
    ## Only\n\n```lua\n\
    return models.get('default').model_id\n```\n";

/// A prompt that asks `models.get` for a Gateway model id and returns how
/// the call ended.
const GETS_A_MODEL_ID: &str = "---\nname: gets-a-model-id\ndescription: d\nrunvil: 0\n---\n\n\
    ## Only\n\n```lua\n\
    local ok, err = pcall(models.get, 'vendor/raw-model')\n\
    return tostring(ok) .. ':' .. tostring(err)\n```\n";

/// A prompt that waits for a message to its main chain and returns it.
const RETURNS_A_MESSAGE: &str = "---\nname: returns-message\ndescription: d\nrunvil: 0\n---\n\n\
    ## Only\n\n```lua\nreturn 'got ' .. tasks.receive()\n```\n";

#[tokio::test]
async fn a_file_the_prompt_stores_round_trips_into_the_recorded_result() {
    let recorder = Arc::new(MemoryRecorder::new());
    let report = plain_harness(&recorder, ScriptedBroker::replying(), FILES)
        .run()
        .await
        .expect("the run is reported");

    assert_eq!(report.result, Ok("seen: # Paper".to_owned()));
    let run_id = report.run_id;
    assert_eq!(
        recorder.result(run_id),
        Some(report.result.clone()),
        "the recorder ends the run with the reported result"
    );

    let records = recorder.records(run_id);
    let kinds: Vec<RecordKind> = records.iter().map(|record| record.kind).collect();
    assert_eq!(
        kinds.last(),
        Some(&RecordKind::Event),
        "the run's end is an event"
    );
    let effects: Vec<usize> = kinds
        .iter()
        .enumerate()
        .filter(|(_, kind)| **kind == RecordKind::Effect)
        .map(|(position, _)| position)
        .collect();
    assert_eq!(
        effects.len(),
        4,
        "the paper's write and read, then the report's write and read"
    );
    for position in effects {
        assert_eq!(
            kinds[position + 1],
            RecordKind::Answer,
            "a serial run's answer follows its effect"
        );
    }
}

#[tokio::test]
async fn a_runs_record_starts_with_its_own_start_and_counts_from_zero() {
    let recorder = Arc::new(MemoryRecorder::new());
    let report = plain_harness(&recorder, ScriptedBroker::replying(), WRITES_TWICE)
        .run()
        .await
        .expect("the run is reported");
    assert_eq!(report.result, Ok("12".to_owned()));

    let records = recorder.records(report.run_id);
    let first = records.first().expect("the run recorded its events");
    assert_eq!(first.kind, RecordKind::Event);
    assert_eq!(
        first.payload["kind"], "run_started",
        "the run's own start leads"
    );
    assert_eq!(
        (first.task_id.as_str(), first.task_seq),
        ("0", 0),
        "the root task counts from zero"
    );
}

/// A broker that lists the model `first` before the scripted model, so
/// the selection and the first listed model are two models.
fn broker_listing_two() -> ScriptedBroker {
    ScriptedBroker::replying().listing(Listing::ListsAfter("first"))
}

#[tokio::test]
async fn a_prompt_reads_the_selected_model_through_a_roles_handle() {
    let recorder = Arc::new(MemoryRecorder::new());
    let report = plain_harness(&recorder, broker_listing_two(), READS_THE_SELECTION)
        .run()
        .await
        .expect("the run is reported");
    assert_eq!(
        report.result,
        Ok(MODEL.to_owned()),
        "the setup's selection fills the role, and the role's handle names that model, \
         though the broker lists another model first"
    );
}

#[tokio::test]
async fn a_run_with_no_selection_binds_the_first_listed_model() {
    let setup = RunSetup::new(
        Arc::new(MemoryRecorder::new()),
        Arc::new(broker_listing_two()),
    );
    let report = launch(
        no_packs(),
        PendingTimer::default(),
        READS_THE_SELECTION,
        setup,
    )
    .run()
    .await
    .expect("the run is reported");
    assert_eq!(
        report.result,
        Ok("first".to_owned()),
        "a setup that selects no model leaves the pick to the broker's list, and the first \
         listed model fills the role"
    );
}

#[tokio::test]
async fn models_get_refuses_a_gateway_model_id_that_is_not_a_role() {
    let recorder = Arc::new(MemoryRecorder::new());
    let report = plain_harness(&recorder, ScriptedBroker::replying(), GETS_A_MODEL_ID)
        .run()
        .await
        .expect("the run is reported");
    let text = report.result.expect("the run completes");
    assert!(
        text.starts_with("false:")
            && text.contains("models.get label \"vendor/raw-model\" is not a model role"),
        "a prompt reaches a model by its role, never by a Gateway model id: {text}"
    );
}

#[tokio::test]
async fn a_run_that_fails_after_it_starts_is_a_recorded_failed_run_not_an_error() {
    let recorder = Arc::new(MemoryRecorder::new());
    let report = plain_harness(&recorder, ScriptedBroker::replying(), RAISES)
        .run()
        .await
        .expect("a run's own failure is its result, not an error");

    let error = report.result.as_ref().expect_err("the section raised");
    assert_eq!(
        error.code,
        ErrorCode::LuaRuntime,
        "the run's own code, not a refusal"
    );
    assert!(
        error.located_message().contains("the section gives up"),
        "{error:?}"
    );
    let run_id = report.run_id;
    assert_eq!(recorder.result(run_id), Some(report.result.clone()));
    assert_eq!(
        answers_to(&recorder.records(run_id), "Vfs"),
        [json!({ "Vfs": { "Ok": "Unit" } })],
        "the run starts and performs its write before it raises"
    );
}

#[tokio::test]
async fn a_vfs_effects_answer_record_directly_follows_its_effect_record() {
    let recorder = Arc::new(MemoryRecorder::new());
    let report = plain_harness(&recorder, ScriptedBroker::replying(), WRITES_TWICE)
        .run()
        .await
        .expect("the run is reported");

    assert_eq!(report.result, Ok("12".to_owned()));
    let records = recorder.records(report.run_id);
    let mut effects = 0;
    for (position, effect) in records.iter().enumerate() {
        if effect.kind == RecordKind::Effect {
            effects += 1;
            assert!(effect.payload.get("Vfs").is_some(), "{effect:?}");
            let answer = &records[position + 1];
            assert_eq!(answer.kind, RecordKind::Answer, "nothing comes between");
            assert_eq!(answer.effect_id, effect.effect_id, "the answer is its own");
        }
    }
    assert_eq!(effects, 4, "the two writes and the two reads");
}

#[tokio::test]
async fn two_runs_in_flight_at_once_share_the_prompt_one_parse_returned() {
    let finds = Arc::new(AtomicUsize::new(0));
    let found = Arc::clone(&finds);
    let host = Arc::new(Host {
        packs: no_packs(),
        prompts: Arc::new(move |_: &str| -> Result<String, Error> {
            found.fetch_add(1, Ordering::SeqCst);
            Ok(RETURNS_A_MESSAGE.to_owned())
        }),
        timer: Arc::new(PendingTimer::default()),
    });
    let prompt = host.parse("message.md").expect("the prompt parses");

    // Each run gets a clone of the one handle and a recorder of its own.
    let over = |recorder: &Arc<MemoryRecorder>| {
        Harness::new(
            Arc::clone(&host),
            Arc::clone(&prompt),
            setup(recorder.clone(), ScriptedBroker::replying()),
        )
    };
    let (seen_first, seen_second) = (
        Arc::new(MemoryRecorder::new()),
        Arc::new(MemoryRecorder::new()),
    );
    let (first, second) = (over(&seen_first), over(&seen_second));
    let (to_first, to_second) = (first.control(), second.control());

    let steer = async {
        until("both runs wait on their inboxes", || {
            main_waits(&seen_first.records(FIRST_RUN)) == 1
                && main_waits(&seen_second.records(FIRST_RUN)) == 1
        })
        .await;
        assert!(to_first.send("one".to_owned()), "the first run is live");
        assert!(to_second.send("two".to_owned()), "the second run is live");
    };
    let (first, second, ()) = tokio::time::timeout(PATIENCE, async {
        tokio::join!(first.run(), second.run(), steer)
    })
    .await
    .expect("both runs end in time");

    assert_eq!(
        first.expect("the first run is reported").result,
        Ok("got one".to_owned())
    );
    assert_eq!(
        second.expect("the second run is reported").result,
        Ok("got two".to_owned())
    );
    assert_eq!(
        finds.load(Ordering::SeqCst),
        1,
        "the one parse found the prompt, and neither run found it again"
    );
}

/// Wakes the thread parked in [`block_on`].
struct Unpark(Thread);

impl Wake for Unpark {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

/// A std-only executor: polls `future` on this thread and parks between
/// polls until its waker unparks the thread.
fn block_on<F: Future + Unpin>(mut future: F) -> F::Output {
    let waker = Waker::from(Arc::new(Unpark(thread::current())));
    let mut cx = Context::from_waker(&waker);
    loop {
        if let Poll::Ready(output) = Pin::new(&mut future).poll(&mut cx) {
            return output;
        }
        thread::park();
    }
}

/// Compiles only for a `Send` value.
fn assert_send<T: Send>(_: &T) {}

#[test]
fn one_run_completes_on_the_tests_own_thread_under_a_thread_parking_block_on() {
    let recorder = Arc::new(MemoryRecorder::new());
    let broker = ScriptedBroker::replying();
    let rounds = broker.rounds();
    let run = plain_harness(&recorder, broker, INFERS_AND_STORES).run();
    assert_send(&run);

    let report = block_on(Box::pin(run)).expect("the run is reported with no runtime");
    assert_eq!(report.result, Ok("re: asked|kept".to_owned()));
    assert_eq!(rounds.lock().unwrap().len(), 1, "the one infer");
}
