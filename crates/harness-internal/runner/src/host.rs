//! What every run of one Host shares: [`Host`], its installed Packs, how
//! it finds prompts, and the report of each of its Plugins;
//! [`HostBuilder`], which names the Host's Plugins, of the C contract and
//! in Rust, loads them in the order written, and builds the Host;
//! and [`Host::parse`], which finds a prompt by its name, parses it and
//! every prompt it links, and checks the `uses:` list of each against
//! the Host's Packs.

use std::fmt::{self, Write as _};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use harness_plugin_loader::{
    HostResources, PluginReport, insert_plugin, insert_plugin_dir, insert_plugin_file, make_report,
};
use runvil_engine::{Error, ErrorCode, ErrorFact, Prompt, PromptFinder};
use runvil_plugin::{PackInfo, PackRegistry};
use runvil_plugin_sys::PluginEntry;
use serde_json::Value;

use crate::packs::{Packs, Refused};

/// What every run of one Host shares: its installed Packs, how it finds
/// prompts, and the report of each of its Plugins.
///
/// [`Host::builder`] starts one, and [`HostBuilder::build`] returns it
/// inside an `Arc`, which is how every run shares it. Its Packs are fixed
/// from then on. It keeps each Plugin of the C contract that its build
/// loaded, so such a Plugin stays loaded for as long as the Host lives.
pub struct Host {
    /// Every Pack the Host's Plugins inserted and the two built-in
    /// Packs, whose factories make each run's instances. The run's
    /// preparation reads them.
    pub(crate) packs: Packs,
    /// Finds each prompt's source by its name: the prompt a parse is
    /// asked for, and each prompt the `prompts:` lists reach from it,
    /// which a run of it may execute.
    prompts: Arc<dyn PromptFinder>,
    /// One report for each step of the builder that named a Plugin, in
    /// the order written. They hold each Plugin of the C contract that
    /// the build loaded.
    reports: Vec<PluginReport>,
}

impl Host {
    /// Starts the description of a Host that finds its prompts through
    /// `prompts`. Each `with_` method of the builder but
    /// [`HostBuilder::with_resources`] names one Plugin, and
    /// [`HostBuilder::build`] installs them and returns the Host.
    #[must_use]
    pub fn builder(prompts: Arc<dyn PromptFinder>) -> HostBuilder {
        HostBuilder {
            prompts,
            resources: HostResources::new(),
            steps: Vec::new(),
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

    /// One report for each step of the builder that named a Plugin, in
    /// the order written: each Plugin of the C contract that the step
    /// loaded, and each failure of the step.
    ///
    /// Every kind of Plugin is reported in one way. A step that names a
    /// directory, a file, or an entry point reports each Plugin that
    /// loaded, and as a failure each library that did not load, an entry
    /// point that refused, and each Pack that the registry refused,
    /// whose Plugin stays in the report. A directory that cannot be read
    /// is the one failure of its step: a `PluginLoadFailed` error with
    /// the `Reason` fact `directory` and the directory as its `Path`
    /// fact. The report of a Rust Plugin holds no Plugin. It is empty
    /// when the Plugin's function returned `Ok`, and otherwise the
    /// function's error is its one failure.
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

/// One step of a build that names a Plugin.
enum Step {
    /// A Rust Plugin, by its function.
    Rust(Install),
    /// Every library Plugin in this directory.
    Dir(PathBuf),
    /// The library Plugin in this file.
    File(PathBuf),
    /// A Plugin of the C contract that is linked into the Host, by its
    /// entry point.
    Entry(PluginEntry),
}

impl fmt::Debug for Step {
    /// Shows the kind of the step and the path it names; omits a
    /// function and an entry point.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Step::Rust(_) => f.write_str("Rust"),
            Step::Dir(directory) => f.debug_tuple("Dir").field(directory).finish(),
            Step::File(path) => f.debug_tuple("File").field(path).finish(),
            Step::Entry(_) => f.write_str("Entry"),
        }
    }
}

/// The description of a Host: its prompt finder, what it offers its
/// Plugins of the C contract, and its Plugins, in the order written.
///
/// [`Host::builder`] starts one, and each `with_` method but
/// [`with_resources`](HostBuilder::with_resources) names one Plugin.
/// [`build`](HostBuilder::build) does the work: no method before it
/// calls a Plugin, opens a library, or reads a directory. The builder is
/// `Send`, so an application may build its Host on another thread.
pub struct HostBuilder {
    prompts: Arc<dyn PromptFinder>,
    /// What the Host offers each Plugin of the C contract.
    resources: HostResources,
    /// Each step that names a Plugin, in the order written.
    steps: Vec<Step>,
}

impl HostBuilder {
    /// Sets what the Host offers each Plugin of the C contract that the
    /// build loads: every tool call of such a Plugin is lent what
    /// `resources` offers as the call starts.
    ///
    /// It applies to every such Plugin, wherever in the chain it stands,
    /// because [`build`](HostBuilder::build) does the loading. A second
    /// call replaces the first, and a builder without one offers empty
    /// resources. It names no Plugin, so the Host keeps no report for it.
    #[must_use]
    pub fn with_resources(mut self, resources: HostResources) -> HostBuilder {
        self.resources = resources;
        self
    }

