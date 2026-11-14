//! The effect loop answers a Vfs effect inline: a run of nothing but Vfs
//! operations completes without a stall, a Vfs chain keeps stepping while
//! another effect is parked, a Vfs effect withdrawn in the step that
//! issued it is dropped without being performed, a step's Vfs effect is
//! answered before the step's next effect is recorded, and a run's Vfs
//! records rebuild its store.

use std::sync::Arc;
use std::time::Duration;

use harness_runner::effect_loop::drive_run;
use harness_runner::recorder::{MemoryRecorder, Record, RecordKind, RunId};
use runvil_engine::cancel::CancelHandle;
use runvil_engine::vfs::{Origin, VfsOp, VfsRef, perform_vfs_op};
use serde_json::json;

use super::{answers, assert_one_answer_per_effect, begun_log, kinds};
use crate::support::{
    GatedTool, TextTool, Unused, WAITS, loop_setup, run, run_of_sections, run_with_child,
};

#[tokio::test]
async fn a_run_of_only_vfs_operations_completes_with_one_answer_per_effect() {
    let (recorder, run_id) = begun_log().await;

    // Every effect of every step is a Vfs effect, so no performer is ever
    // out: a loop that awaited an answer, or reported a stall, after
    // answering them inline would fail the run here.
    let result = drive_run(
        run("store.write('a.md', '1')\n\
             store.append('a.md', '2')\n\
             return store.read('a.md')"),
        &loop_setup(recorder.clone()),
        Arc::new(Unused),
        Arc::new(Unused),
        run_id,
        CancelHandle::new(),
    )
    .await;
    assert_eq!(
        result,
        Ok("12".to_owned()),
        "answering every effect inline is not a stall"
    );

    let records = recorder.records(run_id);
    assert_one_answer_per_effect(&records);
    let answers = answers(&records);
    assert_eq!(answers.len(), 3, "a write, an append, and a read");
    for answer in answers {
        assert_ne!(
            answer.payload,
            json!("Dropped"),
            "an inline answer carries the operation's outcome"
        );
    }
}

/// The effect record for writing `contents` to `path`.
fn write_effect(path: &str, contents: &str) -> serde_json::Value {
    json!({ "Vfs": { "op": { "Write": { "path": path, "contents": contents } } } })
}

/// Whether `records` hold the answer to the write of `second.md`.
fn second_write_is_answered(records: &[Record]) -> bool {
    let second = write_effect("second.md", "2");
    let Some(effect) = records
        .iter()
        .find(|record| record.kind == RecordKind::Effect && record.payload == second)
    else {
        return false;
    };
    records
        .iter()
        .any(|record| record.kind == RecordKind::Answer && record.effect_id == effect.effect_id)
}

/// Polls the recorder until the second write's answer is recorded, then
/// opens `gate` and returns the records seen at that moment. Nothing here
/// measures time: a loop that stopped stepping while the tool call was out
/// would never record the second write, and the caller's bound fails the
/// test.
async fn open_gate_after_the_second_write(
    recorder: &MemoryRecorder,
    run_id: RunId,
    gate: &tokio::sync::Semaphore,
) -> Vec<Record> {
    loop {
        let seen = recorder.records(run_id);
        if second_write_is_answered(&seen) {
            gate.add_permits(1);
            return seen;
        }
        tokio::task::yield_now().await;
    }
}

/// A main section that writes twice while its child's tool call is out.
const WRITES_BESIDE_A_PARKED_CALL: &str = "local t = tasks.spawn('../Child')\n\
     store.write('first.md', '1')\n\
     store.write('second.md', '2')\n\
     local _task, _ok, result = tasks.join_any({ t })\n\
     return result";

