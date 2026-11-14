//! Tests for the access a `ToolCall` effect carries: the call's own
//! identity, forked from the calling chain when the call is issued and
//! ended when its answer is applied or the call is aborted, and rooted at
//! `/` of the handle the caller gave the run. The driver performs each
//! call through that access, and a claims conflict fails the call and so
//! the run, so a run that succeeds touched every file with no conflict.
//! One call hands a file to a later call: the end joins the first call's
//! work into the chain before the chain's next step, and the fork orders
//! the chain's earlier work before the later call's. A task's calls fork
//! from the task, a dropped answer and a canceled task's call are joined
//! like any other, and an access used after its call has ended refuses
//! every operation. A call's access reaches no store: a call sees no file
//! the chain wrote with `store.write`, and `store.read` sees no file a
//! call wrote.

use std::fmt;

use runvil_vfs::{ExecId, Origin, VfsRef};

use super::models_loop::loop_models;
use super::*;
use crate::execute::run::{Effect, EffectAnswer, EffectId, IssuedEffect, Run, Step};
use crate::test_support::scenario::text_of;
use crate::test_support::{drive, perform_locally};

/// The suite's one tool, called by its full id: it is the run's whole
/// catalog.
const FILES: &str = "test/tools/files";

/// The tool the run binds. The driver performs its calls itself, through
/// the effect's access, so its own call never runs.
#[track_caller]
fn files_tool() -> FnTool {
    FnTool::new(FILES, json!({ "type": "object" }), |_| {
        Err(make_tool_error(
            ErrorCode::ToolOther,
            "the suite's driver performs this tool",
        ))
    })
    .description("reads, writes, or lists the run's files")
}

/// Performs one files call through `access`: `read` names a file whose
/// text is the output, `list` is a pattern whose matches are the output,
/// joined with commas between brackets, and `write` names a file that
/// receives `text`.
fn use_access(
    access: &Access,
    args: &Value,
) -> std::result::Result<ToolOutput, runvil_types::error::Error> {
    let failed = |error: VfsError| make_tool_error(ErrorCode::ToolOther, error.to_string());
    if let Some(path) = args["read"].as_str() {
        return access
            .read_string(path)
            .map(ToolOutput::trusted)
            .map_err(failed);
    }
    if let Some(pattern) = args["list"].as_str() {
        return access
            .glob(pattern)
            .map(|paths| ToolOutput::trusted(format!("[{}]", paths.join(","))))
            .map_err(failed);
    }
    let path = args["write"]
        .as_str()
        .expect("a files call names a file to read or write, or a pattern to list");
    let text = args["text"].as_str().unwrap_or_default();
    access.write(path, text.as_bytes()).map_err(failed)?;
    Ok(ToolOutput::trusted("written"))
}

/// The identities a driven run's effects held, in issue order: each tool
/// call's access, and each store effect's view, which keeps its chain's
/// identity.
#[derive(Default)]
struct Identities {
    tools: Vec<ExecId>,
    stores: Vec<ExecId>,
}

impl Identities {
    /// Whether every tool call ran under an identity of its own: distinct
    /// from every other call's and from every chain's.
    fn each_call_is_its_own(&self) -> bool {
        self.tools.iter().enumerate().all(|(index, tool)| {
            !self.stores.contains(tool) && !self.tools[index + 1..].contains(tool)
        })
    }
}

/// A run over `md` with the files tool as its whole catalog, nothing
/// offered, the loop models filled in, and `vfs` as the files its tool
/// calls work in, for the test to step.
#[track_caller]
fn files_run(md: &str, vfs: &VfsRef) -> Run {
    Scenario::new(md)
        .models(loop_models())
        .offering(Vec::new())
        .tool(files_tool())
        .vfs(vfs.clone())
        .into_run()
}

/// Every file `vfs` holds beside its text, in path order, read through an
/// access of the caller's own once the run over `vfs` has ended.
fn files_in(vfs: &VfsRef) -> std::result::Result<Vec<(String, String)>, VfsError> {
    let access = vfs.acquire(Origin::new("the caller's own access"))?;
    access
        .glob("/**")?
        .into_iter()
        .map(|path| {
            let text = access.read_string(&path)?;
            Ok((path, text))
        })
        .collect()
}

/// The one file `path` holding `text`, as [`files_in`] lists it.
fn one_file(path: &str, text: &str) -> Vec<(String, String)> {
    vec![(path.to_owned(), text.to_owned())]
}

/// Performs a store effect, or panics on the model round no files run
/// issues.
fn perform_store(effect: &Effect) -> EffectAnswer {
    perform_locally(effect, &mut |round| {
        panic!("no model round is issued: {round:?}")
    })
}

