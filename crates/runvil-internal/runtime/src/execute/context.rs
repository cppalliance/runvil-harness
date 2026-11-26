//! The execute subtree's ambient run state.
//!
//! [`ChainState`] is built once in [`run`](super::run), as the root
//! chain's state, and travels through the execute subtree as parameter one
//! (`ctx: &ChainState`). It holds the run's one [`SharedState`], the
//! [`PromptState`] of the prompt its chain runs, and the fields a chain's
//! fork replaces. The invariant: a new run-scoped concern becomes a field
//! here, never a new parameter: a field of [`SharedState`] when it is the
//! same for every chain, a field of [`PromptState`] when it is the same
//! for every chain that runs one prompt, and a field of [`ChainState`]
//! when a fork replaces it. Per-call data (a section, a `var` snapshot)
//! stays in parameters or on the per-section frame.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::atomic::AtomicU32;
use std::sync::{Arc, Mutex};

use runvil_types::cancel::CancelHandle;
use runvil_types::emitter::{Emitter, EventSink};
use runvil_types::event::Event;
use runvil_types::ids::{ChainId, TaskId};
use runvil_types::packs::{PackId, Prelude};
use runvil_types::seed::LuaRandom;
use runvil_types::tools::ToolCatalog;
use runvil_types::untrusted::GuardNonce;
use runvil_vfs::{Store, VfsRef};

use crate::Error;
use crate::lua::{RosterEntry, ToolSet};
use crate::model::{ModelBinding, ModelInfo, ModelRole, ModelSet};
use crate::prompt::Prompt;

use super::fill;
use super::params::{RunLimits, RunParams};
use super::requirements::Requirements;
use super::section_context::SectionSys;
use super::section_vm::{SectionVmSetup, VmSeed};

/// A task's reporting handles: its task-scoped event emitter and its
/// model-turn counter.
#[derive(Clone, Debug)]
pub(super) struct ReportingHandles {
    /// The task-scoped event emitter, which also knows whether the run
    /// captures raw model-turn bodies.
    pub(super) emitter: Arc<Emitter>,
    /// The model-turn counter the task advances.
    pub(super) turns: Arc<AtomicU32>,
}

/// The run-wide state every chain of one run shares: one `Arc` per run,
/// built with the root chain's state and never forked.
///
/// Every field is the same for every chain. Its handles are fixed for the
/// run, though the cancel flag and the event buffer change behind them.
struct SharedState {
    /// Each prompt of the run with its sets, under the name
    /// [`Prompt::file`] gives it: the run's own prompt and every prompt
    /// that one links.
    prompts: BTreeMap<String, Arc<PromptState>>,
    /// The untrusted-envelope nonce, derived once here from the run's seed
    /// so every wrap in the run shares it.
    nonce: GuardNonce,
    /// The source of each section VM's `math.random` seed, derived once
    /// here from the run's seed. A section entry's own seed comes from it
    /// and the entry's id.
    lua_random: LuaRandom,
    /// The run's VFS handle: the files the run's tool calls work in. Chain
    /// steps acquire or spawn their access capabilities from it.
    vfs: VfsRef,
    /// The run's resource limits.
    limits: RunLimits,
    /// The run's event buffer, shared by every chain's emitter, spawned
    /// task chains' included, and drained through [`take_events`].
    events: EventSink,
    /// The run's cancel flag: polled between chain steps and installed on
    /// every section VM's instruction hook. It is the one handle the params
    /// hold, the flag the caller supplies with `RunParams::with_cancel` and
    /// keeps its own clone of.
    cancel: CancelHandle,
    /// The refusal the run's first step fails with: what the run's
    /// prompts require that the params lack, or `None` when they lack
    /// nothing.
    refusal: Option<Error>,
    /// The failure the run's first step fails with when the caller's
    /// arguments are not a JSON object or do not match the `args`
    /// declaration of the run's own prompt, or `None` when they match.
    args_refusal: Option<Error>,
    /// The run's `started_at` rendered as RFC 3339, stamped into every
    /// section's `sys.when`.
    when: Arc<str>,
    /// Test-only: installs the raw `tools.call_as_model` shim in every
    /// section VM, so a fixture section can yield one model-issued
    /// `tool_call` at the scheduler's dispatch arm without going through a
    /// loop shim.
    #[cfg(test)]
    raw_shims: bool,
}

/// One prompt of a run with the sets its sections run over: built once
/// with the run, for the run's own prompt and for each prompt that one
/// links, and shared by every chain that runs the prompt.
///
/// Nothing changes a set during the run, so every reader, every section
/// VM among them, shares it with no lock and no copy.
#[derive(Clone)]
struct PromptState {
    /// The prompt.
    prompt: Arc<Prompt>,
    /// The prompt's tool set: the run's catalog and roster, with the Packs
    /// this prompt declares as the declared ones.
    tool_set: Arc<ToolSet>,
    /// The prompt's model set: filled from the params' role map, with the
    /// thinking switch each role of this prompt declares.
    model_set: Arc<ModelSet>,
    /// The preludes of the Packs the prompt declares, in declaration
    /// order: every section VM of the prompt installs each one before
    /// `require`.
    preludes: Arc<[Prelude]>,
}

