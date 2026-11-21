//! A tool call's file operations under `Harness::run`: a Rust Pack reads
//! and writes through its call's files, and each operation is a `Vfs`
//! effect that names the call, recorded with its receipt between the
//! call's effect and its answer; an operation that fails, or whose
//! policy panics, reaches the tool and the record as the failure and the
//! run goes on; two calls in flight at once each get the outcomes of
//! their own requests; and a stop or a cancel raised while a call's
//! requests are pending drops the call. A request whose effect went out
//! before the loop saw the stop or the cancel is performed and recorded,
//! the effect of a request the loop reported as it was raised is
//! answered `Dropped` unperformed, and a request the loop had not taken
//! is never issued.

use std::sync::Arc;

use harness_runner::recorder::{MemoryRecorder, Record, RecordKind};
use harness_runner::{Host, RunControl};
use runvil_engine::Error;
use runvil_engine::vfs::{MemoryBackend, Op, Origin, Policy, Verdict, VfsPath, VfsRef};
use runvil_plugin::{
    BoxFuture, Pack, PackFactory, PackId, PackInfo, ToolContext, ToolFiles, ToolId, ToolInfo,
    ToolOutput, VfsOp, VfsOutcome,
};
use serde_json::{Value, json};

use super::{PATIENCE, hooked_recorder, launch, setup};
use crate::scripted::ScriptedBroker;
use crate::support::{assert_canceled, host_over, registry};

/// The fixture Pack `files`, whose every tool works through its call's
/// files. `files/copy` reads the file its `from` names and writes what it
/// read to the one its `to` names. `files/cat` answers with the text of
/// the file its `path` names. `files/put` writes its `text` to that file,
/// and `files/append` reads the file and writes it back with its `text`
/// added. The info of each of those two says that it changes files.
/// `files/pair` runs the hook its factory was given and then asks for two
/// writes at once.
const FILES_PACK: PackInfo = PackInfo::new("tests/files");

/// What `files/pair` runs as it starts, before it asks for anything.
type OnPair = Arc<dyn Fn() + Send + Sync>;

struct FilesPack {
    tools: Vec<ToolInfo>,
    on_pair: OnPair,
}

/// The fixture Pack's factory, whose `files/pair` runs `on_pair` as it
/// starts.
fn files_pack(on_pair: impl Fn() + Send + Sync + 'static) -> impl PackFactory {
    let on_pair: OnPair = Arc::new(on_pair);
    move |name: &PackId| -> Result<Arc<dyn Pack>, Error> {
        let tool = |tool: &str, description: &str| {
            ToolInfo::new(
                ToolId::parse(&format!("{name}/{tool}")).unwrap(),
                description,
                json!({ "type": "object", "properties": {} }),
            )
        };
        Ok(Arc::new(FilesPack {
            tools: vec![
                tool("copy", "Copy one file to another."),
                tool("cat", "Read one file."),
                tool("put", "Write one file.").changes_files(true),
                tool("append", "Add text to the end of one file.").changes_files(true),
                tool("pair", "Write two files at once."),
            ],
            on_pair: Arc::clone(&on_pair),
        }))
    }
}

/// A Host holding the fixture Pack, whose `files/pair` runs `on_pair` as
/// it starts.
pub(super) fn files_host(on_pair: impl Fn() + Send + Sync + 'static) -> Arc<Host> {
    let mut packs = registry(&[]);
    packs.insert(FILES_PACK, files_pack(on_pair)).unwrap();
    host_over(packs)
}

/// The operation that writes `text` to `path`.
fn write(path: &str, text: &str) -> VfsOp {
    VfsOp::Write {
        path: path.to_owned(),
        contents: text.as_bytes().to_vec(),
    }
}

/// The whole text of the file at `path`, read through `files`, or what a
/// fixture tool answers when the read fails: the failure as text.
async fn read_text(files: &dyn ToolFiles, path: String) -> Result<String, ToolOutput> {
    let read = VfsOp::Read {
        path,
        start: None,
        end: None,
    };
    match files.request(read).await {
        Ok(VfsOutcome::Text(text)) => Ok(text),
        Ok(other) => Err(ToolOutput::trusted(format!("a read answered {other:?}"))),
        Err(error) => Err(ToolOutput::trusted(format!("failed: {error}"))),
    }
}

/// Writes `text` to `path` through `files` and answers with `text`, or
/// with the failure of the write as text.
async fn write_text(files: &dyn ToolFiles, path: &str, text: String) -> ToolOutput {
    match files.request(write(path, &text)).await {
        Ok(_) => ToolOutput::trusted(text),
        Err(error) => ToolOutput::trusted(format!("failed: {error}")),
    }
}