/// One operation of each kind tried through `access`, each beside its
/// name: a read of `file`, a write of `late.txt`, an existence check and
/// a removal of `file`, and a listing.
fn every_operation(
    access: &Access,
    file: &str,
) -> Vec<(&'static str, std::result::Result<(), VfsError>)> {
    vec![
        ("read", access.read(file).map(drop)),
        ("write", access.write("late.txt", b"after the call ended")),
        ("exists", access.exists(file).map(drop)),
        ("glob", access.glob("**").map(drop)),
        ("remove", access.remove(file, false).map(drop)),
    ]
}

/// Asserts that every result in `tried` is the refusal of an access
/// whose tool call has ended. The run and the tool call end an access as
/// the same variant; the callers hold the run open, so the call's end is
/// the one refused.
#[track_caller]
fn assert_all_refused_as_ended<T: fmt::Debug>(
    tried: Vec<(&'static str, std::result::Result<T, VfsError>)>,
) {
    assert!(!tried.is_empty(), "an operation was tried");
    for (operation, result) in tried {
        match result {
            Err(VfsError::ScopeEnded { .. }) => {}
            other => panic!("expected the ended-call refusal of the {operation}, got {other:?}"),
        }
    }
}

/// The serial driver's performer for a files run: each files call is
/// performed through its own access and answered with its output, each
/// store effect runs on its view, and `seen` notes the identity each
/// held.
fn files_answers(seen: &mut Identities) -> impl FnMut(EffectId, &Effect) -> EffectAnswer + '_ {
    move |_, effect| match effect {
        Effect::ToolCall { access, args, .. } => {
            seen.tools.push(access.id());
            EffectAnswer::ToolCall(use_access(access, args))
        }
        other => {
            if let Effect::Vfs { access, .. } = other {
                seen.stores.push(access.id());
            }
            perform_store(other)
        }
    }
}

#[test]
fn one_tool_call_hands_a_file_to_a_later_tool_call() -> std::result::Result<(), VfsError> {
    // Both accesses are rooted at `/` of the run's handle: the first call
    // writes `handoff.txt` below its root, the second reads
    // `/handoff.txt`, and the caller's own access finds the file there
    // once the run has ended. The first call's end joins its write into
    // the chain, and the second call forks from the chain after that, so
    // the write is ordered before the read.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ## Only\n\n\
        ```lua\n\
        tools.call('test/tools/files', { write = 'handoff.txt', text = 'first call wrote this' })\n\
        return tools.call('test/tools/files', { read = '/handoff.txt' })\n\
        ```\n";
    let vfs = VfsRef::default();
    let (result, _) = drive(
        files_run(md, &vfs),
        files_answers(&mut Identities::default()),
    );
    assert_eq!(text_of(result), "first call wrote this");
    assert_eq!(
        files_in(&vfs)?,
        one_file("/handoff.txt", "first call wrote this")
    );
    Ok(())
}

#[test]
fn a_tool_calls_work_is_ordered_after_its_chains_earlier_work_and_before_its_chains_next_step() {
    // Two writes of one file by two identities conflict unless one is
    // ordered before the other, and a conflict fails the call. The first
    // call's end joins its write into the chain, so it is ordered before
    // the chain's next step; the second call forks from the chain at that
    // step, so the chain's earlier work, the first call's included, is
    // ordered before the second call's write; and the third call reads
    // what the second wrote the same way.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ## Only\n\n\
        ```lua\n\
        tools.call('test/tools/files', { write = 'turn.txt', text = 'first call' })\n\
        tools.call('test/tools/files', { write = 'turn.txt', text = 'second call' })\n\
        return tools.call('test/tools/files', { read = 'turn.txt' })\n\
        ```\n";
    let (result, _) = drive(
        files_run(md, &VfsRef::default()),
        files_answers(&mut Identities::default()),
    );
    assert_eq!(text_of(result), "second call");
}

#[test]
fn each_tool_call_has_an_identity_of_its_own() {
    // The store effects' views keep the chain's identity, which is how
    // the driver learns it: one identity for the chain before, between,
    // and after its calls, and another for each call.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ## Only\n\n\
        ```lua\n\
        store.exists('before.txt')\n\
        tools.call('test/tools/files', { write = 'a.txt', text = 'first call' })\n\
        store.exists('between.txt')\n\
        tools.call('test/tools/files', { read = 'a.txt' })\n\
        store.exists('after.txt')\n\
        return 'done'\n\
        ```\n";
    let mut seen = Identities::default();
    let (result, _) = drive(files_run(md, &VfsRef::default()), files_answers(&mut seen));
    assert_eq!(text_of(result), "done");
    assert_eq!(seen.tools.len(), 2, "two calls were issued");
    assert_eq!(
        seen.stores, [seen.stores[0]; 3],
        "the chain keeps one identity across its calls"
    );
    assert!(
        seen.each_call_is_its_own(),
        "each call ran under an identity forked from the chain, not the chain's own"
    );
}

