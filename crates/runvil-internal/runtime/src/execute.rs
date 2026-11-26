//! Section lifecycle execution and fall-through.
//!
//! The public design - the section walk, the caller's step loop, reporting,
//! tool binding, and running without a runtime - is documented on the
//! `runvil-engine` facade's crate page and its role modules.
//!
//! # Module layout
//!
//! [`perform_vfs_op`] is defined here; the rest lives in focused private
//! children:
//!
//! - `context` - the ambient run state: the run-wide `SharedState`, the
//!   `PromptState` of each prompt the run holds, and each chain's
//!   `ChainState`.
//! - `fill` - the fill function, which builds each prompt's model set
//!   from the caller's role map when the run is built.
//! - `infer` - the nested-inference round's answer.
//! - `params` - the public [`RunParams`], which carries the caller's
//!   role map for the five model roles, and [`RunLimits`]; installing
//!   Packs and taking each run's catalog from them is the Harness's job.
//! - `requirements` - what a run's params lack for its prompts, which the
//!   run's first step reports as its refusal notice.
//! - `run` - the boundary with the caller: the `Run` state machine with its
//!   `step` and `resume`, the `Step` it returns, and the effect
//!   vocabulary (the `Effect` a leaf arm issues, its JSON record, and the
//!   `EffectAnswer` the caller returns).
//! - `scheduler` - the chain scheduler driving the coroutine protocol:
//!   the root walk, call chains, executed prompts, fanout, and the `chat`
//!   and `tool_call` rounds that `models.loop` yields over the section's
//!   `messages`.
//! - `scope` - the merge that builds and checks a section entry's frozen
//!   sets, and the schemas a round projects from its Static Set.
//! - `section_context` - the per-section `SectionContext` frame the
//!   scheduler's chains construct, run, and tear down.
//! - `section_vm` - the section VM setup every section entry runs, a
//!   spawned task's first entry included.
//! - `turn` - the model-turn counter and the round report every served
//!   completion goes through.
//! - `walk_target` - the walk-target resolution helpers.

pub(crate) mod context;
mod fill;
mod infer;
mod params;
mod requirements;
mod run;
pub(crate) mod scheduler;
pub(crate) mod scope;
mod section_context;
pub(crate) mod section_vm;
mod turn;
mod walk_target;

// Public API surface.
pub use params::{RunLimits, RunParams};
#[cfg(test)]
pub(crate) use run::{AnswerRecord, ChatAnswerRecord, EffectRecord};
pub use run::{
    Effect, EffectAnswer, EffectId, IssuedEffect, PackToolSets, Round, Run, Step, ToolCallOrigin,
    ToolCaller, VfsAsker, VfsReceipt,
};
use runvil_vfs::{Access, VfsError, VfsOp, VfsOutcome};

/// Performs the operation of an [`Effect::Vfs`] and returns its outcome.
///
/// `access` is the access the effect carries: for a `store` call, a view
/// of the calling chain's store under the chain's identity, made when the
/// effect is dispatched, and for a tool's request, the tool call's own
/// access.
/// The function runs `op` with [`Access::perform`] on that access, so the
/// caller answers a `Vfs` effect exactly as the Engine's own test drivers
/// do. Line bounds arrive as `i64` and convert to `usize`. A read that
/// gives an `end` and omits `start` is refused as an invalid range.
///
/// The function is synchronous because every file operation is: the
/// storage underneath is synchronous by nature. The caller runs it
/// inline, on the thread that runs its effect loop, and the run waits
/// for it.
///
/// # Errors
/// Returns the [`VfsError`] that [`Access::perform`] returns for the
/// operation.
/// When the caller resumes the run with that answer to a `store` call,
/// the Engine raises the failure at the prompt author's call site,
/// except a conflict, which ends the run with a determinism error that
/// no `pcall` catches.
pub fn perform_vfs_op(access: &Access, op: &VfsOp) -> std::result::Result<VfsOutcome, VfsError> {
    access.perform(op)
}

#[cfg(test)]
mod tests;
