//! The request arms: one dispatch per validated protocol request from a
//! suspended chain. Every leaf arm builds its [`Effect`] and issues it
//! through the chain view's one `issue` path; the arm performs nothing and
//! emits nothing for the answer, which `apply_answer` handles when it
//! lands. Every store operation is a leaf yield, handed to the caller
//! with no inline fast path, so the run's record holds each one. A refusal
//! decided before dispatch may still report its failure and answer inline,
//! so the arm returns without the chain ever parking.
//!
//! This file keeps four routing arms: `infer`, `call`, `execute`, and
//! `store`. Every other group lives in its own module - `chat` the `chat`
//! arm, `tasks` the `spawn` arm, `tool_call` the `tool_call`,
//! `tool_calls`, and `adhoc_tool_done` arms, `timer` the `timer` arm,
//! `waits` the task wait, inspection, note, concurrency, and cancel arms,
//! and `inbox` the `receive`, `send`, `reply`, and `arrivals` arms and the
//! inbox check a `chat` request passes before the `chat` arm sends its
//! round.

use std::sync::Arc;

use runvil_types::error::place;
use runvil_types::event::ReplyOrigin;
use runvil_vfs::VfsOp;

use crate::execute::run::{Effect, VfsAsker};
use crate::execute::section_context::TaskSeed;
use crate::lua::{Answer, LibraryKey, Request};
use crate::model::{Message, ModelBinding};
use crate::section_pos::{SectionIndex, SectionPos};
use crate::{Result, error};

use super::chain::ChainStart;
use super::chain_record::{ChainKind, Parked};
use super::{ChainIndex, ChainOps, Continuation, Counters, Scheduler};

/// How one leaf dispatch resolved when its preparation succeeded: an
/// effect issued, with the chain parked on the effect table, or an answer
/// settled on the driver thread without leaving. A preparation failure
/// returns `Err` instead.
#[derive(Debug)]
pub(super) enum Dispatched {
    Issued,
    Answered(Answer),
}

/// What a chain parks on when it yields `request`; `None` for the arms
/// answered inline, whose chain is back on the ready queue before anyone
/// can look. A wait's set and a batch's slots start empty, and an inbox
/// wait starts not waiting; their arms fill them in when the chain parks.
fn parked_on(request: &Request) -> Option<Parked> {
    match request {
        Request::Infer { .. } | Request::Chat { .. } => Some(Parked::Chat),
        Request::Call { .. } | Request::Execute { .. } => Some(Parked::Call),
        Request::JoinAny { .. } => Some(Parked::Tasks(Vec::new())),
        Request::Receive | Request::Reply { .. } => Some(Parked::Inbox { waiting: false }),
        Request::ToolCall { .. } => Some(Parked::ToolCall),
        Request::ToolCalls { .. } => Some(Parked::Batch(Vec::new())),
        Request::Store { .. } => Some(Parked::Store),
        Request::Spawn { .. }
        | Request::Timer { .. }
        | Request::Ready { .. }
        | Request::Status { .. }
        | Request::Pending
        | Request::Concurrency { .. }
        | Request::Note { .. }
        | Request::Send { .. }
        | Request::Arrivals
        | Request::Cancel { .. }
        | Request::AdhocToolDone { .. } => None,
    }
}

impl Scheduler {
    /// Dispatches one validated request from a suspended chain.
    ///
    /// # Errors
    /// Returns the store arm's error when the chain's access capability is
    /// gone, the `adhoc_tool_done` arm's error when no adhoc tool call is
    /// open, or an `Internal` failure when an arm's answer finds no
    /// suspended block to take it.
    pub(super) fn dispatch(&mut self, id: ChainIndex, request: Request) -> Result<()> {
        self.chains[id].parked = parked_on(&request);
        match request {
            Request::Infer { prompt, binding } => {
                self.with_chain(id, |ops| ops.dispatch_infer(&prompt, binding))
            }
            Request::Call {
                target,
                args,
                var,
                from,
            } => self.dispatch_call(id, &target, args, &var, from.as_ref()),
            Request::Execute { name, args } => self.dispatch_execute(id, &name, args),
            Request::Spawn {
                target,
                args,
                item,
                index,
                var,
                from,
            } => self.dispatch_spawn(
                id,
                &target,
                args,
                TaskSeed { item, index },
                var,
                from.as_ref(),
            ),
            Request::Timer { duration } => self.dispatch_timer(id, duration),
            Request::JoinAny { tasks } => self.dispatch_join_any(id, tasks),
            Request::Ready { task } => self.dispatch_ready(id, &task),
            Request::Status { task } => self.dispatch_status(id, &task),
            Request::Pending => self.dispatch_pending(id),
            Request::Concurrency { limit } => self.dispatch_concurrency(id, limit),
            Request::Note { text } => self.dispatch_note(id, text),
            Request::Receive => self.dispatch_receive(id),
            Request::Send { task, text } => self.dispatch_send(id, &task, text),
            Request::Reply { text } => self.dispatch_reply(id, text),
            Request::Arrivals => self.dispatch_arrivals(id),
            Request::Cancel { task } => self.dispatch_cancel(id, &task),
            Request::ToolCall { alias, args, model } => {
                self.with_chain(id, |ops| ops.dispatch_tool_call(&alias, args, model))
            }
            Request::ToolCalls { calls, turn } => {
                self.with_chain(id, |ops| ops.dispatch_tool_calls(calls, turn))
            }
            Request::AdhocToolDone { outcome } => {
                self.with_chain(id, |ops| ops.dispatch_adhoc_tool_done(outcome))
            }
            Request::Store { op } => self.with_chain(id, |ops| ops.dispatch_vfs(op)),
            Request::Chat { list, binding } => self.dispatch_round(id, &list, binding),
        }
    }

