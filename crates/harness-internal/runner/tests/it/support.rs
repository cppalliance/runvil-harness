//! Fixtures shared by the runner's suites: a one-section prompt, a run
//! over it, fake performers that answer by script, a Pack registry
//! holding a suite's Packs, and a Host over them.

use std::fmt::Write as _;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::Poll;
use std::time::Duration;

use harness_runner::performers::{InferenceBroker, PieceSink, Timer, ToolPerformer};
use harness_runner::recorder::{
    MemoryRecorder, Record, RunId, RunMeta, RunRecorder, make_recorder_error,
};
use harness_runner::{Host, PromptSet, RunSetup};
use runvil_engine::cancel::CancelHandle;
use runvil_engine::effect::{Round, ToolCallOrigin};
use runvil_engine::model::{
    Completion, CompletionOptions, Message, ModelBinding, ModelCatalog, ToolSchema,
};
use runvil_engine::{BoxFuture, Error};
use runvil_engine::{Prompt, PromptFinder, Run, RunParams, RunSeed, Timestamp};
use runvil_engine::{ToolCatalog, ToolId, ToolInfo, ToolOutput};
use runvil_plugin::{Pack, PackId, PackInfo, PackRegistry, ToolFiles};
use serde_json::{Value, json};

/// The fixed seed every fixture run and recorded meta takes.
pub(crate) const TEST_SEED: RunSeed = RunSeed::from_bytes([7; 32]);

/// Asserts a run's `result` is a cancel: the error whose code is
/// `canceled`.
#[track_caller]
pub(crate) fn assert_canceled(result: &Result<String, Error>) {
    assert!(
        result.as_ref().is_err_and(Error::is_canceled),
        "the run ends canceled: {result:?}"
    );
}

/// The factory of a fixture Pack that shares nothing: it makes the Pack
/// under the Pack id it receives.
pub(crate) type Make = fn(&PackId) -> Result<Arc<dyn Pack>, Error>;

/// A registry holding each of `packs`, an info beside its factory, which
/// a suite builds its Host over.
pub(crate) fn registry(packs: &[(PackInfo, Make)]) -> PackRegistry {
    let mut registry = PackRegistry::new();
    for (info, make) in packs {
        registry.insert(*info, *make).unwrap();
    }
    registry
}

/// A Host that holds every Pack of `packs` and finds its prompts through
/// `prompts`. Its one Rust Plugin puts the suite's filled registry in the
/// place of the empty one the build lends it.
pub(crate) fn host_finding(packs: PackRegistry, prompts: Arc<dyn PromptFinder>) -> Arc<Host> {
    Host::builder(prompts)
        .with_rust_plugin(move |registry| {
            *registry = packs;
            Ok(())
        })
        .build()
}

/// A Host over `packs`. Its finder serves no prompt: a suite parses its
/// own with [`parsed`], so a prompt the Host's Packs cannot serve still
/// reaches the run.
pub(crate) fn host_over(packs: PackRegistry) -> Arc<Host> {
    host_finding(packs, Arc::new(PromptSet::new()))
}

/// A Host with no Pack but the built-ins, whose finder serves no prompt.
pub(crate) fn bare_host() -> Arc<Host> {
    Host::builder(Arc::new(PromptSet::new())).build()
}

/// `source` parsed as the prompt named `prompt`, behind the handle a
/// Harness takes.
pub(crate) fn parsed(source: &str) -> Arc<Prompt> {
    let prompt = Prompt::parse(&|_: &str| Ok(source.to_owned()), "prompt");
    Arc::new(prompt.expect("the suite's prompt parses"))
}

/// A section body that calls the fixture wait tool by its full id and
/// returns its answer: the section parks until the test's tool performer
/// answers.
pub(crate) const WAITS: &str = "return tools.call('tests/runner/wait')";