#[tokio::test]
async fn a_vfs_chain_keeps_stepping_while_a_tool_call_is_parked() {
    let (recorder, run_id) = begun_log().await;
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let tool = Arc::new(GatedTool {
        gate: Arc::clone(&gate),
    });
    let setup = loop_setup(recorder.clone());

    // The first write and the child's gated tool call are issued in one
    // step. The loop answers the write inline and steps again, so the
    // second write is issued, answered, and recorded while the tool call
    // is still parked; only then does the watcher open the gate.
    let drive = drive_run(
        run_with_child(WRITES_BESIDE_A_PARKED_CALL, WAITS),
        &setup,
        Arc::new(Unused),
        tool,
        run_id,
        CancelHandle::new(),
    );
    let (result, before_gate) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(
            drive,
            open_gate_after_the_second_write(&recorder, run_id, &gate)
        )
    })
    .await
    .expect("the loop kept stepping the Vfs chain while the tool call was parked");
    assert_eq!(result, Ok("opened".to_owned()));

    let before_gate_answers = answers(&before_gate);
    assert_eq!(
        before_gate_answers.len(),
        2,
        "only the two writes were answered before the gate opened"
    );
    for answer in before_gate_answers {
        assert_eq!(answer.payload, json!({ "Vfs": { "Ok": "Unit" } }));
    }
    assert_one_answer_per_effect(&recorder.records(run_id));
}

/// Main spawns Slow, whose write stays out, and Fast, waits on Fast,
/// cancels Slow, and asks the store whether Slow's file exists: one step
/// issues Slow's write, withdraws it, and issues Main's question.
const WITHDRAWS_A_WRITE: [(&str, &str); 3] = [
    (
        "Main",
        "local slow = tasks.spawn('../Slow')\n\
         local fast = tasks.spawn('../Fast')\n\
         local _task, _ok, result = tasks.join_any({ fast })\n\
         tasks.cancel(slow)\n\
         return result .. '|' .. tostring(store.exists('slow.md'))",
    ),
    ("Slow", "store.write('slow.md', 'never')\nreturn 'slow'"),
    ("Fast", "return 'fast'"),
];

#[tokio::test]
async fn a_vfs_effect_withdrawn_in_the_step_that_issued_it_is_dropped_unperformed() {
    let (recorder, run_id) = begun_log().await;

    let result = drive_run(
        run_of_sections(&WITHDRAWS_A_WRITE),
        &loop_setup(recorder.clone()),
        Arc::new(Unused),
        Arc::new(Unused),
        run_id,
        CancelHandle::new(),
    )
    .await;
    assert_eq!(
        result,
        Ok("fast|false".to_owned()),
        "the store holds no file of the withdrawn write"
    );

    let records = recorder.records(run_id);
    assert_one_answer_per_effect(&records);
    let issued: Vec<(RecordKind, serde_json::Value)> = records
        .iter()
        .filter(|record| record.kind != RecordKind::Event)
        .map(|record| (record.kind, record.payload.clone()))
        .collect();
    assert_eq!(
        issued,
        [
            (RecordKind::Effect, write_effect("slow.md", "never")),
            (RecordKind::Answer, json!("Dropped")),
            (
                RecordKind::Effect,
                json!({ "Vfs": { "op": { "Exists": { "path": "slow.md" } } } })
            ),
            (
                RecordKind::Answer,
                json!({ "Vfs": { "Ok": { "Bool": false } } })
            ),
        ],
        "Slow's write is answered Dropped unperformed, and Main's question then finds no file"
    );
}

/// A label for one record: an event's kind, or the effect or answer kind
/// with the operation's name.
fn label(record: &Record) -> String {
    let named = |value: &serde_json::Value| match value {
        serde_json::Value::Object(map) => map.keys().next().cloned().unwrap_or_default(),
        other => other.to_string(),
    };
    match record.kind {
        RecordKind::Event => format!("event:{}", record.payload["kind"].as_str().unwrap()),
        RecordKind::Effect => format!("effect:{}", named(&record.payload)),
        RecordKind::Answer => format!("answer:{}", named(&record.payload)),
    }
}