    /// Names every library Plugin in `directory`.
    /// [`build`](HostBuilder::build) opens each dynamic library in it, in
    /// file-name order, loads its Plugin, and inserts the Packs that
    /// Plugin offers. One library's failure never stops the others.
    ///
    /// A Plugin name is unique within one directory: a library that
    /// repeats a name this step already loaded is a failure. A directory
    /// that cannot be read is the one failure of its step. The build
    /// reads the path, so a relative `directory` resolves against the
    /// working directory of that moment.
    #[must_use]
    pub fn with_plugin_dir(mut self, directory: impl Into<PathBuf>) -> HostBuilder {
        self.steps.push(Step::Dir(directory.into()));
        self
    }

    /// Names the library Plugin in the file `path`.
    /// [`build`](HostBuilder::build) opens that dynamic library, loads
    /// its Plugin, and inserts the Packs that Plugin offers, as it does
    /// for each library of a directory. The build reads the path, so a
    /// relative `path` resolves against the working directory of that
    /// moment.
    #[must_use]
    pub fn with_plugin_file(mut self, path: impl Into<PathBuf>) -> HostBuilder {
        self.steps.push(Step::File(path.into()));
        self
    }

    /// Names a Plugin of the C contract that is linked into the Host, by
    /// its entry point. [`build`](HostBuilder::build) loads it and
    /// inserts the Packs it offers, as it does for the entry point of a
    /// library. `entry` must follow the C contract in `runvil-plugin-sys`
    /// and must not unwind.
    #[must_use]
    pub fn with_plugin(mut self, entry: PluginEntry) -> HostBuilder {
        self.steps.push(Step::Entry(entry));
        self
    }

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
        self.steps.push(Step::Rust(Box::new(install)));
        self
    }

    /// Builds the Host. It makes an empty registry and runs the steps in
    /// the order written, whatever kind of Plugin each one names: it
    /// calls the function of a Rust Plugin with the registry, and it
    /// loads each Plugin of the C contract and inserts the Packs that
    /// Plugin offers. It keeps one [`PluginReport`] for each step. Then
    /// it adds the two built-in Packs, `runvil/subagents` and
    /// `runvil/user`, and returns the Host inside an `Arc`.
    ///
    /// A failure leaves the Host built, and the build goes on with the
    /// next step. The first Plugin that inserts a Pack id keeps it, so
    /// the registry refuses a later Pack of the same Pack id, and each
    /// refusal is a failure of its step's report. The error of a Rust
    /// Plugin's function is the one failure of its report, and each Pack
    /// the function inserted before it failed stays.
    ///
    /// The Host keeps each Plugin of the C contract that loaded, through
    /// the reports, so a Plugin that inserted no Pack stays loaded for as
    /// long as the Host lives.
    ///
    /// Each report goes to `tracing` as it is made: each Plugin it holds
    /// at the info level, and each failure at the warn level.
    ///
    /// This safe function runs foreign code: opening a library runs its
    /// initialization code, and each entry point it calls has no way to
    /// be checked, because Rust cannot check the C contract. So a Host
    /// names only libraries and entry points that it trusts, and each
    /// entry point must follow the contract in `runvil-plugin-sys` and
    /// must not unwind.
    #[must_use]
    pub fn build(self) -> Arc<Host> {
        let HostBuilder {
            prompts,
            resources,
            steps,
        } = self;
        let mut registry = PackRegistry::new();
        let reports = steps
            .into_iter()
            .map(|step| {
                let report = match step {
                    Step::Rust(install) => make_report(install(&mut registry)),
                    Step::Dir(directory) => {
                        insert_plugin_dir(&mut registry, &directory, &resources).unwrap_or_else(
                            |cause| make_report(Err(make_directory_error(&directory, &cause))),
                        )
                    }
                    Step::File(path) => insert_plugin_file(&mut registry, &path, &resources),
                    Step::Entry(entry) => insert_plugin(&mut registry, entry, &resources),
                };
                log_report(&report);
                report
            })
            .collect();
        Arc::new(Host {
            packs: Packs::new(registry),
            prompts,
            reports,
        })
    }
}

impl fmt::Debug for HostBuilder {
    /// Shows the resources and each step; omits the finder.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostBuilder")
            .field("resources", &self.resources)
            .field("steps", &self.steps)
            .finish_non_exhaustive()
    }
}

/// The failure of a step whose Plugin directory cannot be read: a
/// `PluginLoadFailed` error with the `Reason` fact `directory`, the
/// directory as its `Path` fact, and the system's reason as its cause.
fn make_directory_error(directory: &Path, cause: &io::Error) -> Error {
    let code = ErrorCode::PluginLoadFailed;
    let mut error =
        Error::new(code, "the Plugin directory could not be read", code.prose()).with_cause(cause);
    error.facts.insert(ErrorFact::Reason, "directory".into());
    error
        .facts
        .insert(ErrorFact::Path, directory.display().to_string().into());
    error
}

/// Logs the report of one step: each Plugin it holds, at the info level,
/// and each failure, at the warn level, with the path the failure names
/// when it names one.
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
