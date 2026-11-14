//! Guarding and watching the files a run's tool calls work in, and the
//! run's own store, which shares no file with them. The loop here
//! performs each tool call itself, through the call's own access.

use std::sync::{Arc, Mutex};

use runvil_engine::effect::{Effect, EffectAnswer, IssuedEffect};
use runvil_engine::vfs::{
    Access, MemoryBackend, Mode, ModePolicy, Op, OpEvent, Origin, VfsError, VfsOp, VfsRef,
    VfsRefBuilder, perform_vfs_op,
};
use runvil_engine::{Run, Step, ToolCatalog, ToolOutput};
use serde_json::{Value, json};

use super::support::{Built, Scenario, TestResult, greeter, note, tool_info};

/// The id of the one tool a run here may call, which [`drive`] performs.
const KEEP: &str = "example/note/keep";

/// What a test has [`KEEP`] do with one call's access and arguments: the
/// text the tool answers with.
type Perform = fn(&Access, &Value) -> TestResult<String>;

/// A run this module's own loop stepped to its end.
struct Ran {
    /// The run's final text.
    text: String,
    /// Each `Vfs` effect's store view, in issue order, kept once the loop
    /// had performed the effect through it.
    views: Vec<Arc<Access>>,
    /// The run itself, at `Done` and not yet dropped.
    run: Run,
}

/// Runs `source` over `vfs` with [`KEEP`] as its one catalog tool, as a
/// caller's loop does: it performs each store effect through that
/// effect's view, and each tool call through `tool`, which it lends the
/// call's access.
fn drive(vfs: &VfsRef, source: &str, tool: Perform) -> TestResult<Ran> {
    let catalog = ToolCatalog::new([tool_info(KEEP, json!({ "type": "object" }))?])?;
    let vfs = vfs.clone();
    let scenario = Scenario::default()
        .params(move |ctx| ctx.with_vfs(vfs.clone()).with_tools(catalog.clone()));
    let Built { prompt, ctx } = scenario.build(source)?;
    let mut run = Run::new(Arc::new(prompt), "{}", ctx);
    let mut views = Vec::new();
    loop {
        // Each prompt here runs one chain to its end, so no step
        // withdraws an effect.
        let effects = match run.step() {
            Step::Pending { effects, .. } => effects,
            Step::Done { result, .. } => {
                return Ok(Ran {
                    text: result?,
                    views,
                    run,
                });
            }
        };
        for IssuedEffect { id, effect, .. } in effects {
            let answer = match effect {
                Effect::Vfs { access, op } => {
                    let outcome = perform_vfs_op(&access, op);
                    views.push(access);
                    EffectAnswer::Vfs(outcome)
                }
                Effect::ToolCall { access, args, .. } => {
                    EffectAnswer::ToolCall(Ok(ToolOutput::trusted(tool(&access, &args)?)))
                }
                other => return Err(format!("no prompt here asks for {other:?}").into()),
            };
            run.resume(id, answer);
        }
    }
}

/// A prompt that has [`KEEP`] keep `hello` and returns the tool's answer.
fn keeps_hello() -> String {
    greeter("return tools.call('example/note/keep', { text = 'hello' })")
}

/// [`KEEP`] as a tool that keeps a note: it writes the call's `text` to
/// `/note.md` through `access` and answers with the file read back.
fn keep_note(access: &Access, args: &Value) -> TestResult<String> {
    let text = args["text"].as_str().ok_or("the call passes its `text`")?;
    access.write("/note.md", text.as_bytes())?;
    Ok(access.read_string("/note.md")?)
}

/// [`KEEP`] under a prompt that calls no tool: a call fails the test.
fn no_tool(_: &Access, _: &Value) -> TestResult<String> {
    Err("the prompt calls no tool".into())
}

/// Each watched operation's kind, path, and origin.
type Watched = Arc<Mutex<Vec<(Op, String, Origin)>>>;

/// A builder holding one empty memory mount at `/` and a watcher, beside
/// the operations the watcher saw.
fn watched() -> (VfsRefBuilder, Watched) {
    let log = Watched::default();
    let sink_log = Arc::clone(&log);
    let builder =
        VfsRef::builder()
            .mount("/", MemoryBackend::new())
            .on_op(move |event: OpEvent<'_>| {
                if let Ok(mut entries) = sink_log.lock() {
                    entries.push((event.op(), event.path().to_string(), event.origin().clone()));
                }
            });
    (builder, log)
}

#[test]
fn agent_mode_lets_a_tool_call_write_and_ask_mode_refuses_a_later_write() -> TestResult {
    let policy = ModePolicy::new(Mode::Agent);
    let mode = policy.handle();
    let vfs = VfsRef::builder()
        .mount("/", MemoryBackend::new())
        .policy(policy)
        .build();

    assert_eq!(drive(&vfs, &keeps_hello(), keep_note)?.text, "hello");

    mode.set(Mode::Ask);
    let access = vfs.acquire(Origin::new("caller"))?;
    let refused = access.write("/note.md", b"later");
    assert!(
        matches!(&refused, Err(VfsError::ApprovalRequired { path, .. }) if path == "/note.md"),
        "{refused:?}"
    );
    assert_eq!(access.read_string("/note.md")?, "hello");
    Ok(())
}

