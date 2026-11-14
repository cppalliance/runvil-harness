//! Parses a prompt's source into a [`Prompt`]: frontmatter and sections.

use std::sync::Arc;

use runvil_types::error::Error;

use crate::frontmatter::SplitSource;
use crate::sections::{self, SectionWalker};
use crate::{
    Frontmatter, Prompt, PromptFinder, Result, Section, contract, error, fence, frontmatter,
};

impl Prompt {
    /// Finds the prompt `name` through `finder` and parses it. A find
    /// failure is the parse result. `name` names the prompt in failures
    /// and in [`Prompt::file`].
    ///
    /// # Errors
    /// Returns the error `finder` returned, unchanged, or an [`Error`]
    /// whose code is:
    ///
    /// - `FrontmatterInvalid` when the `---` delimiters are missing, the
    ///   frontmatter fails to decode, a `uses:` entry is neither a Pack id
    ///   nor vendor/name, two entries share a Pack id, an entry names a
    ///   Pack id the Engine reserves, or the `args:` declaration is outside
    ///   its subset of JSON Schema. A refused `uses:` list has the `Reason`
    ///   fact `pack_duplicate` or `pack_reserved` and the Pack id as its
    ///   `Name` fact, and is located on the `uses:` line when one line
    ///   declares the key. An `args:` declaration is refused when a node
    ///   holds a keyword other than `type`, `properties`, `required`,
    ///   `additionalProperties`, `items`, `enum`, `description`, and
    ///   `default`, or holds one twice; a node has no `type`, or one other
    ///   than `string`, `boolean`, `integer`, `number`, `array`, and
    ///   `object`; the root is not an `object`; an `array` has no `items`;
    ///   `items` is on a node that is not an `array`, `enum` on one that is
    ///   neither a `string` nor an `integer`, or `properties`, `required`,
    ///   or `additionalProperties` on one that is not an `object`; the root
    ///   or a list item has a `default`; a `default` has no JSON form or
    ///   does not match its node; an `enum` is empty, repeats a value, or
    ///   lists a value of another kind than its node; `required` names a
    ///   field that `properties` does not declare, or names one twice; or
    ///   an argument name is not `[A-Za-z][A-Za-z0-9_-]{0,63}` or is
    ///   declared twice. The message of a refused declaration names the
    ///   YAML path of the node or keyword, such as `args.properties.query`,
    ///   and the failure is located at the line and column the YAML parser
    ///   reports for it.
    /// - `StructureInvalid` when a heading skips a level (such as an H4
    ///   directly under an H2, or a first heading deeper than a later
    ///   one), a heading is empty, or two sibling sections have the same
    ///   name.
    /// - `FenceInvalid` when a `lua` fence is left open, or anywhere in the
    ///   body a fence or line breaks the strict Lua fence rule. A `lua`
    ///   fence opens with a line that is exactly three backticks then `lua`
    ///   and closes with a line that is exactly three backticks. Label: a
    ///   fence's label starts with `lua` in any letter case without being
    ///   exactly `lua`, such as `Lua` or `lua extra`. Opener: a fence
    ///   labeled `lua` opens with any other line, such as one that is
    ///   indented, uses tildes or four backticks, or sits in a block quote
    ///   or list item. Closer: a fence with an exact opening line closes
    ///   with any other line. Lookalike: a line outside every fence looks
    ///   like a `lua` opening line, such as one in an indented code block.
    ///   The first violation in source order fails the parse at its line,
    ///   before any Lua compiles, with the rule it breaks as its `Reason`
    ///   fact: `label`, `opener`, `closer`, or `lookalike`. Lines inside the
    ///   body of another fenced code block are never inspected, so a
    ///   document can show a Lua fence as an example.
    /// - `LuaCompile` when a Lua block fails to compile as Lua.
    /// - `Internal` when the parser fails through no fault of the prompt:
    ///   one of its own invariants breaks, or the Lua compiler cannot start.
    ///
    /// The failure's location, when known, is in `name`, except that an
    /// `Internal` failure is located at the Rust source position of the
    /// fault. A failure found past the frontmatter, other than an
    /// `Internal` one, has the prompt's frontmatter name as its `Name` fact.
    pub fn parse(finder: &dyn PromptFinder, name: &str) -> std::result::Result<Prompt, Error> {
        finder
            .find(name)
            .and_then(|source| Self::parse_source(&source, name))
    }

    /// Parses `input`, the source of the prompt `file`, and adds the
    /// prompt's context to a failure.
    fn parse_source(input: &str, file: &str) -> std::result::Result<Prompt, Error> {
        // Everything past the frontmatter postdates the prompt's name, so a
        // failure from there on names it.
        let (name, result) = match Self::parse_frontmatter(input) {
            Ok((frontmatter, split)) => (
                Some(frontmatter.name().to_owned()),
                Self::parse_body(frontmatter, split, file),
            ),
            Err(error) => (None, Err(error)),
        };
        result.map_err(|error| error::with_prompt_context(error, name.as_deref(), file))
    }

    /// Splits the frontmatter from the body, decodes it, and checks its
    /// `uses:` list, placing a refused list on the `uses:` line.
    fn parse_frontmatter(input: &str) -> Result<(Frontmatter, SplitSource)> {
        let split = frontmatter::split_frontmatter(input)?;
        let frontmatter: Frontmatter = serde_yaml_ng::from_str(&split.yaml)
            .map_err(|cause| error::make_frontmatter_decode_error(&cause))?;
        contract::check_distinct_packs(frontmatter.uses())
            .and_then(|()| contract::check_no_reserved_pack(frontmatter.uses()))
            .map_err(|refusal| error::on_line(refusal, split.key_lines.get("uses").copied()))?;
        Ok((frontmatter, split))
    }

    fn parse_body(frontmatter: Frontmatter, split: SplitSource, file: &str) -> Result<Prompt> {
        let frontmatter_lines = split.frontmatter_lines;
        let shared: Arc<str> = Arc::from(split.body);
        let body: &str = &shared;
        let headings = sections::collect_headings(body)?;
        if let Some(violation) = fence::first_fence_violation(body, frontmatter_lines) {
            return Err(violation);
        }

        // The top level is the shallowest heading level in the body, so
        // every heading at that level is a top-level section, and a first
        // heading deeper than a later one fails the walker's level check.
        let top = headings.iter().map(|h| h.level).min().unwrap_or(1);
        let mut walker = SectionWalker::new(&shared, &headings, frontmatter_lines, file);
        let mut sections: Vec<Section> = walker.build_preamble(top)?.into_iter().collect();
        sections.extend(walker.build_sections(top.saturating_sub(1))?);

        Ok(Prompt {
            file: file.to_owned(),
            frontmatter,
            sections,
            key_lines: split.key_lines,
        })
    }
}