/// What the params give every prompt of a run alike, which each
/// [`PromptState`] is built over.
struct RunSets<'a> {
    /// The caller's role map.
    models: &'a BTreeMap<ModelRole, ModelInfo>,
    /// The run's tool catalog.
    tools: &'a ToolCatalog,
    /// The run's Pack roster, when the caller gave one.
    roster: Option<&'a BTreeMap<PackId, RosterEntry>>,
    /// The run's Pack preludes.
    preludes: &'a [Prelude],
}

impl PromptState {
    /// Builds the state of `prompt`, one of the run's prompts, over `run`,
    /// and records in `requirements` what the prompt requires that `run`
    /// lacks: each declared Pack the roster lacks or lists as unable to
    /// serve, and each declared role requirement the role's model falls
    /// short of. `linked` says whether `prompt` is one the run's own
    /// prompt links, so its gaps name it.
    ///
    /// The prompt's preludes are those of `run` whose Pack the prompt
    /// declares, in the prompt's declaration order, so a section installs
    /// no prelude of a Pack that only another prompt declares.
    fn new(
        prompt: Arc<Prompt>,
        linked: bool,
        run: &RunSets<'_>,
        requirements: &mut Requirements,
    ) -> Self {
        let by = linked.then(|| prompt.file());
        let model_set = fill::fill_model_set(&prompt, by, run.models, requirements);
        requirements.check_packs(&prompt, by, run.roster);
        let uses = prompt.frontmatter().uses();
        // Nothing is bound here: each section entry binds the tools it
        // offers when it freezes its sets.
        let tool_set = ToolSet::new(
            uses.iter().map(|entry| entry.id().clone()).collect(),
            run.roster.cloned().unwrap_or_default(),
            run.tools.clone(),
        );
        let preludes = uses
            .iter()
            .flat_map(|entry| {
                run.preludes
                    .iter()
                    .filter(move |prelude| prelude.pack() == entry.id())
            })
            .cloned()
            .collect();
        Self {
            prompt,
            tool_set: Arc::new(tool_set),
            model_set: Arc::new(model_set),
            preludes,
        }
    }

    /// The resolved tool-loop cap per `models.loop` call in the prompt's
    /// sections: its frontmatter's `max_tool_iterations` over the default
    /// of `limits`, held to their ceiling.
    fn max_tool_iterations(&self, limits: &RunLimits) -> usize {
        let cap = self
            .prompt
            .frontmatter()
            .max_tool_iterations()
            .unwrap_or(limits.max_tool_iterations);
        cap.min(limits.tool_iterations_ceiling).get() as usize
    }

    /// The prompt's top-level section count, the preamble included, which
    /// its sections read as `sys.section_count`.
    fn section_count(&self) -> usize {
        crate::prompt::detail::sections(&self.prompt).len()
    }
}

/// A store, with the id of the chain it was made for.
pub(super) struct ChainStore {
    /// The store: a set of files in memory that sits outside the run's
    /// VFS handle and never leaves the run, so the caller reaches it only
    /// through the store view of a `Vfs` effect.
    pub(super) store: Store,
    /// The id of the chain the store was made for: the root chain's for
    /// the store the run starts with, and the chain of an executed prompt
    /// for a store an `execute` made. A `Vfs` effect names its store by
    /// it.
    pub(super) made_for: ChainId,
}

impl ChainStore {
    /// A new, empty store made for the chain `made_for`.
    fn new(made_for: ChainId) -> Arc<Self> {
        Arc::new(Self {
            store: Store::new(),
            made_for,
        })
    }
}

/// One chain's state: the run's [`SharedState`], the [`PromptState`] of
/// the prompt the chain runs, and the fields a chain's fork replaces.
///
/// Cheap to clone: a clone shares the run's state, the prompt's state,
/// this chain's store, and its reporting handles, so it advances the same
/// turn counter.
/// The three sanctioned forks: [`with_task`](Self::with_task) giving a
/// spawned chain its own task emitter and turn counter,
/// [`with_args`](Self::with_args) giving the chain of a `call` or a
/// `tasks.spawn` the arguments it was passed, and
/// [`with_prompt`](Self::with_prompt) giving the chain of an `execute`
/// another of the run's prompts to run, with its arguments and a store of
/// its own.
#[derive(Clone)]
pub(crate) struct ChainState {
    /// The run-wide state every chain of the run shares.
    shared: Arc<SharedState>,
    /// The prompt this chain runs, with its sets: the run's own prompt for
    /// the root chain, and the prompt of the chain it was forked from
    /// unless [`with_prompt`](Self::with_prompt) replaced it.
    prompt: Arc<PromptState>,
    /// The store this chain's `store` calls operate on: the one made with
    /// the run for the root chain, and the store of the chain it was
    /// forked from unless [`with_prompt`](Self::with_prompt) made it a new
    /// one. `None` once [`release_store`](Self::release_store) let go of
    /// it at the chain's end, so a store is freed when the last chain that
    /// shared it has ended.
    store: Option<Arc<ChainStore>>,
    /// This context's reporting handles: the root task's at construction,
    /// a spawned chain's own after [`with_task`](Self::with_task). The Lua
    /// layer's seams (`require`'s library loads, `log`, teardown, the
    /// shared tool-dispatch body) take the emitter too, so their reports
    /// land in the buffer in order with the scheduler's own.
    reporting: ReportingHandles,
    /// The chain's arguments, a JSON object: the `args` global of every
    /// section the chain enters and the `{{ args }}` substitution source.
    /// Nothing writes it.
    args: Arc<serde_json::Value>,
}

