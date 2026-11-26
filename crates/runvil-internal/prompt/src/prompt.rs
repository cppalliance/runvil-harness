//! Holds the parsed prompt tree: [`Prompt`], its [`Section`]s, and their [`Block`]s.
//!
//! Code outside the Engine reads a prompt's file, its frontmatter, and the
//! prompts its lists reach; the tree below them is the Engine's, reached
//! through [`crate::detail`].
//!
//! Construction happens in the parsing modules; this module holds the value
//! types and their accessors.

use std::collections::BTreeMap;
use std::num::NonZeroU32;
use std::sync::Arc;

use crate::{BodyText, Frontmatter};
use runvil_lua::LuaProgram;

/// One executable block inside a section: a compiled Lua fence or prose.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Block {
    /// An exact `lua` fence compiled at parse time.
    Lua(LuaProgram),
    /// Author prose: the pending Markdown accumulated since the nearest
    /// preceding heading, `lua` fence, or thematic break, or since the
    /// body's start.
    ///
    /// The executor installs it as the following Lua block's lazy `prose`
    /// template.
    #[non_exhaustive]
    Prose {
        /// Captured Markdown, trimmed of leading and trailing whitespace.
        text: BodyText,
        /// The 1-based line of the prompt file that `text` starts on.
        line: NonZeroU32,
    },
}

/// One section of a prompt: a heading, ordered blocks, and children.
///
/// The preamble, the content before the body's first heading, is a section
/// too when it holds a `lua` fence: it has an empty name, no children, and
/// the top level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// The heading text (the section's address), empty for the preamble.
    pub(crate) name: String,
    /// The heading level, 1 through 6.
    pub(crate) level: u8,
    /// Ordered lua/prose blocks for this section.
    pub(crate) blocks: Vec<Block>,
    /// Child sections nested under this one (deeper heading levels).
    pub(crate) children: Vec<Section>,
    /// The section's own raw Markdown, without its heading line or children.
    pub(crate) markdown: BodyText,
    /// The 1-based line of the prompt file that `markdown` starts on.
    pub(crate) markdown_line: NonZeroU32,
}

impl Section {
    /// Returns the heading text (the section's address), which is empty
    /// for the preamble alone.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the heading level (1 through 6).
    #[must_use]
    pub fn level(&self) -> u8 {
        self.level
    }

    /// Returns the ordered Lua and prose blocks of this section.
    #[must_use]
    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    /// Returns the child sections nested under this one.
    #[must_use]
    pub fn children(&self) -> &[Section] {
        &self.children
    }

    /// The section's own raw Markdown: from just after its heading line to the next
    /// heading of any level, leading blank lines and trailing whitespace trimmed.
    #[must_use]
    pub fn markdown(&self) -> &str {
        self.markdown.as_str()
    }

    /// The section's own raw Markdown as a stretch of the shared prompt
    /// text, which a reader takes without copying it.
    #[must_use]
    pub fn markdown_text(&self) -> &BodyText {
        &self.markdown
    }

    /// The 1-based line of the prompt file that the section's own raw
    /// Markdown starts on.
    #[must_use]
    pub fn markdown_line(&self) -> NonZeroU32 {
        self.markdown_line
    }

    /// The prologue: the first block, when it is Lua.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn prologue(&self) -> Option<&LuaProgram> {
        match self.blocks.first() {
            Some(Block::Lua(program)) => Some(program),
            _ => None,
        }
    }

    /// Text of the last prose block, or `""` when the section has none.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn prose(&self) -> &str {
        self.blocks
            .iter()
            .rev()
            .find_map(|block| match block {
                Block::Prose { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .unwrap_or("")
    }

    /// The epilog: the last block, when it is Lua and not the only block.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn epilog(&self) -> Option<&LuaProgram> {
        match self.blocks.as_slice() {
            [Block::Lua(_)] => None,
            [.., Block::Lua(program)] => Some(program),
            _ => None,
        }
    }
}

/// A fully parsed prompt file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    /// The file the source came from, as the caller of
    /// [`Prompt::parse`](crate::Prompt::parse) named it.
    pub(crate) file: String,
    /// The parsed YAML frontmatter.
    pub(crate) frontmatter: Frontmatter,
    /// The top-level sections in file order: the preamble first, when the
    /// prompt has one, then each heading at the body's shallowest level.
    pub(crate) sections: Vec<Section>,
    /// The file line of each top-level frontmatter key that exactly one
    /// line declares, by the key's name; the Engine reaches it through
    /// [`crate::detail::key_line`].
    pub(crate) key_lines: BTreeMap<String, NonZeroU32>,
    /// Every other prompt the `prompts:` lists reach from this one, by the
    /// name it was found under. Only the prompt a parse returns holds any:
    /// the table of each prompt in it is empty.
    pub(crate) linked: BTreeMap<String, Arc<Prompt>>,
}

impl Prompt {
    /// Returns the name the prompt was found under: the `name` given to
    /// [`parse`](Prompt::parse), or the `prompts:` entry that listed it.
    /// A failure uses it as the file of its location.
    #[must_use]
    pub fn file(&self) -> &str {
        &self.file
    }

    /// Returns the parsed frontmatter.
    #[must_use]
    pub fn frontmatter(&self) -> &Frontmatter {
        &self.frontmatter
    }

    /// Returns every other prompt the `prompts:` lists reach from this
    /// one, in the order of their names: the prompts this one lists, the
    /// prompts those list, and so on, each once and each already parsed.
    /// [`file`](Prompt::file) is the name a prompt was listed under.
    ///
    /// The prompt [`Prompt::parse`](crate::Prompt::parse) returns holds
    /// them all, so each prompt this returns has none of its own. A run
    /// of the returned prompt can execute each of them; a run of one of
    /// these holds no other prompt to execute.
    pub fn linked(&self) -> impl Iterator<Item = &Arc<Prompt>> {
        self.linked.values()
    }

    /// The entry-point section: the first top-level section in file order;
    /// the Engine reaches it through [`crate::detail::entry`].
    #[must_use]
    pub(crate) fn entry(&self) -> Option<&Section> {
        self.sections.first()
    }
}
