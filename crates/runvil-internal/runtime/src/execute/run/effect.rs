//! Effects as values: what the Engine asks its caller to perform, and what
//! the caller answers with.
//!
//! A leaf request a section VM yields - a model round, a bound tool call,
//! a store operation, a timer - is not performed where it is dispatched.
//! The arm builds an [`Effect`], a plain description of the work, and the
//! run returns it from `step` for the caller to perform; the caller's
//! [`EffectAnswer`] comes back through `resume` keyed by the effect's
//! [`EffectId`], and the scheduler applies it on the caller's thread,
//! emitting the round's events there. The Engine thus decides *what* to do
//! and *what it means*; the Harness performs it. A section entry's first
//! model round that offers catalog tools first issues a render, whose
//! answer the Engine merges into the entry's frozen sets before it issues
//! the round. A file operation that an open tool call asks for is an
//! effect too: the caller reports the request, and the run issues it as a
//! `Vfs` effect whose [`VfsAsker`] is the call.
//!
//! An [`Effect`] may hold a live handle (a filesystem access) and so does
//! not serialize itself. [`Effect::record`] projects it onto a JSON value,
//! the effect minus its handles, which round-trips exactly: a run log
//! stores records, and a later replay compares a re-executed run's records
//! against them. An [`EffectAnswer`] likewise contains values a log cannot
//! hold whole (a completion's bodies, and a failure's full message,
//! location, and facts);
//! [`EffectAnswer::record`] projects it onto the answer's outcome as a log
//! stores it. Both values serialize the crate-private record types,
//! [`EffectRecord`] and [`AnswerRecord`].

use std::sync::Arc;
use std::time::Duration;

use runvil_types::event::ReplyOrigin;
use runvil_types::ids::{ChainId, RoundId};
use runvil_types::packs::PackId;
use runvil_types::tools::{OutputTrust, ToolId, ToolInfo, ToolOutput};
pub use runvil_types::tools::{ToolCallOrigin, ToolCaller};
use runvil_vfs::{Access, Stat, VfsError, VfsOp, VfsOutcome};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::{Completion, CompletionResult, Message, ToolSchema};
use crate::model::{ModelBinding, Temperature};

/// The identifier of one in-flight effect, which pairs an issued
/// [`Effect`] with its [`EffectAnswer`].
///
/// The id is opaque. The run allocates it from a run-wide counter, so the
/// same effect may get a different id in another run. Ids order by issue
/// within a run: an effect issued later has a greater id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EffectId(pub(crate) u64);

impl EffectId {
    /// Returns the id as a raw number. The caller can use it as a key, for
    /// example in its log or its task table. The number is meaningful only
    /// within the run that issued it.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for EffectId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The identity of one model round: its id and the path that dispatched
/// it. Every `Effect::Chat` carries one.
///
/// Ids number the run's model rounds from 0 in dispatch order. Chat rounds
/// and nested `models.infer` rounds share one sequence. The thinking,
/// reply, and tool-call events reported from the round's answer carry the
/// same id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Round {
    /// The round's run-wide id.
    pub id: RoundId,
    /// The path that dispatched the round. [`ReplyOrigin::Chat`] marks a
    /// section's `chat` round, the kind `models.loop` runs, whose live
    /// deltas the caller can stream. [`ReplyOrigin::Infer`] marks a nested
    /// `models.infer` round, where only the completed reply is consumed.
    pub origin: ReplyOrigin,
}

impl Round {
    /// Builds the identity of round `id`, dispatched from `origin`.
    #[must_use]
    pub fn new(id: RoundId, origin: ReplyOrigin) -> Round {
        Round { id, origin }
    }
}