impl ChainState {
    /// Builds the run's shared state and the root chain's state for one
    /// run of `prompt` over `params`, which it consumes. The turn counter
    /// is minted here (starting at zero), as are the root chain's store
    /// and the state of each of the run's prompts, `prompt` and every
    /// prompt it links - its tool set built from the catalog and the
    /// roster, its model set filled from the role map (it stays empty for
    /// an empty map), and its preludes - and the refusal for what those
    /// prompts require that `params` lacks; the nonce derives from
    /// `params`'s seed and `when` renders `params`'s `started_at`, so two
    /// states over the same inputs agree on both.
    ///
    /// `args` is the caller's argument text. The state holds the object
    /// the prompt's `args` declaration accepts it as, or, when the
    /// declaration refuses it, the refusal the run's first step fails
    /// with beside an empty object that no section reads.
    #[must_use]
    pub(super) fn root(prompt: Arc<Prompt>, args: &str, params: RunParams) -> Self {
        let RunParams {
            seed,
            started_at,
            report_debug,
            cancel,
            limits,
            vfs,
            models,
            tools,
            packs,
            preludes,
        } = params;
        let mut requirements = Requirements::default();
        requirements.check_run(&models, &tools, packs.as_ref());
        let sets = RunSets {
            models: &models,
            tools: &tools,
            roster: packs.as_ref(),
            preludes: &preludes,
        };
        // The run's own prompt first, then each prompt it links in the
        // order of their names: the order the refusal notice lists their
        // gaps in.
        let own = Arc::new(PromptState::new(prompt, false, &sets, &mut requirements));
        let prompt = &own.prompt;
        let mut prompts = BTreeMap::from([(prompt.file().to_owned(), Arc::clone(&own))]);
        for linked in prompt.linked() {
            let state = PromptState::new(Arc::clone(linked), true, &sets, &mut requirements);
            prompts.insert(linked.file().to_owned(), Arc::new(state));
        }
        let refusal = requirements.refusal(prompt);
        let events = EventSink::default();
        // The root chain - the main walk - is task `0`.
        let emitter = Arc::new(Emitter::new(
            events.clone(),
            TaskId::from(ChainId::root()),
            report_debug,
        ));
        let (args, args_refusal) = match crate::prompt::detail::accept_args(prompt, args) {
            Ok(args) => (args, None),
            Err(refusal) => (
                serde_json::Value::Object(serde_json::Map::new()),
                Some(refusal),
            ),
        };
        Self {
            shared: Arc::new(SharedState {
                prompts,
                nonce: GuardNonce::from_seed(&seed),
                lua_random: LuaRandom::from_seed(&seed),
                vfs,
                limits,
                events,
                cancel,
                refusal,
                args_refusal,
                when: Arc::from(started_at.to_rfc3339()),
                #[cfg(test)]
                raw_shims: false,
            }),
            prompt: own,
            store: Some(ChainStore::new(ChainId::root())),
            reporting: ReportingHandles {
                emitter,
                turns: Arc::new(AtomicU32::new(0)),
            },
            args: Arc::new(args),
        }
    }

    /// The run-wide state, for a test hook that writes it.
    ///
    /// # Panics
    /// Panics once the state has been cloned, because every clone shares
    /// the one [`SharedState`]. The hooks run before the run starts, when
    /// nothing has cloned it yet.
    #[cfg(test)]
    fn shared_mut_for_test(&mut self) -> &mut SharedState {
        Arc::get_mut(&mut self.shared)
            .expect("a test hook writes the run-wide state before anything clones it")
    }

    /// Exposes the raw `tools.call_as_model` shim in every section VM this
    /// run starts, so a test fixture can drive the scheduler's `tool_call`
    /// arm with one model-issued call.
    #[cfg(test)]
    pub(crate) fn expose_raw_shims_for_test(&mut self) {
        self.shared_mut_for_test().raw_shims = true;
    }

