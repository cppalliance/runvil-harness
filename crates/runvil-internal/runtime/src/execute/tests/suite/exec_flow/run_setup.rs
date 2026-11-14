//! What a run starts with: its own empty store, default params, and the
//! tool and model bindings its frontmatter declares.

use super::*;

use crate::execute::run::EffectRecord;
use crate::execute::tests::fixtures::echo_tool;
use runvil_types::error::ErrorCode;
use runvil_types::event::Event;
use runvil_vfs::VfsRef;

#[test]
fn default_params_run_a_pack_free_prompt() {
    // A prompt declaring no Packs runs under default params: no
    // catalog, no roster, no client, no real roots.
    let trace = Scenario::new(one_section("return 'no Packs'")).run();
    assert_eq!(trace.text(), "no Packs");
}

#[test]
fn a_run_under_default_params_writes_and_reads_its_store() {
    // The Engine makes every run a store of its own, so a run that uses
    // the store needs nothing set up for it: one section's write is the
    // next section's read.
    let source = prompt(
        "",
        &[
            ("First", "store.write('default.txt', 'stock')"),
            ("Second", "return store.read('default.txt')"),
        ],
    );
    assert_eq!(Scenario::new(source).run().text(), "stock");
}

#[test]
fn every_run_starts_with_an_empty_store() {
    // Two runs of one prompt over one handle: each counts no file in its
    // store at its start, so the second run sees nothing of the file the
    // first run wrote.
    let source = one_section(
        "
        local found = store.glob('**')
        store.write('left.txt', 'written by this run')
        return #found .. ' at the start, ' .. #store.glob('**') .. ' at the end'",
    );
    let vfs = VfsRef::default();
    for run in ["first", "second"] {
        let trace = Scenario::new(&source).vfs(vfs.clone()).run();
        assert_eq!(
            trace.text(),
            "0 at the start, 1 at the end",
            "the {run} run over the one handle"
        );
    }
}

/// A filesystem that refuses every access.
struct RefusingFiles;

impl runvil_vfs::Vfs for RefusingFiles {
    fn acquire(
        &mut self,
        _cx: &runvil_vfs::AcquireContext,
    ) -> Result<Box<dyn runvil_vfs::VfsAccess>, runvil_vfs::VfsError> {
        Err(runvil_vfs::VfsError::Backend {
            message: "the files are down".to_owned(),
        })
    }

    fn release(&mut self, _id: runvil_vfs::ExecId) -> Result<(), runvil_vfs::VfsError> {
        Ok(())
    }
}

#[test]
fn a_run_whose_files_refuse_its_root_access_ends_with_the_files_failure() {
    // The root chain takes its access from the files the caller gave the
    // run. A refusal ends the run with the files' own failure, which names
    // no store: the run's store is its own, and nothing asked it.
    let trace = Scenario::new(one_section("return 'unreachable'"))
        .vfs(VfsRef::new(RefusingFiles))
        .run();
    let error = trace.error();
    assert_eq!(error.code, ErrorCode::Backend, "{error:?}");
    assert_eq!(
        error.message,
        "file access failed: backend failure: the files are down"
    );
}

/// A filesystem that serves its first access, the run's own, and refuses
/// every access after it.
struct RefusingForks {
    files: runvil_vfs::MemoryBackend,
    served: bool,
}

impl RefusingForks {
    fn handle() -> VfsRef {
        VfsRef::new(RefusingForks {
            files: runvil_vfs::MemoryBackend::new(),
            served: false,
        })
    }
}

impl runvil_vfs::Vfs for RefusingForks {
    fn acquire(
        &mut self,
        cx: &runvil_vfs::AcquireContext,
    ) -> Result<Box<dyn runvil_vfs::VfsAccess>, runvil_vfs::VfsError> {
        if self.served {
            return Err(runvil_vfs::VfsError::Backend {
                message: "the files serve one access".to_owned(),
            });
        }
        self.served = true;
        self.files.acquire(cx)
    }

