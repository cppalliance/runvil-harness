//! A recorder that refuses a write ends the drive: whichever call it
//! refuses, the loop returns the refusal and sends the recorder nothing
//! more; a recorder error under another code is still a `RecorderFailed`
//! refusal; and a refusal mid-run aborts the performers still out. The
//! loop hands each event to the recorder's `append_event`, with its
//! record, and a refusal there ends the drive in the same way.

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_runner::display_chain;
use harness_runner::effect_loop::drive_run;
use harness_runner::recorder::{MemoryRecorder, Record, RecordKind, RunId, RunMeta, RunRecorder};
use runvil_engine::cancel::CancelHandle;
use runvil_engine::event::Event;
use runvil_engine::{BoxFuture, Error, ErrorCode, ErrorFact};
use serde_json::json;

use super::{answers, await_raised, cancel_after, meta};
use crate::prepare::RefusingAppends;
use crate::support::{
    FailingRecorder, PendingTimer, TIMED_MAIN, TextTool, Unused, WAITS, loop_setup, run,
    run_with_child, run_with_child_and_cancel, timed_loop_setup,
};

/// A run of one store write and one read: its every effect is a Vfs
/// effect, so no performer is ever out.
fn store_round_trip() -> runvil_engine::Run {
    run("store.write('a.md', '1')\nreturn store.read('a.md')")
}

#[tokio::test]
async fn a_recorder_that_refuses_its_nth_call_ends_the_drive_with_a_recorder_error() {
    // The run's whole call count, from a recorder that refuses nothing.
    let baseline = Arc::new(FailingRecorder::never_failing());
    let run_id = baseline.inner().begin_run(meta()).await.unwrap();
    drive_run(
        store_round_trip(),
        &loop_setup(baseline.clone()),
        Arc::new(Unused),
        run_id,
        CancelHandle::new(),
    )
    .await
    .unwrap();
    let total = baseline.calls();
    assert!(total >= 5, "events, two effects, two answers, and the end");

    for call in 1..=total {
        let recorder = Arc::new(FailingRecorder::failing_on(call));
        let run_id = recorder.inner().begin_run(meta()).await.unwrap();
        let error = drive_run(
            store_round_trip(),
            &loop_setup(recorder.clone()),
            Arc::new(Unused),
            run_id,
            CancelHandle::new(),
        )
        .await
        .expect_err("a refused call ends the drive");

        assert_eq!(
            error.code,
            ErrorCode::RecorderFailed,
            "call {call} of {total}: a refusal, not the run's result"
        );
        assert!(
            display_chain(&error).contains(&format!("refuses call {call}")),
            "call {call} of {total}: {}",
            display_chain(&error)
        );
        assert_eq!(
            recorder.calls(),
            call,
            "the loop sends nothing after the refused call"
        );
        assert_eq!(
            recorder.inner().records(run_id).len(),
            call - 1,
            "the refused call recorded nothing, and every call before it was a record"
        );
        assert_eq!(
            recorder.inner().result(run_id),
            None,
            "an abandoned run is not ended at the recorder"
        );
    }
}

#[tokio::test]
async fn a_recorder_error_under_another_code_ends_the_drive_as_a_recorder_failure() {
    // Every append fails with the recorder's own `Io` error, so the run's
    // first write is refused.
    let recorder = Arc::new(RefusingAppends(MemoryRecorder::new()));
    let run_id = recorder.begin_run(meta()).await.unwrap();
    let error = drive_run(
        store_round_trip(),
        &loop_setup(recorder.clone()),
        Arc::new(Unused),
        run_id,
        CancelHandle::new(),
    )
    .await
    .expect_err("the refused append ends the drive");

    assert_eq!(
        error.code,
        ErrorCode::RecorderFailed,
        "the recorder's `Io` error becomes a refusal, never the run's result"
    );
    assert_eq!(error.message, "the run recorder failed: the disk is full");
    assert_eq!(error.facts.get(&ErrorFact::Run), Some(&json!(run_id.get())));
    assert_eq!(
        recorder.0.result(run_id),
        None,
        "an abandoned run is not ended at the recorder"
    );
}

