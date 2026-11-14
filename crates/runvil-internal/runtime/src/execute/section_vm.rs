//! The Engine's section VM setup: one section VM's lifecycle from Engine
//! injection through the `require` install.
//!
//! The frame constructor runs the sequence [`setup_section_vm`] lists. What
//! differs from one section entry to the next lives at the call site: the
//! constructor builds the entry's `sys` JSON
//! (the chain's next entry `id`; a task's first entry adds its
//! `index`), picks the [`VmSeed`] (the chain's `var`, plus the item seeded
//! into this section on a task's first entry), and supplies the `read`
//! callback behind the `prompt` reads, resolved from its own section, the
//! library resolver behind `require`, and the renderer for library prose
//! templates.
//!
//! VM construction and the Lua limits install stay with the frame
//! constructor. A construction or limits failure propagates as a bare
//! `?` - routing it through teardown would emit
//! `LUA_TEARDOWN_STARTED`/`SUCCEEDED` events the walk never emitted. Setup
//! consumes the uninjected VM, so a setup failure tears it down here, the
//! one place a VM that never reached its frame is torn down.

use std::num::NonZeroU32;
use std::sync::{Arc, Mutex};

use runvil_types::emitter::Emitter;
use runvil_types::packs::Prelude;

use crate::lua::{Library, LibraryKey, ProseState, SectionRead, SectionVm, Uninjected};
use crate::model::ModelBinding;
use crate::{Error, Result};

/// What a section VM is seeded with beyond the shared Engine values.
///
/// Both fields install through the same serde bridge: `var` seeds the hidden
/// data table behind the guarded `var` proxy, and `item` installs as the
/// `item` global after the Engine globals, ahead of every block.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct VmSeed<'a> {
    /// The chain's current `var`: rolled forward across sections on one
    /// walk, cloned into a `call` chain or a spawned task (a fanout arm
    /// included); `None` seeds an empty table.
    pub(crate) var: Option<&'a serde_json::Value>,
    /// The item seeded into this section, on a spawned task's first entry;
    /// `None` otherwise.
    pub(crate) item: Option<&'a serde_json::Value>,
}

/// The borrowed inputs of one section VM setup.
///
/// Bundled so the frame constructor threads one linear set of borrows
/// into [`setup_section_vm`] rather than a dozen parameters.
pub(crate) struct SectionVmSetup<'a> {
    /// The chain's arguments (`ChainState::args`), a JSON object,
    /// installed read-only as the `args` global.
    pub(crate) args: &'a serde_json::Value,
    /// The `sys` JSON the frame constructor built for this section entry.
    pub(crate) sys: &'a serde_json::Value,
    /// The section's current-model selection: it starts on the `default`
    /// binding, or on nothing when the caller supplied no model, and
    /// `models.use` rewrites it.
    pub(crate) selection: &'a Arc<Mutex<Option<ModelBinding>>>,
    /// The frame constructor's seed: the chain's `var`, plus the item
    /// seeded into this section on a spawned task's first entry.
    pub(crate) seed: VmSeed<'a>,
    /// The chain's emitter: the persistent `log` global captures a clone,
    /// and `require`'s library loads report through it.
    pub(crate) emitter: &'a Emitter,
    /// The section name used in observations and error messages.
    pub(crate) section_name: &'a str,
    /// The file the run's prompt came from, which the VM's Lua failures
    /// are placed in.
    pub(crate) file: &'a str,
    /// The run's tool-loop cap, as `ChainState::max_tool_iterations`
    /// resolves it, captured by the `models.loop` shim as its round cap.
    pub(crate) max_tool_iterations: usize,
    /// The run's application-state snapshot, when the caller supplied one:
    /// its presence gives the section VM the `ui()` global and the
    /// raw-model-id `models.get` fallback. Shared through the run's `Arc`,
    /// so every section VM serializes the one tree.
    pub(crate) ui: Option<&'a Arc<serde_json::Value>>,
    /// The run's Pack preludes, installed in order after the coroutine
    /// shims and before `require`.
    pub(crate) preludes: &'a [Prelude],
    /// Test-only: installs the raw `tools.call_as_model` shim, so a fixture
    /// section can yield one model-issued `tool_call`.
    #[cfg(test)]
    pub(crate) raw_shims: bool,
}

