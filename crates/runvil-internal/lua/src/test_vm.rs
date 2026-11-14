//! The lua test hub: the test VM builder, a section VM set up the way
//! section setup builds one, through construction, the run's limits,
//! Engine injection, and the persistent Engine globals, with the control
//! globals and coroutine shims on request, handed back with named handles
//! to what the VM shares; the access fixture; the block driver, which
//! runs a block as a coroutine and answers its store requests; the
//! one-shot [`run_chunk`] over it; the tool fixtures as data; and the
//! scope a VM's offers resolve to.

use std::collections::BTreeMap;
use std::num::NonZeroU32;
use std::ops::Deref;
use std::sync::{Arc, Mutex};

use mlua::FromLuaMulti;
use runvil_model_client::model::{ModelBinding, ModelSet};
use runvil_types::emitter::Emitter;
use runvil_types::error::place;
use runvil_types::seed::RunSeed;
use runvil_types::tools::{ToolCatalog, ToolId, ToolInfo};
use runvil_types::untrusted::GuardNonce;
use runvil_vfs::{Access, Origin, Store, VfsRef};
use serde_json::{Value as Json, json};

use crate::error::{Result, UNNAMED_FILE, make_lua_runtime_error};
use crate::tests::recording::null_emitter;
use crate::{
    Answer, CoroStep, LuaBlockResult, LuaProgram, OfferedTool, Request, RosterEntry, SectionRead,
    SectionVm, ToolSet, YieldParse, make_store_failure_error, run_store_op, store_error_message,
};

/// The one guard-nonce seed of every test VM.
pub(crate) const TEST_SEED: RunSeed = RunSeed::from_bytes([0x73; 32]);

/// The section a test VM reports under unless `section` names another.
pub(crate) const TEST_SECTION: &str = "Test";

/// The guard nonce [`TEST_SEED`] mints, for a test that drives a section
/// VM's construction itself.
#[track_caller]
pub(crate) fn test_nonce() -> GuardNonce {
    GuardNonce::from_seed(&TEST_SEED)
}

/// What the block driver answers a test VM's store requests with: a
/// store, and the access capability whose identity views it.
#[derive(Clone)]
pub(crate) struct ScopedAccess {
    /// The capability under whose identity the driver views the store.
    pub(crate) access: Arc<Access>,
    /// The store the driver's views operate on.
    pub(crate) store: Store,
}

impl ScopedAccess {
    /// A new, empty store, beside the root capability a fresh default
    /// handle acquires under `origin`.
    #[track_caller]
    pub(crate) fn acquire(origin: Origin) -> ScopedAccess {
        let access = VfsRef::default()
            .acquire(origin)
            .expect("the test handle acquires");
        ScopedAccess {
            access: Arc::new(access),
            store: Store::new(),
        }
    }

    /// A view of the store under this access: the view the block driver
    /// performs each store request through, and the one a test seeds and
    /// reads through under the same identity.
    #[track_caller]
    pub(crate) fn store_view(&self) -> Access {
        self.store
            .view(&self.access)
            .expect("a live access views the store")
    }
}

/// A fresh store beside a fresh access capability: the vended identity is
/// the test's own, so seeding through the store view and the driver's
/// store ops never meet a second live identity.
#[track_caller]
pub(crate) fn fresh_access() -> ScopedAccess {
    ScopedAccess::acquire(Origin::new("lua test fixture"))
}

/// Runs `program` as a block on `vm` to its end, answering each store
/// request the block yields through `access`'s store view, the way the
/// executor answers one: the operation's outcome, or its failure under the
/// model-facing message, placed on the calling line.
///
/// # Errors
/// Returns the block's own failure, a store failure it did not catch
/// included.
///
/// # Panics
/// Panics when the block yields any request but a store one: the driver
/// answers no other.
#[track_caller]
pub(crate) fn drive_block(
    vm: &SectionVm,
    program: &LuaProgram,
    access: &ScopedAccess,
) -> Result<LuaBlockResult> {
    let view = access.store_view();
    let mut step = vm.start_block_coro(program)?;
    loop {
        let (thread, values) = match step {
            CoroStep::Done(result) => return Ok(result),
            CoroStep::Yielded(thread, values) => (thread, values),
        };
        let answer = match vm.request_from_yield(&values) {
            YieldParse::Request(Request::Store { op }) => run_store_op(&view, op.clone())
                .map(Answer::Store)
                .map_err(|cause| {
                    let failure =
                        make_store_failure_error(&cause, store_error_message(&op, &cause));
                    place(failure, UNNAMED_FILE, vm.call_line())
                }),
            YieldParse::Request(other) => {
                panic!("the block driver answers store requests alone, got {other:?}")
            }
            YieldParse::Refused(refusal) => Err(refusal),
            YieldParse::Malformed(failure) => return Err(failure),
        };
        step = vm.resume_block_coro_answer(program, &thread, answer)?;
    }
}

