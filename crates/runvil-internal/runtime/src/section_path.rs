//! Section paths in a prompt: the forms a path takes, and the one lookup
//! every function that names a section resolves through.
//!
//! A section's Lua names another section by a path of titles. `name` is
//! the direct child with that title and `../name` is the sibling, both
//! counted from the path's origin, the section the path starts at. `/a/b`
//! starts at the top of the prompt and names one title per level, so it
//! names the same section from everywhere. The grammar of steps, escapes,
//! and trimming is `runvil_markdown::split`, which a path over parsed
//! text shares. A step is never empty, so no path names the preamble,
//! whose name is empty; a path can start at it, as at any section.
//!
//! Which forms a function takes is that function's rule, so the module
//! has one entry per rule: [`resolve_neighbor`] for the functions that run
//! a section, which take the two relative forms; [`find`] for the `prompt`
//! reads, which take all three; and [`resolve_from_top`] for `require`,
//! which takes only a path from the top. A well-formed path that names no
//! section is a miss, and its error says where the lookup looked and
//! lists nothing.

use runvil_markdown::{Steps, escape_title, split};

use crate::prompt::{Prompt, Section, detail};
use crate::section_pos::{SectionIndex, SectionPos};
use crate::{Error, Result, error};

/// The direct child of the section at `origin` titled `title`.
fn child(origin: &SectionPos, prompt: &Prompt, title: &str) -> Option<SectionPos> {
    let children = origin.section(prompt)?.children();
    position(children, title, None).map(|index| origin.child(index))
}

/// The sibling of the section at `origin` titled `title`: another section
/// under the same parent, never the section itself.
fn sibling(origin: &SectionPos, prompt: &Prompt, title: &str) -> Option<SectionPos> {
    position(origin.siblings(prompt), title, Some(origin.index)).map(|index| SectionPos {
        slice: origin.slice.clone(),
        index,
    })
}

/// The index of the section titled `title` in `slice`, passing over the
/// section at `skip`. Sibling titles are unique, so there is at most one.
fn position(slice: &[Section], title: &str, skip: Option<SectionIndex>) -> Option<SectionIndex> {
    slice
        .iter()
        .enumerate()
        .map(|(index, section)| (SectionIndex::new(index), section))
        .find(|(index, section)| Some(*index) != skip && section.name() == title)
        .map(|(index, _)| index)
}

/// What a well-formed path named.
#[derive(Debug)]
pub(crate) enum Lookup {
    /// The section at this position.
    Found(SectionPos),
    /// No section. The error is the not-found error for the path.
    Missing(Error),
}

impl Lookup {
    /// The section's position, for a function that must reach one.
    ///
    /// # Errors
    /// Returns the not-found error of a miss.
    pub(crate) fn required(self) -> Result<SectionPos> {
        match self {
            Lookup::Found(pos) => Ok(pos),
            Lookup::Missing(error) => Err(error),
        }
    }
}

/// Resolves a path that names a neighbor of `origin`: `name` for a direct
/// child or `../name` for a sibling. These two forms are a section's
/// visible set, and they are all that `call`, `jump`, `tasks.spawn`, and
/// `tasks.fanout` take.
///
/// # Errors
/// Returns a `LuaRuntime` failure when the path is malformed, starts with
/// `/`, is neither relative form, or names no section.
pub(crate) fn resolve_neighbor(
    given: &str,
    prompt: &Prompt,
    origin: &SectionPos,
) -> Result<SectionPos> {
    let path = titles(given)?;
    if path.from_top {
        return Err(error::make_lua_runtime_error(format!(
            "section path \"{given}\" starts with \"/\"; only require, prompt.section, and \
             prompt.section_or take a path from the top"
        )));
    }
    neighbor(given, &path.titles, prompt, origin)?.required()
}

/// Looks up a path a `prompt` read takes: either relative form from
/// `origin`, or a path from the top, which names any section.
///
/// # Errors
/// Returns a `LuaRuntime` failure when the path is malformed, or is
/// relative and neither relative form. A path that names no section is a
/// [`Lookup::Missing`], not an error, so the caller decides whether a miss
/// raises.
pub(crate) fn find(given: &str, prompt: &Prompt, origin: &SectionPos) -> Result<Lookup> {
    let path = titles(given)?;
    if path.from_top {
        from_top(given, &path.titles, prompt)
    } else {
        neighbor(given, &path.titles, prompt, origin)
    }
}

/// Resolves a path from the top of the prompt, the only form `require`
/// takes. It names the same section wherever it is written.
///
/// # Errors
/// Returns a `LuaRuntime` failure when the path is malformed, does not
/// start with `/`, or names no section.
pub(crate) fn resolve_from_top(given: &str, prompt: &Prompt) -> Result<SectionPos> {
    let path = titles(given)?;
    if !path.from_top {
        return Err(error::make_lua_runtime_error(format!(
            "section path \"{given}\" does not start with \"/\"; require takes a path from the \
             top, such as \"/helpers\""
        )));
    }
    from_top(given, &path.titles, prompt)?.required()
}

