//! `runvil-plugin` - the contract a Plugin crate implements, and the
//! one crate it depends on.
//!
//! A Pack has a fully qualified name (FQN) such as `runvil/web`. Its
//! first segment is the vendor, and its Pack id is the second segment,
//! `web`, which prefixes its tools (`web/fetch`). A Plugin is an entry
//! point which installs zero or more Packs. A linked Plugin is the
//! `install` function of a Plugin crate (a `plugin-*` crate), which
//! inserts its Packs into the Host's [`PackRegistry`]. Its signature is
//! its own: it takes the registry and whatever else its Host hands it,
//! as plain arguments, such as a configuration or the handle of the
//! Host's async runtime. A Plugin that takes one configuration value is:
//!
//! ```text
//! pub fn install(packs: &mut PackRegistry, config: &Value) -> Result<(), Error>
//! ```
//!
//! For each Pack it offers, it calls [`PackRegistry::insert`] with a
//! [`PackFactory`] holding what the Pack's instances share, or
//! [`PackRegistry::insert_unavailable`] with the reason its setup failed,
//! and returns the first error. A Plugin can also be a C entry
//! point that follows the C contract in the `runvil-plugin-sys` crate: a
//! dynamic library exports it as `runvil_plugin`, or a Plugin crate links
//! it in under a name of its own.
//! The Harness loads these entry points when a Host inserts them, and
//! inserts each Pack they offer with a factory that reaches the Pack
//! through its entry point.
//! Every Host has two built-in Packs, which the Harness makes itself:
//! `runvil/subagents`, which reserves its id for the Engine's subagent
//! tools, and `runvil/user`, which a prompt that needs a human declares
//! and which the Harness makes unavailable to a run with no user.
//!
//! The Host owns the [`PackRegistry`]; a Plugin only inserts its Packs into it.
//! The registry starts empty and holds one Pack per Pack id, so it
//! refuses a Pack whose Pack id another Pack holds, whatever the vendor
//! of either, and a Pack that takes the Pack id of a built-in.
//!
//! Each Pack is a [`PackInfo`], built with [`PackInfo::new`]: its FQN, an
//! optional Lua prelude, and a one-line description. Its [`PackFactory`]
//! makes each run's [`Pack`] instance: the Harness calls
//! [`PackFactory::new_pack`] for each Pack a run holds, and the instance
//! drops when the run ends. [`Pack::call`] performs one tool call with the
//! [`ToolContext`] the Harness lends it, and asks for each file operation
//! through the context's [`ToolFiles`], as a [`VfsOp`] that resolves with
//! its [`VfsOutcome`].
//! [`Pack::render`] returns the tools the model sees, given the tools a
//! section offered from the Pack and the [`RenderContext`] that names the
//! section entry and its model, or an error when the Pack cannot render
//! for that model, which ends the round that asked as canceled.
//!
//! The Engine never depends on this crate; it sees only `ToolInfo` values,
//! preludes, and ids. The Engine and filesystem names a Plugin author
//! needs are re-exported here, so a Plugin crate names no other
//! runvil crate.
//!
//! The `test-support` feature adds `testing::TestCall`, which lends a
//! [`ToolContext`] to a Plugin crate's own tests.
//!
//! ## Invariants
//!
//! - May depend on: `runvil-types` and `runvil-vfs` among
//!   workspace crates. As an Engine crate it declares no async runtime, no
//!   `async-trait`, and no HTTP client. `cargo test -p build-xtask`
//!   enforces the Engine manifest guard and the product and container
//!   boundaries.
//! - Only `[dev-dependencies]` enable `test-support`, which the
//!   `test-support` leak guard enforces.
//! - Each run makes its own instance of each Pack it holds, through the
//!   Pack's [`PackFactory`], and of no other Pack. The instance drops
//!   when the run ends, so cleanup goes in its `Drop`; there is no
//!   shutdown hook.
//! - A linked Plugin takes what its Packs need from the Host as the
//!   arguments of its `install`. A factory keeps what its instances need
//!   of them, and a call gets only facts about itself, through its
//!   [`ToolContext`].
//! - [`PackFactory::new_pack`] runs inside the run's future, so it must
//!   not block, do I/O, or panic. A refusal or a panic makes the Pack
//!   unavailable to that run alone.
//! - Every tool a Pack offers sits under the Pack id
//!   [`PackFactory::new_pack`] receives, as `web/fetch` sits under `web`.
//!   A tool that does not is dropped from every run's catalog.
//! - [`Pack::ready`] resolves `Ok` once [`Pack::tools`] is complete,
//!   or `Err` with a reason a model can read when the Pack can't serve
//!   now. The Harness awaits it before each run's snapshot, and a Pack
//!   whose `ready` fails is unavailable to that run. A Pack that
//!   recovers serves the runs that start after it does.
//! - [`Pack::ready`] resolves within a bound the Pack owns and
//!   enforces in its own task. It uses no runtime timer, because it may be
//!   polled outside any particular runtime.
//! - [`Pack::ready`] is cancellation-safe, and after its first
//!   resolution it answers at once unless the Pack's state has changed.
//! - [`Pack::ready`] must not panic. A panic is caught at the run's
//!   snapshot, the panic is logged, and the Pack is unavailable to that
//!   run rather than the panic unwinding through the run's setup.
//! - [`Pack::call`] must not block while polled. Every effect of a run
//!   is polled on the same task, so blocking or CPU-heavy work goes to the
//!   Host's runtime. A file operation is the exception: it is synchronous
//!   by design, and the run waits for it. The Harness performs each
//!   operation a call asks for through [`ToolContext::files`] with one
//!   blocking call, between two polls of the call.
//! - [`ToolContext::files`] is a call's one way to the run's files. The
//!   call asks for each file operation through it, so the run's record
//!   holds every file operation of a tool.
//! - A tool whose calls change files sets `changes_files` in its
//!   [`ToolInfo`]. The Engine then issues each of its calls alone, in the
//!   order the model listed them, so the changes of one model reply
//!   apply one at a time.
//! - [`Pack::tools`] must not panic. A panic is caught at the run's
//!   snapshot, the panic is logged, and the Pack is unavailable to that
//!   run rather than the panic unwinding through the run's setup.
//! - [`Pack::call`] must not panic. A panicking call's effect is
//!   answered `Dropped`, and the panic is logged.
//! - [`Pack::call`] marks the trust of every output correctly: output
//!   that embeds data an attacker can influence is
//!   [`ToolOutput::untrusted`].
//! - [`Pack::call`] is cancellation-aware: its future may be dropped
//!   before it finishes, on the occasions its invariants list.
//! - [`Pack::render`] runs in the effect loop with the rules of
//!   [`Pack::call`]: it must not block while polled, it must not panic,
//!   and it is cancellation-aware.
//! - [`Pack::render`] takes and returns plain data: its
//!   [`RenderContext`] and its tool lists hold no Rust-only handle, so a
//!   later C verb can pass them serialized. A fact joins
//!   [`RenderContext`] only as plain data.

mod context;
mod pack;
mod registry;
#[cfg(feature = "test-support")]
pub mod testing;

pub use context::{RenderContext, ToolContext, ToolFiles};
pub use pack::{Pack, PackFactory, PackInfo};
pub use registry::PackRegistry;

pub use runvil_types::error::{Error, ErrorCode, ErrorFact, SourceLoc};
pub use runvil_types::future::BoxFuture;
pub use runvil_types::models::{ModelId, ModelInfo, ThinkingMode};
pub use runvil_types::packs::PackId;
pub use runvil_types::tools::{
    OutputTrust, ToolCallOrigin, ToolCaller, ToolId, ToolInfo, ToolOutput, make_tool_error,
};
pub use runvil_vfs::{Entry, FileType, PathReason, Stat, VfsError, VfsOp, VfsOutcome};