/// Compiles `source` as the one block of a test prompt.
#[track_caller]
pub(crate) fn block(source: &str) -> LuaProgram {
    LuaProgram::compile(source, UNNAMED_FILE, "test program", NonZeroU32::MIN)
        .expect("the test block compiles")
}

/// The text a block ended with: `None` for a block that ran off its end
/// or returned no text.
///
/// # Errors
/// Returns a `LuaRuntime` failure for a block that ended in a `jump`.
pub(crate) fn returned_text(result: LuaBlockResult) -> Result<Option<String>> {
    match result {
        LuaBlockResult::RanOff => Ok(None),
        LuaBlockResult::Returned(value) => Ok(value),
        LuaBlockResult::Jump(path, _) => {
            Err(make_lua_runtime_error(format!("unexpected jump to {path}")))
        }
    }
}

/// The result of running a section's Lua block with [`run_chunk`].
#[derive(Debug, Clone)]
pub(crate) struct LuaOutcome {
    /// The chunk's top-level return value, if it returned one (the finish case).
    pub(crate) returned: Option<String>,
    /// The `var` table after the block ran, as JSON, for prose substitution.
    pub(crate) var: Json,
}

/// Runs a section's Lua chunk as a block with `args` (a JSON object) and
/// `sys` exposed, a writable `var` table available, and every yield shim
/// installed, returning the block's return value and the final `var`. The
/// block driver answers its `store` calls through `access`. `log` reports
/// through `emitter` under `section`; direct `print` is unavailable.
///
/// The `tools` table is the same validating one every section VM installs,
/// over an empty shared set: a chunk that calls `tools.offer(...)` fails
/// loudly because the run offers no tool.
///
/// # Errors
/// Returns the failure if the sandbox cannot be built, `args`, `sys`, or
/// `var` cannot be bridged, the block fails (a `store` failure it did not
/// catch is that store failure), or it returns a value that cannot be
/// rendered as a result string.
pub(crate) fn run_chunk(
    source: &str,
    args: &Json,
    sys: &Json,
    access: &ScopedAccess,
    emitter: &Emitter,
    section: &str,
) -> Result<LuaOutcome> {
    let mut vm = SectionVm::new(
        &GuardNonce::from_seed(&RunSeed::from_bytes([0; 32])),
        emitter,
        section,
    )?
    .inject_test_values(args, sys)
    .map_err(|failure| failure.teardown(emitter, section))?;
    vm.install_engine_globals(emitter, section)?;
    vm.install_control_globals(|path: String, _| {
        Ok::<SectionRead, std::convert::Infallible>(SectionRead::Missing(make_lua_runtime_error(
            format!("section `{path}` not found"),
        )))
    })?;
    vm.install_coro_shims(1)?;
    let returned = returned_text(drive_block(&vm, &block(source), access)?)?;
    let var = vm.var()?;
    Ok(LuaOutcome { returned, var })
}

/// A tool as data under `id`, described as `<id> tool`.
#[track_caller]
pub(crate) fn tool_at(id: &str) -> ToolInfo {
    ToolInfo::new(
        ToolId::parse(id).expect("a fixture tool id is a valid tool id"),
        format!("{id} tool"),
        json!({ "type": "object" }),
    )
}

/// The `tools/echo` fixture tool as data. No implementation is ever
/// called: the VM binds `ToolInfo` values and dispatch sees canned outputs.
#[track_caller]
pub(crate) fn echo_tool() -> ToolInfo {
    tool_at("tools/echo")
}

/// The tool set whose run catalog is `tools`, with nothing declared and no
/// roster.
#[track_caller]
pub(crate) fn catalog_set(tools: impl IntoIterator<Item = ToolInfo>) -> ToolSet {
    ToolSet::new(
        Vec::new(),
        BTreeMap::new(),
        ToolCatalog::new(tools).expect("a fixture catalog's ids differ"),
    )
}

