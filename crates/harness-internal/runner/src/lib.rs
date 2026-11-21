//! harness-runner - the per-run Harness and the effect loop it drives. A
//! Host builds one [`Host`] at startup, holding its installed [`Packs`]
//! and how it finds prompts, and [`Host::parse`] finds a prompt
//! by its name, parses it and every prompt it links, checks the `uses:`
//! list of each against those Packs, and returns the parsed prompt behind
//! an `Arc` that every run of it shares. Three stock finders serve a
//! Host that writes none of its own: [`FindTextPrompt`] one prompt from
//! text, [`FindFilePrompt`] one from a file, and [`PromptSet`] the prompts
//! a Host inserts as text, as files, and as directories.
//! A stock broker, [`performers::OfflineBroker`], serves a Host that runs
//! prompts without a model.
//! A launcher hands each run a [`RunSetup`] with everything that
//! run gets on top of the Host: the recorder, the broker, and the timer
//! it waits on, and its own inputs. A [`Harness`] over the Host, the parsed
//! prompt, and the setup begins the run at the setup's recorder with the
//! inputs the Engine refuses to draw itself, resolves the run's model
//! through the setup's broker, prepares an Engine `Run` (making the run's
//! instance of each of its Packs and taking the run's snapshot of them),
//! steps it, performs each chat, render, tool-call, and timer effect
//! through its performer and answers each Vfs effect inline, feeds the
//! answers back, delivers each message the launcher sends through the
//! run's [`RunControl`] to the main chain, hands every event, effect, and
//! answer to the run's recorder, and ends the run there with its result,
//! the final text or the error that ended it, all inside the one future
//! [`Harness::run`] returns. A cancel is
//! the error whose code is `canceled`. A recorder
//! that refuses a write is the only way a run ends without a recorded
//! result, and that refusal, a `RecorderFailed` error, is the only error
//! [`Harness::run`] returns. A prompt that fails to parse is its caller's
//! error and leaves no run record.
//!
//! ## Invariants
//!
//! - The Harness polls every effect inside the run's own future and starts
//!   no task, so any executor can drive a run. Performers must not block
//!   while polled: one that blocks stalls every other effect of the run,
//!   and a performer with blocking or CPU-heavy work hands it to the
//!   Host's own runtime. A file operation is the exception: it is
//!   synchronous by design and holds the run until it returns. The
//!   crate's async needs come from `futures-util`,
//!   and `tokio` and `tokio-util` appear only under `[dev-dependencies]`
//!   (enforced by the Harness tokio ban in `cargo test -p build-xtask`).
//! - The Harness reads each [`RunSetup`] field once per run: the run's
//!   tool calls work in the one filesystem the setup holds.
//! - The Harness reaches a store of a run, the one the run starts with or
//!   one a prompt it executes works in, only to perform the one operation
//!   of a Vfs effect, through that effect's view. It puts nothing in a
//!   store before a run and reads nothing from one after: a run takes its
//!   input as arguments and hands back its result.
//! - A tool call asks for a file operation through the files of its
//!   context. The Harness reports each request to the run, performs the
//!   Vfs effect the run issues for it on the run's filesystem, records
//!   it, and hands the outcome to the call, so the record holds every
//!   operation a tool asks for this way. It drops the request of a call
//!   that has landed or was aborted.
//! - The recorder is written in loop order: a step's events before the
//!   step's effects are issued, each effect before its performer starts,
//!   each answer before the run resumes with it, and each sent message's
//!   `InboxMessage` event as the loop delivers it, after the events of
//!   the answers resumed before it and before the step that applies it.
//!   Every effect record has exactly one answer record; a dropped
//!   effect's answer is `Dropped`.
//! - The loop never reads an event to decide anything; control comes
//!   from the run's own word (`Step` and the effects it withdraws), the
//!   cancel flag, the Host's stop, and the messages the Host sends.
//! - A Host's [`Packs`] keeps the factory of every Pack its
//!   `PackRegistry` holds and of the two built-in Packs,
//!   `runvil/subagents` and `runvil/user`, which the Harness makes
//!   itself, and only those. The registry refuses a duplicate FQN, a
//!   Pack id it holds already, and the Pack id of a built-in, so a Host
//!   has one Pack per Pack id and no other Pack can take `subagents` or
//!   `user`. A run whose [`RunSetup`] has no user holds `runvil/user`
//!   unavailable, with the reason "this run has no user", and never makes
//!   it.
//! - A run holds every Pack the Host installed, whatever its prompt and
//!   the prompts that one links declare: a `uses:` entry only marks a
//!   Pack as required. The one Pack a run leaves out is a Pack whose Pack
//!   id a `uses:` entry of one of those prompts names under a vendor the
//!   Host did not install it from, and that run is refused. A run makes
//!   its own instance of each of its Packs, waits on the `ready` of each
//!   instance, and drops its instances when its future ends.
//! - A run hands a Pack's factory the Pack id alone, and a tool call
//!   only facts about that call. What a Pack needs from the Host, its
//!   factory holds from the time the Host filled its `PackRegistry`.
//! - A factory that refuses or panics leaves its Pack unavailable to that
//!   run alone: a refusal is logged at debug and a panic at error, and the
//!   next run calls the factory again.
//! - A stock finder reads a file only inside a find, and the Harness calls
//!   a finder only from [`Host::parse`], before the run exists, which
//!   finds every prompt the run holds. So no file read happens inside a
//!   run's future: a prompt the run executes with `execute` is one that
//!   parse linked.

mod display_chain;
pub mod effect_loop;
mod environment;
mod harness;
mod host;
mod packs;
mod packs_builtin;
pub mod performers;
mod performers_offline;
mod prepare;
mod prompt_finders;
pub mod recorder;
mod recorder_memory;
mod setup;
mod tool_files;

pub use display_chain::display_chain;
pub use harness::{Harness, RunControl, RunReport};
pub use host::Host;
pub use packs::Packs;
pub use prompt_finders::{FindFilePrompt, FindTextPrompt, PromptSet};
pub use recorder::{
    MemoryRecorder, Record, RecordKind, RunId, RunMeta, RunRecorder, make_recorder_error,
};
pub use setup::RunSetup;
