//! One chain's step to its next suspension point: resume a suspended
//! coroutine with its delivered answer, or advance the walk, then apply the
//! block coroutine's outcome: park a yielded chain on its request's
//! dispatch, move past a block that ran off its end, end the chain at a
//! block that executed a `return`, and report the chunk's closing
//! observation boundary.

use std::num::NonZeroU32;

use runvil_types::emitter::Emitter;
use runvil_types::error::place;
use runvil_types::event_lifecycle;

use crate::lua::{Answer, CoroStep, LuaBlockResult, Request, YieldParse};
use crate::prompt::Block;
use crate::{Result, error};

use super::chain::ChainStep;
use super::chain_record::{Entered, Running};
use super::{ChainIndex, ChainOps, Scheduler};

/// What a Lua block's coroutine step leaves the chain to do, settled under
/// the borrow of its entered section so the action can touch the
/// scheduler's other fields.
#[derive(Debug)]
enum Settled {
    /// The block suspended on a request for its arm to dispatch.
    Dispatch(Request),
    /// The block suspended on a call answered on the spot.
    Answer(Result<Answer>),
    /// The block jumped to the section path, from the `jump` call on
    /// the prompt line, when known.
    Jump(String, Option<NonZeroU32>),
    /// The block executed a `return`, which ends the chain: the returned
    /// value as text, or `None` when the `return` gave nil or no value.
    Returned(Option<String>),
    /// The block ran off its end; the section's next block is next.
    Advanced,
}

/// Settles one Lua block coroutine step in `entered`, the block's
/// section, and reports the chunk's closing observation boundary under
/// `name`: a yield parses into its request and leaves the block
/// suspended, and a completion moves past the block. A malformed yield
/// settled here is placed in the prompt `file`.
fn settle_block(
    entered: &mut Entered,
    result: Result<CoroStep>,
    file: &str,
    emitter: &Emitter,
    name: &str,
) -> Result<Settled> {
    let step = match result {
        Ok(step) => step,
        Err(error) => {
            emitter.report(name, event_lifecycle::LUA_CHUNK_FAILED);
            return Err(error);
        }
    };
    match step {
        CoroStep::Yielded(coroutine, values) => {
            let settled = match entered.frame.vm().request_from_yield(&values) {
                YieldParse::Request(request) => Settled::Dispatch(request),
                YieldParse::Refused(refusal) => Settled::Answer(Err(refusal)),
                YieldParse::Malformed(malformed) => {
                    emitter.report(name, event_lifecycle::LUA_CHUNK_FAILED);
                    return Err(place(malformed, file, entered.frame.vm().call_line()));
                }
            };
            entered.running = Some(Running {
                coroutine,
                answer: None,
            });
            Ok(settled)
        }
        CoroStep::Done(done) => {
            // A jump is a control transfer, not a failure: the chunk
            // boundary reports success and the walk moves to the
            // resolved target.
            emitter.report(name, event_lifecycle::LUA_CHUNK_SUCCEEDED);
            match done {
                LuaBlockResult::Jump(path, line) => Ok(Settled::Jump(path, line)),
                LuaBlockResult::Returned(value) => Ok(Settled::Returned(value)),
                LuaBlockResult::RanOff => {
                    entered.block = entered.block.next();
                    Ok(Settled::Advanced)
                }
            }
        }
    }
}

impl Scheduler {
    /// Runs one ready chain to its next suspension point: resume a
    /// suspended coroutine with its delivered answer, or advance the walk.
    /// The step never awaits: every dispatch either issues its leaf effect
    /// or answers on the spot.
    ///
    /// The driver owns the chunk observation boundaries: STARTED at a
    /// block's start, SUCCEEDED or FAILED when its coroutine finally
    /// returns or fails - a suspension is neither.
    pub(super) fn step_chain(&mut self, id: ChainIndex) -> Result<ChainStep> {
        let prompt = self.prompt();
        let chain = &mut self.chains[id];
        let blocks = chain.blocks(&prompt);
        let name = chain.section_name().to_owned();
        let emitter = chain.ctx.emitter();
        let Some(entered) = chain.entered.as_mut() else {
            // Between sections: enter the next section, or end the chain
            // when the slice is exhausted.
            return self.advance_entry(id);
        };
        let result = if let Some(running) = entered.running.take() {
            let Running {
                coroutine,
                answer: Some(answer),
            } = running
            else {
                return Err(error::make_internal_error(
                    "a ready chain's suspended coroutine waits on its answer",
                ));
            };
            // The answer ends whatever the chain was parked on.
            chain.parked = None;
            let Some(Block::Lua(program)) = entered.block_in(blocks) else {
                return Err(error::make_internal_error(
                    "a suspended coroutine's block is Lua",
                ));
            };
            entered
                .frame
                .vm()
                .resume_block_coro_answer(program, &coroutine, answer)
        } else {
            match entered.block_in(blocks) {
                Some(Block::Lua(program)) => {
                    // The pending Markdown buffer installs as the block's
                    // fresh read-only lazy `prose` template first.
                    emitter.report(&name, event_lifecycle::LUA_CHUNK_STARTED);
                    let pending = entered.pending_prose.take();
                    let (template, line) = pending
                        .as_ref()
                        .map_or(("", None), |(text, line)| (text.as_str(), Some(*line)));
                    if let Err(error) = entered.frame.install_lazy_prose(&chain.ctx, template, line)
                    {
                        emitter.report(&name, event_lifecycle::LUA_CHUNK_FAILED);
                        return Err(place(error, prompt.file(), Some(program.source_line())));
                    }
                    entered.frame.vm().start_block_coro(program)
                }
                Some(Block::Prose { text, line, .. }) => {
                    // The parser emits one prose block per inter-fence gap,
                    // already accumulated and reset at thematic breaks, so the
                    // block IS the pending buffer the next Lua fence consumes.
                    // Prose never infers: the buffer waits for the following
                    // block's lazy `prose` install, unevaluated until read.
                    entered.pending_prose = Some((text.clone(), *line));
                    entered.block = entered.block.next();
                    self.ready.push_back(id);
                    return Ok(ChainStep::Continue);
                }
                // `Block` is `#[non_exhaustive]` across the crate seam; an
                // unlisted variant has no advance rule.
                Some(_) => {
                    return Err(error::make_internal_error(
                        "an unrecognized block kind cannot advance",
                    ));
                }
                // The section's blocks are exhausted: fall through.
                None => {
                    self.with_chain(id, ChainOps::end_section)?;
                    self.ready.push_back(id);
                    return Ok(ChainStep::Continue);
                }
            }
        };
        let settled = settle_block(entered, result, prompt.file(), emitter, &name)?;
        self.apply_settled(id, settled)
    }

    /// Acts on a settled Lua block for chain `id`: dispatches a suspended
    /// block's request or answers it on the spot, moves the walk for a
    /// jump, ends the chain for a `return`, and requeues the
    /// chain for its section's next block when the block ran off its end.
    fn apply_settled(&mut self, id: ChainIndex, settled: Settled) -> Result<ChainStep> {
        match settled {
            Settled::Dispatch(request) => self.dispatch(id, request)?,
            Settled::Answer(answer) => self.answer_inline(id, answer)?,
            Settled::Jump(path, line) => {
                self.with_chain(id, |ops| ops.apply_jump(&path, line))?;
                self.ready.push_back(id);
            }
            // A `return` ends the chain it fired in, with the value it
            // gave or with none.
            Settled::Returned(value) => return Ok(self.finish(id, Ok(value))),
            Settled::Advanced => self.ready.push_back(id),
        }
        Ok(ChainStep::Continue)
    }
}