/// The run the offer tests share: `web` (`web/fetch`, `web/search`) and
/// `user-input` (`user-input/ask`) declared; `gh` (`gh/issues.list`,
/// `gh/search`) in the catalog but undeclared; `subagents`, with no tool,
/// able to serve; and `mcp` unable to, because "the mcp server is down".
/// Every tool takes its default wire name.
#[track_caller]
pub(crate) fn offer_set() -> ToolSet {
    let pack = |name: &str| {
        runvil_types::packs::PackId::parse(name).expect("a fixture Pack id is a valid Pack id")
    };
    let usable = |name: &str| {
        (
            pack(name),
            RosterEntry::Usable {
                vendor: "acme".to_owned(),
            },
        )
    };
    ToolSet::new(
        vec![pack("web"), pack("user-input")],
        [
            usable("gh"),
            (
                pack("mcp"),
                RosterEntry::Unavailable {
                    reason: "the mcp server is down".to_owned(),
                },
            ),
            usable("subagents"),
            usable("user-input"),
            usable("web"),
        ]
        .into_iter()
        .collect(),
        ToolCatalog::new(
            [
                "gh/issues.list",
                "gh/search",
                "user-input/ask",
                "web/fetch",
                "web/search",
            ]
            .map(tool_at),
        )
        .expect("the fixture ids differ"),
    )
}

/// The offers `vm` recorded, each as `<mode>:<tool or Pack id>`, the mode
/// `static` or `dynamic`, in call order.
#[track_caller]
pub(crate) fn offers(vm: &SectionVm) -> Vec<String> {
    vm.offers()
        .lock()
        .expect("the offers mutex is not poisoned")
        .offers
        .iter()
        .map(|offer| {
            let mode = match offer.mode {
                crate::OfferMode::Static => "static",
                crate::OfferMode::Dynamic => "dynamic",
            };
            match &offer.target {
                crate::scope::OfferTarget::Tool(id) => format!("{mode}:{id}"),
                crate::scope::OfferTarget::Pack(pack) => format!("{mode}:{pack}"),
            }
        })
        .collect()
}

/// What `source` raises on `vm`, as text, failing the test when it runs
/// clean.
#[track_caller]
pub(crate) fn raised(vm: &SectionVm, source: &str) -> String {
    vm.lua()
        .load(format!(
            "local ok, err = pcall(function() {source} end)\n\
             assert(not ok, 'the chunk must raise')\n\
             return tostring(err)"
        ))
        .eval()
        .unwrap_or_else(|error| panic!("{source}: {error}"))
}

/// A test VM whose `tools` table is installed over `set`.
#[track_caller]
pub(crate) fn lua_over(set: ToolSet) -> BuiltVm {
    TestVm::new().tools(set).build()
}

/// The wire names of the tools `vm`'s static offers name, in order: a
/// catalog tool's wire name and an adhoc tool's name.
#[track_caller]
pub(crate) fn scoped(vm: &SectionVm) -> Vec<String> {
    let (tool_set, offers) = vm.tool_set_and_offers();
    crate::current_static_set(&tool_set, &offers, vm.adhoc_tools())
        .expect("the scope resolves")
        .iter()
        .map(|tool| match tool {
            OfferedTool::Catalog(info) => crate::wire_name_of(info).into_owned(),
            OfferedTool::Adhoc(binding) => binding.alias().to_owned(),
        })
        .collect()
}

/// The ids of the tools `vm`'s dynamic offers name, in order.
#[track_caller]
pub(crate) fn dynamic(vm: &SectionVm) -> Vec<String> {
    let (tool_set, offers) = vm.tool_set_and_offers();
    let adhoc = vm.adhoc_tools();
    let bound = crate::current_static_set(&tool_set, &offers, adhoc).expect("the scope resolves");
    crate::current_dynamic_set(&tool_set, &offers, adhoc, &bound)
        .expect("the dynamic set resolves")
        .iter()
        .map(|tool| tool.id().to_string())
        .collect()
}

type Selection = Arc<Mutex<Option<ModelBinding>>>;

