//! Operations on the parser's facade-visible types that only
//! `runvil-runtime` performs.
//!
//! The `runvil-engine` facade never re-exports this module, so only Engine
//! crates reach it.

use std::num::NonZeroU32;

use runvil_types::error::Error;

use crate::{Prompt, Section};

/// The name of the one argument a prompt with no `args:` key declares,
/// and the field a string given as a chain's arguments goes under.
pub const TEXT_FIELD: &str = crate::contract::TEXT_FIELD;

/// Reads `text`, the arguments a caller passes a run of `prompt`, against
/// the prompt's `args` declaration, and returns the object the run holds:
/// the caller's fields, with the declared default of each field left out.
///
/// # Errors
/// Returns an `ArgsInvalid` failure with no location when `text` is not
/// JSON, is JSON but no object, or holds a value the declaration refuses.
/// Its `Reason` fact is `not_json`, `not_an_object`, `required`, `type`,
/// `enum`, or `undeclared`, and for the last four its `Name` fact is the
/// path of the refused argument.
pub fn accept_args(prompt: &Prompt, text: &str) -> Result<serde_json::Value, Error> {
    crate::contract::accept_args(prompt.frontmatter().args(), text)
}

/// Gives a parse failure the Engine raises about a parsed prompt the
/// prompt's frontmatter `name` as its `Name` fact, its place in the
/// prompt's `file`, and the prose a failure the parse returns has.
#[must_use]
pub fn with_prompt_context(error: Error, name: &str, file: &str) -> Error {
    crate::error::with_prompt_context(error, Some(name), file)
}

/// Returns the top-level sections in file order: the preamble first, when
/// the prompt has one, then each heading at the body's shallowest level.
#[must_use]
pub fn sections(prompt: &Prompt) -> &[Section] {
    &prompt.sections
}

/// The entry-point section: the first top-level section in file order.
#[must_use]
pub fn entry(prompt: &Prompt) -> Option<&Section> {
    prompt.entry()
}

/// Returns the file line of the frontmatter's top-level `key`, such as
/// `uses`, when exactly one line declares it at column 0.
///
/// The line comes from a scan of the text, not the YAML parse, so it is
/// meaningful only for a key that decoded to a value.
#[must_use]
pub fn key_line(prompt: &Prompt, key: &str) -> Option<NonZeroU32> {
    prompt.key_lines.get(key).copied()
}