#[test]
fn a_tasks_tool_calls_fork_from_the_task_and_the_owners_call_reads_their_file_after_the_join() {
    // The task's second call reads the file its first call wrote, which
    // only the task's identity orders before it; the owner's call reads
    // the file the task's last call wrote once the task is joined, which
    // joins the task's calls with it.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ## Main\n\n\
        ```lua\n\
        local t = tasks.spawn('../Worker')\n\
        tasks.join({ t })\n\
        return tools.call('test/tools/files', { read = 'out.txt' })\n\
        ```\n\n\
        ## Worker\n\n\
        ```lua\n\
        tools.call('test/tools/files', { write = 'in.txt', text = 'task call wrote this' })\n\
        local seen = tools.call('test/tools/files', { read = 'in.txt' })\n\
        tools.call('test/tools/files', { write = 'out.txt', text = 'later call saw: ' .. seen })\n\
        return 'done'\n\
        ```\n";
    let mut seen = Identities::default();
    let (result, _) = drive(files_run(md, &VfsRef::default()), files_answers(&mut seen));
    assert_eq!(text_of(result), "later call saw: task call wrote this");
    assert_eq!(
        seen.tools.len(),
        4,
        "the task issued three calls and its owner one"
    );
    assert!(
        seen.each_call_is_its_own(),
        "each call ran under its own identity"
    );
}

#[test]
fn a_tool_call_answered_dropped_is_still_joined_and_its_access_refuses_every_operation()
-> std::result::Result<(), VfsError> {
    // The driver performs the first call, answers it `Dropped`, and keeps
    // its access. The second call reads what the first wrote, so the
    // dropped call was joined into the chain; while the driver performs
    // the second call, with the run's scope still open, it tries the
    // first call's access.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ## Only\n\n\
        ```lua\n\
        local ok, err = pcall(tools.call, 'test/tools/files', { write = 'dropped.txt', text = 'tool wrote this' })\n\
        return tostring(ok) .. ':' .. err.kind .. '|' .. tools.call('test/tools/files', { read = 'dropped.txt' })\n\
        ```\n";
    let vfs = VfsRef::default();
    let mut dropped: Option<Arc<Access>> = None;
    let mut late = None;
    let (result, _) = drive(files_run(md, &vfs), |_, effect| match effect {
        Effect::ToolCall { access, args, .. } => {
            let output = use_access(access, args);
            if let Some(earlier) = &dropped {
                late = Some(every_operation(earlier, "dropped.txt"));
                EffectAnswer::ToolCall(output)
            } else {
                dropped = Some(Arc::clone(access));
                EffectAnswer::Dropped
            }
        }
        other => perform_store(other),
    });
    assert_eq!(text_of(result), "false:canceled|tool wrote this");
    assert_all_refused_as_ended(late.expect("the second call tried the dropped call's access"));
    assert_eq!(
        files_in(&vfs)?,
        one_file("/dropped.txt", "tool wrote this"),
        "the refused operations left the files as they were"
    );
    Ok(())
}

#[test]
fn an_access_kept_past_its_answer_refuses_every_operation_and_changes_nothing()
-> std::result::Result<(), VfsError> {
    // The driver keeps the first call's access and tries it while
    // performing the second call, with the run's scope still open.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ## Only\n\n\
        ```lua\n\
        tools.call('test/tools/files', { write = 'first.txt', text = 'first call' })\n\
        return tools.call('test/tools/files', { read = 'first.txt' })\n\
        ```\n";
    let vfs = VfsRef::default();
    let mut kept: Option<Arc<Access>> = None;
    let mut late = None;
    let (result, _) = drive(files_run(md, &vfs), |_, effect| match effect {
        Effect::ToolCall { access, args, .. } => {
            if let Some(earlier) = &kept {
                late = Some(every_operation(earlier, "first.txt"));
            }
            kept = Some(Arc::clone(access));
            EffectAnswer::ToolCall(use_access(access, args))
        }
        other => perform_store(other),
    });
    assert_eq!(text_of(result), "first call");
    assert_all_refused_as_ended(late.expect("the second call tried the first call's access"));
    assert_eq!(
        files_in(&vfs)?,
        one_file("/first.txt", "first call"),
        "the refused operations left the files as they were"
    );
    Ok(())
}

