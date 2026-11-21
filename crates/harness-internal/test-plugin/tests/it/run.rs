//! A prompt calls the fixture library's tools through the real Harness:
//! the Pack the library inserted is declared, each run gets its own
//! instance, a call that takes several steps finishes once the library
//! has woken it for each one, from a step or from a thread of its own,
//! each call is lent the Host's resources as they were when it started,
//! each file operation of a tool is performed
//! on the run's files and recorded as a file effect that names the
//! call, an operation the policy denies reaches the tool as a file
//! error, two tasks that write one path both succeed, and a canceled run
//! cancels its parked call and deletes its instance. The fixture's
//! counters show what the library still holds and how many steps it
//! got.

use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use harness_gateway_client::TokioTimer;
use harness_plugin_loader::{HostResources, PluginReport, insert_plugin_dir};
use runvil::harness::{
    FindTextPrompt, MemoryRecorder, OfflineBroker, Packs, Record, RecordKind, RunId, RunMeta,
    RunRecorder,
};
use runvil::vfs::{MemoryBackend, Op, Origin, Policy, Verdict, VfsError, VfsPath, VfsRef};
use runvil::{BoxFuture, Error, Harness, Host, RunControl, RunReport, RunSetup};
use runvil_plugin::PackRegistry;
use serde_json::{Value, json};

use crate::support::{already_loaded, copy_fixture, counter, remove, scratch};

/// The name each Host finds its prompt by.
const PROMPT: &str = "prompt.md";

/// A prompt that declares the fixture's Pack and holds `sections`.
fn prompt_of(sections: &str) -> String {
    format!(
        "---\nname: fixture\ndescription: calls the fixture's tools\nrunvil: 0\n\
         uses:\n  - fixture/test\n---\n\n{sections}"
    )
}

/// A prompt that declares the fixture's Pack and runs `lua`.
fn prompt(lua: &str) -> String {
    prompt_of(&format!("## Only\n\n```lua\n{lua}\n```\n"))
}

/// A Host over the library Plugins in `dir` that offers them no
/// resources and finds `source` by [`PROMPT`], and the report of what
/// `dir` loaded.
fn host_over(dir: &Path, source: String) -> (Arc<Host>, PluginReport) {
    host_offering(dir, source, &HostResources::new())
}

/// A Host over the library Plugins in `dir` that offers them `resources`
/// and finds `source` by [`PROMPT`], and the report of what `dir` loaded.
fn host_offering(
    dir: &Path,
    source: String,
    resources: &HostResources,
) -> (Arc<Host>, PluginReport) {
    let mut packs = PackRegistry::new();
    let report = insert_plugin_dir(&mut packs, dir, resources).expect("the directory reads");
    assert!(report.failures().is_empty(), "{:?}", report.failures());
    let host = Arc::new(Host {
        packs: Packs::new(packs),
        prompts: FindTextPrompt::new(PROMPT, source),
    });
    (host, report)
}

/// A Harness for one run of the Host's prompt.
fn harness(host: &Arc<Host>) -> Harness {
    harness_over(host, VfsRef::default())
}

/// A Harness for one run of the Host's prompt over the files in `vfs`.
fn harness_over(host: &Arc<Host>, vfs: VfsRef) -> Harness {
    harness_recorded(host, vfs, Arc::new(MemoryRecorder::new()))
}

/// A Harness for one run of the Host's prompt over the files in `vfs`,
/// which records at `recorder`.
fn harness_recorded(host: &Arc<Host>, vfs: VfsRef, recorder: Arc<dyn RunRecorder>) -> Harness {
    let prompt = host.parse(PROMPT).expect("the prompt parses");
    let setup = RunSetup::new(recorder, Arc::new(OfflineBroker), Arc::new(TokioTimer)).vfs(vfs);
    Harness::new(Arc::clone(host), prompt, setup)
}

/// How long a run that waits on a wake of the library may take. The wake
/// comes far sooner, so a run that has not ended by then waits on a wake
/// that is missing.
const WAKE_LIMIT: Duration = Duration::from_secs(60);

/// Runs `harness` to its end. A run whose tool call the library parked
/// and never woke would wait for good: it fails its test after 60
/// seconds, with a message that says so, and does not hang it.
async fn run_woken(harness: Harness) -> RunReport {
    tokio::time::timeout(WAKE_LIMIT, harness.run())
        .await
        .expect(
            "the run ends within 60 seconds: a tool call whose step returned RV_PENDING with no \
             file request goes on only after the library wakes it",
        )
        .expect("the recorder takes the run")
}