    /// Sets the model set of the run's own prompt to `set`, so a test can
    /// run a prompt over bindings it builds by hand. Call it on the root
    /// chain's state before the run starts: every section VM of that
    /// prompt shares the set it finds here.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn with_models_for_test(mut self, set: ModelSet) -> Self {
        let own = Arc::new(PromptState {
            model_set: Arc::new(set),
            ..PromptState::clone(&self.prompt)
        });
        self.shared_mut_for_test()
            .prompts
            .insert(own.prompt.file().to_owned(), Arc::clone(&own));
        self.prompt = own;
        self
    }

    /// The prompt this chain runs.
    pub(super) fn prompt(&self) -> &Arc<Prompt> {
        &self.prompt.prompt
    }

    /// The run's cancel flag.
    pub(super) fn cancel(&self) -> &CancelHandle {
        &self.shared.cancel
    }

    /// The run's untrusted-envelope nonce.
    pub(super) fn nonce(&self) -> &GuardNonce {
        &self.shared.nonce
    }

    /// The run's source of each section VM's `math.random` seed.
    pub(super) fn lua_random(&self) -> &LuaRandom {
        &self.shared.lua_random
    }

    /// The run's VFS handle.
    pub(super) fn vfs(&self) -> &VfsRef {
        &self.shared.vfs
    }

    /// The store this chain's `store` calls operate on, with the id of the
    /// chain it was made for. Every store view the Engine makes for the
    /// chain derives from it.
    ///
    /// # Errors
    /// Returns an `Internal` failure once the chain has ended and let go
    /// of its store: a live chain always holds one.
    pub(super) fn store(&self) -> crate::Result<&ChainStore> {
        self.store
            .as_deref()
            .ok_or(crate::error::make_internal_error(
                "a live chain holds its store",
            ))
    }

    /// Lets go of the chain's store, at the chain's end. The store itself
    /// goes once every chain that shared it has let go: the run's own
    /// with the run, which keeps the root chain's state, and one an
    /// `execute` made when the executed prompt's chain ends, since every
    /// chain it started has ended by then.
    pub(super) fn release_store(&mut self) {
        self.store = None;
    }

    /// The prompt the run holds under `name`, the name [`Prompt::file`]
    /// gives it: the run's own prompt or one it links. `None` when the
    /// run holds no prompt under that name.
    pub(super) fn prompt_named(&self, name: &str) -> Option<&Arc<Prompt>> {
        self.shared.prompts.get(name).map(|state| &state.prompt)
    }

    /// The chain's arguments, a JSON object.
    pub(super) fn args(&self) -> &Arc<serde_json::Value> {
        &self.args
    }

    /// The run's resource limits.
    pub(super) fn limits(&self) -> RunLimits {
        self.shared.limits
    }

    /// This context's reporting handles: its task-scoped emitter and its
    /// model-turn counter.
    pub(super) fn reporting(&self) -> &ReportingHandles {
        &self.reporting
    }

    /// This context's task-scoped emitter: where every report the
    /// scheduler makes on this chain goes.
    pub(super) fn emitter(&self) -> &Arc<Emitter> {
        &self.reporting.emitter
    }

    /// Drains the run's event buffer: every event pushed since the last
    /// drain, in push order. The run's `step` calls this once per step,
    /// and its `deliver` once per message, and each returns the batch to
    /// the caller.
    pub(super) fn take_events(&self) -> Vec<Event> {
        self.shared.events.take()
    }

    /// The model-turn counter this context advances.
    pub(super) fn turns(&self) -> &Arc<AtomicU32> {
        &self.reporting.turns
    }

    /// The tool set of the prompt this chain runs, the one every section
    /// VM of that prompt holds.
    pub(super) fn tool_set(&self) -> &Arc<ToolSet> {
        &self.prompt.tool_set
    }

    /// The binding of the `default` role in the model set of the prompt
    /// this chain runs: the model a chain that starts that prompt starts
    /// on. It is `None` when the caller supplied no model.
    pub(super) fn default_binding(&self) -> Option<ModelBinding> {
        self.prompt
            .model_set
            .binding(ModelRole::Default.as_str())
            .cloned()
    }

    /// The model set of the prompt this chain runs, shared with every
    /// section VM of that prompt.
    pub(super) fn model_set(&self) -> Arc<ModelSet> {
        Arc::clone(&self.prompt.model_set)
    }

    /// The refusal the run's first step fails with, or `None` when the
    /// params lack nothing the run's prompts require.
    pub(super) fn refusal(&self) -> Option<&Error> {
        self.shared.refusal.as_ref()
    }

    /// The failure the run's first step fails with, or `None` when the
    /// `args` declaration of the run's own prompt accepts the caller's
    /// arguments.
    pub(super) fn args_refusal(&self) -> Option<&Error> {
        self.shared.args_refusal.as_ref()
    }

    /// The context a contained chain runs under when a `call` or a
    /// `tasks.spawn` passed it arguments: `args` in place of the caller's.
    /// An object is the chain's arguments as given, and a string is the
    /// one field `text` of them. The prompt's `args` declaration describes
    /// what a caller passes a chain that starts the prompt, so nothing
    /// checks these arguments against it.
    #[must_use]
    pub(super) fn with_args(&self, args: serde_json::Value) -> Self {
        let args = match args {
            serde_json::Value::String(text) => {
                serde_json::json!({ crate::prompt::detail::TEXT_FIELD: text })
            }
            object => object,
        };
        let mut ctx = self.clone();
        ctx.args = Arc::new(args);
        ctx
    }

    /// The context the chain of an `execute` runs under: the prompt the
    /// run holds under `name`, with that prompt's own sets; `args`, a JSON
    /// object, as the chain's arguments; and a new, empty store made for
    /// the chain `made_for`, in place of this chain's. The reporting
    /// handles stay, as does everything the run shares. `None` when the
    /// run holds no prompt under `name`.
    ///
    /// Nothing here checks `args` against that prompt's `args`
    /// declaration, and nothing checks that this chain's prompt lists
    /// `name`: the `execute` dispatch does both first.
    #[must_use]
    pub(super) fn with_prompt(
        &self,
        name: &str,
        args: serde_json::Value,
        made_for: ChainId,
    ) -> Option<Self> {
        let prompt = Arc::clone(self.shared.prompts.get(name)?);
        let mut ctx = self.clone();
        ctx.prompt = prompt;
        ctx.args = Arc::new(args);
        ctx.store = Some(ChainStore::new(made_for));
        Some(ctx)
    }

    /// The context a spawned task chain runs under: an emitter stamping
    /// the chain's own `task` on every report, and `turns` in place of the
    /// run's counter, so its turn numbers, and `tasks.status`'s `turns`,
    /// count only its own rounds.
    #[must_use]
    pub(super) fn with_task(&self, task: TaskId, turns: Arc<AtomicU32>) -> Self {
        let mut ctx = self.clone();
        ctx.reporting = ReportingHandles {
            emitter: Arc::new(self.reporting.emitter.for_task(task)),
            turns,
        };
        ctx
    }

    /// The borrowed VM-setup inputs of one section entry, sourcing
    /// the run-wide inputs from this context; the frame constructor supplies
    /// the entry's own: the `sys` JSON, the seed, the section's
    /// current-model selection, and the section name.
    pub(super) fn vm_setup<'a>(
        &'a self,
        sys: &'a serde_json::Value,
        seed: VmSeed<'a>,
        selection: &'a Arc<Mutex<Option<ModelBinding>>>,
        section_name: &'a str,
    ) -> SectionVmSetup<'a> {
        SectionVmSetup {
            args: &self.args,
            sys,
            selection,
            seed,
            emitter: &self.reporting.emitter,
            section_name,
            file: self.prompt.prompt.file(),
            max_tool_iterations: self.prompt.max_tool_iterations(&self.shared.limits),
            preludes: &self.prompt.preludes,
            #[cfg(test)]
            raw_shims: self.shared.raw_shims,
        }
    }

    /// The `sys` for one section entry of this run: the five shared fields
    /// in one construction, with the constructor supplying only the
    /// section entry's hierarchical id, the entering chain's task id, and
    /// the section name. A task's first entry sets `index` at its own call
    /// site.
    pub(super) fn section_sys(&self, id: &str, task_id: &TaskId, section_name: &str) -> SectionSys {
        SectionSys {
            when: Arc::clone(&self.shared.when),
            id: id.to_owned(),
            taskid: task_id.clone(),
            section_name: section_name.to_owned(),
            section_count: self.prompt.section_count(),
            index: None,
            model: None,
        }
    }
}