#[test]
fn an_operation_watcher_sees_a_tool_calls_write_then_read_under_the_calls_origin() -> TestResult {
    let (builder, log) = watched();

    assert_eq!(
        drive(&builder.build(), &keeps_hello(), keep_note)?.text,
        "hello"
    );

    let entries = log.lock().map_err(|_| "the watcher panicked")?;
    let [(Op::Write, written, first), (Op::Read, read, second)] = entries.as_slice() else {
        return Err(format!("the watcher saw {entries:?}").into());
    };
    assert_eq!((written.as_str(), read.as_str()), ("/note.md", "/note.md"));
    assert_eq!(first, second, "one call's operations share its origin");
    assert_eq!(
        (first.label.as_str(), first.file.as_str()),
        (KEEP, "greeter"),
        "a tool call's origin holds the tool's id and the prompt's frontmatter `name`"
    );
    Ok(())
}

#[test]
fn a_mode_policy_and_an_operation_watcher_never_see_a_store_operation() -> TestResult {
    let (builder, log) = watched();
    let vfs = builder.policy(ModePolicy::new(Mode::Ask)).build();

    assert_eq!(
        drive(&vfs, &note("'hello'"), no_tool)?.text,
        "hello",
        "the Ask mode, which refuses every change to the caller's files, lets the store write"
    );

    let entries = log.lock().map_err(|_| "the watcher panicked")?;
    assert!(entries.is_empty(), "the watcher saw {entries:?}");
    Ok(())
}

#[test]
fn each_run_starts_with_an_empty_store_that_shares_no_file_with_the_callers_filesystem()
-> TestResult {
    let vfs = VfsRef::default();
    vfs.acquire(Origin::new("caller"))?
        .write("/notes.md", b"from the caller")?;
    let source = greeter(
        "
        local found = store.exists('notes.md')
        local held = #store.glob('**')
        store.write('notes.md', 'from the run')
        store.write('summary.md', 'from the run')
        return tostring(found) .. ' ' .. held",
    );

    // The second run finds neither the caller's file nor the first run's.
    for _ in 0..2 {
        assert_eq!(drive(&vfs, &source, no_tool)?.text, "false 0");
    }

    let access = vfs.acquire(Origin::new("caller"))?;
    assert_eq!(access.read_string("/notes.md")?, "from the caller");
    let names: Vec<String> = access
        .list("/")?
        .into_iter()
        .map(|entry| entry.name)
        .collect();
    assert_eq!(names, ["notes.md"], "no run's store write is a file here");
    Ok(())
}

#[test]
fn a_tool_call_and_the_runs_store_share_no_file() -> TestResult {
    let source = greeter(
        "
        store.write('note.md', 'from the store')
        local found = tools.call('example/note/keep')
        return found .. ' ' .. store.read('note.md')",
    );
    // The tool looks for the store's note, then writes one of its own.
    let ran = drive(&VfsRef::default(), &source, |access, _| {
        let found = access.exists("/note.md")?;
        access.write("/note.md", b"from the tool")?;
        Ok(found.to_string())
    })?;
    assert_eq!(ran.text, "false from the store");
    Ok(())
}

#[test]
fn a_store_view_is_the_callers_one_way_to_the_runs_store_and_refuses_every_operation_once_the_run_is_done()
-> TestResult {
    let vfs = VfsRef::default();
    let ran = drive(&vfs, &note("'hello'"), no_tool)?;
    assert_eq!(
        ran.text, "hello",
        "the read through its view returns what the write through its view stored"
    );
    assert!(
        vfs.acquire(Origin::new("caller"))?.list("/")?.is_empty(),
        "the filesystem the caller gave the run holds none of the store"
    );

    let path = || "note.md".to_owned();
    let ops = [
        VfsOp::Write {
            path: path(),
            contents: "late".to_owned(),
        },
        VfsOp::Append {
            path: path(),
            contents: "late".to_owned(),
        },
        VfsOp::Read {
            path: path(),
            start: None,
            end: None,
        },
        VfsOp::ReadNumbered {
            path: path(),
            start: None,
            end: None,
        },
        VfsOp::StrReplace {
            path: path(),
            old: "hello".to_owned(),
            new: "late".to_owned(),
        },
        VfsOp::Delete { path: path() },
        VfsOp::Glob {
            pattern: "*.md".to_owned(),
        },
        VfsOp::Exists { path: path() },
    ];
    assert_eq!(ran.views.len(), 2, "the write's view and the read's");
    for view in &ran.views {
        for op in &ops {
            let refused = perform_vfs_op(view, op.clone());
            assert!(
                matches!(refused, Err(VfsError::ScopeEnded { .. })),
                "{op:?}: {refused:?}"
            );
        }
    }
    // The run outlived every refusal above, so `Done` ended its scope,
    // not its drop.
    drop(ran.run);
    Ok(())
}
