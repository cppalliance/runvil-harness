//! The Gateway's model list as the Engine's catalog: the client fetches the
//! list, and this module applies the Harness's rules to it.

use std::num::NonZeroU32;

use gateway_client::{Model, Thinking, escape_controls};
use runvil::Error;
use runvil::model::{ModelCatalog, ModelId, ModelInfo, ThinkingMode};

use crate::convert::IntoEngine;
use crate::fail::{make_malformed_error, with_detail};

/// The largest gateway text kept in a catalog diagnostic, in bytes.
const MAX_CATALOG_ERROR_BODY: usize = 2000;

/// Builds the Engine's catalog from the list the client fetched.
///
/// Three rules decide an entry. An entry with no context window is skipped,
/// because it is not an inference model. A zero window is refused. An entry
/// that has a window and no thinking mode is refused.
///
/// # Errors
/// Returns a `MalformedResponse` error for an entry the rules refuse, for an
/// id the Engine refuses, and for a list the Engine's catalog refuses.
pub(crate) fn catalog_from(models: Vec<Model>) -> Result<ModelCatalog, Error> {
    let mut infos = Vec::with_capacity(models.len());
    for entry in models {
        // An entry with no context window is not an inference model (the
        // gateway lists its transcription models here too): it gets no
        // model info and must not fail the catalog.
        let Some(context) = entry.context else {
            continue;
        };
        let id = ModelId::gateway(entry.id).map_err(|error| {
            make_malformed_error(format!("model catalog entry has an invalid id: {error}"))
        })?;
        let context = NonZeroU32::new(context).ok_or_else(|| {
            with_detail(
                make_malformed_error("a model declares a zero-token context window"),
                &escape_controls(id.name(), MAX_CATALOG_ERROR_BODY),
            )
        })?;
        let thinking = entry.thinking.map(IntoEngine::into_engine).ok_or_else(|| {
            with_detail(
                make_malformed_error("a model declares a context window but no thinking mode"),
                &escape_controls(id.name(), MAX_CATALOG_ERROR_BODY),
            )
        })?;
        let info = ModelInfo::new(id, entry.description, context, thinking);
        infos.push(match entry.provider {
            Some(provider) => info.with_provider(provider),
            None => info,
        });
    }
    ModelCatalog::new(infos).map_err(|error| {
        with_detail(
            make_malformed_error("gateway returned an inconsistent model catalog"),
            &escape_controls(&error.to_string(), MAX_CATALOG_ERROR_BODY),
        )
    })
}

impl IntoEngine for Thinking {
    type Engine = ThinkingMode;

    fn into_engine(self) -> ThinkingMode {
        match self {
            Thinking::Never => ThinkingMode::Never,
            Thinking::Always => ThinkingMode::Always,
            Thinking::Switchable => ThinkingMode::Switchable,
        }
    }
}

#[cfg(test)]
#[path = "catalog-tests.rs"]
mod tests;