impl fmt::Debug for ChainState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChainState")
            .field("shared", &self.shared)
            .field("prompt", &self.prompt)
            .field(
                "store_made_for",
                &self.store.as_ref().map(|held| &held.made_for),
            )
            .field("reporting", &self.reporting)
            .field("args", &self.args)
            .finish()
    }
}

impl fmt::Debug for SharedState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut state = f.debug_struct("SharedState");
        #[cfg(test)]
        state.field("raw_shims", &self.raw_shims);
        state
            .field("prompts", &self.prompts.keys().collect::<Vec<_>>())
            .field("nonce", &self.nonce)
            .field("lua_random", &self.lua_random)
            .field("vfs", &"<VfsRef>")
            .field("limits", &self.limits)
            .field("events", &self.events)
            .field("cancel", &self.cancel)
            .field("refusal", &self.refusal)
            .field("args_refusal", &self.args_refusal)
            .field("when", &self.when)
            .finish()
    }
}

impl fmt::Debug for PromptState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PromptState")
            .field("prompt", &self.prompt)
            .field("tool_set", &self.tool_set)
            .field("model_set", &self.model_set)
            .field(
                "preludes",
                &self.preludes.iter().map(Prelude::pack).collect::<Vec<_>>(),
            )
            .finish()
    }
}

#[cfg(test)]
mod tests {
    //! Tests for `ChainState` construction, forking, task sequence seeding,
    //! the model set a run binds from its filled roles, and the sets each
    //! prompt of a run gets.

    use std::collections::BTreeMap;
    use std::num::NonZeroU32;

    use runvil_types::packs::PackId;
    use runvil_types::tools::ToolId;

    use serde_json::json;

    use super::*;
    use crate::execute::section_vm::VmSeed;
    use crate::lua::RosterEntry;
    use crate::model::{ModelRole, ThinkingMode};
    use crate::test_support::scenario::{TEST_SEED, fresh_store_view, test_model, test_model_with};

    fn test_prompt() -> Prompt {
        let source = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n## Only\n\ndone\n";
        Prompt::parse(&|_: &str| Ok(source.to_owned()), "prompt").expect("the test prompt parses")
    }

    /// The prompt `prompt`, whose frontmatter adds `front` and lists
    /// `review.md`, a prompt of two sections whose frontmatter adds
    /// `review`.
    fn linking_prompt(front: &str, review: &str) -> Prompt {
        let main = format!(
            "---\nname: t\ndescription: d\nrunvil: 0\n{front}prompts: [review.md]\n---\n\n\
             ## Only\n\ndone\n"
        );
        let review = format!(
            "---\nname: review\ndescription: d\nrunvil: 0\n{review}---\n\n\
             ## First\n\none\n\n## Second\n\ntwo\n"
        );
        let finder = move |name: &str| -> Result<String, Error> {
            Ok(if name == "review.md" {
                review.clone()
            } else {
                main.clone()
            })
        };
        Prompt::parse(&finder, "prompt").expect("the two test prompts parse")
    }

