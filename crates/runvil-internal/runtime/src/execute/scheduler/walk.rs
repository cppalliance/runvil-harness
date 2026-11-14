//! The section walk: sections run in fall-through order, `var` rolls
//! forward across sections and jumps, every section entry takes the
//! chain's next entry id (the chain's hierarchical id extended by its
//! local entry counter), and a jump transfers control - a sibling move
//! within the chain's slice, or a descent into the jumper's child slice
//! with the parent position suspended on the chain's own walk stack
//! until the child level exhausts. The root walk is the run's first chain:
//! it starts at the prompt's first top-level section, the preamble when
//! the prompt has one, and every other chain starts where a `call` or a
//! spawn names.

use std::num::NonZeroU32;
use std::sync::Arc;

use runvil_types::error::place;
use runvil_types::ids::ChainId;

use crate::execute::section_context::SectionContext;
use crate::execute::walk_target::{JumpTarget, resolve_jump_target};
use crate::lua::LibraryKey;
use crate::prompt::Block;
use crate::section_pos::{SectionIndex, SectionPos};
use crate::{Result, error};

use super::chain::{ChainStart, ChainStep};
use super::chain_record::{CallDepth, ChainKind, Entered};
use super::{ChainIndex, ChainOps, Counters, Scheduler, prompt_origin};

impl Scheduler {
    /// Starts the root walk chain over the top-level sections, seeded with
    /// an empty `var`, on the `default` model, under the run's concurrency
    /// ceiling, and enqueues it. The walk is the root chain `0`, at the
    /// root call depth, and the first section it enters takes entry `0.0`.
    /// A prompt with no section starts the same chain, which ends on its
    /// first step with no text.
    ///
    /// # Errors
    /// Returns an `Internal` failure when the run's chain count exceeds `u32`,
    /// or a store failure when the backend refuses acquisition.
    pub(super) fn start_root_walk(&mut self) -> Result<()> {
        // The root walk is the run's one serial thread of execution at the
        // top, so it acquires the run's root capability. It serves every
        // section in turn, so its label is the prompt's own; the line is
        // where the walk starts.
        let prompt = self.ctx.prompt();
        let blocks: &[Block] =
            crate::prompt::detail::entry(prompt).map_or(&[], |section| section.blocks());
        let origin = prompt_origin(prompt, prompt.frontmatter().name(), blocks);
        let (access, scope) = self
            .ctx
            .vfs()
            .acquire_scope(origin)
            .map_err(|cause| error::make_store_error(&cause))?;
        self.scope = Some(scope);
        let id = self.start_chain(ChainStart {
            lineage: ChainId::root(),
            counters: Counters::default(),
            ctx: self.ctx.clone(),
            model: self.ctx.default_binding(),
            access: Arc::new(access),
            start: SectionPos::top(SectionIndex::FIRST),
            kind: ChainKind::Root,
            var: serde_json::json!({}),
            call_depth: CallDepth::ROOT,
            concurrency: self.ctx.limits().max_concurrency,
        })?;
        self.ready.push_back(id);
        Ok(())
    }

    /// Enters the chain's next section and requeues it, or finishes the
    /// chain when its slice is exhausted.
    ///
    /// # Errors
    /// Returns the failure of frame construction, as documented on
    /// [`SectionContext::new`].
    pub(super) fn advance_entry(&mut self, id: ChainIndex) -> Result<ChainStep> {
        if self.with_chain(id, ChainOps::enter_section)?
            || self.with_chain(id, ChainOps::pop_position)
        {
            // A section was entered, or a jump-started child level
            // exhausted and the parent walk resumes after the jumper.
            self.ready.push_back(id);
            return Ok(ChainStep::Continue);
        }
        // The walk ran off the slice's last section: the chain ends.
        Ok(self.finish(id, Ok(None)))
    }
}