#[tokio::test]
async fn a_step_with_a_vfs_effect_records_its_inline_answer_before_the_next_effect() {
    let (recorder, run_id) = begun_log().await;

    drive_run(
        run_with_child(WRITES_BESIDE_A_PARKED_CALL, WAITS),
        &loop_setup(recorder.clone()),
        Arc::new(Unused),
        Arc::new(TextTool("done")),
        run_id,
        CancelHandle::new(),
    )
    .await
    .unwrap();

    let records = recorder.records(run_id);
    assert_one_answer_per_effect(&records);
    let first_effect = records
        .iter()
        .position(|record| record.kind == RecordKind::Effect)
        .expect("the run issues effects");
    let first_tool_answer = records
        .iter()
        .position(|record| label(record) == "answer:ToolCall")
        .expect("the tool call is answered");
    let order: Vec<String> = records[first_effect..=first_tool_answer]
        .iter()
        .map(label)
        .collect();
    // The first step issues a write and the child's tool call: the write
    // is answered the moment it is performed, before the tool call's
    // effect is recorded. The next step's events precede the second
    // write's effect, and the tool call's answer waits for the loop to
    // await it.
    assert_eq!(
        order,
        [
            "effect:Vfs",
            "answer:Vfs",
            "effect:ToolCall",
            "event:vfs_write_succeeded",
            "effect:Vfs",
            "answer:Vfs",
            "event:vfs_write_succeeded",
            "answer:ToolCall",
        ]
    );
    assert!(
        kinds(&records[..first_effect])
            .iter()
            .all(|kind| *kind == RecordKind::Event),
        "the run opens with its own events"
    );
}

/// A section that writes, appends to, edits, and deletes store files,
/// reads the deleted one under a `pcall`, and returns what it reads back
/// of the file it kept.
const EDITS_ITS_STORE: &str = "store.write('notes.md', 'one')\n\
     store.append('notes.md', ' two')\n\
     store.str_replace('notes.md', 'one', 'three')\n\
     store.write('draft.md', 'gone')\n\
     store.delete('draft.md')\n\
     pcall(store.read, 'draft.md')\n\
     return store.read('notes.md')";

#[tokio::test]
async fn a_runs_vfs_records_rebuild_its_store() {
    let (recorder, run_id) = begun_log().await;

    let returned = drive_run(
        run(EDITS_ITS_STORE),
        &loop_setup(recorder.clone()),
        Arc::new(Unused),
        Arc::new(Unused),
        run_id,
        CancelHandle::new(),
    )
    .await
    .expect("the run completes");
    assert_eq!(returned, "three two");

    // The store is gone with the run. Each Vfs effect's record holds its
    // operation whole, so the operations the store performed, applied in
    // record order to an empty filesystem, give the files the store held.
    let records = recorder.records(run_id);
    let files = VfsRef::default()
        .acquire(Origin::new("store rebuild"))
        .expect("an empty memory filesystem acquires");
    let mut applied = 0;
    for effect in &records {
        if effect.kind != RecordKind::Effect {
            continue;
        }
        let Some(vfs) = effect.payload.get("Vfs") else {
            continue;
        };
        let answer = records
            .iter()
            .find(|record| {
                record.kind == RecordKind::Answer && record.effect_id == effect.effect_id
            })
            .expect("every effect is answered");
        if answer.payload["Vfs"].get("Ok").is_none() {
            continue;
        }
        let op: VfsOp = serde_json::from_value(vfs["op"].clone())
            .expect("a Vfs effect's record holds its operation whole");
        perform_vfs_op(&files, op).expect("an operation the store performed applies again");
        applied += 1;
    }
    assert_eq!(applied, 6, "every operation but the refused read");
    assert_eq!(
        files.read_string("notes.md"),
        Ok(returned),
        "the rebuilt file reads as the text the run returned"
    );
    assert_eq!(
        files.exists("draft.md"),
        Ok(false),
        "the deleted file is absent"
    );
}