#[tokio::test]
async fn a_recorder_that_refuses_an_answer_aborts_the_performers_still_out() {
    // Two effects are out in the first step, a timer that never fires and
    // a tool call that answers at once. A run canceled after the answer
    // lands shows which call records it: the first answer in the stream.
    let dry = Arc::new(FailingRecorder::never_failing());
    let dry_run = dry.inner().begin_run(meta()).await.unwrap();
    let cancel = CancelHandle::new();
    cancel_after(&cancel, Duration::from_millis(50));
    let dry_result = drive_run(
        run_with_child_and_cancel(TIMED_MAIN, WAITS, &cancel),
        &timed_loop_setup(dry.clone(), Arc::new(PendingTimer::default())),
        Arc::new(TextTool("hi")),
        dry_run,
        cancel,
    )
    .await;
    assert_ne!(
        dry_result.map_err(|error| error.code),
        Err(ErrorCode::RecorderFailed),
        "the dry run is not refused"
    );
    let first_answer = dry
        .inner()
        .records(dry_run)
        .iter()
        .position(|record| record.kind == RecordKind::Answer)
        .expect("the tool call is answered");

    // The same run, with the recorder refusing exactly that call: the
    // timer's performer is still out when the drive fails.
    let timer_dropped = Arc::new(AtomicBool::new(false));
    let timer = Arc::new(PendingTimer {
        dropped: Arc::clone(&timer_dropped),
    });
    let recorder = Arc::new(FailingRecorder::failing_on(first_answer + 1));
    let run_id = recorder.inner().begin_run(meta()).await.unwrap();
    let error = drive_run(
        run_with_child(TIMED_MAIN, WAITS),
        &timed_loop_setup(recorder.clone(), timer),
        Arc::new(TextTool("hi")),
        run_id,
        CancelHandle::new(),
    )
    .await
    .expect_err("the refused answer ends the drive");

    assert_eq!(
        error.code,
        ErrorCode::RecorderFailed,
        "a refusal: {error:?}"
    );
    assert!(
        display_chain(&error).contains(&format!("refuses call {}", first_answer + 1)),
        "the chain names the recorder's reason: {}",
        display_chain(&error)
    );
    assert!(
        answers(&recorder.inner().records(run_id)).is_empty(),
        "the refused answer was not recorded"
    );
    await_raised(
        &timer_dropped,
        "the timer's performer is aborted when the drive fails",
    )
    .await;
}

/// One call that the loop made to a [`CallLog`] once the run had begun.
#[derive(Debug, Clone)]
enum Call {
    /// `append`, with the kind of its record.
    Append(RecordKind),
    /// `append_event`, with its record and its event. The event is
    /// boxed because it is far larger than the other variants.
    AppendEvent(Record, Box<Event>),
    /// `end_run`.
    End,
}

/// A recorder over a memory recorder that logs each call the loop makes
/// to it, and so which of `append` and `append_event` handed it each
/// record. With `refusing_events` set, it refuses every event with an
/// error under a code of its own.
struct CallLog {
    inner: MemoryRecorder,
    refusing_events: bool,
    calls: Mutex<Vec<Call>>,
}

impl CallLog {
    fn new(refusing_events: bool) -> Arc<CallLog> {
        Arc::new(CallLog {
            inner: MemoryRecorder::new(),
            refusing_events,
            calls: Mutex::new(Vec::new()),
        })
    }

    fn log(&self, call: Call) {
        self.calls.lock().unwrap().push(call);
    }

    /// The calls so far, in the order the loop made them.
    fn calls(&self) -> Vec<Call> {
        self.calls.lock().unwrap().clone()
    }
}

