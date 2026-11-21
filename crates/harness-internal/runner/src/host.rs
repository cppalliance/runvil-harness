//! What every run of one Host shares: [`Host`], its installed Packs and
//! how it finds prompts, and [`Host::parse`], which finds a
//! prompt by its name, parses it and every prompt it links, and checks
//! the `uses:` list of each against the Host's Packs.

use std::fmt::{self, Write as _};
use std::sync::Arc;

use runvil_engine::{Error, ErrorCode, Prompt, PromptFinder};

use crate::packs::{Packs, Refused};

/// What every run of one Host shares: its installed Packs and how it
/// finds prompts. A Host builds one and shares it behind an `Arc`.
pub struct Host {
    /// Every Pack the Host installed in its registry, whose factories
    /// make each run's instances.
    pub packs: Packs,
    /// Finds each prompt's source by its name: the prompt a parse is
    /// asked for, and each prompt the `prompts:` lists reach from it,
    /// which a run of it may execute.
    pub prompts: Arc<dyn PromptFinder>,
}

impl Host {
    /// Parses the prompt `name` with `Prompt::parse` over `prompts`, which
    /// finds and parses every prompt its `prompts:` lists reach too, then
    /// checks the `uses:` of each of them against `packs`. Returns the
    /// parse failure, a finder's included, as given, or
    /// `RequirementsUnmet` naming each Pack that `Packs::refusals`
    /// reports, with its reason: one not installed, or inserted
    /// unavailable. The line of a Pack that only a linked prompt declares
    /// starts with that prompt's name, as in
    /// `- review.md: web is unavailable: runvil/web is not installed`.
    /// Parsing makes no Pack, so a factory that would refuse a run is
    /// refused at that run's first step instead.
    ///
    /// The parsed prompt comes back behind an `Arc`, which every run of
    /// it shares: a Host parses a prompt once and hands a clone of the
    /// handle to the `Harness` of each run.
    ///
    /// The parse's outcome goes to `tracing` at debug level, as one line;
    /// no run exists yet to record it.
    ///
    /// # Errors
    /// Returns the finder's or the parser's failure unchanged, and
    /// `RequirementsUnmet` whose message and prose list each declared
    /// Pack this Host cannot serve with the reason.
    pub fn parse(&self, name: &str) -> Result<Arc<Prompt>, Error> {
        let parsed = Prompt::parse(&*self.prompts, name);
        match &parsed {
            Ok(_) => tracing::debug!(prompt = name, "prompt parsed"),
            Err(error) => tracing::debug!(prompt = name, %error, "prompt failed to parse"),
        }
        let prompt = parsed?;
        let refusals = self.packs.refusals(&prompt);
        if refusals.is_empty() {
            return Ok(Arc::new(prompt));
        }
        let mut notice = String::from("the environment cannot satisfy this prompt:");
        for Refused { by, id, reason } in &refusals {
            // Writing to a String is infallible.
            let _ = match by {
                Some(linked) => write!(notice, "\n- {linked}: {id} is unavailable: {reason}"),
                None => write!(notice, "\n- {id} is unavailable: {reason}"),
            };
        }
        Err(Error::new(
            ErrorCode::RequirementsUnmet,
            notice.clone(),
            notice,
        ))
    }
}

impl fmt::Debug for Host {
    /// Shows the Packs; omits the finder.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Host")
            .field("packs", &self.packs)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
#[path = "host-tests.rs"]
mod tests;
