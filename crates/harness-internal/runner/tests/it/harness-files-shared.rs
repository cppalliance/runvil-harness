//! One path used by two parties of a run. The Host's files are outside
//! the claims check: two tasks whose models write one path both succeed and
//! the later write is the content, a task whose model reads a path holds
//! back no write of another task, and a prompt's own `tools.call` and a
//! model's call on one path both succeed. A store has the check: two
//! tasks that write one path of the run's store end the run with a
//! determinism error.

use std::sync::Arc;

use harness_runner::RunReport;
use harness_runner::recorder::{MemoryRecorder, Record, RecordKind};
use runvil_engine::ErrorCode;
use runvil_engine::model::{Completion, CompletionResult, Role, ToolCall};
use runvil_engine::vfs::{Origin, VfsRef};
use serde_json::json;

use super::files::files_host;
use super::{launch, setup};
use crate::scripted::{MODEL, ScriptedBroker, reply};

/// The frontmatter every prompt here opens with: it requires the fixture
/// Pack.
const FRONT: &str = "---\nname: shares\ndescription: d\nrunvil: 0\n\
    uses:\n  - tests/files\n---\n\n";

/// A main section that spawns Left and Right, joins both, and returns
/// their two results.
const JOINS_BOTH: &str = "## Main\n\n```lua\n\
    local left = tasks.spawn('../Left')\n\
    local right = tasks.spawn('../Right')\n\
    local done = tasks.join({ left, right })\n\
    return done[1].result .. '|' .. done[2].result\n```\n\n";

/// A section `name` that offers `tool` to a model loop over the one user
/// message `order`, and returns the tool's result as the model got it.
fn worker(name: &str, tool: &str, order: &str) -> String {
    format!(
        "## {name}\n\n```lua\n\
         tools.offer('{tool}')\n\
         messages:user('{order}')\n\
         models.loop()\n\
         return messages[3].content\n```\n\n"
    )
}

/// A broker whose model obeys the user message of each loop, an order of
/// the form `<wire name> <path> <text>`: the loop's first round calls
/// that tool with that path and text, and its next round replies `done`.
fn obeying() -> ScriptedBroker {
    ScriptedBroker::new(|_round, messages| {
        let called = messages.iter().any(|message| message.role() == Role::Tool);
        let order = messages
            .iter()
            .find(|message| message.role() == Role::User)
            .map(|message| message.content().to_owned())
            .unwrap_or_default();
        Box::pin(async move {
            if called {
                return reply("done");
            }
            let mut words = order.split(' ');
            let mut word = || words.next().unwrap_or_default().to_owned();
            let (tool, path, text) = (word(), word(), word());
            let args = json!({ "path": path, "text": text });
            let call = ToolCall::from_parts(format!("call-{text}"), tool, args)?;
            Completion::from_result(CompletionResult::ToolCalls(vec![call]), MODEL).map(Box::new)
        })
    })
}

/// Runs the prompt of `sections` on a Host holding the fixture Pack, over
/// a filesystem whose `/shared.md` holds `old`. Returns the run's report
/// and records beside the filesystem.
async fn shared_run(sections: &str) -> (RunReport, Vec<Record>, VfsRef) {
    let vfs = VfsRef::default();
    vfs.acquire(Origin::new("shared files seed"))
        .unwrap()
        .write("/shared.md", b"old")
        .unwrap();
    let recorder = Arc::new(MemoryRecorder::new());
    let harness = launch(
        files_host(|| ()),
        &format!("{FRONT}{sections}"),
        setup(recorder.clone(), obeying()).vfs(vfs.clone()),
    );

    let report = harness.run().await.expect("the run is reported");

    let records = recorder.records(report.run_id);
    (report, records, vfs)
}

/// The text of `/shared.md` in `vfs`, read once the run has ended.
fn shared_text(vfs: &VfsRef) -> String {
    vfs.acquire(Origin::new("shared files test"))
        .unwrap()
        .read_string("/shared.md")
        .unwrap()
}

/// What the tool calls of `records` did to `/shared.md`, in record order:
/// `read` for a read the loop performed, and `write <text>` for a write
/// it performed. An operation the loop refused or dropped is left out.
fn done_to_shared(records: &[Record]) -> Vec<String> {
    let performed = |effect: &Record| {
        records.iter().any(|answer| {
            answer.kind == RecordKind::Answer
                && answer.effect_id == effect.effect_id
                && answer.payload["VfsReceipt"].get("Ok").is_some()
        })
    };
    records
        .iter()
        .filter(|record| record.kind == RecordKind::Effect && performed(record))
        .filter_map(|effect| {
            let op = &effect.payload["Vfs"]["op"];
            if op["Read"]["path"] == "/shared.md" {
                return Some("read".to_owned());
            }
            let write = op.get("Write")?;
            (write["path"] == "/shared.md").then(|| format!("write {}", write["contents"]))
        })
        .collect()
}