/// Runs one section VM's setup sequence against a constructed, limited VM.
///
/// The sequence is fixed and shared: the prompt's file label, then Engine
/// injection with the frame
/// constructor's [`VmSeed`], preceded by the raw-model-id opt-in when the run
/// supplied a snapshot (Engine injection's `models` table install reads that
/// flag, so the opt-in must land first), then
/// [`SectionVm::install_engine_globals`], the
/// `ui()` global when the run supplied a snapshot, the `item` global when
/// the seed includes one, the control surface
/// ([`SectionVm::install_control_globals`] for `jump` and the
/// `prompt` reads, plus [`SectionVm::install_coro_shims`] for the
/// suspending calls, which the scheduler drives as yield shims,
/// `models.loop`, `models.compact`, and the `store` functions included),
/// the run's Pack preludes, and [`SectionVm::install_require`] over
/// `resolve` and `render`.
///
/// # Errors
/// Returns the [`Error`] of whichever step failed, after tearing the VM
/// down under the setup's section: Engine injection, Engine global
/// install, `ui()` install, `item` install, control-global install, the
/// yield shims' install, the prelude install (a prelude that fails to
/// load, as one that calls a tool or the store does, or whose global
/// collides), or the `require` install.
pub(crate) fn setup_section_vm<R, L, P>(
    mut vm: SectionVm<Uninjected>,
    setup: &SectionVmSetup<'_>,
    read: R,
    resolve: L,
    render: P,
) -> Result<SectionVm>
where
    R: Fn(String, Option<LibraryKey>) -> std::result::Result<SectionRead, Error>
        + Send
        + Sync
        + 'static,
    L: Fn(String) -> std::result::Result<Library, Error> + Send + Sync + 'static,
    P: Fn(&str, Option<NonZeroU32>, ProseState<'_>) -> mlua::Result<String> + Send + Sync + 'static,
{
    vm.set_prompt_file(setup.file);
    // The raw-model-id fallback reads its flag during Engine injection, so
    // the snapshot's opt-in lands first.
    if setup.ui.is_some() {
        vm.allow_raw_model_ids();
    }
    let mut vm = vm
        .inject_values(setup.args, setup.sys, setup.seed.var, setup.selection)
        .map_err(|failure| failure.teardown(setup.emitter, setup.section_name))?;
    match install_after_injection(&mut vm, setup, read, resolve, render) {
        Ok(()) => Ok(vm),
        Err(error) => {
            vm.teardown(setup.emitter, setup.section_name);
            Err(error)
        }
    }
}

/// The setup steps after Engine injection, in [`setup_section_vm`]'s order.
fn install_after_injection<R, L, P>(
    vm: &mut SectionVm,
    setup: &SectionVmSetup<'_>,
    read: R,
    resolve: L,
    render: P,
) -> Result<()>
where
    R: Fn(String, Option<LibraryKey>) -> std::result::Result<SectionRead, Error>
        + Send
        + Sync
        + 'static,
    L: Fn(String) -> std::result::Result<Library, Error> + Send + Sync + 'static,
    P: Fn(&str, Option<NonZeroU32>, ProseState<'_>) -> mlua::Result<String> + Send + Sync + 'static,
{
    vm.install_engine_globals(setup.emitter, setup.section_name)?;
    if let Some(snapshot) = setup.ui {
        crate::lua::install_ui(vm.lua(), Arc::clone(snapshot))?;
    }
    if let Some(item) = setup.seed.item {
        vm.set_global_json("item", item)?;
    }
    vm.install_control_globals(read)?;
    vm.install_coro_shims(setup.max_tool_iterations)?;
    // The preludes read `tools` and `store` from `_G` as they install, so
    // they follow the yield shims. A prelude runs as a main chunk, not a
    // coroutine, so a tool or store call it makes while it installs fails
    // where the shim yields, and performs nothing.
    crate::lua::install_preludes(vm.lua(), setup.preludes)?;
    // `require` follows the preludes, so a prelude that calls it while it
    // installs finds none.
    vm.install_require(resolve, render, setup.emitter, setup.section_name)?;
    #[cfg(test)]
    if setup.raw_shims {
        runvil_lua::test_support::install_model_tool_call_shim(vm.lua())?;
    }
    Ok(())
}
