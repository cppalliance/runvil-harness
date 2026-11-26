//! Splits a prompt's YAML frontmatter from its body and models what it declares.

use std::collections::BTreeMap;
use std::fmt;
use std::num::NonZeroU32;
use std::sync::Arc;

use runvil_types::error::ErrorCode;
use runvil_types::models::ModelRole;
use runvil_types::packs::PackRef;

use crate::contract::{RoleMap, check_alias};
use crate::{Annotations, ArgsSchema, Result, RoleRequirements, error};

/// The parsed frontmatter of a prompt file.
///
/// Parsing accepts only the keys this schema defines. Any other key, such as
/// a misspelled or extra one, is a prompt authoring error and fails the parse.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Frontmatter {
    /// The prompt's identifier, from the `name:` key, in the grammar
    /// `[A-Za-z][A-Za-z0-9_-]{0,63}`, so it is a valid tool name.
    #[serde(deserialize_with = "prompt_name")]
    name: String,
    /// The prompt's one-line summary, from the `description:` key.
    description: String,
    /// The prompt's display name, from the `title:` key: non-empty text
    /// on one line, with no control character and no leading or trailing
    /// whitespace.
    #[serde(default, deserialize_with = "title")]
    title: Option<String>,
    /// The runvil Engine major this file targets, when declared.
    ///
    /// Its presence marks the file as a runvil prompt; `None` means the
    /// file is not one.
    #[serde(default)]
    runvil: Option<u32>,
    /// Maximum model rounds one `models.loop` call may start.
    ///
    /// An absent value leaves the runtime's own cap in force. A declared
    /// value is a positive, bounded count: deserialization rejects `0`,
    /// negatives, and values above [`MAX_TOOL_ITERATIONS`], so no invalid
    /// cap can reach execution.
    #[serde(default, deserialize_with = "max_tool_iterations")]
    max_tool_iterations: Option<NonZeroU32>,
    /// Packs the prompt declares, each a bare Pack id or vendor/name, in
    /// declaration order: each is required, and its prelude runs in every
    /// section.
    #[serde(default)]
    uses: Vec<PackRef>,
    /// The arguments the prompt declares; an absent `args:` key yields
    /// the default [`ArgsSchema`].
    #[serde(default)]
    args: ArgsSchema,
    /// The requirements declared per model role.
    #[serde(default)]
    models: RoleMap,
    /// The prompts the prompt lists, which are the ones its Lua may run
    /// with `execute`, each by the name a
    /// [`PromptFinder`](crate::PromptFinder) finds it under, in
    /// declaration order.
    #[serde(default)]
    prompts: Vec<String>,
    /// The behavior hints the prompt declares for publishing it as a tool;
    /// an absent or null `annotations:` key leaves every hint unset.
    #[serde(default, deserialize_with = "annotations")]
    annotations: Annotations,
}

/// Deserializes the prompt's `name:`, refusing one outside the grammar
/// `[A-Za-z][A-Za-z0-9_-]{0,63}`.
fn prompt_name<'de, D>(deserializer: D) -> std::result::Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    deserializer.deserialize_string(NameVisitor)
}

/// The visitor behind [`prompt_name`]. The YAML parser locates only a
/// refusal raised while it visits the value, so the check runs here.
struct NameVisitor;

impl serde::de::Visitor<'_> for NameVisitor {
    type Value = String;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a prompt name")
    }

    fn visit_str<E>(self, name: &str) -> std::result::Result<String, E>
    where
        E: serde::de::Error,
    {
        check_alias(name, "prompt name").map_err(E::custom)?;
        Ok(name.to_owned())
    }
}

/// Deserializes the optional `title:`, refusing text that is empty or all
/// whitespace, that holds a control character or a line break, or that
/// starts or ends with whitespace.
fn title<'de, D>(deserializer: D) -> std::result::Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    deserializer.deserialize_option(TitleVisitor)
}

/// The visitor behind [`title`]. The YAML parser locates only a refusal
/// raised while it visits the value, so the check runs here.
struct TitleVisitor;