/// The catalog every fixture run is prepared against: the one wait tool
/// [`WAITS`] calls, so a section reaches it by full id without binding an
/// alias.
fn catalog() -> ToolCatalog {
    let wait = ToolInfo::new(
        ToolId::parse("tests/runner/wait").expect("the wait tool id is valid"),
        "Wait until the test's tool performer answers.",
        json!({ "type": "object", "properties": {} }),
    );
    ToolCatalog::new([wait]).expect("the one-tool catalog is valid")
}

/// A prompt whose sections are `sections`, each a section's name and the
/// Lua it runs as its only block, in order.
fn prompt(sections: &[(&str, &str)]) -> Arc<Prompt> {
    let mut source =
        String::from("---\nname: runner-test\ndescription: a runner fixture\nrunvil: 0\n---\n");
    for (name, lua) in sections {
        write!(source, "\n## {name}\n\n```lua\n{lua}\n```\n").expect("a String takes any write");
    }
    let prompt = Prompt::parse(&|_: &str| Ok(source.clone()), "prompt");
    Arc::new(prompt.expect("the fixture prompt parses"))
}

/// A Pack-free run over `prompt` with a fixed seed and start and the
/// empty object `{}` as its arguments, prepared against the fixture
/// [`catalog`], under the cancel flag `cancel`. Its tool calls work in
/// the params' default filesystem, one empty memory mount at `/`, and
/// its store is the empty one the Engine makes for it.
fn prepared(prompt: Arc<Prompt>, cancel: &CancelHandle) -> Run {
    let ctx = RunParams::new(TEST_SEED, Timestamp::UNIX_EPOCH)
        .with_tools(catalog())
        .with_cancel(cancel.clone());
    Run::new(prompt, "{}", ctx)
}

/// A Pack-free run over `lua` with a fixed seed and start.
pub(crate) fn run(lua: &str) -> Run {
    run_of_sections(&[("Only", lua)])
}

/// A Pack-free run over `sections`, each a section's name and the Lua it
/// runs.
pub(crate) fn run_of_sections(sections: &[(&str, &str)]) -> Run {
    prepared(prompt(sections), &CancelHandle::new())
}

/// [`run`] under the cancel flag `cancel`, which the test also hands the
/// loop.
pub(crate) fn run_with_cancel(lua: &str, cancel: &CancelHandle) -> Run {
    prepared(prompt(&[("Only", lua)]), cancel)
}

/// A Pack-free run over two sections: `## Main` runs `main`, and
/// `## Child` runs `child` when the main spawns it as a task.
pub(crate) fn run_with_child(main: &str, child: &str) -> Run {
    run_with_child_and_cancel(main, child, &CancelHandle::new())
}

/// [`run_with_child`] under the cancel flag `cancel`, which the test also
/// hands the loop.
pub(crate) fn run_with_child_and_cancel(main: &str, child: &str, cancel: &CancelHandle) -> Run {
    let sections = [("Main", main), ("Child", child)];
    prepared(prompt(&sections), cancel)
}

/// A main section that parks on a 30-second timer beside its child: the
/// child's own tool call and the timer are two effects out at once.
pub(crate) const TIMED_MAIN: &str = "local t = tasks.spawn('../Child')\n\
     local _first, _ok, result = tasks.join_any({ t }, { timeout = 30 })\n\
     return result";

/// A performer for every kind that no test here expects to be issued;
/// reaching one is the test's failure.
pub(crate) struct Unused;

impl InferenceBroker for Unused {
    fn models(&self) -> BoxFuture<'static, Result<ModelCatalog, Error>> {
        unreachable!("the effect loop never lists models")
    }

    fn supports_streaming(&self) -> bool {
        false
    }

    fn chat(
        &self,
        _binding: ModelBinding,
        _messages: Vec<Message>,
        _tools: Vec<ToolSchema>,
        _options: CompletionOptions,
        _round: Round,
        _on_piece: Option<PieceSink>,
    ) -> BoxFuture<'static, Result<Box<Completion>, Error>> {
        unreachable!("no test issues a Chat effect")
    }
}

