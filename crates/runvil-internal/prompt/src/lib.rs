//! Loads a prompt file: its frontmatter, its Markdown structure, and its Lua.
//!
//! A prompt is one markdown file: YAML frontmatter, then a body. Every
//! heading in the body starts a section, and the body's shallowest heading
//! level is its top level. The preamble, the content before the first
//! heading, is the first top-level section when it holds a `lua` fence, and
//! commentary otherwise. A section's content is an alternating sequence of
//! exact `lua` fences and prose ([`Block`]). Sections nest recursively, each
//! heading one level deeper than its parent, through H6. A section with a
//! prologue, prose, and an epilog is exactly `[Lua, Prose, Lua]`.
//!
//! Prose capture follows the pending-Markdown model: Markdown accumulates
//! from the body's start and after each heading or ordinary `lua` fence, and each prose block is the
//! pending buffer the following `lua` fence consumes. A `---` thematic break
//! resets the pending buffer without becoming part of the prose; it has
//! no control-flow meaning, so content below a break parses and runs
//! normally. Markdown left after the final `lua` fence is inert trailing
//! commentary, never an error.
//!
//! Every `lua` fence in the body compiles at load time, so
//! a loaded [`Prompt`] is ready to run: a syntax error surfaces here, never
//! partway through a run.
//!
//! The frontmatter's `args` key declares the one JSON object a caller
//! passes the prompt, as a subset of JSON Schema ([`ArgsSchema`]), and
//! [`detail::accept_args`] checks a caller's arguments against it for the
//! executor.
//!
//! ## Invariants
//!
//! - May depend on: `runvil-types` and `runvil-lua`.
//!   `cargo test -p build-xtask` enforces the product and container
//!   boundaries.

mod body_text;
mod contract;
pub mod detail;
mod error;
mod fence;
mod finder;
mod frontmatter;
mod parse;
mod position;
mod prompt;
mod sections;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

pub use body_text::BodyText;
pub use contract::{ArgsSchema, RoleRequirements};
use error::Result;
pub use finder::PromptFinder;
pub use frontmatter::{FileDecl, Frontmatter, MAX_TOOL_ITERATIONS};
pub use prompt::{Block, Prompt, Section};

#[cfg(test)]
mod tests;
