//! One performer trait for chat effects, one for tool-call and render
//! effects, and one for timer effects: the traits the effect loop
//! performs a run's effects through.
//!
//! The Engine issues an [`Effect`](runvil_engine::effect::Effect) as a
//! value and waits for its
//! [`EffectAnswer`](runvil_engine::effect::EffectAnswer); a performer is
//! the Harness code that turns the one into the other. Each trait takes the
//! effect's fields and returns the answer's payload for its kind, so a
//! performer never sees the run, the recorder, or another kind's effects. The
//! effect loop owns the correlation: it hands each result back to the run
//! under the effect's id and writes the answer's record.
//!
//! Each performer returns a boxed `'static` future that the loop polls
//! inside the run's own future, beside every other effect in flight, so a
//! performer must move what its future needs into it. A performer must not
//! block while polled: one that blocks stalls every other effect of the
//! run, and the run's stop and cancel with them. A performer with blocking
//! or CPU-heavy work hands it to the Host's own runtime and awaits the
//! result. A `Vfs` effect has no performer: it is one file operation, on
//! a store of the run or, for a tool call that asked through its files,
//! on the run's filesystem, and file operations are synchronous by
//! design, so the loop answers it inline.
//!
//! The runner supplies the tool performer itself: the run's snapshot of
//! the Host's Packs, taken at preparation. The Host supplies the
//! [`InferenceBroker`] and the [`Timer`]. [`OfflineBroker`] is the stock
//! broker of a Host that runs prompts without a model.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use runvil_engine::effect::{PackToolSets, Round, ToolCallOrigin};
use runvil_engine::model::{
    Completion, CompletionOptions, Message, ModelBinding, ModelCatalog, ModelInfo, ToolSchema,
};
use runvil_engine::{BoxFuture, Error, Timestamp};
use runvil_engine::{ToolId, ToolInfo, ToolOutput};
use runvil_plugin::ToolFiles;
use serde_json::Value;

pub use crate::performers_offline::OfflineBroker;

/// One live piece of a model reply: a fragment of the answer text or of
/// the model's reasoning text.
///
/// A broker hands each piece to the `on_piece` callback of its
/// [`InferenceBroker::chat`] call as the model produces it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Delta {
    /// A fragment of the answer text.
    Text(String),
    /// A fragment of the model's reasoning text.
    Reasoning(String),
}

/// The callback a broker calls with each [`Delta`] of a reply as it forms.
///
/// The callback returns at once, and it calls neither the broker nor the
/// Harness. It is an `Arc` so that a broker which wraps another broker
/// can hand the same callback to the one it wraps.
pub type PieceSink = Arc<dyn Fn(Delta) + Send + Sync>;

/// The model inference the Host supplies to the Harness.
///
/// A broker lists the models it serves, says whether it streams, and
/// performs each `Chat` effect as one model round.
///
/// The Harness polls each round inside the run's own future, so a broker
/// must not block while polled. A broker sends blocking or CPU-heavy work
/// to the Host's own runtime.
///
/// Each method but `supports_streaming` returns a [`BoxFuture`] for the
/// Harness to poll. The future is `Send` and `'static`. It can move
/// between threads, and it owns the data it uses.
pub trait InferenceBroker: Send + Sync {
    /// Lists the models the broker serves.
    fn models(&self) -> BoxFuture<'static, Result<ModelCatalog, Error>>;

    /// Tells whether the broker calls `on_piece` while a reply forms.
    ///
    /// The answer is a hint for a Host's display: a Host that shows live
    /// text can tell the person using it that this broker streams nothing.
    /// The Harness passes `on_piece` to `chat` whatever the answer is.
    fn supports_streaming(&self) -> bool;

    /// Runs one model round and returns the finished reply.
    ///
    /// The round runs under `binding`. It sends `messages` to the model
    /// with `tools` advertised, using `options`, the fixed request settings
    /// built from `binding`. `round` holds the round's run-wide id and the
    /// path that dispatched it.
    ///
    /// `on_piece` is the callback for the reply's live pieces. A broker
    /// that streams calls it once for each piece, in the order the model
    /// produced the pieces, one call at a time, inline from the round's
    /// future, and never after that future has ended or been dropped. The
    /// callback returns at once, and it calls neither the broker nor the
    /// Harness. A broker that cannot stream leaves it unused. The
    /// finished reply is whole either way.
    fn chat(
        &self,
        binding: ModelBinding,
        messages: Vec<Message>,
        tools: Vec<ToolSchema>,
        options: CompletionOptions,
        round: Round,
        on_piece: Option<PieceSink>,
    ) -> BoxFuture<'static, Result<Box<Completion>, Error>>;
}

/// Performs a `ToolCall` effect: resolves `tool` to an implementation and
/// calls it with `args`, lending it the call's `origin` and `files`. Also
/// performs a `Render` effect: the tools each offered Pack shows the
/// model.
///
/// Each method returns a [`BoxFuture`] for the Harness to poll. The future
/// is `Send` and `'static`. It can move between threads, and it owns the
/// data it uses.
pub trait ToolPerformer: Send + Sync {
    /// Calls the tool. `alias` is the tool's wire name for a model call and
    /// its canonical id for a script call, for the performer's own
    /// diagnostics; `tool` is the identity it resolves. `origin` says who
    /// made the call. `files` is the call's desk at the effect loop, and
    /// the tool's one way to the run's files: the tool asks it for each
    /// file operation, and the loop performs and records the operation
    /// and resolves the request. The returned future owns both until it
    /// finishes, and drops them when it finishes or is dropped.
    fn call(
        &self,
        tool: ToolId,
        alias: String,
        origin: ToolCallOrigin,
        args: Value,
        files: Arc<dyn ToolFiles>,
    ) -> BoxFuture<'static, Result<ToolOutput, Error>>;

    /// Renders the tools the section entry `entry` offered from each Pack
    /// in `offered`, for `model`, the model of the entry's first round, and
    /// answers every Pack asked, in the order asked, with the tools it
    /// shows the model for each set. The default passes every tool
    /// through. An error from a render fails the whole render: the loop
    /// logs it and answers the effect `Dropped`, as it does for a render
    /// that panics.
    fn render(
        &self,
        _entry: String,
        _model: ModelInfo,
        offered: Vec<PackToolSets<ToolInfo>>,
    ) -> BoxFuture<'static, Result<Vec<PackToolSets<ToolInfo>>, Error>> {
        Box::pin(async move { Ok(offered) })
    }
}

/// The Host's clock, which performs each `Timer` effect as one sleep and
/// stamps each run's start.
///
/// The Harness polls each sleep inside the run's own future, so a timer
/// must not block while polled. It waits on the Host's own runtime.
pub trait Timer: Send + Sync {
    /// Resolves once `duration` has passed.
    ///
    /// The returned [`BoxFuture`] is `Send` and `'static`. It can move
    /// between threads, and it owns the data it uses.
    fn sleep(&self, duration: Duration) -> BoxFuture<'static, ()>;

    /// The time now. The Harness stamps each run's
    /// `started_at` with it. The default reads the
    /// system clock.
    fn now(&self) -> Timestamp {
        // A clock before the epoch or beyond `i64` milliseconds, neither
        // reachable on a real machine, saturates to the epoch rather than
        // refusing the run.
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|elapsed| i64::try_from(elapsed.as_millis()).ok())
            .map_or(Timestamp::UNIX_EPOCH, Timestamp::from_unix_millis)
    }
}