/// The text of the file at `path` in `vfs`, read once a run has ended.
fn text_at(vfs: &VfsRef, path: &str) -> Result<String, VfsError> {
    vfs.acquire(Origin::new("fixture check"))
        .expect("a memory filesystem acquires")
        .read_string(path)
}

/// Whether `record` is the effect record of a file operation that a tool
/// call asked for.
fn is_call_file_effect(record: &Record) -> bool {
    record.kind == RecordKind::Effect && record.payload["Vfs"].get("call").is_some()
}

/// The answer record of the effect `effect` in `records`, as its payload.
fn answer_to(records: &[Record], effect: &Record) -> Value {
    records
        .iter()
        .find(|record| record.kind == RecordKind::Answer && record.effect_id == effect.effect_id)
        .map(|record| record.payload.clone())
        .expect("every effect record has an answer record")
}

/// For each tool call of `records`, in call order: the operation of each
/// file effect between the call's effect record and its answer record,
/// beside the receipt the operation was answered with. Each of those
/// file effects must name the call.
fn asked_by_each_call(records: &[Record]) -> Vec<Vec<(Value, Value)>> {
    let is_call = |record: &Record| {
        record.kind == RecordKind::Effect && record.payload.get("ToolCall").is_some()
    };
    records
        .iter()
        .enumerate()
        .filter(|(_, record)| is_call(record))
        .map(|(position, call)| {
            let id = call.effect_id.expect("an effect record holds its id");
            let ends_call =
                |record: &Record| record.kind == RecordKind::Answer && record.effect_id == Some(id);
            records[position + 1..]
                .iter()
                .take_while(|record| !ends_call(record))
                .filter(|record| record.kind == RecordKind::Effect)
                .map(|effect| {
                    assert_eq!(
                        effect.payload["Vfs"]["call"],
                        json!(id),
                        "a file effect inside a call names that call: {}",
                        effect.payload
                    );
                    let receipt = answer_to(records, effect)["VfsReceipt"].clone();
                    (effect.payload["Vfs"]["op"].clone(), receipt)
                })
                .collect()
        })
        .collect()
}