/// One catalog Pack's tools for each set of a section entry, in order: the
/// tools the section offered from the Pack, or the tools the Pack's render
/// returned for them. A set the section did not offer the Pack into is
/// empty. A render effect and its answer hold each tool's [`ToolInfo`],
/// and the effect's record holds each tool's [`ToolId`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PackToolSets<T> {
    /// The Pack.
    pub pack: PackId,
    /// The Pack's tools for the Static Set, which every model round of the
    /// section entry advertises.
    pub static_set: Vec<T>,
    /// The Pack's tools for the Dynamic Set, which the model reaches
    /// through the Engine's two discovery tools.
    pub dynamic_set: Vec<T>,
}

impl<T> PackToolSets<T> {
    /// Builds the Pack `pack`'s tools: `static_set` for the Static Set and
    /// `dynamic_set` for the Dynamic Set.
    #[must_use]
    pub fn new(pack: PackId, static_set: Vec<T>, dynamic_set: Vec<T>) -> PackToolSets<T> {
        PackToolSets {
            pack,
            static_set,
            dynamic_set,
        }
    }
}

/// Who asked for the operation of an [`Effect::Vfs`]. The asker settles
/// the files the operation works on: the prompt's Lua works on its
/// chain's store, and a tool call works on the filesystem the caller gave
/// the run.
///
/// Closed on purpose, like [`Effect`]: the caller's `match` names every
/// asker and needs no wildcard arm. A new asker breaks every caller's
/// build, rather than reaching a caller that gives its effect the answer
/// of another asker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VfsAsker {
    /// A `store` call of the prompt's Lua, on the store made for this
    /// chain.
    Lua {
        /// Which store the operation is on: the id of the chain the store
        /// was made for. That is the root chain's id, `0`, for the store
        /// the run starts with, and for a store an `execute` made, the id
        /// of the chain that runs the executed prompt. Two operations on
        /// one path are on the same file only when this id is the same.
        store: ChainId,
    },
    /// The open tool call with this id, on the filesystem the caller gave
    /// the run.
    Tool {
        /// The id of the tool call's [`ToolCall`](Effect::ToolCall)
        /// effect.
        call: EffectId,
    },
}

/// What a run's record keeps of the outcome of a file operation that a
/// tool call asked for: a count where the outcome holds content.
///
/// The caller hands the tool the whole [`VfsOutcome`] and answers the
/// effect with the receipt, so the run holds no copy of the bytes of a
/// read. The tool's own result is the answer of its
/// [`ToolCall`](Effect::ToolCall) effect.
///
/// The enum is `#[non_exhaustive]`, so a `match` on it outside this crate
/// needs a wildcard arm.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum VfsReceipt {
    /// The operation succeeded and returned nothing.
    Done,
    /// The operation returned this many bytes: the text of a `Read` or a
    /// `ReadNumbered`, or the bytes of a `ReadRange`.
    Bytes(u64),
    /// The operation returned this many items: the entries of a `List`,
    /// or the paths of a `Glob`.
    Count(u64),
    /// Whether the path of an `Exists` exists.
    Bool(bool),
    /// The metadata of the path of a `Stat`.
    Stat(Stat),
}

impl VfsReceipt {
    /// Returns the receipt of `outcome`: the outcome itself where it is a
    /// stat, a bool, or nothing, and the count of its bytes, entries, or
    /// paths otherwise.
    #[must_use]
    pub fn of(outcome: &VfsOutcome) -> VfsReceipt {
        let count = |len: usize| u64::try_from(len).unwrap_or(u64::MAX);
        match outcome {
            VfsOutcome::Text(text) => VfsReceipt::Bytes(count(text.len())),
            VfsOutcome::Bytes(bytes) => VfsReceipt::Bytes(count(bytes.len())),
            VfsOutcome::Paths(paths) => VfsReceipt::Count(count(paths.len())),
            VfsOutcome::Entries(entries) => VfsReceipt::Count(count(entries.len())),
            VfsOutcome::Bool(flag) => VfsReceipt::Bool(*flag),
            VfsOutcome::Stat(stat) => VfsReceipt::Stat(stat.clone()),
            VfsOutcome::Unit => VfsReceipt::Done,
        }
    }
}