    /// Dispatches a `call` request: constructs the child chain and
    /// enqueues it; the parent blocks until the child's finish delivers
    /// its final text as the answer, or no text, for which the call
    /// returns nil. Its preparation fails on the depth cap, target
    /// resolution, or child construction.
    fn dispatch_call(
        &mut self,
        id: ChainIndex,
        target: &str,
        args: Option<serde_json::Value>,
        var: &serde_json::Value,
        from: Option<&LibraryKey>,
    ) -> Result<()> {
        match self.prepare_call(id, target, args, var, from) {
            Ok(child) => {
                self.ready.push_back(child);
                Ok(())
            }
            Err(error) => self.answer_inline(id, Err(error)),
        }
    }

    /// The fallible half of call dispatch: the depth cap checked against
    /// the caller's call-depth field, the target resolved over the
    /// caller's visible set, and the child chain constructed one level
    /// deeper under the call's args and `var` snapshot.
    fn prepare_call(
        &mut self,
        id: ChainIndex,
        target: &str,
        args: Option<serde_json::Value>,
        var: &serde_json::Value,
        from: Option<&LibraryKey>,
    ) -> Result<ChainIndex> {
        let chain = &self.chains[id];
        let depth = chain.call_depth.deeper()?;
        // A call that passes arguments gives the chain its own `args`; a
        // call that passes none hands the chain the caller's context
        // whole, so the chain reads the caller's `args`.
        let child_ctx = match args {
            Some(args) => chain.ctx.with_args(args),
            None => chain.ctx.clone(),
        };
        // A call chain is a blocking child: it borrows the caller's access
        // capability (the same serial thread of execution), so the caller's
        // standing claims never false-conflict with the child's ops.
        let access = Arc::clone(chain.access()?);
        // A call chain's effective limit starts with its caller's: the
        // tasks it spawns run within the caller's share.
        let child_concurrency = chain.concurrency;
        // The child starts on a copy of the caller's current model, taken
        // now, at dispatch.
        let model = chain.current_model()?;
        // `chain`'s arena borrow ends here; the resolution names the
        // target's slice by path, so nothing borrows the arena across it.
        let resolved = self.with_chain(id, |ops| ops.resolve_chain_target(target, from))?;
        // The child's id is the caller's next child id.
        let lineage = self.chains[id].next_child_id()?;
        self.start_chain(ChainStart {
            lineage,
            counters: Counters::default(),
            ctx: child_ctx,
            model,
            access,
            start: resolved.pos,
            kind: ChainKind::Call { parent: id },
            var: var.clone(),
            call_depth: depth,
            concurrency: child_concurrency,
        })
    }

    /// Dispatches an `execute` request: constructs the chain that runs
    /// the named prompt and enqueues it; the caller blocks, as on a
    /// `call`, until that chain's finish delivers its final text as the
    /// answer, or no text, for which the `execute` returns nil. Its
    /// preparation fails on the depth cap, a name the caller's prompt does
    /// not list, arguments the named prompt's declaration refuses, or
    /// chain construction.
    fn dispatch_execute(
        &mut self,
        id: ChainIndex,
        name: &str,
        args: Option<serde_json::Value>,
    ) -> Result<()> {
        match self.prepare_execute(id, name, args) {
            Ok(child) => {
                self.ready.push_back(child);
                Ok(())
            }
            Err(error) => self.answer_inline(id, Err(error)),
        }
    }

