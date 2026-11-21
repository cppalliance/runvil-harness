//! The turns the effect loop gives its executor while a tool call asks
//! for files: a call that keeps asking does not hold the thread of an
//! executor that takes its tasks in turn, so a cancel that a second task
//! raises lands long before the call is done, and a call that asks for
//! more files than one turn allows loses no request.

use std::sync::Arc;

use harness_runner::Harness;
use harness_runner::recorder::{MemoryRecorder, Record, RecordKind};
use runvil_engine::Error;
use runvil_engine::vfs::{Origin, VfsRef};
use runvil_plugin::{
    BoxFuture, Pack, PackFactory, PackId, PackInfo, ToolContext, ToolId, ToolInfo, ToolOutput,
    VfsOp, VfsOutcome,
};
use serde_json::{Value, json};
use tokio::sync::Notify;

use super::{PATIENCE, launch, setup};
use crate::scripted::ScriptedBroker;
use crate::support::{assert_canceled, host_over, registry};

/// The fixture Pack `reads`. Its one tool, `reads/many`, says that it
/// has started and then reads `/in.md` as many times as its `count`
/// says, one read after another. It answers `read <count>` when each
/// read gave it the text [`SEEDED`], and says what the first other read
/// gave it otherwise.
const READS_PACK: PackInfo = PackInfo::new("tests/reads");

/// The text of `/in.md`.
const SEEDED: &str = "x";

struct ReadsPack {
    tools: Vec<ToolInfo>,
    started: Arc<Notify>,
}

/// The fixture Pack's factory, whose tool notifies `started` as it
/// starts, before it asks for anything.
fn reads_pack(started: Arc<Notify>) -> impl PackFactory {
    move |name: &PackId| -> Result<Arc<dyn Pack>, Error> {
        let many = ToolInfo::new(
            ToolId::parse(&format!("{name}/many")).unwrap(),
            "Read one file many times.",
            json!({ "type": "object", "properties": {} }),
        );
        Ok(Arc::new(ReadsPack {
            tools: vec![many],
            started: Arc::clone(&started),
        }))
    }
}

/// The operation that reads the whole of `/in.md`.
fn read_in() -> VfsOp {
    VfsOp::Read {
        path: "/in.md".to_owned(),
        start: None,
        end: None,
    }
}

impl Pack for ReadsPack {
    fn tools(&self) -> Vec<ToolInfo> {
        self.tools.clone()
    }

    fn call<'a>(
        &'a self,
        cx: ToolContext<'a>,
        args: Value,
    ) -> BoxFuture<'a, Result<ToolOutput, Error>> {
        Box::pin(async move {
            self.started.notify_one();
            let count = args["count"].as_u64().unwrap_or_default();
            for _ in 0..count {
                match cx.files().request(read_in()).await {
                    Ok(VfsOutcome::Text(text)) if text == SEEDED => {}
                    other => return Ok(ToolOutput::trusted(format!("a read gave {other:?}"))),
                }
            }
            Ok(ToolOutput::trusted(format!("read {count}")))
        })
    }
}

/// A Harness whose prompt calls `reads/many` once, for `count` reads of
/// a one-byte `/in.md`, beside the recorder of its run. The tool
/// notifies `started` as it starts. The recorder is in memory, so every
/// write of the record is done at its first poll.
fn reading(count: u32, started: &Arc<Notify>) -> (Harness, Arc<MemoryRecorder>) {
    let vfs = VfsRef::default();
    vfs.acquire(Origin::new("turns seed"))
        .unwrap()
        .write("/in.md", SEEDED.as_bytes())
        .unwrap();
    let mut packs = registry(&[]);
    packs
        .insert(READS_PACK, reads_pack(Arc::clone(started)))
        .unwrap();
    let source = format!(
        "---\nname: reads\ndescription: d\nrunvil: 0\n\
         uses:\n  - tests/reads\n---\n\n\
         ## Only\n\n```lua\n\
         return tools.call('reads/many', {{ count = {count} }})\n```\n"
    );
    let recorder = Arc::new(MemoryRecorder::new());
    let harness = launch(
        host_over(packs),
        &source,
        setup(recorder.clone(), ScriptedBroker::replying()).vfs(vfs),
    );
    (harness, recorder)
}

/// Whether `record` is the effect record of a file operation that a tool
/// call asked for.
fn is_call_file_effect(record: &Record) -> bool {
    record.kind == RecordKind::Effect && record.payload["Vfs"].get("call").is_some()
}

/// The id of the one tool call of `records`, beside the payloads of the
/// effect and answer records that sit between the call's effect record
/// and its answer record.
fn inside_the_call(records: &[Record]) -> (u64, Vec<Value>) {
    let records: Vec<&Record> = records
        .iter()
        .filter(|record| record.kind != RecordKind::Event)
        .collect();
    let asked = records
        .iter()
        .position(|record| record.payload.get("ToolCall").is_some())
        .expect("the run made a tool call");
    let call = records[asked]
        .effect_id
        .expect("an effect record holds its id");
    let answered = records
        .iter()
        .position(|record| record.kind == RecordKind::Answer && record.effect_id == Some(call))
        .expect("the call's answer is recorded");
    let inside = records[asked + 1..answered]
        .iter()
        .map(|record| record.payload.clone())
        .collect();
    (call, inside)
}

#[tokio::test]
async fn a_second_tasks_cancel_lands_long_before_a_call_that_keeps_asking_for_files_is_done() {
    let started = Arc::new(Notify::new());
    let (harness, recorder) = reading(5_000, &started);
    let control = harness.control();
    // The second task of the executor's one thread. It runs only when the
    // run's future returns to the executor, and it waits for the tool to
    // start, so a run that returned once before the call began does not
    // pass for a loop that gives turns.
    let canceler = tokio::spawn(async move {
        started.notified().await;
        control.cancel();
    });

    // One deadline holds the run and the second task, so a run that ends
    // with the tool never started fails here and leaves nothing waiting.
    let (report, second_task) =
        tokio::time::timeout(PATIENCE, async { tokio::join!(harness.run(), canceler) })
            .await
            .expect("the run and the second task end in time");
    second_task.expect("the second task ends");
    let report = report.expect("the run is reported");

    assert_canceled(&report.result);
    let asked = recorder
        .records(report.run_id)
        .iter()
        .filter(|record| is_call_file_effect(record))
        .count();
    assert!(
        (1..500).contains(&asked),
        "the cancel landed once the call was asking, long before its 5,000 reads were done: \
         the record holds {asked} file effects"
    );
}

#[tokio::test]
async fn a_call_that_asks_for_more_files_than_one_turn_allows_loses_no_request() {
    let started = Arc::new(Notify::new());
    let (harness, recorder) = reading(100, &started);

    let report = tokio::time::timeout(PATIENCE, harness.run())
        .await
        .expect("the run ends in time")
        .expect("the run is reported");

    assert_eq!(report.result, Ok("read 100".to_owned()));
    let (call, inside) = inside_the_call(&recorder.records(report.run_id));
    let one_read = [
        json!({ "Vfs": { "call": call, "op": {
            "Read": { "path": "/in.md", "start": null, "end": null }
        } } }),
        json!({ "VfsReceipt": { "Ok": { "Bytes": 1 } } }),
    ];
    let expected: Vec<Value> = std::iter::repeat_n(one_read, 100).flatten().collect();
    assert_eq!(
        inside, expected,
        "each of the 100 reads is a file effect and its receipt, in order, inside the call"
    );
}