impl Pack for FilesPack {
    fn tools(&self) -> Vec<ToolInfo> {
        self.tools.clone()
    }

    fn call<'a>(
        &'a self,
        cx: ToolContext<'a>,
        args: Value,
    ) -> BoxFuture<'a, Result<ToolOutput, Error>> {
        Box::pin(async move {
            let files = cx.files();
            let arg = |name: &str| args[name].as_str().unwrap_or_default().to_owned();
            let output = match cx.tool().name() {
                "copy" => match read_text(files, arg("from")).await {
                    Ok(text) => write_text(files, &arg("to"), text).await,
                    Err(failed) => failed,
                },
                "cat" => match read_text(files, arg("path")).await {
                    Ok(text) => ToolOutput::trusted(text),
                    Err(failed) => failed,
                },
                "put" => write_text(files, &arg("path"), arg("text")).await,
                "append" => match read_text(files, arg("path")).await {
                    Ok(text) => write_text(files, &arg("path"), text + &arg("text")).await,
                    Err(failed) => failed,
                },
                _ => {
                    (self.on_pair)();
                    let (first, second) = tokio::join!(
                        files.request(write("/first.md", "1")),
                        files.request(write("/second.md", "2")),
                    );
                    ToolOutput::trusted(format!("{first:?}|{second:?}"))
                }
            };
            Ok(output)
        })
    }
}

/// Panics on every read under `/broken` and allows every other
/// operation.
struct BrokenReads;

impl Policy for BrokenReads {
    fn check(&self, op: Op, path: &VfsPath) -> Verdict {
        let path = path.as_str();
        assert!(
            !(matches!(op, Op::Read) && path.starts_with("/broken")),
            "the policy broke on {path}"
        );
        Verdict::Allow
    }
}

/// A prompt that copies `/in.md` to `/out.md` with the fixture tool, then
/// asks it to copy a file that is absent and one whose read panics in the
/// Host's policy, and returns the three answers.
const COPIES: &str = "---\nname: copies\ndescription: d\nrunvil: 0\n\
    uses:\n  - tests/files\n---\n\n\
    ## Only\n\n```lua\n\
    local function copy(from, to)\n\
    \x20   return tools.call('files/copy', { from = from, to = to })\n\
    end\n\
    return table.concat({\n\
    \x20   copy('/in.md', '/out.md'),\n\
    \x20   copy('/absent.md', '/never.md'),\n\
    \x20   copy('/broken/a.md', '/never.md'),\n\
    }, '|')\n```\n";

/// A prompt that calls `files/pair` under a `pcall`, writes to its store,
/// and returns how the call ended. The store write keeps the section
/// going after a dropped call, so the run has not ended when a file
/// effect of that call goes out.
const PAIRS: &str = "---\nname: pairs\ndescription: d\nrunvil: 0\n\
    uses:\n  - tests/files\n---\n\n\
    ## Only\n\n```lua\n\
    local ok, err = pcall(tools.call, 'files/pair')\n\
    store.write('after.md', 'kept')\n\
    return tostring(ok) .. ':' .. err.kind\n```\n";

/// The effect record of the file operation `op` that tool call `call`
/// asked for.
fn file_effect(call: u64, op: &Value) -> Value {
    json!({ "Vfs": { "call": call, "op": op } })
}

/// The answer record of a file operation that failed as `kind`, saying
/// `message`.
fn failed(kind: &str, message: &str) -> Value {
    json!({ "VfsReceipt": { "Err": { "kind": kind, "message": message } } })
}

/// The recorded form of the operation that reads the whole of `path`.
fn read_op(path: &str) -> Value {
    json!({ "Read": { "path": path, "start": null, "end": null } })
}

/// The recorded form of the operation that writes `text` to `path`.
fn write_op(path: &str, text: &str) -> Value {
    json!({ "Write": { "path": path, "contents": text } })
}

/// For each tool call of `records`, in call order: its effect id beside
/// the payloads of the effect and answer records that sit between the
/// call's effect record and its answer record.
fn inside_each_call(records: &[Record]) -> Vec<(u64, Vec<Value>)> {
    let is_call = |record: &Record| {
        record.kind == RecordKind::Effect && record.payload.get("ToolCall").is_some()
    };
    records
        .iter()
        .enumerate()
        .filter(|(_, record)| is_call(record))
        .map(|(position, call)| {
            let id = call.effect_id.expect("an effect record holds its id");
            let answers_call =
                |record: &Record| record.kind == RecordKind::Answer && record.effect_id == Some(id);
            let inside = records[position + 1..]
                .iter()
                .take_while(|record| !answers_call(record))
                .filter(|record| record.kind != RecordKind::Event)
                .map(|record| record.payload.clone())
                .collect();
            (id, inside)
        })
        .collect()
}