    fn pack(id: &str) -> PackId {
        PackId::parse(id).expect("a static valid Pack id")
    }

    /// The store `ctx` holds, with the id of the chain it was made for.
    #[track_caller]
    fn held(ctx: &ChainState) -> &ChainStore {
        ctx.store()
            .expect("a state that has not ended holds a store")
    }

    /// The Pack of each prelude a section VM of `ctx`'s prompt installs,
    /// in install order.
    fn prelude_packs(ctx: &ChainState) -> Vec<String> {
        let sys = json!({});
        let selection = Arc::new(Mutex::new(None));
        let setup = ctx.vm_setup(&sys, VmSeed::default(), &selection, "Only");
        setup
            .preludes
            .iter()
            .map(|prelude| prelude.pack().to_string())
            .collect()
    }

    /// The tool-loop cap a section VM of `ctx`'s prompt gets.
    fn loop_cap(ctx: &ChainState) -> usize {
        let sys = json!({});
        let selection = Arc::new(Mutex::new(None));
        ctx.vm_setup(&sys, VmSeed::default(), &selection, "Only")
            .max_tool_iterations
    }

    /// The `sys.section_count` a section of `ctx`'s prompt reads.
    fn section_count(ctx: &ChainState) -> usize {
        ctx.section_sys("0.0", &TaskId::from(ChainId::root()), "Only")
            .section_count
    }

    fn test_state(prompt: &Prompt) -> ChainState {
        state_over(
            prompt,
            RunParams::new(TEST_SEED, runvil_types::timestamp::Timestamp::UNIX_EPOCH),
        )
    }

    /// The state a run of `prompt` assembles from `params`, as `Run::new`
    /// assembles it.
    fn state_over(prompt: &Prompt, params: RunParams) -> ChainState {
        ChainState::root(Arc::new(prompt.clone()), "{}", params)
    }

    #[test]
    fn root_builds_a_context_over_the_prompt() {
        let prompt = test_prompt();
        let ctx = test_state(&prompt);
        assert_eq!(ctx.prompt().frontmatter().name(), "t");
    }

    #[test]
    fn accessor_returns_the_run_prompt() {
        let prompt = test_prompt();
        let ctx = test_state(&prompt);
        assert_eq!(**ctx.prompt(), prompt);
    }

    #[test]
    fn clones_share_the_prompt_allocation() {
        let ctx = test_state(&test_prompt());
        let clone = ctx.clone();
        assert!(Arc::ptr_eq(ctx.prompt(), clone.prompt()));
    }

    #[test]
    fn derived_values_come_from_the_prompt_and_limits() {
        let prompt = test_prompt();
        let ctx = test_state(&prompt);
        assert_eq!(
            section_count(&ctx),
            runvil_prompt::detail::sections(&prompt).len()
        );
        assert_eq!(loop_cap(&ctx), 24);
    }

    #[test]
    fn the_ceiling_caps_the_limits_default_too() {
        let limits = RunLimits {
            max_tool_iterations: NonZeroU32::new(50).expect("50 is non-zero"),
            tool_iterations_ceiling: NonZeroU32::new(10).expect("10 is non-zero"),
            ..RunLimits::default()
        };
        let ctx = state_over(
            &test_prompt(),
            RunParams::new(TEST_SEED, runvil_types::timestamp::Timestamp::UNIX_EPOCH)
                .with_limits(limits),
        );
        assert_eq!(loop_cap(&ctx), 10);
    }