#[tokio::test]
async fn a_prompt_calls_a_one_step_tool_and_a_three_step_tool_of_the_library() {
    let dir = scratch("a_prompt_calls_the_library_tools");
    let file =
        std::path::absolute(copy_fixture(&dir, "plugin")).expect("the working directory is known");
    let lua = "local hello = tools.call('test/hello', { name = 'x' })\n\
               local steps = tools.call('test/steps', {})\n\
               return hello .. '|' .. steps";
    let (host, report) = host_over(&dir, prompt(lua));
    let library = already_loaded(&file).expect("the loader holds the library open");

    for run in 1..=2 {
        let report = run_woken(harness(&host)).await;
        assert_eq!(
            report.result,
            Ok(r#"hello {"name":"x"}|3 steps"#.to_owned()),
            "run {run}"
        );
        assert_eq!(
            (counter(&library, "packs"), counter(&library, "calls")),
            (0, 0),
            "run {run}: the run's Pack instance and its calls' state are gone when it ends"
        );
        assert_eq!(
            counter(&library, "steps"),
            4 * run,
            "run {run}: one step for `hello`, and for `steps` its first step and one more for \
             each of its two wakes"
        );
    }

    drop(report);
    assert_eq!(
        counter(&library, "live"),
        1,
        "the Host's Packs keep the Plugin loaded"
    );
    drop(host);
    assert_eq!(
        (counter(&library, "unloads"), counter(&library, "live")),
        (1, 0)
    );
    drop(library);
    remove(&dir);
}

#[tokio::test]
async fn each_call_of_a_library_tool_is_lent_the_hosts_resources_as_they_were_when_it_started() {
    let dir = scratch("each_call_is_lent_the_hosts_resources");
    let file =
        std::path::absolute(copy_fixture(&dir, "plugin")).expect("the working directory is known");
    // The Host's function counts how often it is asked, so each reading
    // gives another value. A fixed resource stands beside it.
    let reads = Arc::new(AtomicU32::new(0));
    let counted = Arc::clone(&reads);
    let mut resources = HostResources::new();
    resources
        .provide("fixture/reading", move || {
            Some(json!(counted.fetch_add(1, Ordering::SeqCst) + 1))
        })
        .expect("the name is a vendor and a name");
    resources
        .provide("acme/fixed", || {
            Some(json!({ "url": "http://127.0.0.1:9/" }))
        })
        .expect("the name is a vendor and a name");
    let lua = "local first = tools.call('test/resources', {})\n\
               local second = tools.call('test/resources', {})\n\
               return first .. '|' .. second";
    let (host, report) = host_offering(&dir, prompt(lua), &resources);
    let library = already_loaded(&file).expect("the loader holds the library open");
    assert_eq!(
        reads.load(Ordering::SeqCst),
        0,
        "loading the library reads no resource"
    );

    let report_of_run = run_woken(harness(&host)).await;
    // The tool answers in its second step, with what that step is lent.
    // Each call saw one reading, the one made as the call started.
    assert_eq!(
        report_of_run.result,
        Ok([
            r#"{"acme/fixed":{"url":"http://127.0.0.1:9/"},"fixture/reading":1}"#,
            r#"{"acme/fixed":{"url":"http://127.0.0.1:9/"},"fixture/reading":2}"#,
        ]
        .join("|"))
    );
    assert_eq!(
        reads.load(Ordering::SeqCst),
        2,
        "the Host's function was asked once for each call, and not for each step"
    );
    assert_eq!(
        (counter(&library, "packs"), counter(&library, "calls")),
        (0, 0)
    );

    drop((host, report));
    assert_eq!(counter(&library, "live"), 0);
    drop(library);
    remove(&dir);
}

#[tokio::test]
async fn a_call_that_a_thread_of_the_library_wakes_stays_parked_until_then_and_finishes() {
    let dir = scratch("a_call_that_a_thread_of_the_library_wakes");
    let file =
        std::path::absolute(copy_fixture(&dir, "plugin")).expect("the working directory is known");
    let (host, report) = host_over(&dir, prompt("return tools.call('test/later', {})"));
    let library = already_loaded(&file).expect("the loader holds the library open");

    // The first step of `later` starts a thread in the library, which
    // wakes the call a little later, from outside this test's runtime.
    // Nothing else is due meanwhile, so the run goes on only when that
    // wake reaches the task that polls it.
    let report_of_run = run_woken(harness(&host)).await;
    assert_eq!(
        report_of_run.result,
        Ok("woken".to_owned()),
        "the call got its second step after the thread's wake, and not before"
    );
    assert_eq!(
        counter(&library, "steps"),
        2,
        "the call got its first step and the one its wake asked for"
    );
    assert_eq!(
        (counter(&library, "packs"), counter(&library, "calls")),
        (0, 0),
        "the call's state is gone, and with it the thread the call started"
    );

    drop((host, report));
    assert_eq!(counter(&library, "live"), 0);
    drop(library);
    remove(&dir);
}

#[tokio::test]
async fn a_prompt_reads_the_files_of_its_run_through_the_librarys_file_tools() {
    let dir = scratch("a_prompt_reads_the_files_of_its_run");
    let file =
        std::path::absolute(copy_fixture(&dir, "plugin")).expect("the working directory is known");
    let lua = "local function tool(name, path)\n\
               \x20 return tools.call('test/' .. name, { path = path })\n\
               end\n\
               return table.concat({\n\
               \x20 tool('stat', '/notes/a.md'), tool('stat', '/notes'),\n\
               \x20 tool('list', '/notes'), tool('read', 'notes/a.md'),\n\
               \x20 tool('read', '/absent'), tool('list', '/notes/a.md'),\n\
               \x20 tool('read', '/notes'),\n\
               }, '|')";
    let (host, report) = host_over(&dir, prompt(lua));
    let library = already_loaded(&file).expect("the loader holds the library open");

    let vfs = VfsRef::default();
    let seed = vfs
        .acquire(Origin::new("fixture seed"))
        .expect("a fresh memory filesystem acquires");
    seed.write("/notes/a.md", b"hello")
        .expect("a seeded file is written");
    seed.write("/notes/sub/b.md", b"")
        .expect("a seeded file is written");
    drop(seed);

    let report_of_run = harness_over(&host, vfs)
        .run()
        .await
        .expect("the recorder takes the run");
    assert_eq!(
        report_of_run.result,
        Ok([
            "file 5",
            "directory",
            "a.md,sub/",
            "hello",
            "failed (1): not found: /absent",
            "failed (2): not a directory: /notes/a.md",
            "failed (3): is a directory: /notes",
        ]
        .join("|"))
    );
    assert_eq!(
        (counter(&library, "packs"), counter(&library, "calls")),
        (0, 0)
    );

    drop((host, report));
    assert_eq!(counter(&library, "live"), 0);
    drop(library);
    remove(&dir);
}

/// The receipt `receipt` with the stat of a `Stat` cut down to its file
/// type and its size, which are all a test here states about one.
fn plain(mut receipt: Value) -> Value {
    if let Some(stat) = receipt["Ok"].get("Stat").cloned() {
        receipt["Ok"]["Stat"] = json!({ "file_type": stat["file_type"], "size": stat["size"] });
    }
    receipt
}

/// The Lua of a prompt that calls each file tool of the fixture, the
/// last three for an operation that fails, and returns the answers.
const EVERY_OPERATION: &str = "local function tool(name, args)\n\
    \x20 return tools.call('test/' .. name, args)\n\
    end\n\
    return table.concat({\n\
    \x20 tool('write', { path = '/out/a.md', text = 'one' }),\n\
    \x20 tool('append', { path = '/out/a.md', text = ' two' }),\n\
    \x20 tool('mkdir', { path = '/made/deep', recursive = true }),\n\
    \x20 tool('copy', { from = '/out/a.md', to = '/out/b.md' }),\n\
    \x20 tool('rename', { from = '/out/b.md', to = '/moved/c.md' }),\n\
    \x20 tool('read', { path = '/moved/c.md' }),\n\
    \x20 tool('stat', { path = '/moved/c.md' }),\n\
    \x20 tool('list', { path = '/out' }),\n\
    \x20 tool('remove', { path = '/made', recursive = true }),\n\
    \x20 tool('mkdir', { path = '/out', recursive = true }),\n\
    \x20 tool('remove', { path = '/absent' }),\n\
    \x20 tool('remove', { path = '/out' }),\n\
    }, '|')";

/// What the calls of [`EVERY_OPERATION`] answer, in order.
const EVERY_ANSWER: [&str; 12] = [
    "done",
    "done",
    "done",
    "done",
    "done",
    "one two",
    "file 7",
    "a.md",
    "done",
    "failed (7): already exists: /out",
    "failed (1): not found: /absent",
    "failed (8): directory not empty: /out",
];

/// What the record holds of the calls of [`EVERY_OPERATION`], in order:
/// the operation each call asked for, beside its receipt or its failure.
fn every_recorded_operation() -> [(Value, Value); 12] {
    let done = || json!({ "Ok": "Done" });
    let failed = |kind: &str, message: &str| json!({ "Err": { "kind": kind, "message": message } });
    [
        (
            json!({ "Write": { "path": "/out/a.md", "contents": "one" } }),
            done(),
        ),
        (
            json!({ "Append": { "path": "/out/a.md", "contents": " two" } }),
            done(),
        ),
        (
            json!({ "Mkdir": { "path": "/made/deep", "recursive": true } }),
            done(),
        ),
        (
            json!({ "Copy": { "from": "/out/a.md", "to": "/out/b.md" } }),
            done(),
        ),
        (
            json!({ "Rename": { "from": "/out/b.md", "to": "/moved/c.md" } }),
            done(),
        ),
        (
            json!({ "ReadRange": { "path": "/moved/c.md", "offset": 0, "len": u64::MAX } }),
            json!({ "Ok": { "Bytes": 7 } }),
        ),
        (
            json!({ "Stat": { "path": "/moved/c.md" } }),
            json!({ "Ok": { "Stat": { "file_type": "file", "size": 7 } } }),
        ),
        (
            json!({ "List": { "path": "/out" } }),
            json!({ "Ok": { "Count": 1 } }),
        ),
        (
            json!({ "Remove": { "path": "/made", "recursive": true } }),
            done(),
        ),
        (
            json!({ "Mkdir": { "path": "/out", "recursive": true } }),
            failed("already_exists", "already exists: /out"),
        ),
        (
            json!({ "Remove": { "path": "/absent", "recursive": false } }),
            failed("not_found", "not found: /absent"),
        ),
        (
            json!({ "Remove": { "path": "/out", "recursive": false } }),
            failed("directory_not_empty", "directory not empty: /out"),
        ),
    ]
}

#[tokio::test]
async fn each_file_operation_of_a_library_tool_is_performed_on_the_runs_files_and_recorded() {
    let dir = scratch("each_file_operation_of_a_library_tool");
    let file =
        std::path::absolute(copy_fixture(&dir, "plugin")).expect("the working directory is known");
    let (host, report) = host_over(&dir, prompt(EVERY_OPERATION));
    let library = already_loaded(&file).expect("the loader holds the library open");

    let vfs = VfsRef::default();
    let recorder = Arc::new(MemoryRecorder::new());
    let report_of_run = harness_recorded(&host, vfs.clone(), recorder.clone())
        .run()
        .await
        .expect("the recorder takes the run");
    assert_eq!(
        report_of_run.result,
        Ok(EVERY_ANSWER.join("|")),
        "each tool got the answer to its request, a failure included, and the run went on"
    );
    assert_eq!(text_at(&vfs, "/out/a.md"), Ok("one two".to_owned()));
    assert_eq!(text_at(&vfs, "/moved/c.md"), Ok("one two".to_owned()));
    assert_eq!(
        text_at(&vfs, "/out/b.md"),
        Err(VfsError::NotFound {
            path: "/out/b.md".to_owned()
        }),
        "the rename left nothing behind"
    );
    let files = vfs
        .acquire(Origin::new("fixture check"))
        .expect("a memory filesystem acquires");
    assert_eq!(files.exists("/made"), Ok(false), "the remove took the tree");

    let asked: Vec<Vec<(Value, Value)>> =
        asked_by_each_call(&recorder.records(report_of_run.run_id))
            .into_iter()
            .map(|call| {
                call.into_iter()
                    .map(|(op, receipt)| (op, plain(receipt)))
                    .collect()
            })
            .collect();
    assert_eq!(
        asked,
        every_recorded_operation().map(|asked| vec![asked]),
        "each call asked for one operation, which is a file effect that names the call, \
         answered with a receipt or the failure, inside the call"
    );
    assert_eq!(
        (counter(&library, "packs"), counter(&library, "calls")),
        (0, 0)
    );

    drop((host, report));
    assert_eq!(counter(&library, "live"), 0);
    drop(library);
    remove(&dir);
}

/// Denies every read under `/secret` and every change under `/locked`,
/// and panics on every read under `/broken`. A read here is a `Read`, a
/// `List`, or a `Stat`. The file system asks about the source of a copy
/// or a rename as a `Read` as well, so the first arm also refuses a copy
/// and a rename out of `/secret`. A write elsewhere passes, so a test
/// can seed what it reads.
struct Guard;

impl Policy for Guard {
    fn check(&self, op: Op, path: &VfsPath) -> Verdict {
        let path = path.as_str();
        match op {
            Op::Read | Op::List | Op::Stat if path.starts_with("/secret") => {
                Verdict::Deny("This run may not read under /secret.".to_owned())
            }
            Op::Read | Op::List | Op::Stat => {
                assert!(!path.starts_with("/broken"), "the policy broke on {path}");
                Verdict::Allow
            }
            Op::Write | Op::Append | Op::Delete | Op::Rename | Op::Mkdir | Op::Copy
                if path.starts_with("/locked") =>
            {
                Verdict::Deny("This run may not change files under /locked.".to_owned())
            }
            _ => Verdict::Allow,
        }
    }
}

/// A receipt that holds a success alone as the record holds it, a
/// receipt that holds a failure alone as the kind of that failure, and
/// anything else as null.
fn kind_or_receipt(receipt: Value) -> Value {
    match (receipt.get("Ok"), receipt.get("Err")) {
        (Some(_), None) => receipt,
        (ok, err) => ok.xor(err).map_or(Value::Null, |it| it["kind"].clone()),
    }
}

#[tokio::test]
async fn an_operation_the_policy_denies_or_panics_on_is_a_file_error_and_the_run_goes_on() {
    let dir = scratch("an_operation_the_policy_denies_or_panics_on");
    let file =
        std::path::absolute(copy_fixture(&dir, "plugin")).expect("the working directory is known");
    let lua = "local function tool(name, args)\n\
               \x20 return tools.call('test/' .. name, args)\n\
               end\n\
               return table.concat({\n\
               \x20 tool('read', { path = '/secret/key' }),\n\
               \x20 tool('stat', { path = '/secret/key' }),\n\
               \x20 tool('list', { path = '/secret' }),\n\
               \x20 tool('copy', { from = '/secret/key', to = '/notes/key' }),\n\
               \x20 tool('rename', { from = '/secret/key', to = '/notes/moved' }),\n\
               \x20 tool('write', { path = '/locked/a.md', text = 'x' }),\n\
               \x20 tool('mkdir', { path = '/locked/made', recursive = true }),\n\
               \x20 tool('rename', { from = '/notes/a.md', to = '/locked/a.md' }),\n\
               \x20 tool('remove', { path = '/locked', recursive = true }),\n\
               \x20 tool('read', { path = '/broken/a.md' }),\n\
               \x20 tool('read', { path = '/notes/a.md' }),\n\
               }, '|')";
    let (host, report) = host_over(&dir, prompt(lua));
    let library = already_loaded(&file).expect("the loader holds the library open");

    let vfs = VfsRef::builder()
        .mount("/", MemoryBackend::new())
        .policy(Guard)
        .build();
    let seed = vfs
        .acquire(Origin::new("fixture seed"))
        .expect("a fresh memory filesystem acquires");
    for path in ["/secret/key", "/broken/a.md", "/notes/a.md"] {
        seed.write(path, b"hello")
            .expect("a seeded file is written");
    }
    drop(seed);

    let recorder = Arc::new(MemoryRecorder::new());
    let report_of_run = harness_recorded(&host, vfs.clone(), recorder.clone())
        .run()
        .await
        .expect("the recorder takes the run");
    let unread = "failed (4): This run may not read under /secret.";
    let locked = "failed (4): This run may not change files under /locked.";
    let panicked = "failed (0): the file operation panicked";
    assert_eq!(
        report_of_run.result,
        Ok([&[unread; 5][..], &[locked; 4], &[panicked, "hello"]]
            .concat()
            .join("|")),
        "a copy and a rename out of /secret get the denial of the read of their source"
    );
    let files = vfs
        .acquire(Origin::new("fixture check"))
        .expect("a memory filesystem acquires");
    assert_eq!(
        (files.exists("/locked"), files.exists("/notes/a.md")),
        (Ok(false), Ok(true)),
        "no denied change was performed"
    );
    assert_eq!(
        ["/notes/key", "/notes/moved", "/secret/key"].map(|path| files.exists(path)),
        [Ok(false), Ok(false), Ok(true)],
        "the denied copy and the denied rename made nothing and moved nothing"
    );
    let receipts: Vec<Value> = asked_by_each_call(&recorder.records(report_of_run.run_id))
        .into_iter()
        .flatten()
        .map(|(_, receipt)| kind_or_receipt(receipt))
        .collect();
    let mut expected = vec![json!("permission_denied"); 9];
    expected.extend([json!("backend"), json!({ "Ok": { "Bytes": 5 } })]);
    assert_eq!(
        receipts, expected,
        "each refused operation with the kind of its failure, and the read with its receipt"
    );
    assert_eq!(
        (counter(&library, "packs"), counter(&library, "calls")),
        (0, 0)
    );

    drop((host, report));
    assert_eq!(counter(&library, "live"), 0);
    drop(library);
    remove(&dir);
}

#[tokio::test]
async fn two_tasks_that_write_one_path_both_succeed_and_the_later_write_is_the_content() {
    let dir = scratch("two_tasks_that_write_one_path");
    let file =
        std::path::absolute(copy_fixture(&dir, "plugin")).expect("the working directory is known");
    let writer = |name: &str, text: &str| {
        format!(
            "## {name}\n\n```lua\n\
             return tools.call('test/write', {{ path = '/shared.md', text = '{text}' }})\n```\n\n"
        )
    };
    let sections = [
        "## Main\n\n```lua\n\
         local left = tasks.spawn('../Left')\n\
         local right = tasks.spawn('../Right')\n\
         local done = tasks.join({ left, right })\n\
         return done[1].result .. '|' .. done[2].result\n```\n\n"
            .to_owned(),
        writer("Left", "left"),
        writer("Right", "right"),
    ]
    .concat();
    let (host, report) = host_over(&dir, prompt_of(&sections));
    let library = already_loaded(&file).expect("the loader holds the library open");

    let vfs = VfsRef::default();
    vfs.acquire(Origin::new("fixture seed"))
        .expect("a fresh memory filesystem acquires")
        .write("/shared.md", b"old")
        .expect("a seeded file is written");
    let recorder = Arc::new(MemoryRecorder::new());
    let report_of_run = harness_recorded(&host, vfs.clone(), recorder.clone())
        .run()
        .await
        .expect("the recorder takes the run");
    assert_eq!(
        report_of_run.result,
        Ok("done|done".to_owned()),
        "the write of each task succeeded"
    );
    let records = recorder.records(report_of_run.run_id);
    let written: Vec<String> = records
        .iter()
        .filter(|record| is_call_file_effect(record))
        .filter(|effect| {
            answer_to(&records, effect)["VfsReceipt"]
                .get("Ok")
                .is_some()
        })
        .filter_map(|effect| {
            let write = &effect.payload["Vfs"]["op"]["Write"];
            if write["path"] != "/shared.md" {
                return None;
            }
            write["contents"].as_str().map(str::to_owned)
        })
        .collect();
    let mut both = written.clone();
    both.sort();
    assert_eq!(
        both,
        ["left", "right"],
        "the Harness performed both writes, though nothing orders the two tasks"
    );
    assert_eq!(
        text_at(&vfs, "/shared.md").ok().as_ref(),
        written.last(),
        "the later write is the content"
    );
    assert_eq!(
        (counter(&library, "packs"), counter(&library, "calls")),
        (0, 0)
    );

    drop((host, report));
    assert_eq!(counter(&library, "live"), 0);
    drop(library);
    remove(&dir);
}

#[tokio::test]
async fn a_canceled_run_cancels_its_parked_call_and_deletes_its_pack_instance() {
    /// How many turns of the loop below the call stays parked for before
    /// the run is canceled. The run is polled in each turn.
    const PARKED_TURNS: u32 = 50;

    let dir = scratch("a_canceled_run_cancels_its_parked_call");
    let file =
        std::path::absolute(copy_fixture(&dir, "plugin")).expect("the working directory is known");
    let (host, report) = host_over(&dir, prompt("return tools.call('test/never', {})"));
    let library = already_loaded(&file).expect("the loader holds the library open");

    let harness = harness(&host);
    let control = harness.control();
    let mut run = std::pin::pin!(harness.run());
    let mut parked_turns = 0;
    let mut canceled = false;
    let report_of_run = loop {
        tokio::select! {
            biased;
            report = &mut run => break report.expect("the recorder takes the run"),
            () = tokio::task::yield_now() => {
                // The call holds state from its first step on, and it is
                // parked, so the run cannot end by itself.
                if !canceled && counter(&library, "calls") == 1 {
                    parked_turns += 1;
                }
                if !canceled && parked_turns == PARKED_TURNS {
                    assert_eq!(counter(&library, "packs"), 1, "the run holds its instance");
                    assert_eq!(
                        counter(&library, "steps"),
                        1,
                        "the `never` tool wakes nothing, so its call got its first step and no \
                         other, however often the run was polled"
                    );
                    control.cancel();
                    canceled = true;
                }
            }
        }
    };
    assert!(canceled, "the call was parked when the run was canceled");
    assert_eq!(
        counter(&library, "steps"),
        1,
        "the cancel sent the call no step"
    );
    assert!(
        report_of_run.result.as_ref().is_err_and(Error::is_canceled),
        "the run ends canceled: {:?}",
        report_of_run.result
    );
    assert_eq!(
        (counter(&library, "calls"), counter(&library, "packs")),
        (0, 0),
        "the cancel freed the call's state, and the run's end deleted its instance"
    );

    drop((host, report));
    assert_eq!(counter(&library, "live"), 0);
    drop(library);
    remove(&dir);
}

/// Records at a [`MemoryRecorder`], and cancels its run as it appends the
/// effect record of a file operation that a tool call asked for: after
/// the call's request, and before the operation is performed and
/// answered.
struct CancelsAtFileEffect {
    records: Arc<MemoryRecorder>,
    /// The control of the run this recorder records, once the test has
    /// it.
    control: OnceLock<RunControl>,
}

impl RunRecorder for CancelsAtFileEffect {
    fn begin_run(&self, meta: RunMeta) -> BoxFuture<'_, Result<RunId, Error>> {
        self.records.begin_run(meta)
    }

    fn append(&self, run: RunId, record: Record) -> BoxFuture<'_, Result<(), Error>> {
        if is_call_file_effect(&record) {
            self.control
                .get()
                .expect("the run's control is set before the run starts")
                .cancel();
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

#[tokio::test]
async fn a_cancel_between_a_request_and_its_answer_leaves_the_operation_done_and_cancels_the_call()
{
    let dir = scratch("a_cancel_between_a_request_and_its_answer");
    let file =
        std::path::absolute(copy_fixture(&dir, "plugin")).expect("the working directory is known");
    let lua = "return tools.call('test/write', { path = '/out.md', text = 'kept' })";
    let (host, report) = host_over(&dir, prompt(lua));
    let library = already_loaded(&file).expect("the loader holds the library open");

    let vfs = VfsRef::default();
    let records = Arc::new(MemoryRecorder::new());
    let recorder = Arc::new(CancelsAtFileEffect {
        records: Arc::clone(&records),
        control: OnceLock::new(),
    });
    let harness = harness_recorded(&host, vfs.clone(), recorder.clone());
    assert!(
        recorder.control.set(harness.control()).is_ok(),
        "the control is set once"
    );
    let report_of_run = harness.run().await.expect("the recorder takes the run");

    assert!(
        report_of_run.result.as_ref().is_err_and(Error::is_canceled),
        "the run ends canceled: {:?}",
        report_of_run.result
    );
    // Each effect and answer record of the run, in order: the call's own
    // effect as `ToolCall`, a file effect as its operation, and an
    // answer as its payload.
    let trail: Vec<Value> = records
        .records(report_of_run.run_id)
        .iter()
        .filter(|record| record.kind != RecordKind::Event)
        .map(|record| {
            if record.payload.get("ToolCall").is_some() {
                json!(["effect", "ToolCall"])
            } else if record.kind == RecordKind::Effect {
                json!(["effect", record.payload["Vfs"]["op"]])
            } else {
                json!(["answer", record.payload])
            }
        })
        .collect();
    assert_eq!(
        Value::Array(trail),
        json!([
            ["effect", "ToolCall"],
            ["effect", { "Write": { "path": "/out.md", "contents": "kept" } }],
            ["answer", { "VfsReceipt": { "Ok": "Done" } }],
            ["answer", "Dropped"],
        ]),
        "the operation the Harness had taken is performed and answered with its receipt, \
         and then the call is dropped, before a step is lent the answer"
    );
    assert_eq!(
        text_at(&vfs, "/out.md"),
        Ok("kept".to_owned()),
        "the operation happened in full"
    );
    assert_eq!(
        (counter(&library, "calls"), counter(&library, "packs")),
        (0, 0),
        "the cancel freed the call's state, and the run's end deleted its instance"
    );

    drop((host, report));
    assert_eq!(counter(&library, "live"), 0);
    drop(library);
    remove(&dir);
}

/// How many files the measurement reads.
const READS: u32 = 100_000;

/// What each of those files holds.
const SMALL_FILE: &[u8] = b"one small file\n";

/// The measurement of file operations as effects: one call of the
/// fixture's `read_many` tool reads [`READS`] small files, each read a
/// file request that the Harness performs and records. It prints the
/// elapsed time of the run and the count of its records. Run it with
/// `cargo nextest run -p harness-test-plugin --run-ignored ignored-only
/// --no-capture`, in the `release` profile for the numbers of a release
/// build.
#[tokio::test]
#[ignore = "a measurement that prints its numbers, not a check"]
async fn measure_100_000_reads_of_one_library_tool_call_under_the_harness() {
    let dir = scratch("measure_100_000_reads");
    let file =
        std::path::absolute(copy_fixture(&dir, "plugin")).expect("the working directory is known");
    let lua = format!("return tools.call('test/read_many', {{ path = '/many', count = {READS} }})");
    let (host, report) = host_over(&dir, prompt(&lua));
    let library = already_loaded(&file).expect("the loader holds the library open");

    let vfs = VfsRef::default();
    let seed = vfs
        .acquire(Origin::new("measurement seed"))
        .expect("a fresh memory filesystem acquires");
    for index in 0..READS {
        seed.write(&format!("/many/{index}"), SMALL_FILE)
            .expect("a seeded file is written");
    }
    drop(seed);

    let recorder = Arc::new(MemoryRecorder::new());
    let harness = harness_recorded(&host, vfs, recorder.clone());
    let started = Instant::now();
    let report_of_run = harness.run().await.expect("the recorder takes the run");
    let elapsed = started.elapsed();

    let bytes = u64::from(READS) * u64::try_from(SMALL_FILE.len()).expect("a small length");
    assert_eq!(
        report_of_run.result,
        Ok(format!("read {READS} files, {bytes} bytes, 0 failed"))
    );
    let records = recorder.records(report_of_run.run_id);
    let file_effects = records
        .iter()
        .filter(|record| is_call_file_effect(record))
        .count();
    assert_eq!(
        u32::try_from(file_effects).expect("a count of records"),
        READS,
        "each read is one file effect"
    );
    let per_read = (elapsed / READS).as_secs_f64() * 1e6;
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    println!(
        "measurement ({profile} build): {READS} reads in one tool call took {elapsed:.3?}, \
         {per_read:.1} microseconds for each read, and the run has {} records",
        records.len()
    );

    drop((host, report));
    assert_eq!(counter(&library, "live"), 0);
    drop(library);
    remove(&dir);
}