#[tokio::test]
async fn a_tool_reads_and_writes_through_its_files_and_each_operation_is_recorded_inside_its_call()
{
    let vfs = VfsRef::builder()
        .mount("/", MemoryBackend::new())
        .policy(BrokenReads)
        .build();
    let seed = vfs.acquire(Origin::new("tool files seed")).unwrap();
    seed.write("/in.md", b"seeded").unwrap();
    drop(seed);
    let recorder = Arc::new(MemoryRecorder::new());
    let harness = launch(
        files_host(|| ()),
        COPIES,
        setup(recorder.clone(), ScriptedBroker::replying()).vfs(vfs.clone()),
    );

    let report = harness.run().await.expect("the run is reported");

    assert_eq!(
        report.result,
        Ok([
            "seeded",
            "failed: not found: /absent.md",
            "failed: backend failure: the file operation panicked",
        ]
        .join("|")),
        "each call got the outcome of each operation it asked for, a failure included, and \
         the run went on"
    );
    let files = vfs.acquire(Origin::new("tool files test")).unwrap();
    assert_eq!(
        files.read_string("/out.md"),
        Ok("seeded".to_owned()),
        "the Host reads the tool's copy from the setup's filesystem after the run"
    );
    assert_eq!(
        files.exists("/never.md"),
        Ok(false),
        "a copy whose read failed wrote nothing"
    );

    let calls = inside_each_call(&recorder.records(report.run_id));
    let [(copied, _), (absent, _), (broken, _)] = calls.as_slice() else {
        panic!("the run made three tool calls: {calls:?}");
    };
    assert_eq!(
        calls,
        [
            (
                *copied,
                vec![
                    file_effect(*copied, &read_op("/in.md")),
                    json!({ "VfsReceipt": { "Ok": { "Bytes": 6 } } }),
                    file_effect(*copied, &write_op("/out.md", "seeded")),
                    json!({ "VfsReceipt": { "Ok": "Done" } }),
                ]
            ),
            (
                *absent,
                vec![
                    file_effect(*absent, &read_op("/absent.md")),
                    failed("not_found", "not found: /absent.md"),
                ]
            ),
            (
                *broken,
                vec![
                    file_effect(*broken, &read_op("/broken/a.md")),
                    failed("backend", "backend failure: the file operation panicked"),
                ]
            ),
        ],
        "each operation is a Vfs effect that names its call, answered with a receipt or the \
         failure, between the call's effect record and its answer record"
    );
}

/// A prompt whose two tasks each copy a file of their own with the
/// fixture tool, so two calls ask for files while both are in flight,
/// and which returns both answers.
const COPIES_TWICE: &str = "---\nname: copies-twice\ndescription: d\nrunvil: 0\n\
    uses:\n  - tests/files\n---\n\n\
    ## Main\n\n```lua\n\
    local left = tasks.spawn('../Left')\n\
    local right = tasks.spawn('../Right')\n\
    local results = tasks.join({ left, right })\n\
    return results[1].result .. '|' .. results[2].result\n```\n\n\
    ## Left\n\n```lua\n\
    return tools.call('files/copy', { from = '/left.md', to = '/left.out' })\n```\n\n\
    ## Right\n\n```lua\n\
    return tools.call('files/copy', { from = '/right.md', to = '/right.out' })\n```\n";

/// The operations of the file effects in `records` that name the tool
/// call whose `from` argument is `from`, in record order.
fn asked_by_the_copy_of(records: &[Record], from: &str) -> Vec<Value> {
    let call = records
        .iter()
        .find(|record| record.payload["ToolCall"]["args"]["from"] == from)
        .and_then(|record| record.effect_id)
        .expect("the run copied the file");
    records
        .iter()
        .filter(|record| is_call_file_effect(record) && record.payload["Vfs"]["call"] == call)
        .map(|record| record.payload["Vfs"]["op"].clone())
        .collect()
}

