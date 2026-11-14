//! Engine injection: the one step that turns an uninjected section VM into
//! an injected one, installing `args`, `sys`, `var`, the `tools`,
//! `models`, and `compactors` tables, and the VM's `messages` list.

use std::sync::{Arc, Mutex};

use runvil_model_client::model::ModelBinding;
use runvil_types::emitter::Emitter;
use serde_json::Value as Json;

use super::{Injected, SectionVm, Uninjected};
use crate::compactors::install_compactors;
use crate::error::{self, Error, Result};
use crate::messages::install_messages;
use crate::models::install_models;
use crate::sys::seal_sys;
use crate::tools::install_tools;
use crate::var::guarded_var;

/// A refused Engine injection: the error, beside the uninjected VM it was
/// refused on, which the caller tears down at its own boundary.
#[derive(Debug)]
pub struct InjectFailure {
    vm: Box<SectionVm<Uninjected>>,
    error: Box<Error>,
}

impl InjectFailure {
    /// Why the injection failed.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn error(&self) -> &Error {
        &self.error
    }

    /// Tears the refused VM down under `section` and returns why the
    /// injection failed.
    #[must_use]
    pub fn teardown(self, emitter: &Emitter, section: &str) -> Error {
        self.vm.teardown(emitter, section);
        *self.error
    }
}

impl SectionVm<Uninjected> {
    /// Installs Engine values while seeding `var` from an earlier VM, and
    /// gives back the injected VM.
    ///
    /// Injection consumes the uninjected VM, so it runs once.
    ///
    /// The `var` global is a guarded proxy (see `guarded_var`): writes are
    /// validated for JSON-representability at the assigning line, and the
    /// hidden data table behind it is what [`var`](SectionVm::var) reads
    /// back.
    ///
    /// `args` is the chain's arguments, one JSON object, which the `args`
    /// global holds as a read-only table.
    ///
    /// `selection` holds the current model: `models.use` writes it, and
    /// [`current_model`](SectionVm::current_model) reads it.
    ///
    /// # Errors
    /// Returns the [`InjectFailure`] holding the bridging failure if Engine
    /// values cannot be bridged, or holding an `Internal` failure if `args`
    /// or the `var` seed is not a JSON object.
    pub fn inject_values(
        self,
        args: &Json,
        sys: &Json,
        initial_var: Option<&Json>,
        selection: &Arc<Mutex<Option<ModelBinding>>>,
    ) -> std::result::Result<SectionVm, InjectFailure> {
        if let Err(error) = self.install_values(args, sys, initial_var, selection) {
            return Err(InjectFailure {
                vm: Box::new(self),
                error: Box::new(error),
            });
        }
        Ok(self.injected(Injected {
            sys_live: Arc::new(Mutex::new(sys.clone())),
            selection: Arc::clone(selection),
        }))
    }

    fn install_values(
        &self,
        args: &Json,
        sys: &Json,
        initial_var: Option<&Json>,
        selection: &Arc<Mutex<Option<ModelBinding>>>,
    ) -> Result<()> {
        let globals = self.lua.globals();
        crate::args::install(&self.lua, args)?;
        let sys_table = seal_sys(&self.lua, sys)?;
        globals
            .raw_set("sys", sys_table)
            .map_err(error::make_lua_error)?;
        let var = guarded_var(&self.lua, initial_var)?;
        globals.raw_set("var", var).map_err(error::make_lua_error)?;
        install_tools(
            &self.lua,
            &globals,
            &self.tool_set,
            &self.offers,
            &self.adhoc_tools,
            &self.messages,
        )?;
        install_models(
            &self.lua,
            &globals,
            &self.bound_models,
            selection,
            self.raw_model_ids,
        )?;
        install_messages(&self.lua, &self.messages)?;
        install_compactors(&self.lua, &globals, &self.messages)
    }

    /// The same VM at the injected stage, holding `state`.
    fn injected(self, state: Injected) -> SectionVm {
        let SectionVm {
            lua,
            tool_set,
            bound_models,
            offers,
            jump_slot,
            adhoc_handler_depth,
            loop_running,
            library_load_depth,
            library_programs,
            log_budget,
            log_byte_budget,
            adhoc_tools,
            cancel_hook,
            raw_model_ids,
            messages,
            state: Uninjected,
        } = self;
        SectionVm {
            lua,
            tool_set,
            bound_models,
            offers,
            jump_slot,
            adhoc_handler_depth,
            loop_running,
            library_load_depth,
            library_programs,
            log_budget,
            log_byte_budget,
            adhoc_tools,
            cancel_hook,
            raw_model_ids,
            messages,
            state,
        }
    }
}