impl ChainOps<'_> {
    /// Enters the chain's next section and reports whether one was entered:
    /// constructs the frame with the chain's next entry id and its task id,
    /// seeded from the chain's `var` slot (and, on a spawned
    /// chain's first entry, its `item` and `sys.index` seeds). The pending
    /// Markdown buffer
    /// resets: a previous section's unconsumed prose never crosses the
    /// boundary. `Ok(false)` means the
    /// slice is exhausted and the chain ends.
    ///
    /// # Errors
    /// Returns the failure of frame construction, as documented on
    /// [`SectionContext::new`].
    fn enter_section(&mut self) -> Result<bool> {
        let prompt = self.prompt();
        let chain = &mut *self.chain;
        // `section` borrows the shared prompt tree, not the arena, so the
        // frame construction can borrow the chain's own context and slots.
        let Some(section) = chain.walk.current().section(&prompt) else {
            return Ok(false);
        };
        let section_id = chain.next_entry_id()?;
        let task = chain.task.clone();
        // A spawned chain's first entry consumes its `item` and `sys.index`
        // seeds; every later entry, and every other chain's, has none.
        let seed = std::mem::take(&mut chain.seed);
        let frame = SectionContext::new(
            &chain.ctx,
            &chain.selection,
            chain.walk.current(),
            &section_id,
            &task,
            &chain.var,
            seed,
        )?;
        chain.entered = Some(Entered::new(frame));
        section.name().clone_into(&mut chain.entered_name);
        Ok(true)
    }

    /// Resumes a jump-suspended parent position when a child level
    /// exhausts, returning `false` when the chain holds no suspended
    /// position - meaning its own root slice exhausted and the chain ends.
    /// The child walk shared the `var` slot, so it already holds the
    /// child level's last value.
    fn pop_position(&mut self) -> bool {
        self.chain.walk.pop()
    }

    /// Falls the chain through at its section's end: the section's final
    /// `var` replaces the chain's `var` table, read back while the VM is
    /// live; the frame's drop is
    /// the teardown boundary, firing `SECTION_FINISHED` for this completed
    /// section; then the walk advances to the next section.
    ///
    /// # Errors
    /// Returns a `LuaRuntime` failure when the final `var` read-back fails (the
    /// frame drops unarmed), placed on the section's last Lua block, or an
    /// `Internal` failure when the chain holds no frame.
    pub(super) fn end_section(&mut self) -> Result<()> {
        let prompt = self.prompt();
        let chain = &mut *self.chain;
        let blocks = chain.blocks(&prompt);
        let Some(Entered {
            mut frame, block, ..
        }) = chain.entered.take()
        else {
            return Err(error::make_internal_error(
                "a section end implies a live frame",
            ));
        };
        chain.var = frame
            .read_var()
            .map_err(|error| place(error, prompt.file(), block.lua_line(blocks)))?;
        frame.mark_completed();
        drop(frame);
        chain.walk.advance();
        Ok(())
    }

    /// Applies a jump's control transfer: closes the jumper's frame as
    /// completed (the final `var`
    /// rolled forward; the armed drop firing `SECTION_FINISHED`, a jump
    /// being a completion), resolves the path from the jumper, and moves
    /// the walk. A sibling target replaces the walk's current position; a
    /// child target suspends the jumper on the chain's walk stack and
    /// descends into the jumper's child slice from the target.
    ///
    /// # Errors
    /// Returns a `LuaRuntime` failure when the `var` read-back fails (the
    /// frame drops unarmed), placed on the jumping block's line, or when
    /// the path is refused or names no section, placed on `line`, the
    /// `jump` call's - the jumper's frame has already closed as
    /// completed.
    pub(super) fn apply_jump(&mut self, path: &str, line: Option<NonZeroU32>) -> Result<()> {
        let prompt = self.prompt();
        {
            let chain = &mut *self.chain;
            let blocks = chain.blocks(&prompt);
            let Some(Entered {
                mut frame, block, ..
            }) = chain.entered.take()
            else {
                return Err(error::make_internal_error("a jump implies a live frame"));
            };
            chain.var = frame
                .read_var()
                .map_err(|error| place(error, prompt.file(), block.lua_line(blocks)))?;
            frame.mark_completed();
            drop(frame);
        }
        // A jump moves the running walk, so its path starts at the running
        // section, in a library's code too.
        let target = self
            .resolve_chain_target(path, None)
            .map_err(|error| place(error, prompt.file(), line))?;
        self.chain.walk.jump(target);
        Ok(())
    }

    /// Resolves the relative `path` into the position the walk or a
    /// contained chain continues from: a direct child's place in the
    /// starting section's child slice, or a sibling's in its own. The
    /// path starts at the library `from` names, when a library's code
    /// made the request, and otherwise at the chain's current section.
    ///
    /// # Errors
    /// Returns a `LuaRuntime` failure when the path is malformed, is not
    /// one of the two relative forms, or names no section, and an
    /// `Internal` failure for a key that names no section position (see
    /// [`resolve_jump_target`]).
    pub(super) fn resolve_chain_target(
        &self,
        path: &str,
        from: Option<&LibraryKey>,
    ) -> Result<JumpTarget> {
        let prompt = self.prompt();
        resolve_jump_target(path, &prompt, self.chain.walk.current(), from)
    }
}