#[tokio::test]
async fn two_calls_in_flight_at_once_each_get_the_outcomes_of_their_own_requests() {
    let vfs = VfsRef::default();
    let seed = vfs.acquire(Origin::new("tool files seed")).unwrap();
    seed.write("/left.md", b"left").unwrap();
    seed.write("/right.md", b"right").unwrap();
    drop(seed);
    let recorder = Arc::new(MemoryRecorder::new());
    let harness = launch(
        files_host(|| ()),
        COPIES_TWICE,
        setup(recorder.clone(), ScriptedBroker::replying()).vfs(vfs.clone()),
    );

    let report = harness.run().await.expect("the run is reported");

    assert_eq!(
        report.result,
        Ok("left|right".to_owned()),
        "each call read the file it asked for"
    );
    let files = vfs.acquire(Origin::new("tool files test")).unwrap();
    assert_eq!(files.read_string("/left.out"), Ok("left".to_owned()));
    assert_eq!(files.read_string("/right.out"), Ok("right".to_owned()));

    let records = recorder.records(report.run_id);
    // Each call makes two requests, so the first two file effects name
    // two calls only when the second call asked before the first call's
    // second request.
    let askers: Vec<&Value> = records
        .iter()
        .filter(|record| is_call_file_effect(record))
        .map(|record| &record.payload["Vfs"]["call"])
        .collect();
    assert_ne!(
        askers[0], askers[1],
        "both calls were in flight when the second file effect went out: {askers:?}"
    );
    for (from, to, text) in [
        ("/left.md", "/left.out", "left"),
        ("/right.md", "/right.out", "right"),
    ] {
        assert_eq!(
            asked_by_the_copy_of(&records, from),
            [read_op(from), write_op(to, text)],
            "the record holds the two operations of the copy of {from} under its own call"
        );
    }
}

/// What a test raises on its run while a call's requests are pending.
#[derive(Clone, Copy)]
enum Raise {
    Stop,
    Cancel,
}

impl Raise {
    fn on(self, control: &RunControl) {
        match self {
            Raise::Stop => control.stop_round(),
            Raise::Cancel => control.cancel(),
        }
    }
}

/// When a test raises its stop or cancel.
#[derive(Clone, Copy, PartialEq, Eq)]
enum At {
    /// As `files/pair` starts, before it asks for its two writes. The
    /// loop takes the call's first request off its desk and reports it to
    /// the run, and then sees what was raised.
    TheAsk,
    /// As the loop records the effect of the call's first request, before
    /// it performs it. The second request is still at the call's desk.
    TheFirstEffect,
}

/// Whether `record` is the effect record of a file operation that a tool
/// call asked for.
fn is_call_file_effect(record: &Record) -> bool {
    record.kind == RecordKind::Effect && record.payload["Vfs"].get("call").is_some()
}

/// What a run of [`PAIRS`] left behind.
struct PairRun {
    result: Result<String, Error>,
    /// The id of the run's one `ToolCall` effect.
    call: u64,
    /// The run's effect and answer records, as [`trail`] shows them.
    trail: Value,
    /// Whether the setup's filesystem holds `/first.md`, and whether it
    /// holds `/second.md`.
    written: (bool, bool),
}

/// Runs [`PAIRS`] on a Host holding the fixture Pack and raises `raise`
/// on the run at `at`.
async fn pair_run(raise: Raise, at: At) -> PairRun {
    let records = Arc::new(MemoryRecorder::new());
    let (recorder, slot) = hooked_recorder(&records, move |record, control| {
        if at == At::TheFirstEffect && is_call_file_effect(record) {
            raise.on(control);
        }
    });
    let asking = Arc::clone(&slot);
    let on_pair = move || {
        if at == At::TheAsk {
            raise.on(asking.get().expect("the run's control is in the slot"));
        }
    };
    let vfs = VfsRef::default();
    let harness = launch(
        files_host(on_pair),
        PAIRS,
        setup(recorder, ScriptedBroker::replying()).vfs(vfs.clone()),
    );
    slot.set(harness.control()).unwrap();

    let report = tokio::time::timeout(PATIENCE, harness.run())
        .await
        .expect("the run ends in time")
        .expect("the run is reported");
    let (call, trail) = trail(&records.records(report.run_id));
    let files = vfs.acquire(Origin::new("tool files test")).unwrap();
    let written = (
        files.exists("/first.md").unwrap(),
        files.exists("/second.md").unwrap(),
    );
    PairRun {
        result: report.result,
        call,
        trail,
        written,
    }
}

