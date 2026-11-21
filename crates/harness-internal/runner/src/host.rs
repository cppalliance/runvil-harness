//! What every run of one Host shares: [`Host`], its installed Packs, how
//! it finds prompts, and the report of each of its Plugins;
//! [`HostBuilder`], which names the Host's Plugins and builds the Host;
//! and [`Host::parse`], which finds a prompt by its name, parses it and
//! every prompt it links, and checks the `uses:` list of each against
//! the Host's Packs.

use std::fmt::{self, Write as _};
use std::sync::Arc;

use harness_plugin_loader::{PluginReport, make_report};
use runvil_engine::{Error, ErrorCode, ErrorFact, Prompt, PromptFinder};
use runvil_plugin::{PackInfo, PackRegistry};
use serde_json::Value;

use crate::packs::{Packs, Refused};

/// What every run of one Host shares: its installed Packs, how it finds
/// prompts, and the report of each of its Plugins.
///
/// [`Host::builder`] starts one, and [`HostBuilder::build`] returns it
/// inside an `Arc`, which is how every run shares it. Its Packs are fixed
/// from then on.
pub struct Host {
    /// Every Pack the Host's Plugins inserted and the two built-in
    /// Packs, whose factories make each run's instances. The run's
    /// preparation reads them.
    pub(crate) packs: Packs,
    /// Finds each prompt's source by its name: the prompt a parse is
    /// asked for, and each prompt the `prompts:` lists reach from it,
    /// which a run of it may execute.
    prompts: Arc<dyn PromptFinder>,
    /// The report of each Plugin the builder named, in the order written.
    reports: Vec<PluginReport>,
}

impl Host {
    /// Starts the description of a Host that finds its prompts through
    /// `prompts`. Each `with_` method of the builder names one Plugin,
    /// and [`HostBuilder::build`] installs them and returns the Host.
    #[must_use]
    pub fn builder(prompts: Arc<dyn PromptFinder>) -> HostBuilder {
        HostBuilder {
            prompts,
            plugins: Vec::new(),
        }
    }

    /// Parses the prompt `name` with `Prompt::parse` over the Host's
    /// finder, which finds and parses every prompt its `prompts:` lists
    /// reach too, then checks the `uses:` of each of them against the
    /// Host's Packs. Returns the parse failure, a finder's included, as
    /// given, or `RequirementsUnmet` naming each declared Pack that no
    /// run of this Host can hold, with its reason: one not installed, or
    /// inserted unavailable. The line of a Pack that only a linked prompt
    /// declares starts with that prompt's name, as in
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

    /// Every Pack of the Host, in the order of their names: each Pack
    /// its Plugins inserted, one inserted as unavailable included, and
    /// the two built-in Packs, `runvil/subagents` and `runvil/user`.
    pub fn packs(&self) -> impl Iterator<Item = &PackInfo> {
        self.packs.list()
    }

    /// The report of each Plugin the builder named, in the order
    /// written: what that Plugin's step loaded and what failed in it.
    ///
    /// The report of a Rust Plugin holds no Plugin. It is empty when the
    /// Plugin's function returned `Ok`, and otherwise the function's
    /// error is its one failure.
    #[must_use]
    pub fn plugin_reports(&self) -> &[PluginReport] {
        &self.reports
    }
}

impl fmt::Debug for Host {
    /// Shows the Packs and the report of each Plugin; omits the finder.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Host")
            .field("packs", &self.packs)
            .field("reports", &self.reports)
            .finish_non_exhaustive()
    }
}

/// A Rust Plugin as the builder keeps it until the build: the function
/// that inserts the Plugin's Packs into the registry it is lent.
type Install = Box<dyn FnOnce(&mut PackRegistry) -> Result<(), Error> + Send>;

/// The description of a Host: its prompt finder and its Plugins, in the
/// order written.
///
/// [`Host::builder`] starts one, each `with_` method names one Plugin,
/// and [`build`](HostBuilder::build) does the work. The builder is
/// `Send`, so an application may build its Host on another thread.
pub struct HostBuilder {
    prompts: Arc<dyn PromptFinder>,
    /// The function of each Rust Plugin, in the order written.
    plugins: Vec<Install>,
}

impl HostBuilder {
    /// Names a Rust Plugin, which is a function that inserts its Packs
    /// into the registry it is lent. The builder keeps `install`, and
    /// [`build`](HostBuilder::build) calls it with the Host's registry.
    ///
    /// The `install` function of a Plugin crate takes what its Packs
    /// need as further arguments, so a Host passes a closure that
    /// supplies them, as in
    /// `.with_rust_plugin(move |packs| plugin_mcp::install(packs, servers, runtime))`.
    #[must_use]
    pub fn with_rust_plugin(
        mut self,
        install: impl FnOnce(&mut PackRegistry) -> Result<(), Error> + Send + 'static,
    ) -> HostBuilder {
        self.plugins.push(Box::new(install));
        self
    }

    /// Builds the Host. It makes an empty registry, calls the function of
    /// each Plugin with it in the order written, and keeps one
    /// [`PluginReport`] for each. Then it adds the two built-in Packs,
    /// `runvil/subagents` and `runvil/user`, and returns the Host inside
    /// an `Arc`.
    ///
    /// A function that returns an error leaves the Host built. The error
    /// is the one failure of that Plugin's report, and each Pack the
    /// function inserted before it failed stays. The first Plugin that
    /// inserts a Pack id keeps it, so the registry refuses a later Pack
    /// of the same Pack id.
    ///
    /// Each report goes to `tracing` as it is made: each Plugin it holds
    /// at the info level, and each failure at the warn level.
    #[must_use]
    pub fn build(self) -> Arc<Host> {
        let mut registry = PackRegistry::new();
        let reports = self
            .plugins
            .into_iter()
            .map(|install| {
                let report = make_report(install(&mut registry));
                log_report(&report);
                report
            })
            .collect();
        Arc::new(Host {
            packs: Packs::new(registry),
            prompts: self.prompts,
            reports,
        })
    }
}

impl fmt::Debug for HostBuilder {
    /// Shows how many Plugins the builder names; omits the finder and the
    /// function of each Plugin.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostBuilder")
            .field("plugins", &self.plugins.len())
            .finish_non_exhaustive()
    }
}

/// Logs the report of one Plugin: each Plugin it holds, at the info
/// level, and each failure, at the warn level, with the path the failure
/// names when it names one.
fn log_report(report: &PluginReport) {
    for plugin in report.plugins() {
        tracing::info!(
            name = plugin.name(),
            version = plugin.version(),
            source = ?plugin.source(),
            "Plugin loaded"
        );
    }
    for error in report.failures() {
        let path = error.facts.get(&ErrorFact::Path).and_then(Value::as_str);
        tracing::warn!(path, %error, "Plugin failed");
    }
}

#[cfg(test)]
#[path = "host-tests.rs"]
mod tests;