    #[test]
    fn root_holds_the_object_the_declaration_accepts() {
        let prompt = test_prompt();
        let params = RunParams::new(TEST_SEED, runvil_types::timestamp::Timestamp::UNIX_EPOCH);
        let ctx = ChainState::root(Arc::new(prompt), r#"{"text":"papers"}"#, params);
        assert_eq!(**ctx.args(), json!({ "text": "papers" }));
        assert_eq!(ctx.args_refusal(), None, "the declaration accepts them");
    }

    #[test]
    fn root_keeps_the_refusal_of_arguments_the_declaration_refuses() {
        let prompt = test_prompt();
        let params = RunParams::new(TEST_SEED, runvil_types::timestamp::Timestamp::UNIX_EPOCH);
        let ctx = ChainState::root(Arc::new(prompt), r#"{"query":"papers"}"#, params);
        let refusal = ctx.args_refusal().expect("the declaration refuses them");
        assert_eq!(refusal.code, runvil_types::error::ErrorCode::ArgsInvalid);
        assert_eq!(refusal.at, None, "the caller's gap points at no line");
        assert_eq!(**ctx.args(), json!({}), "the state holds an empty object");
    }

    #[test]
    fn vm_setup_hands_the_vm_the_chains_args() {
        let prompt = test_prompt();
        let ctx = test_state(&prompt).with_args(json!({ "query": "papers" }));
        let sys = json!({});
        let selection = Arc::new(Mutex::new(None));
        let setup = ctx.vm_setup(&sys, VmSeed::default(), &selection, "Only");
        assert_eq!(
            setup.args,
            &json!({ "query": "papers" }),
            "the setup holds the args the chain was passed"
        );
    }

    #[test]
    fn a_string_passed_as_a_chains_args_is_their_one_text_field() {
        let ctx = test_state(&test_prompt()).with_args(json!("chain text"));
        assert_eq!(**ctx.args(), json!({ "text": "chain text" }));
    }

    #[test]
    fn forks_swap_only_their_own_fields() {
        let ctx = test_state(&test_prompt());
        let chain = ctx.with_args(json!({ "n": 1 }));
        assert_eq!(**chain.args(), json!({ "n": 1 }));
        assert!(Arc::ptr_eq(&ctx.shared, &chain.shared));
        assert_eq!(**ctx.args(), json!({}));

        let turns = Arc::new(AtomicU32::new(7));
        let task: TaskId = "0.4".parse().expect("a task id parses");
        let arm = ctx.with_task(task.clone(), Arc::clone(&turns));
        assert!(Arc::ptr_eq(arm.turns(), &turns));
        assert!(Arc::ptr_eq(&ctx.shared, &arm.shared));
        assert_eq!(arm.emitter().task(), &task);
        assert_eq!(ctx.emitter().task(), &TaskId::from(ChainId::root()));
    }

    #[test]
    fn each_prompt_of_a_run_gets_its_own_tool_set_model_set_and_preludes() {
        let prompt = linking_prompt(
            "uses: [web]\nmodels: { strong: { thinking: true } }\n",
            "uses: [kit, web]\nmodels: { strong: { thinking: false } }\nmax_tool_iterations: 3\n",
        );
        // The preludes come in no prompt's order, and `stray` is of a Pack
        // neither prompt declares.
        let preludes = ["web", "stray", "kit"]
            .map(|id| Prelude::new(pack(id), format!("{id}_loaded = true")))
            .to_vec();
        let ctx = state_over(
            &prompt,
            RunParams::new(TEST_SEED, runvil_types::timestamp::Timestamp::UNIX_EPOCH)
                .with_models(BTreeMap::from([(ModelRole::Default, test_model("m"))]))
                .with_preludes(preludes),
        );
        let review = ctx
            .with_prompt("review.md", json!({}), ChainId::root().child(0))
            .expect("the run holds the prompt its prompt links");
        assert_eq!(ctx.prompt().file(), "prompt");
        assert_eq!(review.prompt().file(), "review.md");
        assert_eq!(ctx.tool_set().declared(), [pack("web")]);
        assert_eq!(review.tool_set().declared(), [pack("kit"), pack("web")]);
        let thinking = |ctx: &ChainState| {
            let set = ctx.model_set();
            let strong = set.binding("strong").expect("the role map fills the role");
            strong.invocation().thinking
        };
        assert_eq!(thinking(&ctx), Some(true));
        assert_eq!(thinking(&review), Some(false));
        assert_eq!(
            ctx.default_binding()
                .expect("the role map fills the role")
                .invocation()
                .thinking,
            None,
            "neither prompt declares a thinking switch for `default`"
        );
        assert_eq!(prelude_packs(&ctx), ["web"]);
        assert_eq!(
            prelude_packs(&review),
            ["kit", "web"],
            "each prompt's own Packs' preludes, in the order it declares them"
        );
        assert_eq!((section_count(&ctx), section_count(&review)), (1, 2));
        assert_eq!((loop_cap(&ctx), loop_cap(&review)), (24, 3));
        assert_eq!(ctx.refusal(), None, "the params lack nothing");
    }

    #[test]
    fn a_prompt_fork_swaps_the_prompt_the_arguments_and_the_store_and_keeps_the_rest() {
        let ctx = test_state(&linking_prompt("", ""));
        let made_for = ChainId::root().child(3);
        let child = ctx
            .with_prompt("review.md", json!({ "topic": "tea" }), made_for.clone())
            .expect("the run holds the prompt its prompt links");
        assert!(Arc::ptr_eq(&ctx.shared, &child.shared));
        assert!(Arc::ptr_eq(ctx.turns(), child.turns()));
        assert!(Arc::ptr_eq(ctx.emitter(), child.emitter()));
        assert_eq!(**child.args(), json!({ "topic": "tea" }));
        assert_eq!(**ctx.args(), json!({}), "the fork leaves its source alone");
        assert_eq!(held(&ctx).made_for, ChainId::root());
        assert_eq!(held(&child).made_for, made_for);

        // A file in one store is no file of the other.
        let write = |ctx: &ChainState, path: &str| {
            fresh_store_view(&held(ctx).store, "the fork test's write")
                .and_then(|view| view.write(path, b"x"))
                .expect("a fresh view writes");
        };
        let exists = |ctx: &ChainState, path: &str| {
            fresh_store_view(&held(ctx).store, "the fork test's read")
                .and_then(|view| view.exists(path))
                .expect("a fresh view reads")
        };
        write(&ctx, "own.md");
        write(&child, "child.md");
        assert!(exists(&ctx, "own.md") && !exists(&ctx, "child.md"));
        assert!(exists(&child, "child.md") && !exists(&child, "own.md"));

        // The other forks keep the prompt and the store.
        let task: TaskId = "0.3.1".parse().expect("a task id parses");
        for fork in [
            child.with_args(json!("text")),
            child.with_task(task, Arc::new(AtomicU32::new(0))),
        ] {
            assert_eq!(fork.prompt().file(), "review.md");
            assert!(std::ptr::eq(held(&fork), held(&child)));
        }

        // A fork back onto the run's own prompt gets a store of its own
        // too, and a name the run holds no prompt under gets nothing.
        let back = child
            .with_prompt("prompt", json!({}), made_for.child(0))
            .expect("the run holds its own prompt under its name");
        assert_eq!(back.prompt().file(), "prompt");
        assert!(Arc::ptr_eq(ctx.prompt(), back.prompt()));
        assert!(!exists(&back, "own.md"));
        assert!(
            ctx.with_prompt("absent.md", json!({}), made_for).is_none(),
            "the run holds no prompt under that name"
        );
    }

    #[test]
    fn a_store_goes_when_the_last_state_that_shares_it_lets_go() {
        let ctx = test_state(&linking_prompt("", ""));
        let mut child = ctx
            .with_prompt("review.md", json!({}), ChainId::root().child(0))
            .expect("the run holds the prompt its prompt links");
        let mut sharer = child.with_args(json!({ "n": 1 }));
        let store = Arc::downgrade(child.store.as_ref().expect("the fork made a store"));

        sharer.release_store();
        assert!(
            sharer.store().is_err(),
            "a state that let go holds no store"
        );
        assert!(
            store.upgrade().is_some(),
            "the state the store was made for still holds it"
        );
        child.release_store();
        assert!(store.upgrade().is_none(), "the last holder let go");
        assert!(
            ctx.store().is_ok(),
            "the state the child forked from keeps its own store"
        );
    }

    /// The file of the prompt `ctx`'s run holds under `name`.
    fn prompt_named<'a>(ctx: &'a ChainState, name: &str) -> Option<&'a str> {
        ctx.prompt_named(name).map(|prompt| prompt.file())
    }