/// One piece of work the Engine asks the caller to perform.
///
/// The enum is closed on purpose: it has no `#[non_exhaustive]`, so the
/// caller's `match` names every kind and needs no wildcard arm. A new
/// effect kind breaks every caller's build, rather than reaching a caller
/// that cannot perform it.
#[derive(Debug)]
pub enum Effect {
    /// One model round: send `messages` to the model with `tools`
    /// advertised, using the options `binding` freezes. A nested
    /// `models.infer` round sends one user message with an empty tool
    /// list, and the Engine consumes only its completed reply.
    Chat {
        /// The binding the round runs under.
        binding: ModelBinding,
        /// The conversation to send, in wire order.
        messages: Vec<Message>,
        /// The earlier round whose request this one extends, the previous
        /// send of the same section's `messages` list, with how many
        /// leading messages this request shares with that round's request.
        /// `None` on a section's first send and on a nested `models.infer`
        /// round.
        after: Option<(RoundId, u64)>,
        /// The tool schemas advertised to the model for this round.
        tools: Vec<ToolSchema>,
        /// The round's id and origin. The caller needs to forward live
        /// deltas only for a round whose origin is [`ReplyOrigin::Chat`].
        /// The effect's record keeps the id and drops the origin, because
        /// the request sent to the model is the same for either origin.
        round: Round,
    },
    /// The render of the catalog Packs a section entry offered, issued at
    /// the entry's first loop round before that round's request is
    /// built. The caller answers with the tools each Pack shows the model
    /// for each set, and the Engine merges them with the section's adhoc
    /// tools into the entry's frozen sets, which every round of the entry
    /// then sends. A caller that changes nothing answers with the offered
    /// tools. The effect's record keeps the binding's alias and each
    /// offered tool's id.
    Render {
        /// The section entry, the value its Lua reads as `sys.id`, which
        /// the origin of each of the entry's tool calls also names.
        entry: String,
        /// The binding the entry's first round runs under.
        binding: ModelBinding,
        /// Each offered catalog Pack once, in the order its first tool was
        /// offered, with the tools the section offered from it into each
        /// set.
        offered: Vec<PackToolSets<ToolInfo>>,
    },
    /// One call to a Pack's tool: a run catalog tool, or a tool only the
    /// model sees, from the section entry's render. The caller resolves
    /// `tool` to an implementation. The effect holds no way to a file:
    /// the tool asks the caller for each file operation, and the caller
    /// reports the request with `Run::request_file`. The effect's record
    /// keeps every field but `survives_stop`, which the run catalog and
    /// the render's answer already give.
    ToolCall {
        /// The tool's stable live identity.
        tool: ToolId,
        /// The tool's wire name for a model call, or its id for a script call.
        alias: String,
        /// The call's arguments.
        args: Value,
        /// Who made the call: the section and its entry, and the
        /// section's script or the model whose round asked for it.
        origin: ToolCallOrigin,
        /// Whether a stop leaves the call in flight, from the binding the
        /// call resolved to: the run catalog's flag for a catalog tool, and
        /// the flag the section entry's render gave a tool only the model
        /// sees.
        survives_stop: bool,
    },
    /// One file operation, which the caller performs with
    /// `Access::perform` on `access`. `asker` says who asked for it, and
    /// with that which files it works on and which answer it takes.
    ///
    /// The prompt's Lua asks with one of the eight `store.*` calls, and
    /// the operation works on a store of the run. A store is the run's
    /// own: the Engine makes it empty, in memory. The run starts with
    /// one, which is gone when the run ends, and each `execute` makes one
    /// more for the prompt it runs, which is gone when that prompt's
    /// chain ends. A store is no part of the filesystem the caller gave
    /// the run, so no path of that filesystem reaches it, and a tool sees
    /// none of it. `access` is then a view of the calling chain's store
    /// under the chain's identity, which the Engine makes at dispatch,
    /// and it is the caller's one way to that store. The caller answers
    /// with [`EffectAnswer::Vfs`], the whole outcome.
    ///
    /// An open tool call asks through the caller: the caller reports the
    /// tool's request with `Run::request_file`, and the operation works
    /// on the filesystem the caller gave the run. `access` is then the
    /// call's own identity, which the Engine forks from the calling
    /// chain's access when it issues the call and joins back into it
    /// when the call's answer is applied or the call is aborted. So the
    /// chain's earlier work happens before the tool's, and the tool's
    /// before the chain's next step. That access is rooted at `/` of the
    /// filesystem, reaches no store, and refuses every operation once the
    /// call has ended. The caller hands the whole outcome to the tool and
    /// answers with [`EffectAnswer::VfsReceipt`].
    ///
    /// The caller must use the access exactly as given, for this one
    /// operation. When the caller drops the access does not affect
    /// the run's correctness. Store claims follow happens-before order
    /// within the run's scope. The run ends that scope at `Done` or when
    /// the run is dropped, and after that the access refuses every
    /// operation.
    Vfs {
        /// Who asked for the operation.
        asker: VfsAsker,
        /// The access the operation runs on: the chain's view of its
        /// store, or the tool call's own access.
        access: Arc<Access>,
        /// The validated operation.
        op: VfsOp,
    },
    /// One sleep of `duration`, which the Engine uses as the timeout
    /// behind a timed wait.
    Timer {
        /// How long the sleep lasts.
        ///
        /// The effect holds a [`Duration`], but its record stores fractional
        /// seconds as an `f64`, so a half-second timer records as
        /// `{"Timer":{"seconds":0.5}}`. A sub-second duration keeps its
        /// fraction there rather than rounding to whole seconds.
        duration: Duration,
    },
}

