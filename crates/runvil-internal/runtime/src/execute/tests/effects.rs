//! Effects as values: every leaf request kind a section yields - `infer`
//! and `chat`, `tool_call`, `store`, `timer` - issues
//! exactly one `Effect` out of the run's `step`, after the one `Render`
//! a section's first round with catalog offers issues, and each effect's
//! record round-trips through serde; a run's store records, applied in
//! order to a new store, rebuild the run's store; a section's round
//! carries the `Chat` origin and a nested infer round the `Infer` origin.
//! The run's answer rules (a drop, an orphan, a wrong kind) are pinned
//! beside `Run` itself.

use runvil_prompt::test_support::{one_section, prompt};
use runvil_types::event::ReplyOrigin;
use runvil_types::ids::RoundId;
use runvil_types::packs::PackId;
use runvil_vfs::Store;

use super::fixtures::echo_tool;
use super::models_loop::{loop_model_name, loop_scenario};
use super::scheduler::scheduler_scenario;
use super::*;
use crate::execute::run::{
    AnswerRecord, EffectRecord, PackToolSets, Round, ToolCallOrigin, ToolCaller,
};
use crate::lua::{VfsOp, run_store_op};
use crate::test_support::rig::Rig;
use crate::test_support::scenario::{fresh_store_view, round_trip};

/// Asserts every recorded effect survives the round trip unchanged.
#[track_caller]
fn assert_round_trips(records: &[EffectRecord]) {
    for record in records {
        assert_eq!(&round_trip(record), record, "the record round-trips");
    }
}

/// The origin of a call made by `caller` in the entry `entry` of
/// `section` of the test run.
fn origin(section: &str, entry: &str, caller: ToolCaller) -> ToolCallOrigin {
    ToolCallOrigin::new(section, caller).with_entry(entry)
}

#[test]
fn models_infer_issues_exactly_one_chat_effect_over_one_user_message() {
    let gateway = ScriptedChat::new(vec![text("answer")]);
    let source = one_section("return models.infer('ask')");
    let rig = Rig::run(loop_scenario(source).chat(&gateway));
    assert_eq!(rig.trace().text(), "answer");

    assert_eq!(
        rig.issued(),
        vec![EffectRecord::Chat {
            round: RoundId::new(0),
            alias: "default".to_owned(),
            after: None,
            keep: 0,
            messages: vec![json!({ "role": "user", "content": "ask" })],
            tools: Vec::new(),
            temperature: None,
            max_tokens: None,
            thinking: None,
        }],
        "an infer is one tool-free chat effect"
    );
    assert_round_trips(rig.issued());
}