    #[test]
    fn a_run_holds_its_own_prompt_and_each_prompt_it_links_under_their_names() {
        let ctx = test_state(&linking_prompt("", ""));
        assert_eq!(prompt_named(&ctx, "prompt"), Some("prompt"));
        assert_eq!(prompt_named(&ctx, "review.md"), Some("review.md"));
        assert_eq!(prompt_named(&ctx, "absent.md"), None);
    }

    #[test]
    fn a_test_model_set_replaces_the_set_of_the_runs_own_prompt_alone() {
        let ctx = test_state(&linking_prompt("", "")).with_models_for_test(ModelSet::default());
        assert!(ctx.model_set().bindings().is_empty());
        let back = ctx
            .with_prompt("prompt", json!({}), ChainId::root().child(0))
            .expect("the run holds its own prompt under its name");
        assert!(
            Arc::ptr_eq(&ctx.model_set(), &back.model_set()),
            "a fork onto the run's own prompt finds the replaced set"
        );
    }

    #[test]
    fn a_task_fork_reports_into_the_shared_buffer_under_its_own_task() {
        let ctx = test_state(&test_prompt());
        let task: TaskId = "0.1".parse().expect("a task id parses");
        let arm = ctx.with_task(task.clone(), Arc::new(AtomicU32::new(0)));
        ctx.emitter()
            .report("Only", runvil_types::event_lifecycle::SECTION_STARTED);
        arm.emitter()
            .report("Only", runvil_types::event_lifecycle::SECTION_STARTED);
        // Both emitters share one buffer, drained through either context.
        let events = arm.take_events();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].provenance().task, TaskId::from(ChainId::root()));
        assert_eq!(events[1].provenance().task, task);
        assert_eq!(events[1].provenance().seq, 0);
    }

    #[test]
    fn a_filled_role_binds_with_the_catalog_info_its_render_receives() {
        let prompt = test_prompt();
        let info = test_model_with("catalog-model", 65_536, ThinkingMode::Switchable)
            .with_provider("acme");
        let ctx = RunParams::new(TEST_SEED, runvil_types::timestamp::Timestamp::UNIX_EPOCH)
            .with_models(BTreeMap::from([(ModelRole::Default, info.clone())]));
        let state = state_over(&prompt, ctx);
        let set = state.model_set();
        let binding = set.binding("default").expect("the filled role binds");
        assert_eq!(binding.render_info(), info);
        assert_eq!(state.refusal(), None, "the prompt requires nothing");
    }

    #[test]
    fn an_unavailable_roster_pack_records_no_vendor_and_a_pack_listed_twice_keeps_its_last_entry() {
        let web = PackId::parse("web").expect("a static valid Pack id");
        let usable = RosterEntry::Usable {
            vendor: "acme".to_owned(),
        };
        let down = RosterEntry::Unavailable {
            reason: "the server is down".to_owned(),
        };
        let state = state_over(
            &test_prompt(),
            RunParams::new(TEST_SEED, runvil_types::timestamp::Timestamp::UNIX_EPOCH)
                .with_packs([(web.clone(), usable), (web.clone(), down.clone())]),
        );
        let tool = ToolId::parse("web/fetch").expect("a static valid tool id");
        assert_eq!(
            state.tool_set().vendor_for(&tool),
            None,
            "an unavailable Pack has no vendor"
        );
        assert_eq!(
            state.tool_set().roster_for_test().get(&web),
            Some(&down),
            "one roster entry per Pack, the last one given, carrying its reason"
        );
    }
}