/// One value's serde wire form. Every type recorded here serializes
/// infallibly (strings, numbers, and JSON values), so the `Null` fallback
/// is unreachable in practice and stands only so the projection stays
/// total.
fn wire_value<T: Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

/// One offered Pack's tools as their ids, the form a render's record
/// keeps.
fn offered_ids(sets: &PackToolSets<ToolInfo>) -> PackToolSets<ToolId> {
    let ids = |tools: &[ToolInfo]| tools.iter().map(|tool| tool.id.clone()).collect();
    PackToolSets::new(
        sets.pack.clone(),
        ids(&sets.static_set),
        ids(&sets.dynamic_set),
    )
}

impl Effect {
    /// Returns the effect's record: the same request with its live handles
    /// removed, as the JSON value that a run log stores and a replay
    /// compares.
    ///
    /// The value is one object keyed by the effect's kind, such as
    /// `{"Timer":{"seconds":0.5}}`, and it round-trips exactly through
    /// `serde_json`. A `Chat` record holds the round's id, the alias of the
    /// model slot it ran under, the frozen invocation settings, the
    /// advertised tool names, and the request's messages in wire form after
    /// the first `keep`, which repeat round `after`'s request. So a caller
    /// that logs records must record every `Chat` effect a step returns,
    /// including ones it drops, for each `after` to name a recorded round.
    #[must_use]
    pub fn record(&self) -> Value {
        wire_value(&self.to_record())
    }

