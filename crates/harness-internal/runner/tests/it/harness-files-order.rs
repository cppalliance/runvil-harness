//! The order of one reply's tool calls when a tool changes files: two
//! changes to one file both apply, in the order the model listed them; a
//! call listed before a change is answered before the change starts, and
//! one listed after it starts once the change is answered; and calls
//! that change no file are still in flight together.

use std::collections::HashMap;
use std::sync::Arc;

use harness_runner::recorder::{MemoryRecorder, Record, RecordKind};
use runvil_engine::model::{Completion, CompletionResult, ToolCall};
use runvil_engine::vfs::{Origin, VfsRef};
use serde_json::{Value, json};

use super::files::files_host;
use super::{launch, setup};
use crate::scripted::{MODEL, ScriptedBroker, reply};

/// A prompt that offers the fixture Pack's reader and its changing tool
/// to one model loop and returns once the loop ends.
const LOOPS: &str = "---\nname: orders\ndescription: d\nrunvil: 0\n\
    uses:\n  - tests/files\n---\n\n\
    ## Only\n\n```lua\n\
    tools.offer('files/cat')\n\
    tools.offer('files/append')\n\
    messages:user('go')\n\
    models.loop()\n\
    return 'done'\n```\n";

/// The reader's wire name, as the model calls it.
const CAT: &str = "files_cat";

/// The changing tool's wire name, as the model calls it.
const APPEND: &str = "files_append";

/// A broker whose model asks for `calls` in its first round, each a
/// tool's wire name beside its arguments, and replies with text in every
/// later round.
fn asking(calls: Vec<(&'static str, Value)>) -> ScriptedBroker {
    ScriptedBroker::new(move |round, _messages| {
        let calls = calls.clone();
        Box::pin(async move {
            if round.id.get() > 0 {
                return reply("done");
            }
            let calls = calls
                .into_iter()
                .enumerate()
                .map(|(index, (name, args))| ToolCall::from_parts(format!("c{index}"), name, args))
                .collect::<Result<Vec<_>, _>>()?;
            Completion::from_result(CompletionResult::ToolCalls(calls), MODEL).map(Box::new)
        })
    })
}

/// When each tool call of `records` started and ended, in record order:
/// `start <label>` at a call's effect record and `end <label> = <text>`
/// at its answer record. The label is the call's `label` argument, and
/// the text is what the tool answered.
fn call_trail(records: &[Record]) -> Vec<String> {
    let mut labels: HashMap<u64, String> = HashMap::new();
    let mut trail = Vec::new();
    for record in records {
        let Some(id) = record.effect_id else {
            continue;
        };
        if record.kind == RecordKind::Effect {
            if let Some(label) = record.payload["ToolCall"]["args"]["label"].as_str() {
                labels.insert(id, label.to_owned());
                trail.push(format!("start {label}"));
            }
        } else if let Some(label) = labels.get(&id) {
            let text = record.payload["ToolCall"]["Ok"]["text"]
                .as_str()
                .unwrap_or("no text");
            trail.push(format!("end {label} = {text}"));
        }
    }
    trail
}

/// Runs [`LOOPS`] over a filesystem that holds `seed`, each a path beside
/// its text, with a model that asks for `calls` in one reply. Returns
/// when each call started and ended, as [`call_trail`] shows it, beside
/// the filesystem.
async fn order_run(
    seed: &[(&str, &str)],
    calls: Vec<(&'static str, Value)>,
) -> (Vec<String>, VfsRef) {
    let vfs = VfsRef::default();
    let files = vfs.acquire(Origin::new("call order seed")).unwrap();
    for (path, text) in seed {
        files.write(path, text.as_bytes()).unwrap();
    }
    drop(files);
    let recorder = Arc::new(MemoryRecorder::new());
    let harness = launch(
        files_host(|| ()),
        LOOPS,
        setup(recorder.clone(), asking(calls)).vfs(vfs.clone()),
    );

    let report = harness.run().await.expect("the run is reported");

    assert_eq!(report.result, Ok("done".to_owned()));
    (call_trail(&recorder.records(report.run_id)), vfs)
}

#[tokio::test]
async fn two_changes_to_one_file_in_one_reply_both_apply_in_the_order_the_model_listed_them() {
    let (trail, vfs) = order_run(
        &[("/log.md", "0")],
        vec![
            (
                APPEND,
                json!({ "label": "first", "path": "/log.md", "text": "a" }),
            ),
            (
                APPEND,
                json!({ "label": "second", "path": "/log.md", "text": "b" }),
            ),
        ],
    )
    .await;

    assert_eq!(
        trail,
        [
            "start first",
            "end first = 0a",
            "start second",
            "end second = 0ab",
        ],
        "the second change starts once the first is answered, and reads what the first wrote"
    );
    let files = vfs.acquire(Origin::new("call order test")).unwrap();
    assert_eq!(
        files.read_string("/log.md"),
        Ok("0ab".to_owned()),
        "both changes applied, in the listed order"
    );
}

#[tokio::test]
async fn a_change_starts_once_the_calls_listed_before_it_are_answered_and_the_calls_after_it_wait()
{
    let (trail, _vfs) = order_run(
        &[("/log.md", "old")],
        vec![
            (CAT, json!({ "label": "before", "path": "/log.md" })),
            (
                APPEND,
                json!({ "label": "change", "path": "/log.md", "text": " new" }),
            ),
            (CAT, json!({ "label": "after", "path": "/log.md" })),
        ],
    )
    .await;

    assert_eq!(
        trail,
        [
            "start before",
            "end before = old",
            "start change",
            "end change = old new",
            "start after",
            "end after = old new",
        ],
        "the reader listed first ends before the change starts, and the one listed last \
         starts once the change is answered and reads it"
    );
}

#[tokio::test]
async fn calls_that_change_no_file_are_in_flight_together() {
    let (trail, _vfs) = order_run(
        &[("/a.md", "a"), ("/b.md", "b")],
        vec![
            (CAT, json!({ "label": "left", "path": "/a.md" })),
            (CAT, json!({ "label": "right", "path": "/b.md" })),
        ],
    )
    .await;

    assert_eq!(
        trail[..2],
        ["start left", "start right"],
        "both calls started before either ended: {trail:?}"
    );
    assert_eq!(trail.len(), 4, "each call ended once: {trail:?}");
}