impl ToolPerformer for Unused {
    fn call(
        &self,
        _tool: ToolId,
        _alias: String,
        _origin: ToolCallOrigin,
        _args: Value,
        _files: Arc<dyn ToolFiles>,
    ) -> BoxFuture<'static, Result<ToolOutput, Error>> {
        unreachable!("this test issues no ToolCall effect")
    }
}

impl Timer for Unused {
    fn sleep(&self, _duration: Duration) -> BoxFuture<'static, ()> {
        unreachable!("no test issues a Timer effect")
    }
}

/// The setup of a run the effect loop drives alone, recorded through
/// `recorder`, whose broker and timer no test here reaches; a test that
/// issues `Chat` effects sets its own broker, and one that issues `Timer`
/// effects its own timer.
pub(crate) fn loop_setup(recorder: Arc<dyn RunRecorder>) -> RunSetup {
    RunSetup::new(recorder, Unused, Unused)
}

/// [`loop_setup`] for a run that issues `Timer` effects, which `timer`
/// performs.
pub(crate) fn timed_loop_setup(recorder: Arc<dyn RunRecorder>, timer: Arc<dyn Timer>) -> RunSetup {
    RunSetup {
        timer,
        ..loop_setup(recorder)
    }
}

/// Answers every tool call with the same trusted text.
pub(crate) struct TextTool(pub(crate) &'static str);

impl ToolPerformer for TextTool {
    fn call(
        &self,
        _tool: ToolId,
        _alias: String,
        _origin: ToolCallOrigin,
        _args: Value,
        _files: Arc<dyn ToolFiles>,
    ) -> BoxFuture<'static, Result<ToolOutput, Error>> {
        let text = self.0;
        Box::pin(async move { Ok(ToolOutput::trusted(text)) })
    }
}

/// Stays pending forever: the tool call that never returns.
pub(crate) struct PendingTool;

impl ToolPerformer for PendingTool {
    fn call(
        &self,
        _tool: ToolId,
        _alias: String,
        _origin: ToolCallOrigin,
        _args: Value,
        _files: Arc<dyn ToolFiles>,
    ) -> BoxFuture<'static, Result<ToolOutput, Error>> {
        Box::pin(std::future::pending())
    }
}

/// Panics instead of answering: a performer the Harness lost to a bug. The
/// call panics on its first poll.
pub(crate) struct PanickingTool;

impl ToolPerformer for PanickingTool {
    fn call(
        &self,
        _tool: ToolId,
        _alias: String,
        _origin: ToolCallOrigin,
        _args: Value,
        _files: Arc<dyn ToolFiles>,
    ) -> BoxFuture<'static, Result<ToolOutput, Error>> {
        Box::pin(std::future::poll_fn(
            |_cx| -> Poll<Result<ToolOutput, Error>> {
                panic!("the tool performer panics instead of answering")
            },
        ))
    }
}

/// Ends the run at the recorder before answering, so the loop's next
/// write is refused: the recorder failing under a live run.
pub(crate) struct ClosingTool {
    pub(crate) recorder: Arc<MemoryRecorder>,
    pub(crate) run_id: RunId,
}

impl ToolPerformer for ClosingTool {
    fn call(
        &self,
        _tool: ToolId,
        _alias: String,
        _origin: ToolCallOrigin,
        _args: Value,
        _files: Arc<dyn ToolFiles>,
    ) -> BoxFuture<'static, Result<ToolOutput, Error>> {
        let recorder = Arc::clone(&self.recorder);
        let run_id = self.run_id;
        Box::pin(async move {
            recorder
                .end_run(run_id, Err(Error::canceled()))
                .await
                .expect("the open run ends");
            Ok(ToolOutput::trusted("late"))
        })
    }
}