    /// Returns the effect's record in the typed form that
    /// [`record`](Self::record) serializes.
    pub(crate) fn to_record(&self) -> EffectRecord {
        match self {
            Effect::Chat {
                binding,
                messages,
                after,
                tools,
                round,
                ..
            } => {
                let invocation = binding.invocation();
                let (after, keep) = match *after {
                    Some((after, keep)) => (Some(after), keep),
                    None => (None, 0),
                };
                let kept = usize::try_from(keep).unwrap_or(usize::MAX);
                EffectRecord::Chat {
                    round: round.id,
                    alias: binding.alias().to_owned(),
                    after,
                    keep,
                    messages: messages.iter().skip(kept).map(wire_value).collect(),
                    tools: tools
                        .iter()
                        .map(|schema| schema.name().to_owned())
                        .collect(),
                    temperature: invocation.temperature.map(Temperature::get),
                    max_tokens: invocation.max_tokens.map(std::num::NonZeroU32::get),
                    thinking: invocation.thinking,
                }
            }
            Effect::Render {
                entry,
                binding,
                offered,
            } => EffectRecord::Render {
                alias: binding.alias().to_owned(),
                entry: entry.clone(),
                offered: offered.iter().map(offered_ids).collect(),
            },
            Effect::ToolCall {
                tool,
                alias,
                args,
                origin,
                ..
            } => EffectRecord::ToolCall {
                tool: tool.clone(),
                alias: alias.clone(),
                args: args.clone(),
                origin: origin.clone(),
            },
            Effect::Vfs { asker, op, .. } => EffectRecord::Vfs {
                asker: asker.clone(),
                op: op.clone(),
            },
            Effect::Timer { duration } => EffectRecord::Timer {
                seconds: duration.as_secs_f64(),
            },
        }
    }
}

/// An [`Effect`] with its live handles removed, as a run log stores it. A
/// replay compares each re-issued effect against its record.
///
/// The `Chat` record holds the round's id, the alias of the model slot it
/// ran under, the frozen invocation settings (temperature, generation cap,
/// and thinking switch), the advertised tool names, and the request's
/// messages in wire form after the first `keep`, which repeat round
/// `after`'s request. So a caller that logs records must record every
/// Chat effect a step returns, including ones it drops, for each `after` to
/// name a recorded round. It names the slot by alias, because the
/// caller may serve the slot with a different model from the bound one.
/// The answer's [`ChatAnswerRecord`] names the model that served the round.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) enum EffectRecord {
    /// One model round.
    Chat {
        /// The round's run-wide id, which the round's content events also
        /// carry.
        round: RoundId,
        /// The prompt-local alias of the slot the round ran under.
        alias: String,
        /// The earlier round whose request this one extends, or `None`
        /// when the request starts here.
        after: Option<RoundId>,
        /// How many leading messages of `after`'s request this request
        /// repeats; 0 when `after` is `None`.
        keep: u64,
        /// The request's messages after the first `keep`, one wire-form
        /// message per entry.
        messages: Vec<Value>,
        /// The advertised tool names, in schema order.
        tools: Vec<String>,
        /// The frozen sampling temperature, when the bind declared one.
        temperature: Option<f64>,
        /// The frozen generation cap, when the bind declared one.
        max_tokens: Option<u32>,
        /// The frozen thinking switch, when the bind declared one.
        thinking: Option<bool>,
    },
    /// One render of a section entry's offered catalog Packs. The answer's
    /// record holds the rendered tools whole.
    Render {
        /// The prompt-local alias of the slot the entry's first round runs
        /// under.
        alias: String,
        /// The section entry, the value its Lua reads as `sys.id`.
        entry: String,
        /// Each offered Pack once, in the order its first tool was offered,
        /// with the ids of the tools the section offered from it into each
        /// set.
        offered: Vec<PackToolSets<ToolId>>,
    },
    /// One call to a Pack's tool, a run catalog tool or a tool only the
    /// model sees.
    ToolCall {
        /// The tool's stable live identity.
        tool: ToolId,
        /// The tool's wire name for a model call, or its id for a script call.
        alias: String,
        /// The call's arguments.
        args: Value,
        /// Who made the call: the section and its entry, and the
        /// section's script or the model whose round asked for it.
        origin: ToolCallOrigin,
    },
    /// One file operation.
    Vfs {
        /// Who asked, written as one key beside the operation. A `store`
        /// call of the prompt's Lua writes `store`, the id of the chain
        /// the store was made for, `0` for the store the run starts with:
        /// the records that name one store, in order, are every operation
        /// on that store. A tool call writes `call`, the id of its
        /// `ToolCall` effect, and its operation worked on the filesystem
        /// the caller gave the run.
        #[serde(
            flatten,
            serialize_with = "super::vfs_record::serialize_asker",
            deserialize_with = "super::vfs_record::deserialize_asker"
        )]
        asker: VfsAsker,
        /// The validated operation, written in its record form: one
        /// object keyed by the operation's name. The bytes of a write or
        /// an append are text under `contents` when they are UTF-8, and
        /// hex under `contents_hex` otherwise.
        #[serde(
            serialize_with = "super::vfs_record::serialize_op",
            deserialize_with = "super::vfs_record::deserialize_op"
        )]
        op: VfsOp,
    },
    /// One sleep.
    Timer {
        /// The duration in seconds.
        seconds: f64,
    },
}