/// The path from the top that names the section at `pos`, such as
/// `/Main/Kid`: how the Engine prints a section's address.
pub(crate) fn path_from_top(prompt: &Prompt, pos: &SectionPos) -> String {
    let mut path = String::new();
    let mut slice = detail::sections(prompt);
    for &index in pos.slice.indices().iter().chain([&pos.index.get()]) {
        let Some(section) = slice.get(index) else {
            break;
        };
        path.push('/');
        path.push_str(&escape_title(section.name()));
        slice = section.children();
    }
    path
}

/// A path's titles, after the checks every path in a prompt shares.
struct Titles {
    /// Whether the path starts with `/`.
    from_top: bool,
    /// The title of each step, in order. There is at least one.
    titles: Vec<String>,
}

/// Splits `given` into its titles.
///
/// # Errors
/// Returns a `LuaRuntime` failure for a path the grammar refuses, and for
/// the two forms only a path over parsed text has: `//` and an index.
fn titles(given: &str) -> Result<Titles> {
    let Steps { from_top, steps } = split(given)
        .map_err(|message| error::make_lua_runtime_error(format!("section {message}")))?;
    if steps.iter().any(|step| step.deep) {
        return Err(error::make_lua_runtime_error(format!(
            "section path \"{given}\" has an empty step"
        )));
    }
    if steps.iter().any(|step| step.index.is_some()) {
        return Err(error::make_lua_runtime_error(format!(
            "section path \"{given}\" has an index; a path in a prompt takes none"
        )));
    }
    Ok(Titles {
        from_top,
        titles: steps.into_iter().map(|step| step.title).collect(),
    })
}

/// What a lookup wanted of the section it searched.
#[derive(Clone, Copy)]
enum Wanted {
    /// A direct child with the title.
    Child,
    /// A sibling with the title.
    Sibling,
}

/// Looks up the relative path whose titles are `titles` from `origin`.
///
/// # Errors
/// Returns a `LuaRuntime` failure when the titles are neither `name` nor
/// `..` and a name.
fn neighbor(
    given: &str,
    titles: &[String],
    prompt: &Prompt,
    origin: &SectionPos,
) -> Result<Lookup> {
    let (wanted, title) = match titles {
        [name] if name == ".." => {
            return Err(error::make_lua_runtime_error(format!(
                "section path \"{given}\" needs a name after \"..\""
            )));
        }
        [name] => (Wanted::Child, name),
        [parent, name] if parent == ".." && name != ".." => (Wanted::Sibling, name),
        _ => {
            return Err(error::make_lua_runtime_error(format!(
                "section path \"{given}\" has more than one name; a relative path is a child's \
                 name, or \"../\" and a sibling's name"
            )));
        }
    };
    let found = match wanted {
        Wanted::Child => child(origin, prompt, title),
        Wanted::Sibling => sibling(origin, prompt, title),
    };
    Ok(match found {
        Some(pos) => Lookup::Found(pos),
        None => Lookup::Missing(make_not_found_error(
            given,
            prompt,
            Some(origin),
            wanted,
            title,
        )),
    })
}

/// Looks up the path from the top whose titles are `titles`: one child
/// per step, starting from the top-level sections.
///
/// # Errors
/// Returns a `LuaRuntime` failure when a title is `..`, and an `Internal`
/// failure for a path with no step.
fn from_top(given: &str, titles: &[String], prompt: &Prompt) -> Result<Lookup> {
    if titles.iter().any(|title| title == "..") {
        return Err(error::make_lua_runtime_error(format!(
            "section path \"{given}\" uses \"..\"; a path from the top takes none"
        )));
    }
    let mut reached: Option<SectionPos> = None;
    for title in titles {
        let found = match &reached {
            None => position(detail::sections(prompt), title, None).map(SectionPos::top),
            Some(parent) => child(parent, prompt, title),
        };
        let Some(pos) = found else {
            // The walk stopped where it had reached, which has no such
            // section below it.
            let error = make_not_found_error(given, prompt, reached.as_ref(), Wanted::Child, title);
            return Ok(Lookup::Missing(error));
        };
        reached = Some(pos);
    }
    reached
        .map(Lookup::Found)
        .ok_or(error::make_internal_error("a section path has no step"))
}

/// The error for a path that names no section: the path as it was given,
/// the section that was searched, or `None` for the top level, and what
/// was wanted there. It lists no other section, so the error cannot show
/// the rest of the prompt's structure. The preamble is named in words,
/// because its address, the path of an empty name, would read as the top
/// level.
fn make_not_found_error(
    given: &str,
    prompt: &Prompt,
    searched: Option<&SectionPos>,
    wanted: Wanted,
    title: &str,
) -> Error {
    let looked = match searched {
        None => format!("the top level has no section `{title}`"),
        Some(pos) => {
            let preamble = pos
                .section(prompt)
                .is_some_and(|section| section.name().is_empty());
            let searched = if preamble {
                "the preamble".to_owned()
            } else {
                format!("`{}`", path_from_top(prompt, pos))
            };
            let wanted = match wanted {
                Wanted::Child => "child",
                Wanted::Sibling => "sibling",
            };
            format!("{searched} has no {wanted} `{title}`")
        }
    };

    error::make_lua_runtime_error(format!("section `{}` not found: {looked}", given.trim()))
}

#[cfg(test)]
#[path = "section_path-tests.rs"]
mod tests;