/// A section VM's setup as data. Unset, the VM shares an empty tool and
/// model set, reports nowhere under [`TEST_SECTION`], is driven over a
/// fresh store, and gets an empty `args`, `sys`, and `var`.
pub(crate) struct TestVm<'a> {
    tools: ToolSet,
    models: ModelSet,
    access: Option<ScopedAccess>,
    emitter: Emitter,
    section: &'a str,
    args: Option<&'a Json>,
    sys: Option<&'a Json>,
    var: Option<&'a Json>,
    limits: Option<(usize, u32)>,
    raw_model_ids: bool,
    /// The reader behind the `prompt` reads, set when the VM takes the
    /// control globals and the coroutine shims.
    shims: Option<TestRead>,
}

/// The reader behind a test VM's `prompt` reads: the path as written, and
/// the library whose code made the read, if any.
type TestRead = Box<dyn Fn(String, Option<crate::LibraryKey>) -> SectionRead + Send + Sync>;

impl<'a> TestVm<'a> {
    #[track_caller]
    pub(crate) fn new() -> Self {
        Self {
            tools: ToolSet::default(),
            models: ModelSet::default(),
            access: None,
            emitter: null_emitter(),
            section: TEST_SECTION,
            args: None,
            sys: None,
            var: None,
            limits: None,
            raw_model_ids: false,
            shims: None,
        }
    }

    /// The run's tool set, shared with the VM.
    #[track_caller]
    pub(crate) fn tools(mut self, tools: ToolSet) -> Self {
        self.tools = tools;
        self
    }

    /// The run's model set, shared with the VM.
    #[track_caller]
    pub(crate) fn models(mut self, models: ModelSet) -> Self {
        self.models = models;
        self
    }

    /// The store the block driver answers the VM's store requests in,
    /// beside the access whose identity views it.
    #[track_caller]
    pub(crate) fn access(mut self, access: ScopedAccess) -> Self {
        self.access = Some(access);
        self
    }

    /// The emitter every setup step and the teardown report through.
    #[track_caller]
    pub(crate) fn emitter(mut self, emitter: &Emitter) -> Self {
        self.emitter = emitter.clone();
        self
    }

    /// The section every report names.
    #[track_caller]
    pub(crate) fn section(mut self, section: &'a str) -> Self {
        self.section = section;
        self
    }

    /// The chain's arguments, a JSON object, which Engine injection
    /// installs as the read-only `args` table; unset, an empty object.
    #[track_caller]
    pub(crate) fn args(mut self, args: &'a Json) -> Self {
        self.args = Some(args);
        self
    }

    /// The `sys` value Engine injection seals.
    #[track_caller]
    pub(crate) fn sys(mut self, sys: &'a Json) -> Self {
        self.sys = Some(sys);
        self
    }

    /// The value the guarded `var` starts from.
    #[track_caller]
    pub(crate) fn var(mut self, var: &'a Json) -> Self {
        self.var = Some(var);
        self
    }

    /// The run's Lua heap ceiling and `log()` event budget, applied before
    /// Engine injection.
    #[track_caller]
    pub(crate) fn limits(mut self, memory_bytes: usize, log_events: u32) -> Self {
        self.limits = Some((memory_bytes, log_events));
        self
    }

    /// The raw-model-id fallback `models.get` takes for an undeclared alias.
    #[track_caller]
    pub(crate) fn raw_model_ids(mut self) -> Self {
        self.raw_model_ids = true;
        self
    }

    /// The control globals, whose prompt reads find no section, and every
    /// coroutine yield shim.
    #[track_caller]
    pub(crate) fn shims(self) -> Self {
        self.reading(|path, _| {
            SectionRead::Missing(crate::make_lua_runtime_error(format!(
                "section `{path}` not found"
            )))
        })
    }

    /// [`shims`](Self::shims), with the `prompt` reads answered by `read`.
    #[track_caller]
    pub(crate) fn reading(
        mut self,
        read: impl Fn(String, Option<crate::LibraryKey>) -> SectionRead + Send + Sync + 'static,
    ) -> Self {
        self.shims = Some(Box::new(read));
        self
    }

    /// Builds the VM through Engine injection and stops there, so nothing
    /// installs after it, the persistent Engine globals included, failing
    /// the test at the first setup step that fails. [`shims`](Self::shims)
    /// needs the Engine globals, so a VM stopped here does not take it.
    #[track_caller]
    pub(crate) fn inject(mut self) -> SectionVm {
        assert!(
            self.shims.is_none(),
            "shims() needs the Engine globals, which inject() stops before"
        );
        self.run_injection().0
    }