/// The answer to one [`Effect`]: one variant per effect kind, a second
/// one for a `Vfs` effect that a tool call asked for, plus
/// [`Dropped`](EffectAnswer::Dropped) for an effect the caller gave up on.
/// Every effect receives exactly one answer.
///
/// Closed on purpose, like [`Effect`]: a new answer kind arrives with a
/// new effect kind, and both break every caller's build.
#[derive(Debug)]
pub enum EffectAnswer {
    /// The model round's completion or its failure.
    Chat(std::result::Result<Box<Completion>, runvil_types::error::Error>),
    /// Each offered Pack's rendered tools for each set, in the order the
    /// render asked: the same Packs, with each set the Pack was not
    /// offered into left empty. Every rendered tool's id starts with its
    /// Pack's id, a run catalog tool may appear only in a set the section
    /// offered it in, and a catalog tool keeps the schema it was offered
    /// with; an answer that breaks one of these rules, or the order, fails
    /// the round. A catalog tool keeps the catalog's flags and output kind,
    /// so a render changes only its wire name and description, and any other
    /// tool is one only the model can call, with the flags the render gave
    /// it. A tool repeated within a set keeps its first copy.
    Render(Vec<PackToolSets<ToolInfo>>),
    /// The tool's own output or its own failure, before the Engine applies
    /// its trust rule.
    ToolCall(std::result::Result<ToolOutput, runvil_types::error::Error>),
    /// The answer to a `Vfs` effect that the prompt's Lua asked for: the
    /// whole outcome of the operation on the effect's store view, or the
    /// store's own structured failure. The `store` call resumes with it.
    Vfs(std::result::Result<VfsOutcome, VfsError>),
    /// The answer to a `Vfs` effect that a tool call asked for: the
    /// receipt of the operation's outcome, or the operation's failure.
    /// It resumes no chain: the caller has handed the outcome, or the
    /// failure, to the tool, whose call stays open.
    VfsReceipt(std::result::Result<VfsReceipt, VfsError>),
    /// The timer fired.
    Timer,
    /// The caller gave up on the effect and dropped it, for example
    /// because the run was canceled or the effect's task ended first. If
    /// the chain still waits on the effect, it resumes with a canceled
    /// error. Uncaught, that error ends the run as a cancel. A drop is an
    /// answer like any other, so it counts as the effect's one answer.
    Dropped,
}

impl EffectAnswer {
    /// Returns the answer's record: the parts of its outcome that a log can
    /// hold whole, as a JSON value.
    ///
    /// The value is keyed by the answer's kind, such as
    /// `{"ToolCall":{"Ok":{"text":"done","trust":"trusted"}}}` or
    /// `"Dropped"`, and it round-trips exactly through `serde_json`. A
    /// failure is recorded as `{"kind":..,"message":..}`: its stable tag and
    /// its text, which is the prose of a model or tool failure and the
    /// display text of a file operation's failure. A completion is recorded
    /// as its reply or its requested tool names beside the model and finish
    /// reason, and a render as its rendered tools whole. A `Vfs` answer is
    /// recorded as the whole outcome, and a `VfsReceipt` answer as the
    /// receipt, such as `{"VfsReceipt":{"Ok":{"Bytes":12}}}`. The round's
    /// request and response bodies travel as debug events, and its metrics
    /// travel in the turn's event.
    #[must_use]
    pub fn record(&self) -> Value {
        wire_value(&self.to_record())
    }