#[tokio::test]
async fn two_tasks_whose_models_write_one_path_both_succeed_and_the_later_write_is_the_content() {
    let sections = [
        JOINS_BOTH.to_owned(),
        worker("Left", "files/put", "files_put /shared.md left"),
        worker("Right", "files/put", "files_put /shared.md right"),
    ]
    .concat();

    let (report, records, vfs) = shared_run(&sections).await;

    assert_eq!(
        report.result,
        Ok("left|right".to_owned()),
        "each model's call answered with the text it wrote"
    );
    assert_eq!(
        done_to_shared(&records),
        ["write \"left\"", "write \"right\""],
        "the loop performed both writes, though nothing orders the two tasks"
    );
    assert_eq!(shared_text(&vfs), "right", "the later write is the content");
}

#[tokio::test]
async fn a_task_whose_model_reads_a_path_holds_back_no_write_of_another_task() {
    let sections = [
        JOINS_BOTH.to_owned(),
        worker("Left", "files/cat", "files_cat /shared.md"),
        worker("Right", "files/put", "files_put /shared.md new"),
    ]
    .concat();

    let (report, records, vfs) = shared_run(&sections).await;

    assert_eq!(
        report.result,
        Ok("old|new".to_owned()),
        "the reader got the text it read, and the writer's call succeeded"
    );
    assert_eq!(
        done_to_shared(&records),
        ["read", "write \"new\""],
        "the write ran after the other task's read of the path"
    );
    assert_eq!(shared_text(&vfs), "new");
}

#[tokio::test]
async fn a_prompts_own_tool_call_and_a_models_call_on_one_path_both_succeed() {
    let main = "## Main\n\n```lua\n\
        local worker = tasks.spawn('../Worker')\n\
        local mine = tools.call('files/put', { path = '/shared.md', text = 'script' })\n\
        local done = tasks.join({ worker })\n\
        return mine .. '|' .. done[1].result\n```\n\n";
    let sections = [
        main.to_owned(),
        worker("Worker", "files/put", "files_put /shared.md model"),
    ]
    .concat();

    let (report, records, vfs) = shared_run(&sections).await;

    assert_eq!(
        report.result,
        Ok("script|model".to_owned()),
        "the script's call and the model's call both answered with the text they wrote"
    );
    let done = done_to_shared(&records);
    assert_eq!(
        done.len(),
        2,
        "the loop performed both writes, though nothing orders the script's call and the \
         task's: {done:?}"
    );
    let later = done.last().expect("two writes were performed");
    assert_eq!(
        *later,
        format!("write {:?}", shared_text(&vfs)),
        "the later write is the content"
    );
}

#[tokio::test]
async fn two_tasks_that_write_one_path_of_the_store_end_the_run_as_a_determinism_error() {
    // Each task first writes the Host's file, where nothing is checked,
    // and then one path of the run's store, where the second write
    // conflicts with the first.
    let task = |name: &str| {
        format!(
            "## {name}\n\n```lua\n\
             tools.call('files/put', {{ path = '/shared.md', text = '{name}' }})\n\
             store.write('same.md', '{name}')\n\
             return '{name}'\n```\n\n"
        )
    };
    let sections = [JOINS_BOTH.to_owned(), task("Left"), task("Right")].concat();

    let (report, records, vfs) = shared_run(&sections).await;

    let error = report.result.expect_err("the store conflict ends the run");
    assert_eq!(error.code, ErrorCode::Determinism, "{error:?}");
    assert!(
        error.located_message().contains("same.md"),
        "the error names the store path: {error:?}"
    );
    assert_eq!(
        done_to_shared(&records),
        ["write \"Left\"", "write \"Right\""],
        "both tasks wrote the Host's file before the store refused the second task"
    );
    assert_eq!(shared_text(&vfs), "Right");
    let refused: Vec<&str> = records
        .iter()
        .filter(|record| record.kind == RecordKind::Answer)
        .filter_map(|answer| answer.payload["Vfs"]["Err"]["kind"].as_str())
        .collect();
    assert_eq!(
        refused,
        ["conflict"],
        "one store write is answered with the conflict"
    );
}