impl<'de> serde::de::Visitor<'de> for TitleVisitor {
    type Value = Option<String>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a title")
    }

    fn visit_none<E>(self) -> std::result::Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_unit<E>(self) -> std::result::Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_some<D>(self, deserializer: D) -> std::result::Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_string(self)
    }

    fn visit_str<E>(self, title: &str) -> std::result::Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        // U+2028 and U+2029 break a line but are not control characters.
        let breaks = |c: char| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}');
        let expected = if title.trim().is_empty() {
            "text that is not empty or all whitespace"
        } else if title.contains(breaks) {
            "one line with no control character"
        } else if title.trim() != title {
            "no leading or trailing whitespace"
        } else {
            return Ok(Some(title.to_owned()));
        };
        Err(E::custom(format!(
            "invalid title {title:?}: expected {expected}"
        )))
    }
}

// `serde(default)` handles an absent key, not an explicit null.
fn annotations<'de, D>(deserializer: D) -> std::result::Result<Annotations, D::Error>
where
    D: serde::Deserializer<'de>,
{
    <Option<Annotations> as serde::Deserialize>::deserialize(deserializer)
        .map(Option::unwrap_or_default)
}

/// The largest explicit `max_tool_iterations` a prompt may declare.
pub const MAX_TOOL_ITERATIONS: u32 = 1000;

/// Deserializes the `max_tool_iterations` cap: absent, or a positive count no
/// larger than [`MAX_TOOL_ITERATIONS`].
///
/// Deserialization reads an `i64` so negatives and values above `u32` are
/// rejected instead of wrapped, and rejects `0` so every accepted value is a
/// [`NonZeroU32`] and no invalid cap can reach execution.
fn max_tool_iterations<'de, D>(deserializer: D) -> std::result::Result<Option<NonZeroU32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let Some(raw) = <Option<i64> as serde::Deserialize>::deserialize(deserializer)? else {
        return Ok(None);
    };
    if raw <= 0 {
        return Err(serde::de::Error::custom(format!(
            "max_tool_iterations must be a positive integer (>= 1), got {raw}"
        )));
    }
    u32::try_from(raw)
        .ok()
        .and_then(NonZeroU32::new)
        .filter(|v| v.get() <= MAX_TOOL_ITERATIONS)
        .map(Some)
        .ok_or_else(|| {
            serde::de::Error::custom(format!(
                "max_tool_iterations must be <= {MAX_TOOL_ITERATIONS}, got {raw}"
            ))
        })
}

impl Frontmatter {
    /// Returns the prompt's identifier, read from the `name:` key.
    ///
    /// The name follows the grammar `[A-Za-z][A-Za-z0-9_-]{0,63}`, so it
    /// is a valid name for publishing the prompt as a tool.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the prompt's one-line summary.
    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }

    /// Returns the prompt's human-readable display name, read from the
    /// `title:` key, or `None` when the prompt leaves it out.
    ///
    /// The title is non-empty, unpadded text on one line, with no control
    /// characters. [`Frontmatter::name`] stays the identifier that events
    /// and errors use.
    #[must_use]
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Returns the Engine major version that the `runvil:` key declares,
    /// when present.
    #[must_use]
    pub fn runvil(&self) -> Option<u32> {
        self.runvil
    }

    /// Returns the declared cap on model round trips in each section's
    /// tool-call loop.
    ///
    /// Returns `None` when the prompt omits the cap, so the caller's default
    /// applies.
    #[must_use]
    pub fn max_tool_iterations(&self) -> Option<NonZeroU32> {
        self.max_tool_iterations
    }

    /// Returns the `uses:` entries, in declaration order.
    ///
    /// Every declared Pack is required, and no two entries share a Pack
    /// id.
    #[must_use]
    pub fn uses(&self) -> &[PackRef] {
        &self.uses
    }

    /// Returns the arguments the prompt declares.
    ///
    /// A prompt that omits the `args:` key yields the default
    /// [`ArgsSchema`].
    #[must_use]
    pub fn args(&self) -> &ArgsSchema {
        &self.args
    }

    /// Returns the requirements the prompt declares for `role`, or `None`
    /// when it does not name the role.
    #[must_use]
    pub fn role_requirements(&self, role: ModelRole) -> Option<&RoleRequirements> {
        self.models.get(role)
    }

    /// Returns the `prompts:` entries, in declaration order: the name of
    /// each prompt this one lists, as a
    /// [`PromptFinder`](crate::PromptFinder) finds it. The prompt's Lua
    /// may run each of them with `execute`, and no other prompt.
    ///
    /// No entry is empty, and no two entries are the same name.
    #[must_use]
    pub fn prompts(&self) -> &[String] {
        &self.prompts
    }

    /// Returns the behavior hints the `annotations:` key declares, each
    /// `None` unless the prompt wrote it.
    #[must_use]
    pub fn annotations(&self) -> &Annotations {
        &self.annotations
    }
}