#[test]
fn a_models_loop_round_issues_one_chat_effect_and_one_tool_call_effect_per_call() {
    let gateway = ScriptedChat::new(vec![
        calls(&[("call_1", "echo", r#"{"value":"hi"}"#)]),
        text("done"),
    ]);
    let source = one_section(
        "tools.offer('tools/echo')\n\
         messages:user('hello')\n\
         models.loop()\n\
         return messages[#messages].content",
    );
    let rig = Rig::run(
        loop_scenario(source)
            .offer("echo", echo_tool())
            .chat(&gateway),
    );
    assert_eq!(rig.trace().text(), "done");

    let records = rig.issued();
    assert_eq!(
        records.len(),
        4,
        "the render, two rounds, and one call: {records:?}"
    );
    assert_eq!(
        records[0],
        EffectRecord::Render {
            alias: "default".to_owned(),
            entry: "0.0".to_owned(),
            offered: vec![PackToolSets::new(
                PackId::parse("tools").expect("a valid Pack id"),
                vec![ToolId::parse("tools/echo").expect("a valid id")],
                Vec::new(),
            )],
        },
        "the first round's render asks for the one offered tool"
    );
    assert!(
        matches!(&records[1], EffectRecord::Chat { tools, after: None, keep: 0, messages, .. }
            if tools == &["echo".to_owned()] && messages.len() == 1),
        "the first round advertises the scope over the author's list, which starts here: {:?}",
        records[1]
    );
    assert_eq!(
        records[2],
        EffectRecord::ToolCall {
            tool: ToolId::parse("tools/echo").expect("a valid id"),
            alias: "echo".to_owned(),
            args: json!({ "value": "hi" }),
            origin: origin("Only", "0.0", ToolCaller::Model).with_model(loop_model_name("default")),
        },
        "the model's call is one tool_call effect naming the bound identity and the model"
    );
    let (after, keep, messages) = match &records[3] {
        EffectRecord::Chat {
            after,
            keep,
            messages,
            ..
        } => (after, keep, messages),
        other => panic!("the fourth effect is the second round: {other:?}"),
    };
    assert_eq!(
        (*after, *keep),
        (Some(RoundId::new(0)), 1),
        "the second round extends the first, repeating its one message"
    );
    let roles: Vec<&Value> = messages.iter().map(|message| &message["role"]).collect();
    assert_eq!(
        roles,
        [&json!("assistant"), &json!("tool")],
        "the second round logs only the assistant tool-call record and the tool record: \
         {messages:?}"
    );
    assert!(
        messages[0]["tool_calls"].is_array() && messages[1]["tool_call_id"] == json!("call_1"),
        "the logged pair is the call and its result: {messages:?}"
    );
    assert_round_trips(records);
}

#[test]
fn a_script_tools_call_issues_exactly_one_tool_call_effect() {
    let source = one_section(
        "tools.offer('tools/echo')\n\
         return tools.call('tools/echo', { value = 'hi' })",
    );
    let rig = Rig::run(loop_scenario(source).offer("echo", echo_tool()));
    assert_eq!(rig.trace().text(), "echoed: hi");

    assert_eq!(
        rig.issued(),
        vec![EffectRecord::ToolCall {
            tool: ToolId::parse("tools/echo").expect("a valid id"),
            alias: "tools/echo".to_owned(),
            args: json!({ "value": "hi" }),
            origin: origin("Only", "0.0", ToolCaller::Script),
        }]
    );
    assert_round_trips(rig.issued());
}

#[test]
fn a_script_tool_call_records_the_section_that_made_it() {
    let source = "---\nname: loop\ndescription: d\nrunvil: 0\n---\n\n\
         ## First\n\n```lua\ntools.offer('tools/echo')\ntools.call('tools/echo', { value = 'a' })\n```\n\n\
         ## Second\n\n```lua\ntools.offer('tools/echo')\n\
         return tools.call('tools/echo', { value = 'b' })\n```\n";
    let rig = Rig::run(loop_scenario(source).offer("echo", echo_tool()));
    assert_eq!(rig.trace().text(), "echoed: b");

    let origins: Vec<ToolCallOrigin> = rig
        .issued()
        .iter()
        .filter_map(|record| match record {
            EffectRecord::ToolCall { origin, .. } => Some(origin.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        origins,
        vec![
            origin("First", "0.0", ToolCaller::Script),
            origin("Second", "0.1", ToolCaller::Script),
        ],
        "each call names the section and the entry it was made in"
    );
}

#[test]
fn a_store_operation_issues_exactly_one_store_effect() {
    let source = one_section(
        "store.write('notes.md', 'kept')\n\
         return store.read('notes.md')",
    );
    let rig = Rig::run(loop_scenario(source));
    assert_eq!(rig.trace().text(), "kept");

    assert_eq!(
        rig.issued(),
        vec![
            EffectRecord::Vfs {
                op: VfsOp::Write {
                    path: "notes.md".to_owned(),
                    contents: "kept".to_owned(),
                },
            },
            EffectRecord::Vfs {
                op: VfsOp::Read {
                    path: "notes.md".to_owned(),
                    start: None,
                    end: None,
                },
            },
        ],
        "each store call is one store effect holding its validated op"
    );
    assert_round_trips(rig.issued());
}

/// Every file of a store beside its text, in path order, as `glob` lists
/// the store's paths and `read` reads one.
fn files_of(
    glob: impl Fn(&str) -> std::result::Result<Vec<String>, VfsError>,
    read: impl Fn(&str) -> std::result::Result<String, VfsError>,
) -> std::result::Result<Vec<(String, String)>, VfsError> {
    glob("**")?
        .into_iter()
        .map(|path| {
            let text = read(&path)?;
            Ok((path, text))
        })
        .collect()
}

#[test]
fn a_runs_store_effect_records_rebuild_its_store() -> std::result::Result<(), VfsError> {
    // Two sections and three fanout arms write, append to, replace in,
    // and delete files of the run's store, and the last section's read of
    // a deleted file fails.
    let source = prompt(
        "",
        &[
            (
                "Draft",
                "
                store.write('outline.md', 'one\\ntwo')
                store.append('outline.md', '\\nthree')
                store.write('scratch.md', 'deleted by the next section')
                tasks.fanout('Part', { 'a', 'b', 'c' })
                store.append('log.md', 'drafted\\n')",
            ),
            (
                "### Part",
                "
                local path = 'parts/' .. sys.index .. '.md'
                store.write(path, 'part ' .. item)
                store.append(path, ', more of ' .. item)
                store.str_replace(path, 'part', 'section')
                return item",
            ),
            (
                "Finish",
                "
                store.str_replace('outline.md', 'two', 'TWO')
                store.delete('scratch.md')
                store.delete('parts/2.md')
                store.append('log.md', 'finished\\n')
                return tostring(pcall(store.read, 'scratch.md'))",
            ),
        ],
    );
    let trace = Scenario::new(source).run();
    assert_eq!(trace.text(), "false");

    // Each operation the run's store performed, applied in record order
    // through one view of a new store. An operation the run's store
    // refused left that store as it was, so it is left out.
    let rebuilt = fresh_store_view(&Store::new(), "the rebuilt store")?;
    for (effect, answer) in trace.effects() {
        if let (EffectRecord::Vfs { op }, AnswerRecord::Vfs(Ok(outcome))) = (effect, answer) {
            assert_eq!(
                &run_store_op(&rebuilt, op.clone())?,
                outcome,
                "{op:?} gives the outcome the run recorded"
            );
        }
    }

    let left = files_of(|pattern| trace.glob(pattern), |path| trace.read(path))?;
    let expected = [
        ("log.md", "drafted\nfinished\n"),
        ("outline.md", "one\nTWO\nthree"),
        ("parts/1.md", "section a, more of a"),
        ("parts/3.md", "section c, more of c"),
    ]
    .map(|(path, text)| (path.to_owned(), text.to_owned()));
    assert_eq!(
        left, expected,
        "the run's store holds what its sections and arms left"
    );
    assert_eq!(
        files_of(
            |pattern| rebuilt.glob(pattern),
            |path| rebuilt.read_string(path)
        )?,
        left,
        "the rebuilt store holds exactly the files and contents the run's own store holds"
    );
    Ok(())
}

#[test]
fn a_timed_wait_issues_exactly_one_timer_effect() {
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ## Main\n\n\
        ```lua\n\
        local t = tasks.spawn('../Child')\n\
        local first, ok, result = tasks.join_any({ t }, { timeout = 30 })\n\
        return result\n\
        ```\n\n\
        ## Child\n\n\
        ```lua\nreturn 'quick'\n```\n";
    let rig = Rig::run(scheduler_scenario(md));
    assert_eq!(rig.trace().text(), "quick");

    assert_eq!(
        rig.issued(),
        vec![EffectRecord::Timer { seconds: 30.0 }],
        "the wait's timeout is one timer effect; the child issued none"
    );
    assert_round_trips(rig.issued());
}

#[test]
fn a_sections_round_is_the_runs_first_with_the_chat_origin() {
    let gateway = ScriptedChat::new(vec![text("answer")]);
    let source = one_section(
        "messages:user('hello')\n\
         models.loop()\n\
         return messages[#messages].content",
    );
    let rig = Rig::run(loop_scenario(source).chat(&gateway));
    assert_eq!(rig.trace().text(), "answer");
    assert_eq!(
        rig.rounds(),
        vec![Round {
            id: RoundId::new(0),
            origin: ReplyOrigin::Chat,
        }],
        "the section's round is the run's first and has the chat origin"
    );
}

#[test]
fn a_nested_infer_round_is_the_runs_first_with_the_infer_origin() {
    let gateway = ScriptedChat::new(vec![text("answer")]);
    let source = one_section("return models.infer('ask')");
    let rig = Rig::run(loop_scenario(source).chat(&gateway));
    assert_eq!(rig.trace().text(), "answer");
    assert_eq!(
        rig.rounds(),
        vec![Round {
            id: RoundId::new(0),
            origin: ReplyOrigin::Infer,
        }],
        "the nested round is the run's first and has the infer origin"
    );
}