/// The id of the one tool call of `records`, beside the run's effect and
/// answer records in order, each as its kind, its effect id counted from
/// the call's, and its payload. The call's own effect record shows as
/// `"ToolCall"`.
fn trail(records: &[Record]) -> (u64, Value) {
    let call = records
        .iter()
        .find(|record| record.payload.get("ToolCall").is_some())
        .and_then(|record| record.effect_id)
        .expect("the run made a tool call");
    let trail = records
        .iter()
        .filter(|record| record.kind != RecordKind::Event)
        .map(|record| {
            let kind = if record.kind == RecordKind::Effect {
                "effect"
            } else {
                "answer"
            };
            let id = record.effect_id.expect("the record holds its effect's id");
            let payload = match record.payload.get("ToolCall") {
                Some(_) => json!("ToolCall"),
                None => record.payload.clone(),
            };
            json!([kind, id - call, payload])
        })
        .collect();
    (call, trail)
}

/// The effect record, as [`trail`] shows it, of the first write of
/// `files/pair`: the first effect after the call's own, naming `call`.
fn first_write(call: u64) -> Value {
    json!(["effect", 1, file_effect(call, &write_op("/first.md", "1"))])
}

/// The two records, as [`trail`] shows them, of the store write that
/// follows the dropped call in [`PAIRS`].
fn store_write_after() -> [Value; 2] {
    let effect = json!({ "Vfs": { "store": "0", "op": write_op("after.md", "kept") } });
    [
        json!(["effect", 2, effect]),
        json!(["answer", 2, { "Vfs": { "Ok": "Unit" } }]),
    ]
}

#[tokio::test]
async fn a_stop_raised_as_a_call_asks_drops_the_call_and_answers_its_file_effect_dropped_unperformed()
 {
    let run = pair_run(Raise::Stop, At::TheAsk).await;

    assert_eq!(
        run.result,
        Ok("false:canceled".to_owned()),
        "the pcall caught the dropped call and the run went on to its end"
    );
    let [store_effect, store_answer] = store_write_after();
    assert_eq!(
        run.trail,
        json!([
            ["effect", 0, "ToolCall"],
            ["answer", 0, "Dropped"],
            first_write(run.call),
            ["answer", 1, "Dropped"],
            store_effect,
            store_answer,
        ]),
        "the stop dropped the call, the file effect the run issued for the request the loop \
         had reported is answered Dropped, and the second request is never issued"
    );
    assert_eq!(run.written, (false, false), "neither write was performed");
}

#[tokio::test]
async fn a_stop_raised_as_a_file_effect_goes_out_performs_it_and_drops_the_calls_later_request() {
    let run = pair_run(Raise::Stop, At::TheFirstEffect).await;

    assert_eq!(
        run.result,
        Ok("false:canceled".to_owned()),
        "the pcall caught the dropped call and the run went on to its end"
    );
    let [store_effect, store_answer] = store_write_after();
    assert_eq!(
        run.trail,
        json!([
            ["effect", 0, "ToolCall"],
            first_write(run.call),
            ["answer", 1, { "VfsReceipt": { "Ok": "Done" } }],
            ["answer", 0, "Dropped"],
            store_effect,
            store_answer,
        ]),
        "the effect that had gone out is performed and answered with its receipt, then the \
         stop drops the call, and the second request is never issued"
    );
    assert_eq!(
        run.written,
        (true, false),
        "the first write was performed and the second was not"
    );
}

#[tokio::test]
async fn a_cancel_raised_as_a_call_asks_ends_the_run_canceled_with_its_file_effect_dropped_unperformed()
 {
    let run = pair_run(Raise::Cancel, At::TheAsk).await;

    assert_canceled(&run.result);
    assert_eq!(
        run.trail,
        json!([
            ["effect", 0, "ToolCall"],
            first_write(run.call),
            ["answer", 1, "Dropped"],
            ["answer", 0, "Dropped"],
        ]),
        "the file effect the run issued for the reported request is answered Dropped with \
         the call, and the second request is never issued"
    );
    assert_eq!(run.written, (false, false), "neither write was performed");
}

#[tokio::test]
async fn a_cancel_raised_as_a_file_effect_goes_out_performs_it_and_ends_the_run_canceled() {
    let run = pair_run(Raise::Cancel, At::TheFirstEffect).await;

    assert_canceled(&run.result);
    assert_eq!(
        run.trail,
        json!([
            ["effect", 0, "ToolCall"],
            first_write(run.call),
            ["answer", 1, { "VfsReceipt": { "Ok": "Done" } }],
            ["answer", 0, "Dropped"],
        ]),
        "the effect that had gone out is performed and answered with its receipt, then the \
         cancel drops the call, and the second request is never issued"
    );
    assert_eq!(
        run.written,
        (true, false),
        "the first write was performed and the second was not"
    );
}