    /// Builds the VM, failing the test at the first setup step that fails.
    #[track_caller]
    pub(crate) fn build(mut self) -> BuiltVm {
        let (mut vm, selection, access) = self.run_injection();
        vm.install_engine_globals(&self.emitter, self.section)
            .expect("the Engine globals install");
        if let Some(read) = self.shims.take() {
            vm.install_control_globals(move |path, from| {
                Ok::<SectionRead, std::convert::Infallible>(read(path, from))
            })
            .expect("the control globals install");
            vm.install_coro_shims(1)
                .expect("the coroutine shims install");
        }
        BuiltVm {
            vm,
            selection,
            access,
            emitter: self.emitter,
            section: self.section.to_owned(),
        }
    }

    /// Construction, the limits, the raw-model-id opt-in, and Engine
    /// injection, handing back the VM beside its selection (which starts on
    /// the default binding, if any) and the access it shares.
    #[track_caller]
    fn run_injection(&mut self) -> (SectionVm, Selection, ScopedAccess) {
        let tools = Arc::new(std::mem::take(&mut self.tools));
        let set = Arc::new(std::mem::take(&mut self.models));
        let selection = Arc::new(Mutex::new(set.binding("default").cloned()));
        let access = self.access.take().unwrap_or_else(fresh_access);
        let mut fresh =
            SectionVm::new_for_section(&test_nonce(), &tools, &set, &self.emitter, self.section)
                .expect("the test VM builds");
        if let Some((memory_bytes, log_events)) = self.limits {
            fresh
                .apply_lua_limits(memory_bytes, log_events)
                .expect("the limits apply");
        }
        if self.raw_model_ids {
            fresh.allow_raw_model_ids();
        }
        let empty = json!({});
        let vm = fresh
            .inject_values(
                self.args.unwrap_or(&empty),
                self.sys.unwrap_or(&empty),
                self.var,
                &selection,
            )
            .expect("the Engine values inject");
        (vm, selection, access)
    }
}

/// A built test VM: the section VM, which it derefs to, beside the selection
/// and access it shares.
pub(crate) struct BuiltVm {
    vm: SectionVm,
    /// The section's current-model selection, which injection received.
    pub(crate) selection: Selection,
    /// The store the block driver answers the VM's store requests in,
    /// beside the access whose identity views it.
    pub(crate) access: ScopedAccess,
    emitter: Emitter,
    section: String,
}

impl BuiltVm {
    /// Runs `program` as a block to its end, the block driver answering
    /// its store requests through the VM's access. The VM needs the
    /// coroutine shims.
    ///
    /// # Errors
    /// Returns the block's own failure.
    #[track_caller]
    pub(crate) fn drive(&self, program: &LuaProgram) -> Result<LuaBlockResult> {
        drive_block(&self.vm, program, &self.access)
    }

    /// Evaluates `source` on the VM's main state under the chunk name
    /// `probe`, failing the test when it raises.
    #[track_caller]
    pub(crate) fn eval<T: FromLuaMulti>(&self, source: &str) -> T {
        self.vm
            .lua()
            .load(source)
            .set_name("probe")
            .eval()
            .expect("the test chunk runs")
    }

    /// Runs `source` on the VM's main state under the chunk name `probe`
    /// for its effects, failing the test when it raises.
    #[track_caller]
    pub(crate) fn exec(&self, source: &str) {
        self.vm
            .lua()
            .load(source)
            .set_name("probe")
            .exec()
            .expect("the test chunk runs");
    }

    /// Runs `source` on the VM's main state under the chunk name `probe`
    /// and returns what it raises, failing the test when it runs clean.
    #[track_caller]
    pub(crate) fn eval_err(&self, source: &str) -> mlua::Error {
        self.vm
            .lua()
            .load(source)
            .set_name("probe")
            .exec()
            .expect_err("the test chunk must raise")
    }

    /// Tears the VM down through its emitter, under its section.
    #[track_caller]
    pub(crate) fn teardown(self) {
        self.vm.teardown(&self.emitter, &self.section);
    }
}

impl Deref for BuiltVm {
    type Target = SectionVm;

    fn deref(&self) -> &SectionVm {
        &self.vm
    }
}