    /// The fallible half of execute dispatch: the depth cap checked
    /// against the caller's call-depth field, the name looked up in the
    /// `prompts:` list of the prompt the caller runs, the arguments
    /// checked against the named prompt's `args` declaration, and the
    /// chain constructed one level deeper. Every check comes before the
    /// chain takes its id, which is also the id of its store.
    ///
    /// The chain is a `call` chain in all but what it runs: it borrows the
    /// caller's access capability and admission limit, and its end
    /// answers the caller. It runs the named prompt from that prompt's
    /// first top-level section, on a store of its own, with the accepted
    /// arguments, an empty `var`, and that prompt's `default` model.
    fn prepare_execute(
        &mut self,
        id: ChainIndex,
        name: &str,
        args: Option<serde_json::Value>,
    ) -> Result<ChainIndex> {
        let chain = &self.chains[id];
        let depth = chain.call_depth.deeper()?;
        let caller = chain.ctx.prompt();
        let listed = caller.frontmatter().prompts();
        if !listed.iter().any(|listed| listed == name) {
            return Err(error::make_prompt_not_listed_error(
                name,
                caller.file(),
                listed,
            ));
        }
        // A parse links every listed prompt, so a run built over the
        // prompt it returned holds this one.
        let executed = chain
            .ctx
            .prompt_named(name)
            .ok_or_else(|| error::make_prompt_not_held_error(name, caller.file()))?;
        // The executed prompt's declaration reads its arguments as it
        // reads a run's: a string is the one field `text`, and none is the
        // empty object, which takes each declared default.
        let given = match args {
            None => serde_json::json!({}),
            Some(serde_json::Value::String(text)) => {
                serde_json::json!({ crate::prompt::detail::TEXT_FIELD: text })
            }
            Some(object) => object,
        };
        let accepted = crate::prompt::detail::accept_args_value(executed, given)?;
        // The executed chain is a blocking child, on the caller's access
        // capability and within the caller's share of the admission
        // limit, as a `call` chain is.
        let access = Arc::clone(chain.access()?);
        let child_concurrency = chain.concurrency;
        // The chain's id is the caller's next child id.
        let lineage = self.chains[id].next_child_id()?;
        let child_ctx = self.chains[id]
            .ctx
            .with_prompt(name, accepted, lineage.clone())
            .ok_or_else(|| {
                error::make_internal_error(
                    "the run holds the prompt whose arguments it just checked",
                )
            })?;
        // Nothing of the caller's walk crosses: the chain starts on the
        // executed prompt's own `default` model and an empty `var`.
        let model = child_ctx.default_binding();
        self.start_chain(ChainStart {
            lineage,
            counters: Counters::default(),
            ctx: child_ctx,
            model,
            access,
            start: SectionPos::top(SectionIndex::FIRST),
            kind: ChainKind::Call { parent: id },
            var: serde_json::json!({}),
            call_depth: depth,
            concurrency: child_concurrency,
        })
    }
}

impl ChainOps<'_> {
    /// Dispatches an `infer` request: resolves the binding, issues the
    /// single tool-free gateway round as a `Chat` effect over one user
    /// message, and parks the chain in the effect table. Its preparation
    /// fails on the binding resolution.
    fn dispatch_infer(&mut self, prompt: &str, binding: Option<ModelBinding>) -> Result<()> {
        match self.prepare_infer(prompt, binding) {
            Ok(()) => Ok(()),
            Err(error) => self.answer(Err(error)),
        }
    }

    /// The fallible half of infer dispatch: the binding resolution (the
    /// handle's frozen binding, else the section's current model) and the
    /// issued effect.
    fn prepare_infer(&mut self, prompt: &str, binding: Option<ModelBinding>) -> Result<()> {
        let chain = &*self.chain;
        let binding = if let Some(binding) = binding {
            binding
        } else {
            chain
                .frame()?
                .vm()
                .current_model()?
                .ok_or_else(|| error::make_model_required_error(chain.section_name()))?
        };
        // A nested infer round consumes only the accumulated completion;
        // its `Infer` origin tells the caller its live deltas have no
        // consumer.
        let round = self.number_round(ReplyOrigin::Infer);
        let effect = Effect::Chat {
            binding,
            messages: vec![Message::user(prompt)],
            after: None,
            tools: Vec::new(),
            round,
        };
        self.issue(effect, Continuation::Infer(round.id));
        Ok(())
    }

    /// Dispatches a `store` request: derives a view of the chain's store
    /// under the chain's access capability and issues the operation
    /// through it as a `Vfs` effect for the caller to perform, parking the
    /// chain in the effect table exactly as a leaf I/O round does. The
    /// effect names the store by the id of the chain it was made for. The
    /// operation's event is pushed when the answer is applied, before the
    /// chain resumes. This is the one place the Engine makes a store view.
    ///
    /// # Errors
    /// Returns an `Internal` failure when the live chain's store or its
    /// access capability is gone, which only the chain-end paths take, or
    /// a store failure when the chain's scope has ended, placed on the
    /// store call's line.
    fn dispatch_vfs(&mut self, op: VfsOp) -> Result<()> {
        let chain = &*self.chain;
        let held = chain.ctx.store()?;
        let view = Arc::new(held.store.view(chain.access()?).map_err(|cause| {
            let line = chain.frame().ok().and_then(|frame| frame.vm().call_line());
            place(
                error::make_store_error(&cause),
                chain.ctx.prompt().file(),
                line,
            )
        })?);
        let effect = Effect::Vfs {
            asker: VfsAsker::Lua {
                store: held.made_for.clone(),
            },
            access: view,
            op: op.clone(),
        };
        self.issue(effect, Continuation::Vfs(op));
        Ok(())
    }
}
