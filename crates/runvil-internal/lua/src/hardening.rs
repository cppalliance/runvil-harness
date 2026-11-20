//! Sandbox hardening for section VMs: the `_G` guard, global removal, the
//! standard-library replacements, and the cancellation hook.

use std::sync::{Arc, OnceLock};

use mlua::{HookTriggers, Lua, Thread, Value, VmState};
use runvil_types::cancel::CancelHandle;

use crate::error::{self, Result};

/// How many instructions between hook firings.
const HOOK_INTERVAL: u32 = 10_000;

/// Removes code-loading, direct output, and reflection globals the base library
/// provides. The `io`, `os`, `package`, and `debug` libraries are never
/// loaded; the coroutine library loads only while the shim chunk installs,
/// and its global is stripped again.
///
/// First installs the `_G` guard with its `setmetatable` and
/// `getmetatable` replacements ([`crate::globals`]).
///
/// Last replaces the standard-library functions whose result no run input
/// fixes, and wraps `table.concat` ([`crate::stdlib`]), so every chunk the
/// VM loads later captures the replacements.
pub(crate) fn harden(lua: &Lua) -> Result<()> {
    // The guard captures the raw functions removed below, and every later
    // chunk, the standard-library replacements included, must capture the
    // guard's replacements rather than the base functions.
    crate::globals::install(lua)?;
    let globals = lua.globals();
    for name in [
        "load",
        "loadstring",
        "dofile",
        "loadfile",
        "collectgarbage",
        "require",
        "getfenv",
        "setfenv",
        "rawget",
        "rawset",
        "rawequal",
        "rawlen",
        "print",
        "warn",
    ] {
        globals
            .set(name, Value::Nil)
            .map_err(error::make_lua_error)?;
    }
    crate::stdlib::install(lua)
}

/// A VM's shared cancellation-hook state.
///
/// The hook polls the run's cancel flag every [`HOOK_INTERVAL`]
/// instructions, so a long-running or infinite loop is legal and only the
/// flag aborts it. The flag is the run's synchronous [`CancelHandle`],
/// installed once by the executor through [`set_cancel`](Self::set_cancel);
/// a VM no executor claimed (a test fixture) is never canceled.
///
/// Instruction hooks are per-coroutine in PUC Lua: the hook installed on the
/// main state at construction never fires inside a resumed coroutine, so
/// every block coroutine receives the same hook (over the same flag) via
/// [`install_on_thread`](Self::install_on_thread), keeping every chunk the
/// VM runs cancellable.
#[derive(Debug, Default, Clone)]
pub(crate) struct CancelHook {
    /// The run's cancel flag, set once; the hook polls it on every firing.
    cancel: Arc<OnceLock<CancelHandle>>,
}

impl CancelHook {
    /// Installs the cancellation hook on a block coroutine.
    ///
    /// # Errors
    /// Returns a `LuaRuntime` failure if the VM rejects the hook callback.
    pub(crate) fn install_on_thread(&self, thread: &Thread) -> Result<()> {
        thread
            .set_hook(
                HookTriggers::new().every_nth_instruction(HOOK_INTERVAL),
                cancel_poll(Arc::clone(&self.cancel)),
            )
            .map_err(error::make_lua_error)
    }

    /// Installs the run's cancel flag. The first install wins: a VM serves
    /// one run, so a second handle is a caller error and is ignored rather
    /// than swapping the flag under a running hook.
    pub(crate) fn set_cancel(&self, cancel: CancelHandle) {
        let _ = self.cancel.set(cancel);
    }

    /// Returns whether the installed cancel flag is set, `false` when no
    /// executor installed one.
    pub(crate) fn is_canceled(&self) -> bool {
        canceled(&self.cancel)
    }
}

/// Returns whether `cancel` holds a set flag, the hook's poll of one
/// `OnceLock` read and a handful of atomic loads.
fn canceled(cancel: &OnceLock<CancelHandle>) -> bool {
    cancel.get().is_some_and(CancelHandle::is_canceled)
}

/// Builds the every-Nth-instruction hook body shared by the main state and
/// every block coroutine: the cancellation poll.
fn cancel_poll(
    cancel: Arc<OnceLock<CancelHandle>>,
) -> impl Fn(&Lua, &mlua::debug::Debug<'_>) -> mlua::Result<VmState> {
    move |_lua, _debug| {
        // Cooperative cancellation: abort a long-running Lua block promptly
        // when the run's CancelHandle is signaled (mapped to
        // a `Canceled` failure at the VM's failure boundary).
        if canceled(&cancel) {
            return Err(mlua::Error::RuntimeError(
                "lua execution canceled".to_string(),
            ));
        }
        Ok(VmState::Continue)
    }
}

/// Installs the every-Nth-instruction hook that keeps a block cancellable.
///
/// The hook covers the main state only; coroutines need
/// [`CancelHook::install_on_thread`] with the returned hook.
///
/// # Errors
/// Returns a `LuaRuntime` failure if the VM rejects the hook callback.
pub(crate) fn install_cancel_hook(lua: &Lua) -> Result<CancelHook> {
    let hook = CancelHook::default();
    lua.set_hook(
        HookTriggers::new().every_nth_instruction(HOOK_INTERVAL),
        cancel_poll(Arc::clone(&hook.cancel)),
    )
    .map_err(error::make_lua_error)?;
    Ok(hook)
}
