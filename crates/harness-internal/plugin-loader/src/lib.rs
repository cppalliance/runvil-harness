//! `harness-plugin-loader` - inserts Plugins into a Host's Pack registry,
//! from the dynamic libraries in a Plugin directory, from one library
//! file, or linked into the Host, and reports each insert the same way:
//! the Plugins that loaded and each failure.
//! Each Plugin loads as it is inserted, and each Pack it offers goes into
//! the registry with a factory that makes one instance of the Pack for
//! each run and performs that instance's tool calls, all through the
//! Plugin's entry point. Each tool call is lent the resources the Host
//! offers its Plugins, and a waker with which the Plugin wakes the call.
//!
//! ## Invariants
//!
//! - May depend on: `runvil-plugin` among workspace crates, and
//!   `runvil-plugin-sys`, the C contract at version 0.7, from its own
//!   repository.
//! - Every entry point gets the same calls, whether it came from a library
//!   or was linked in. The only difference is that a library is opened
//!   when it is inserted and closed after its Plugin is dropped.
//! - Dropping a `Plugin` sends `RV_PLUGIN_UNLOAD` once, with the instance
//!   pointer its `RV_PLUGIN_LOAD` set, then closes its library, if it has
//!   one. Each Pack in the registry and each Pack instance of a run holds
//!   the `Plugin`, so the unload comes after every `RV_PACK_DELETE`.
//! - An entry point whose `RV_PLUGIN_LOAD` fails gets no
//!   `RV_PLUGIN_UNLOAD`, and its library is closed at once; a failing load
//!   must leave nothing running. None of its Packs go into the registry.
//! - Dropping a Pack instance sends `RV_PACK_DELETE` once. A tool call
//!   borrows its instance, so the delete comes after every call of the
//!   instance has ended. An `RV_PACK_NEW` that fails gets no
//!   `RV_PACK_DELETE`.
//! - A render of a Pack instance sends one `RV_PACK_RENDER`, which is
//!   ready when the call returns, and it leaves the instance as it was.
//!   It lends the section entry, the model as the JSON text of an object
//!   with sorted keys, and the offered tools with each name as it is
//!   within its Pack, each parameter schema as compact JSON text, and the
//!   one flag the contract carries; `offered` is NULL for no tool. An
//!   `RV_ERR_UNSUPPORTED` answer lists the offered tools as they are. A
//!   refusal, a misuse, and any other status fail the render with a
//!   message that names the Plugin and the Pack. What the Plugin lists
//!   becomes the render's answer, and the Engine's merge checks it.
//! - Each poll of a tool call's future sends one `RV_TOOL_CALL` at most.
//!   A call whose last step returned `RV_PENDING` gets one
//!   `RV_TOOL_CANCEL` when it fails or its future is dropped, and no
//!   other call does.
//! - A step that returns `RV_PENDING` with no file request parks its
//!   call: the future stays pending and wakes nothing until the Plugin
//!   wakes the call. The slot behind the call's waker forgets every wake
//!   as a step begins, before the entry point is called, so a wake asks
//!   for the next step that begins after it. Parking keeps the task to
//!   wake and checks for a wake as one act, so a wake during a step, or
//!   between a step's return and the park, wakes the task and is not
//!   lost. A poll that no wake asked for still sends a step, unless a
//!   file request waits.
//! - Every step of one call is lent the same `RvWaker`. Its `ctx` points
//!   at a slot that the call's `Plugin` owns. A call takes a slot that no
//!   call holds when it starts, or a new one when there is none, and
//!   gives it back when it ends, after its last step or its
//!   `RV_TOOL_CANCEL` has returned. So a `Plugin` has as many slots as
//!   its calls were ever in flight at once. It frees them only after its
//!   `RV_PLUGIN_UNLOAD` has returned, so a wake on any thread, at any
//!   time until then, reaches live memory. A wake for a call that ended
//!   costs the call that holds the slot next one step at most.
//! - `wake` only notes the wake and wakes the task of a parked call. It
//!   calls nothing of the Plugin's, and it never unwinds into the Plugin.
//!   The loader holds no lock of its own across a call into a Plugin,
//!   because a Plugin wakes a call on the loader's own thread, inside a
//!   step and inside `RV_TOOL_CANCEL`.
//! - Every step of one call is lent the same resources text: the JSON
//!   text of what the Host's `HostResources` offer when the call starts,
//!   read once. Each `Plugin` keeps a clone of the `HostResources` it
//!   was inserted with.
//! - The functions in a call's arguments struct answer only that call's
//!   `ctx`, on its thread, while it runs. `wake` is the one function
//!   that answers on any thread, and it takes the `ctx` of its waker.
//! - A step's file function takes one request and fills nothing. The
//!   loader asks the files of the call's `ToolContext` for the operation
//!   and reaches the run's files in no other way, so the Harness performs
//!   and records every file operation of a library Pack's tool, and the
//!   run's mode and policy apply to each one.
//! - A call whose request waits for its outcome gets no step and wakes
//!   nothing, even when the Plugin wakes the call. The step after the
//!   outcome is lent the answer, which stays valid until that step
//!   returns, and no other step is lent one. The request of a step that
//!   ends its call is not performed.
//! - Each `RV_PLUGIN_LOAD` gets its own `ctx`, a loader-owned allocation
//!   freed when a rejected load returns or after its Plugin's
//!   `RV_PLUGIN_UNLOAD` returns. The `ctx` of every other call is freed
//!   when the call returns.
//! - Nothing the loader keeps points into a library except its entry
//!   point and the pointers the Plugin set for the loader to pass back,
//!   which the loader never reads through; every text is copied.
//! - What a Plugin hands the loader is checked as `testing` in
//!   `runvil-plugin-sdk` checks it: the same order, the same bounds, and
//!   the same words for each misuse.
#![expect(
    unsafe_code,
    reason = "opens dynamic libraries and calls C entry points"
)]

mod call;
#[cfg(test)]
mod fakes;
mod file;
mod host;
mod load;
mod pack;
mod park;
mod plugin;
mod resources;
mod session;

pub use plugin::{
    Plugin, PluginReport, PluginSource, insert_plugin, insert_plugin_dir, insert_plugin_file,
    make_report,
};
pub use resources::HostResources;