/// A recorder that refuses one chosen call and keeps the rest in memory:
/// the recorder failing under a live run. Calls count from one across
/// `begin_run`, `append`, and `end_run` in the order they reach it, and a
/// refused call records nothing. A test that drives the loop alone begins
/// its run on `inner()`, which counts nothing, so the count starts at the
/// loop's first write.
pub(crate) struct FailingRecorder {
    inner: MemoryRecorder,
    fail_on: usize,
    calls: AtomicUsize,
    begun: Mutex<Vec<RunId>>,
}

impl FailingRecorder {
    /// A recorder that refuses its `call`th call, counting from one.
    pub(crate) fn failing_on(call: usize) -> Self {
        Self {
            inner: MemoryRecorder::new(),
            fail_on: call,
            calls: AtomicUsize::new(0),
            begun: Mutex::new(Vec::new()),
        }
    }

    /// The runs this recorder issued through its own `begin_run`, in
    /// order.
    pub(crate) fn begun(&self) -> Vec<RunId> {
        self.begun.lock().unwrap().clone()
    }

    /// A recorder that refuses nothing, for counting the calls of a run.
    pub(crate) fn never_failing() -> Self {
        Self::failing_on(usize::MAX)
    }

    /// The calls that have reached the recorder, refused one included.
    pub(crate) fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    /// The memory recorder behind the wrapper, which holds what was kept.
    pub(crate) fn inner(&self) -> &MemoryRecorder {
        &self.inner
    }

    fn count(&self) -> Result<(), runvil_engine::Error> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        if call == self.fail_on {
            return Err(make_recorder_error(format!(
                "the fixture recorder refuses call {call}"
            )));
        }
        Ok(())
    }
}

impl RunRecorder for FailingRecorder {
    fn begin_run(&self, meta: RunMeta) -> BoxFuture<'_, Result<RunId, Error>> {
        Box::pin(async move {
            self.count()?;
            let run = self.inner.begin_run(meta).await?;
            self.begun.lock().unwrap().push(run);
            Ok(run)
        })
    }

    fn append(&self, run: RunId, record: Record) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(async move {
            self.count()?;
            self.inner.append(run, record).await
        })
    }

    fn end_run(
        &self,
        run: RunId,
        result: Result<String, Error>,
    ) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(async move {
            self.count()?;
            self.inner.end_run(run, result).await
        })
    }
}

/// Raises its flag when dropped: how a test sees a future torn down.
pub(crate) struct RaiseOnDrop(pub(crate) Arc<AtomicBool>);

impl Drop for RaiseOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// Never fires, and raises `dropped` when its sleep is torn down: the
/// timer a cancel or an abort must reach.
#[derive(Default)]
pub(crate) struct PendingTimer {
    pub(crate) dropped: Arc<AtomicBool>,
}

impl Timer for PendingTimer {
    fn sleep(&self, _duration: Duration) -> BoxFuture<'static, ()> {
        let raise = RaiseOnDrop(Arc::clone(&self.dropped));
        Box::pin(async move {
            let _raise = raise;
            std::future::pending::<()>().await;
        })
    }
}

/// Parks every tool call until the test adds a permit to `gate`, then
/// answers it: a tool call that stays out for exactly as long as the test
/// says, with no timer involved.
pub(crate) struct GatedTool {
    pub(crate) gate: Arc<tokio::sync::Semaphore>,
}

impl ToolPerformer for GatedTool {
    fn call(
        &self,
        _tool: ToolId,
        _alias: String,
        _origin: ToolCallOrigin,
        _args: Value,
        _files: Arc<dyn ToolFiles>,
    ) -> BoxFuture<'static, Result<ToolOutput, Error>> {
        let gate = Arc::clone(&self.gate);
        Box::pin(async move {
            gate.acquire()
                .await
                .expect("the gate semaphore is never closed")
                .forget();
            Ok(ToolOutput::trusted("opened"))
        })
    }
}