    fn release(&mut self, id: runvil_vfs::ExecId) -> Result<(), runvil_vfs::VfsError> {
        self.files.release(id)
    }
}

/// The text a block returns for a caught failure `err`: its kind, its
/// code, and whether a store call still works after it.
const DESCRIBE_CAUGHT: &str = "
    store.write('after.txt', 'the store still works')
    return tostring(ok) .. '|' .. err.kind .. '|' .. err.code .. '|' .. store.read('after.txt')";

#[test]
fn a_task_whose_files_refuse_its_fork_fails_the_spawn_and_starts_no_task() {
    // A task works under an access forked from its spawner's. Files that
    // refuse the fork fail the spawn at its call, with the files' own
    // failure, and the run's store, which no file backs, is untouched.
    let main = format!("local ok, err = pcall(tasks.spawn, '../Child'){DESCRIBE_CAUGHT}");
    let caught = prompt(
        "",
        &[("Main", main.as_str()), ("Child", "return 'unreachable'")],
    );
    let trace = Scenario::new(caught).vfs(RefusingForks::handle()).run();
    assert_eq!(trace.text(), "false|store|backend|the store still works");
    assert!(
        !trace
            .events()
            .iter()
            .any(|event| matches!(event, Event::TaskStarted { .. })),
        "a refused spawn starts no task: {:?}",
        trace.events()
    );

    let uncaught = prompt(
        "",
        &[
            ("Main", "tasks.spawn('../Child')"),
            ("Child", "return 'unreachable'"),
        ],
    );
    let trace = Scenario::new(uncaught).vfs(RefusingForks::handle()).run();
    let error = trace.error();
    assert_eq!(error.code, ErrorCode::Backend, "{error:?}");
    assert_eq!(
        error.message,
        "file access failed: backend failure: the files serve one access"
    );
}

#[test]
fn a_tool_call_whose_files_refuse_its_fork_fails_the_call_and_issues_no_effect() {
    // A tool call works under an access forked from its chain's. Files
    // that refuse the fork fail the call at its caller before any effect
    // is issued for it.
    let main = format!(
        "local ok, err = pcall(tools.call, 'tools/echo', {{ value = 'x' }}){DESCRIBE_CAUGHT}"
    );
    let caught = prompt("uses: [tools]", &[("Main", main.as_str())]);
    let trace = Scenario::new(caught)
        .tool(echo_tool())
        .vfs(RefusingForks::handle())
        .run();
    assert_eq!(trace.text(), "false|store|backend|the store still works");
    assert!(
        !trace
            .effects()
            .iter()
            .any(|(effect, _)| matches!(effect, EffectRecord::ToolCall { .. })),
        "a refused fork issues no tool call: {:?}",
        trace.effects()
    );

    let uncaught = prompt(
        "uses: [tools]",
        &[("Main", "return tools.call('tools/echo', { value = 'x' })")],
    );
    let trace = Scenario::new(uncaught)
        .tool(echo_tool())
        .vfs(RefusingForks::handle())
        .run();
    let error = trace.error();
    assert_eq!(error.code, ErrorCode::Backend, "{error:?}");
    assert_eq!(
        error.message,
        "file access failed: backend failure: the files serve one access"
    );
}

#[test]
fn offering_a_tool_its_present_pack_does_not_offer_fails_the_section() {
    // An id whose Pack is active but contributed no such tool is no
    // catalog tool of the run: the offer fails the section before any
    // round advertises it. The catalog holds the Pack's `echo`, never
    // `search`.
    let source = prompt(
        "uses: [tools]",
        &[(
            "Only",
            "
            tools.offer('tools/search')
            return 'unreachable'",
        )],
    );
    let trace = Scenario::new(source).tool(echo_tool()).run();
    let error = trace.error();
    assert_eq!(error.code, ErrorCode::LuaRuntime, "{error}");
    assert!(
        error
            .to_string()
            .contains("tools.offer: \"tools/search\" is not a tool in this run"),
        "the failure names the id: {error}"
    );
}