impl RunRecorder for CallLog {
    fn begin_run(&self, meta: RunMeta) -> BoxFuture<'_, Result<RunId, Error>> {
        self.inner.begin_run(meta)
    }

    fn append(&self, run: RunId, record: Record) -> BoxFuture<'_, Result<(), Error>> {
        self.log(Call::Append(record.kind));
        self.inner.append(run, record)
    }

    fn append_event<'a>(
        &'a self,
        run: RunId,
        record: Record,
        event: &'a Event,
    ) -> BoxFuture<'a, Result<(), Error>> {
        self.log(Call::AppendEvent(record.clone(), Box::new(event.clone())));
        if self.refusing_events {
            let code = ErrorCode::Io;
            return Box::pin(async move {
                Err(Error::new(code, "the event store is full", code.prose()))
            });
        }
        self.inner.append_event(run, record, event)
    }

    fn end_run(
        &self,
        run: RunId,
        result: Result<String, Error>,
    ) -> BoxFuture<'_, Result<(), Error>> {
        self.log(Call::End);
        self.inner.end_run(run, result)
    }
}

#[tokio::test]
async fn the_loop_hands_each_event_to_append_event_with_its_record() {
    let recorder = CallLog::new(false);
    let run_id = recorder.inner.begin_run(meta()).await.unwrap();
    drive_run(
        store_round_trip(),
        &loop_setup(recorder.clone()),
        Arc::new(Unused),
        run_id,
        CancelHandle::new(),
    )
    .await
    .unwrap();

    let calls = recorder.calls();
    let mut events = 0;
    let mut appended = Vec::new();
    for call in &calls {
        match call {
            Call::Append(kind) => appended.push(*kind),
            Call::AppendEvent(record, event) => {
                events += 1;
                assert_eq!(
                    record.kind,
                    RecordKind::Event,
                    "`append_event` takes the record of an event: {record:?}"
                );
                assert_eq!(
                    record.payload,
                    serde_json::to_value(event).unwrap(),
                    "the event is the value that the payload of its record was made from"
                );
            }
            Call::End => {}
        }
    }
    assert!(events > 0, "the run reports events: {calls:?}");
    assert_eq!(
        appended,
        [
            RecordKind::Effect,
            RecordKind::Answer,
            RecordKind::Effect,
            RecordKind::Answer
        ],
        "`append` takes the record of each effect and each answer, and of no event"
    );
    assert!(
        matches!(calls.last(), Some(Call::End)),
        "the run ends at the recorder: {calls:?}"
    );
    assert_eq!(
        recorder.inner.records(run_id).len(),
        events + appended.len(),
        "the store under the override holds the record of each event beside the others"
    );
}

#[tokio::test]
async fn a_refused_append_event_ends_the_drive_as_a_recorder_failure() {
    let recorder = CallLog::new(true);
    let run_id = recorder.inner.begin_run(meta()).await.unwrap();
    let error = drive_run(
        store_round_trip(),
        &loop_setup(recorder.clone()),
        Arc::new(Unused),
        run_id,
        CancelHandle::new(),
    )
    .await
    .expect_err("the refused event ends the drive");

    assert_eq!(
        error.code,
        ErrorCode::RecorderFailed,
        "the recorder's `Io` error becomes a refusal, as one from `append` does"
    );
    assert_eq!(
        error.message,
        "the run recorder failed: the event store is full"
    );
    assert_eq!(error.facts.get(&ErrorFact::Run), Some(&json!(run_id.get())));

    let calls = recorder.calls();
    let refused = calls
        .iter()
        .position(|call| matches!(call, Call::AppendEvent(..)))
        .expect("the run reports an event");
    assert_eq!(
        refused + 1,
        calls.len(),
        "the loop sends the recorder nothing after the refused event: {calls:?}"
    );
    assert_eq!(
        recorder.inner.result(run_id),
        None,
        "an abandoned run is not ended at the recorder"
    );
}
