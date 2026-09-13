//! PromptForge runtime core.
//!
//! This crate holds the pieces that turn a prompt markdown file into a model
//! call: the [`parser`] that reads the file into a [`parser::Prompt`], the
//! [`client`] that talks to an `OpenAI`-compatible chat completions endpoint, and
//! [`execute`] that runs H1 once with live resolution before walking sections
//! top to bottom (fall-through) and
//! returns the run's result. The host-facing vocabulary a run is configured
//! with - the progress observer, the model and tool catalogs, the tool
//! contract - lives in the `shared-promptforge-api` crate
//! (`shared_promptforge_api::observe`, `shared_promptforge_api::models`,
//! `shared_promptforge_api::tools`), and the store handle a host seeds or
//! extracts comes from `shared-vfs` and `promptforge-vfs`.
//! [`execute::run`] takes an [`execute::RunConfig`] carrying the
//! observer the correlated report records go to, and
//! `shared_promptforge_api::observe::NullObserver` is what a caller wanting
//! silence passes.
//! [`debug::DebugCapture`] is an opt-in raw request/response seam on the same
//! config; production hosts leave it unset.
//!
//! A source is a promptforge prompt only when its frontmatter declares a
//! `promptforge:` version; [`promptforge_version`] reports it (or `None`), and
//! the runtime refuses a source that lacks a supported version.
//!
//! # Examples
//!
//! Detect a promptforge source and parse it into a [`Prompt`]:
//!
//! ```
//! use promptforge_api::{Prompt, promptforge_version};
//! use shared_promptforge_api::observe::NullObserver;
//!
//! let source = "---\nname: greeter\ndescription: says hi\npromptforge: 0\n---\n\n# Greeter\n\n## Say hi\n\nSay hello.\n\n```lua\nreturn models.infer(prose)\n```\n";
//!
//! // Version detection gates whether the runtime will accept the source.
//! assert_eq!(promptforge_version(source), Some(0));
//! assert_eq!(promptforge_version("plain text, no frontmatter"), None);
//!
//! let prompt = Prompt::parse(source, "doc-example", &NullObserver::default())?;
//! assert_eq!(prompt.title(), "Greeter");
//! assert_eq!(prompt.sections()[0].name(), "Say hi");
//! # Ok::<(), promptforge_api::ParseError>(())
//! ```
//!
//! Executing a parsed prompt goes through [`run`] with a [`RunConfig`] and a
//! [`ResolutionContext`] (an optional picker, a model catalog, and a tool
//! catalog); the store handle rides on the config, defaulting to the stock
//! in-memory mount. That path can perform gateway I/O, so it is shown as
//! `no_run`:
//!
//! ```no_run
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! use promptforge_api::{Prompt, ResolutionContext, RunConfig, run};
//! use shared_promptforge_api::models::ModelCatalog;
//! use shared_promptforge_api::observe::NullObserver;
//! use shared_promptforge_api::tools::ToolCatalog;
//!
//! let source = "---\nname: greeter\ndescription: says hi\npromptforge: 0\n---\n\n# Greeter\n\n## Say hi\n\nSay hello.\n\n```lua\nreturn models.infer(prose)\n```\n";
//! let prompt = Prompt::parse(source, "run-example", &NullObserver::default())?;
//!
//! // Capability-free agents pass no picker.
//! let models = ModelCatalog::empty();
//! let tools = ToolCatalog::new(&[])?;
//! let answer = run(
//!     &prompt,
//!     "",
//!     ResolutionContext::new(None, &models, &tools),
//!     RunConfig::new("run-example"),
//! )
//! .await?;
//! println!("{answer}");
//! # Ok(())
//! # }
//! ```
//!
pub(crate) mod cancel;
pub mod client;
pub mod debug;
mod error;
pub mod execute;
pub(crate) mod fanout;
pub mod input;
pub(crate) mod lua;
pub(crate) mod model;
pub(crate) mod observe;
pub mod parser;
mod resolve;
pub(crate) mod store;
pub(crate) mod subst;
#[cfg(test)]
pub(crate) mod test_support;
pub(crate) mod tools;
pub(crate) mod untrusted;

pub(crate) use crate::error::{Error, Result};
pub(crate) use crate::tools::NearDuplicateDiagnostic;

pub use crate::client::{CompletionError, CompletionErrorKind};
pub use crate::execute::{ResolutionContext, RunConfig, RunError, RunErrorKind, RunLimits, run};
pub use crate::parser::{ParseError, ParseErrorKind, Prompt, promptforge_version};