/// A prompt file split at its frontmatter by [`split_frontmatter`].
#[derive(Debug)]
pub(crate) struct SplitSource {
    /// The YAML text between the two `---` delimiters.
    pub(crate) yaml: String,
    /// The markdown body after the closing `---` line.
    pub(crate) body: Arc<str>,
    /// The number of lines consumed by the frontmatter block (both `---`
    /// delimiters and everything between them).
    pub(crate) frontmatter_lines: u32,
    /// The file line of each top-level key that exactly one YAML line
    /// declares, by the key's name.
    pub(crate) key_lines: BTreeMap<String, NonZeroU32>,
}

/// Splits a file into its YAML frontmatter and its markdown body, and finds
/// the line of each top-level key.
///
/// The file must open with a `---` line and close the frontmatter with another
/// `---` line starting at column 0; an indented `---` is YAML block scalar
/// text. `str::lines` handles both `\n` and `\r\n`. Whitespace before or after
/// the dashes of the opening line is ignored.
///
/// A YAML line declares a key when it starts at column 0 with the key, plain
/// or in double or single quotes, then optional spaces or tabs, a colon, and
/// the end of the line, a space, or a tab. A plain key runs to the first such
/// colon and never starts with whitespace or a YAML indicator such as `#` or
/// `-`, so a comment, an indented line, block scalar text, and a sequence
/// entry declare none. A key two lines declare has no line.
///
/// # Errors
/// Returns a `FrontmatterInvalid` failure on line 1, the opening delimiter,
/// when the file does not open with `---` or the frontmatter is never closed.
pub(crate) fn split_frontmatter(input: &str) -> Result<SplitSource> {
    let input = input.strip_prefix('\u{feff}').unwrap_or(input); // drop BOM
    let mut lines = input.lines();
    match lines.next() {
        Some(l) if l.trim() == "---" => {}
        _ => {
            return Err(error::on_line(
                error::make_failure_error(
                    ErrorCode::FrontmatterInvalid,
                    "file must begin with a --- frontmatter delimiter",
                ),
                Some(NonZeroU32::MIN),
            ));
        }
    }
    let mut yaml = String::new();
    let mut closed = false;
    let mut line_count: u32 = 1; // opening ---
    let mut declared: BTreeMap<&str, Option<NonZeroU32>> = BTreeMap::new();
    for line in lines.by_ref() {
        line_count += 1;
        if line.trim_end() == "---" {
            closed = true;
            break;
        }
        if let Some(key) = declared_key(line) {
            declared
                .entry(key)
                .and_modify(|first| *first = None)
                .or_insert(NonZeroU32::new(line_count));
        }
        yaml.push_str(line);
        yaml.push('\n');
    }
    if !closed {
        return Err(error::on_line(
            error::make_failure_error(
                ErrorCode::FrontmatterInvalid,
                "frontmatter was not closed with ---",
            ),
            Some(NonZeroU32::MIN),
        ));
    }
    let body = lines.collect::<Vec<_>>().join("\n");
    Ok(SplitSource {
        yaml,
        body: Arc::from(body),
        frontmatter_lines: line_count,
        key_lines: declared
            .into_iter()
            .filter_map(|(key, line)| Some((key.to_owned(), line?)))
            .collect(),
    })
}

/// Returns the top-level key `line` declares, as [`split_frontmatter`] reads
/// a declaration.
fn declared_key(line: &str) -> Option<&str> {
    let ends = |rest: &str| rest.is_empty() || rest.starts_with([' ', '\t']);
    let first = line.chars().next()?;
    let (key, rest) = match first {
        '"' | '\'' => line[1..].split_once(first)?,
        _ if first.is_whitespace() || "#-?:,[]{}&*!|>%@`".contains(first) => return None,
        _ => {
            let (colon, _) = line
                .match_indices(':')
                .find(|&(at, _)| ends(&line[at + 1..]))?;
            (line[..colon].trim_end_matches([' ', '\t']), &line[colon..])
        }
    };
    let rest = rest.trim_start_matches([' ', '\t']).strip_prefix(':')?;
    ends(rest).then_some(key)
}