    /// Returns the answer's record in the typed form that
    /// [`record`](Self::record) serializes.
    pub(crate) fn to_record(&self) -> AnswerRecord {
        match self {
            EffectAnswer::Chat(result) => AnswerRecord::Chat(match result {
                Ok(completion) => Ok(ChatAnswerRecord::from(completion.as_ref())),
                Err(error) => Err(FailureRecord::new(error.code.tag(), error.prose.clone())),
            }),
            EffectAnswer::Render(rendered) => AnswerRecord::Render(rendered.clone()),
            EffectAnswer::ToolCall(result) => AnswerRecord::ToolCall(match result {
                Ok(output) => Ok(ToolAnswerRecord::new(output.text(), output.trust())),
                Err(error) => Err(FailureRecord::new(error.code.tag(), error.prose.clone())),
            }),
            EffectAnswer::Vfs(result) => AnswerRecord::Vfs(match result {
                Ok(outcome) => Ok(outcome.clone()),
                Err(error) => Err(FailureRecord::new(error.tag(), error.to_string())),
            }),
            EffectAnswer::VfsReceipt(result) => AnswerRecord::VfsReceipt(match result {
                Ok(receipt) => Ok(receipt.clone()),
                Err(error) => Err(FailureRecord::new(error.tag(), error.to_string())),
            }),
            EffectAnswer::Timer => AnswerRecord::Timer,
            EffectAnswer::Dropped => AnswerRecord::Dropped,
        }
    }
}

/// An [`EffectAnswer`] as a run log stores it: one variant per answer
/// kind, each holding its outcome with every failure recorded as a
/// [`FailureRecord`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) enum AnswerRecord {
    /// The model round's outcome.
    Chat(std::result::Result<ChatAnswerRecord, FailureRecord>),
    /// Each Pack's rendered tools per set, in the order asked, whole,
    /// because the `Chat` record keeps only tool names. The section's
    /// adhoc tools and the discovery tools join later, inside the Engine,
    /// so they are not here.
    Render(Vec<PackToolSets<ToolInfo>>),
    /// The tool call's outcome.
    ToolCall(std::result::Result<ToolAnswerRecord, FailureRecord>),
    /// The outcome of the operation on the effect's store view: the
    /// whole [`VfsOutcome`] on success, written in its record form, or
    /// the failure's record. `Unit` is the bare string `"Unit"`, and
    /// every other outcome is a one-key object, such as
    /// `{"Text":"..."}`, `{"Paths":["a.txt"]}`, or `{"Bool":true}`.
    Vfs(
        #[serde(
            serialize_with = "super::vfs_record::serialize_answer",
            deserialize_with = "super::vfs_record::deserialize_answer"
        )]
        std::result::Result<VfsOutcome, FailureRecord>,
    ),
    /// The outcome of an operation that a tool call asked for, as its
    /// receipt: `"Done"`, or a one-key object such as `{"Bytes":12}`,
    /// `{"Count":3}`, `{"Bool":true}`, or `{"Stat":{..}}`. A failure is
    /// its record.
    VfsReceipt(
        #[serde(
            serialize_with = "super::vfs_record::serialize_receipt",
            deserialize_with = "super::vfs_record::deserialize_receipt"
        )]
        std::result::Result<VfsReceipt, FailureRecord>,
    ),
    /// The timer fired.
    Timer,
    /// The caller gave up on the effect and dropped it.
    Dropped,
}

