//! The Engine's runtime core: the [`Run`] state machine that executes a
//! parsed prompt.
//!
//! A run walks the prompt's top-level sections top to bottom, the preamble
//! first when the prompt has one. It issues each model round, Pack render, Pack tool call,
//! file operation, and timer as an [`Effect`] the
//! Harness performs and answers, and reports every boundary as an event
//! returned from `step` and from `deliver`. A file operation is a `store`
//! call of the
//! prompt's Lua, or a request of an open tool call that the caller
//! reports with [`Run::request_file`]. A tool call's file request has no
//! event: its effect and the answer are its whole report. Other crates
//! reach these items through the `runvil-engine` facade.
//!
//! ## Invariants
//!
//! - May depend on: `runvil-types`, `runvil-lua`,
//!   `runvil-prompt`, `runvil-markdown`, `runvil-vfs`, and
//!   `runvil-model-client`. `cargo test -p build-xtask` enforces the
//!   product and container boundaries.
//! - A run ends with `Ok` holding its final text or `Err` holding the
//!   error that ended it, never one with a caller code; a cancel is the
//!   error whose code is `Canceled`.

mod error;
mod execute;
mod lua;
mod model;
mod prompt;
mod section_path;
mod section_pos;
mod subst;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

use crate::error::{Error, Result};

// The crate root is the one path to this crate's items; the
// `execute` module stays private and every item that remains public inside it
// is re-exported here, so the `runvil-engine` facade names
// `runvil_runtime::X`, not a module path.
pub use crate::execute::{
    Effect, EffectAnswer, EffectId, IssuedEffect, PackToolSets, Round, Run, RunLimits, RunParams,
    Step, ToolCallOrigin, ToolCaller, VfsAsker, VfsReceipt, perform_vfs_op,
};