#[test]
fn a_canceled_tasks_tool_call_is_joined_and_its_access_refuses_every_later_operation()
-> std::result::Result<(), VfsError> {
    // The driver performs the task's call at once but holds its answer, so
    // the cancel finds the task parked on it. The owner's call reads what
    // the task's call wrote before the cancel; what the driver tries
    // through the task's call once the owner has returned, while the run
    // waits only on the held answer, is refused.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ## Main\n\n\
        ```lua\n\
        local a = tasks.spawn('../Task')\n\
        store.exists('p1')\n\
        store.exists('p2')\n\
        tasks.cancel(a)\n\
        tasks.join_any({ a })\n\
        return tools.call('test/tools/files', { read = 'tool.txt' })\n\
        ```\n\n\
        ## Task\n\n\
        ```lua\n\
        return tools.call('test/tools/files', { write = 'tool.txt', text = 'tool wrote this' })\n\
        ```\n";
    let vfs = VfsRef::default();
    let mut run = files_run(md, &vfs);
    let mut held: Option<(EffectId, Arc<Access>, EffectAnswer)> = None;
    let mut late = None;
    let result = loop {
        let (effects, withdrawn) = match run.step() {
            Step::Done { result, .. } => break result,
            Step::Pending {
                effects, withdrawn, ..
            } => (effects, withdrawn),
        };
        if effects.is_empty() {
            let (id, access, answer) = held
                .take()
                .expect("the decided run waits on the held call's answer");
            assert_eq!(
                withdrawn,
                [id],
                "only the step that decides the run waits on nothing new, and it withdraws the held call"
            );
            late = Some(every_operation(&access, "tool.txt"));
            run.resume(id, answer);
            continue;
        }
        for IssuedEffect { id, effect, .. } in effects {
            match &effect {
                Effect::ToolCall { access, args, .. } if args["write"].is_string() => {
                    let answer = EffectAnswer::ToolCall(use_access(access, args));
                    held = Some((id, Arc::clone(access), answer));
                }
                Effect::ToolCall { access, args, .. } => {
                    run.resume(id, EffectAnswer::ToolCall(use_access(access, args)));
                }
                _ => run.resume(id, perform_store(&effect)),
            }
        }
    };
    assert_eq!(text_of(result), "tool wrote this");
    assert_all_refused_as_ended(late.expect("the driver tried the canceled call's access"));
    assert_eq!(
        files_in(&vfs)?,
        one_file("/tool.txt", "tool wrote this"),
        "the refused operations left the files as they were"
    );
    Ok(())
}

#[test]
fn a_tool_call_sees_no_file_the_chain_wrote_with_store_write() -> std::result::Result<(), VfsError>
{
    // One call lists every file its access reaches, and another reads
    // the name the chain wrote: the listing is empty, and the read finds
    // no file at that name below the root of the run's handle.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ## Only\n\n\
        ```lua\n\
        store.write('note.txt', 'the chain wrote this')\n\
        local listed = tools.call('test/tools/files', { list = '**' })\n\
        local ok, err = pcall(tools.call, 'test/tools/files', { read = 'note.txt' })\n\
        return listed .. '|' .. tostring(ok) .. '|' .. tostring(err) .. '|' .. store.read('note.txt')\n\
        ```\n";
    let vfs = VfsRef::default();
    let (result, _) = drive(
        files_run(md, &vfs),
        files_answers(&mut Identities::default()),
    );
    assert_eq!(
        text_of(result),
        "[]|false|prompt:12: not found: /note.txt|the chain wrote this"
    );
    assert_eq!(
        files_in(&vfs)?,
        Vec::new(),
        "the run's handle holds no file the chain wrote to its store"
    );
    Ok(())
}

#[test]
fn store_read_sees_no_file_a_tool_call_wrote() -> std::result::Result<(), VfsError> {
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ## Only\n\n\
        ```lua\n\
        tools.call('test/tools/files', { write = 'tool.txt', text = 'a tool call wrote this' })\n\
        local ok, err = pcall(store.read, 'tool.txt')\n\
        return tostring(ok) .. '|' .. err.reason .. '|' .. tostring(store.exists('tool.txt')) .. '|' .. #store.glob('**')\n\
        ```\n";
    let vfs = VfsRef::default();
    let (result, _) = drive(
        files_run(md, &vfs),
        files_answers(&mut Identities::default()),
    );
    assert_eq!(text_of(result), "false|not_found|false|0");
    assert_eq!(
        files_in(&vfs)?,
        one_file("/tool.txt", "a tool call wrote this"),
        "the call's file is in the run's handle"
    );
    Ok(())
}