/// A failed answer as the log records it, serialized as
/// `{"kind": .., "message": ..}`.
///
/// `kind` is the stable tag of the failure's kind: the model code's tag for
/// a model round (such as `malformed_response`), the tool code's tag for a
/// tool call (such as `tool_invalid_arguments`), or the `VfsError` tag for a
/// file operation (such as `not_found`). `message` is the failure's
/// display text, which for a model round or a tool call is the prose the
/// model read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FailureRecord {
    /// The stable tag of the failure's kind.
    pub(crate) kind: String,
    /// The failure's display text.
    pub(crate) message: String,
}

impl FailureRecord {
    /// Builds the record of a failure of kind `kind` with `message` as its
    /// display text.
    #[must_use]
    pub(crate) fn new(kind: impl Into<String>, message: impl Into<String>) -> FailureRecord {
        FailureRecord {
            kind: kind.into(),
            message: message.into(),
        }
    }
}

/// A completed model round as the log records it: what identifies the
/// answer. The round's request and response bodies travel separately, as
/// debug events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ChatAnswerRecord {
    /// The model that served the round, as the completion names it.
    pub(crate) model: String,
    /// The provider's finish reason, when it sent one.
    pub(crate) finish_reason: Option<String>,
    /// The reply text, when the round produced text.
    pub(crate) reply: Option<String>,
    /// The names of the tools the model requested, in call order, when it
    /// requested any.
    pub(crate) tool_calls: Vec<String>,
}

#[cfg(test)]
impl ChatAnswerRecord {
    /// Builds the record of a round `model` served, with no finish reason,
    /// no reply, and no tool calls.
    #[must_use]
    pub(crate) fn new(model: impl Into<String>) -> ChatAnswerRecord {
        ChatAnswerRecord {
            model: model.into(),
            finish_reason: None,
            reply: None,
            tool_calls: Vec::new(),
        }
    }

    /// Sets the provider's finish reason.
    #[must_use]
    pub(crate) fn with_finish_reason(mut self, finish_reason: Option<String>) -> ChatAnswerRecord {
        self.finish_reason = finish_reason;
        self
    }

    /// Sets the reply text.
    #[must_use]
    pub(crate) fn with_reply(mut self, reply: Option<String>) -> ChatAnswerRecord {
        self.reply = reply;
        self
    }

    /// Sets the names of the tools the model requested, in call order.
    #[must_use]
    pub(crate) fn with_tool_calls(mut self, tool_calls: Vec<String>) -> ChatAnswerRecord {
        self.tool_calls = tool_calls;
        self
    }
}

impl From<&Completion> for ChatAnswerRecord {
    fn from(completion: &Completion) -> Self {
        let (reply, tool_calls) = match completion.result() {
            CompletionResult::Text(text) => (Some(text.clone()), Vec::new()),
            CompletionResult::ToolCalls(calls) => (
                None,
                calls.iter().map(|call| call.name().to_owned()).collect(),
            ),
            // The vocabulary is `#[non_exhaustive]`; a variant this crate
            // does not know records as a round with neither product.
            _ => (None, Vec::new()),
        };
        ChatAnswerRecord {
            model: completion.model().to_owned(),
            finish_reason: completion.finish_reason().map(str::to_owned),
            reply,
            tool_calls,
        }
    }
}

/// A tool's own output as the log records it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ToolAnswerRecord {
    /// The output text, before the Engine's trust rules apply.
    pub(crate) text: String,
    /// The trust level the tool declared for its output.
    pub(crate) trust: OutputTrust,
}

impl ToolAnswerRecord {
    /// Builds the record of an output of `text` at `trust`.
    #[must_use]
    pub(crate) fn new(text: impl Into<String>, trust: OutputTrust) -> ToolAnswerRecord {
        ToolAnswerRecord {
            text: text.into(),
            trust,
        }
    }
}

#[cfg(test)]
#[path = "effect-tests.rs"]
mod tests;
