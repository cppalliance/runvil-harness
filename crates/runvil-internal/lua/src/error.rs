//! The crate's failures, each the universal [`Error`].
//!
//! A site builds its failure where it raises it, with the constructors
//! here, which the Engine shares for the failures it raises itself, so
//! each failure has one text. A failure raised through Lua crosses the
//! `mlua` boundary as a typed value, `mlua::Error::external(error)`, and
//! the boundary that reads it back downcasts to [`Error`]. A failure that
//! crosses into Lua is placed on the author's line of the prompt's file,
//! which the VM holds, and one the prompt's own Lua caused gets author
//! prose that names what failed.

use std::borrow::Cow;
use std::num::NonZeroU32;

use mlua::Lua;
use runvil_types::error::{ErrorCode, ErrorFact, SourceLoc, make_failure_error, place, with_fact};
use runvil_types::ids::TaskId;

pub(crate) use runvil_types::error::Error;

use crate::chunk_line::{STRING_CHUNK_OPENING, prompt_line};

/// An Engine quota on Lua that a block can exhaust.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaResource {
    /// The per-VM `log()` event budget.
    LogEvent,
    /// The cumulative `log()` byte budget.
    LogByte,
}

impl std::fmt::Display for QuotaResource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            QuotaResource::LogEvent => "log event",
            QuotaResource::LogByte => "log byte",
        })
    }
}

/// The file label of a VM that was given none: the label a prompt that
/// came from no file has.
pub(crate) const UNNAMED_FILE: &str = "prompt";

/// Where a VM's failures are placed, held as the VM's app data.
struct PromptFile {
    /// The file the prompt the VM runs came from.
    file: String,
    /// The author's line at the running block's latest yield.
    call_line: Option<NonZeroU32>,
}

/// A condition the prompt author caused and can fix in Lua, such as an
/// empty `messages` list or a path that names no section, as a
/// `LuaRuntime` failure. The message is the specific failure as a noun
/// phrase, with no `lua error:` label, since a failed run classifies it as
/// a Lua failure.
#[must_use]
pub fn make_lua_runtime_error(message: impl Into<String>) -> Error {
    make_failure_error(ErrorCode::LuaRuntime, message)
}

/// `compactors.fail` exhausted the model's context window, with `reason`:
/// `precheck` or `provider` after an overflow, else the caller's own. The
/// `Reason` fact is `reason`.
#[must_use]
pub fn make_context_exhausted_error(reason: &str) -> Error {
    let text = format!(
        "context exhausted: {}",
        crate::compactors::exhaustion_detail(reason)
    );
    with_fact(
        make_failure_error(ErrorCode::ContextExhausted, text),
        ErrorFact::Reason,
        reason,
    )
}

/// An exhausted Lua quota on `resource`, whose tag (`log_event` or
/// `log_byte`) is the `Reason` fact.
#[must_use]
pub fn make_lua_quota_error(resource: QuotaResource) -> Error {
    let tag = match resource {
        QuotaResource::LogEvent => "log_event",
        QuotaResource::LogByte => "log_byte",
    };
    with_fact(
        make_failure_error(
            ErrorCode::LuaQuota,
            format!("lua {resource} quota exceeded"),
        ),
        ErrorFact::Reason,
        tag,
    )
}

/// A task operation that named `task`, which the caller does not own. An
/// id that names no task at all is refused the same way, so a caller
/// learns nothing about tasks it never started.
#[must_use]
pub fn make_task_not_owned_error(task: &TaskId) -> Error {
    make_task_failure_error(
        ErrorCode::TaskNotOwned,
        format!("task `{task}` is not a task this chain owns"),
        task,
    )
}

/// A wait that named `task`, whose result was already delivered once.
#[must_use]
pub fn make_task_consumed_error(task: &TaskId) -> Error {
    make_task_failure_error(
        ErrorCode::TaskConsumed,
        format!("task `{task}` was already delivered: a task's result is taken by one wait"),
        task,
    )
}

/// The failure a wait delivers for `task`, which its owner canceled
/// instead of letting it end on its own.
#[must_use]
pub fn make_task_canceled_error(task: &TaskId) -> Error {
    make_task_failure_error(
        ErrorCode::TaskCanceled,
        format!("task `{task}` was canceled"),
        task,
    )
}

/// A failure with `code` and `message` about `task`, its `Task` fact.
fn make_task_failure_error(code: ErrorCode, message: String, task: &TaskId) -> Error {
    with_fact(
        make_failure_error(code, message),
        ErrorFact::Task,
        task.to_string(),
    )
}

/// The tool-call loop ran its iteration cap without a final text reply.
#[must_use]
pub fn make_tool_loop_exhausted_error() -> Error {
    make_failure_error(
        ErrorCode::ToolLoopExhausted,
        "tool-call loop did not converge",
    )
}

/// An internal invariant failure constructed at `site`: a state the
/// surrounding code has already guaranteed cannot occur, surfaced rather
/// than skipped. Its location is that Rust source position, which a failed
/// run's message ends with. The message is owned when it names a value.
#[must_use]
pub fn make_internal_at_error(
    message: impl Into<Cow<'static, str>>,
    site: &'static std::panic::Location<'static>,
) -> Error {
    let text = format!("internal invariant violated: {}", message.into());
    let mut error = make_failure_error(ErrorCode::Internal, text);
    error.at = NonZeroU32::new(site.line()).map(|line| SourceLoc::line(site.file(), line));
    error
}

/// Lua source at the prompt region `region`, which starts at prompt line
/// `source_line`, that did not compile, with the compiler's `diagnostic`.
/// The `Section` fact names the region; the compile path gives it its
/// location and prose.
pub(crate) fn make_lua_compile_error(region: &str, source_line: u32, diagnostic: &str) -> Error {
    with_fact(
        make_failure_error(
            ErrorCode::LuaCompile,
            format!("lua compilation error at {region} (line {source_line}): {diagnostic}"),
        ),
        ErrorFact::Section,
        region,
    )
}

/// A dispatched tool's own failure, `error`, under a message that names
/// the tool call and its model-safe prose; the prose, location, and facts
/// stay the tool's. A code outside the tool codes is `ToolOther`, so a
/// tool's failure always classifies as one.
pub(crate) fn tool(mut error: Error) -> Error {
    if !matches!(
        error.code,
        ErrorCode::ToolInvalidArguments
            | ErrorCode::ToolBackend
            | ErrorCode::ToolTransport
            | ErrorCode::ToolOther
    ) {
        error.code = ErrorCode::ToolOther;
    }
    error.message = format!("tool call failure: {}", error.prose);
    error
}

/// The `LuaRuntime` failure an `mlua` failure is, with the `mlua` error's
/// text as its message.
#[expect(
    clippy::needless_pass_by_value,
    reason = "the adapter every VM call's `map_err` passes its error to"
)]
pub(crate) fn make_lua_error(source: mlua::Error) -> Error {
    make_lua_runtime_error(lua_text(&source))
}

/// An internal invariant failure, at the call site as its construction
/// site. The message is owned when it names a value.
#[must_use]
#[track_caller]
pub fn make_internal_error(message: impl Into<Cow<'static, str>>) -> Error {
    make_internal_at_error(message, std::panic::Location::caller())
}

/// An author error raised across the Lua boundary: a `LuaRuntime`
/// failure with `message`, which `pcall` reports as kind `lua`.
pub(crate) fn author_refusal(message: String) -> mlua::Error {
    mlua::Error::external(make_lua_runtime_error(message))
}

/// Strips `mlua`'s wrapping layers (the callback frame and any context)
/// down to the error a callback returned.
pub(crate) fn root_cause(error: &mlua::Error) -> &mlua::Error {
    match error {
        mlua::Error::CallbackError { cause, .. } | mlua::Error::WithContext { cause, .. } => {
            root_cause(cause)
        }
        other => other,
    }
}

/// The text `mlua` renders `error` as, with [`without_address`] applied.
/// Every text of an `mlua` failure that leaves this crate comes from here
/// or from [`root_text`].
pub(crate) fn lua_text(error: &mlua::Error) -> String {
    rendered(error, cfg!(windows))
}

/// The text of the failure at the root of `error`, without `mlua`'s label
/// for its kind: what Lua raised, or the message of the universal failure
/// a callback raised. [`without_address`] applies to what Lua raised.
pub(crate) fn root_text(error: &mlua::Error) -> String {
    match root_cause(error) {
        mlua::Error::RuntimeError(message) => without_address(message).into_owned(),
        mlua::Error::SyntaxError { message, .. } => message.clone(),
        other => other
            .downcast_ref::<Error>()
            .map_or_else(|| other.to_string(), |typed| typed.message.clone()),
    }
}

/// `text`, a failure as Lua rendered it, with the address of a raised
/// value taken out. The text has no label of `mlua` in front: the start of
/// `text` is the start of what Lua raised, so a `__name` that reads
/// `runtime error: Thing` is a name with a `: ` in it. [`lua_text`] is the
/// one for a text that `mlua` rendered.
///
/// When Rust runs the author's Lua (a `__pairs` or `__len` metamethod, a
/// callback) and that Lua raises a value that is not a string, `mlua` turns
/// the value into text before any code of this crate sees it. It does so
/// with Lua's own `luaL_tolstring`, which renders a table, a function, a
/// userdata, or a thread as its type name, or the `__name` of its
/// metatable, then `: ` and its address: `table: 0x55d0c0a1e2f0`. The value
/// is gone by then and `mlua` offers no hook, so the text is all there is
/// to go by. The address differs from one run to the next.
///
/// The address is the last thing Lua writes, so it ends the text or stands
/// right before a traceback. A `__name` is any string, one that holds the
/// heading of a traceback included, so the text before each heading in
/// `text` is looked at, and the end of `text`, and every address found
/// there goes. Where the text up to the first address is a plain name, not
/// empty and with no `: ` and no line break in it, it reads
/// `error object is a <name> value`, the words stock Lua has for an error
/// that is not a string. Any other address is replaced by `(address)` and
/// the text before it stays.
///
/// The rule reads a text, so it cannot tell such a value from a string
/// that ends the same way, and it changes both: no address stays, at the
/// cost of a string that only looks like one. On Linux and macOS
/// `error('size: 0x1f', 0)` reads `error object is a size value`, and
/// `error('bad byte: 0x1f')`, which has its position in front, ends in
/// `bad byte: (address)`. On Windows both stay, and a string that ends in
/// the shape [`is_address`] takes there changes the same way. A number
/// with more hex digits than a pointer has is no address and stays.
pub(crate) fn without_address(text: &str) -> Cow<'_, str> {
    reworded(text, cfg!(windows)).map_or(Cow::Borrowed(text), Cow::Owned)
}

/// Records `file` as the file the prompt this VM runs came from, which
/// its failures are placed in.
pub(crate) fn set_prompt_file(lua: &Lua, file: &str) {
    if let Some(mut held) = lua.app_data_mut::<PromptFile>() {
        file.clone_into(&mut held.file);
        return;
    }
    lua.set_app_data(PromptFile {
        file: file.to_owned(),
        call_line: None,
    });
}

/// The file the prompt this VM runs came from, or `prompt` when the VM
/// was given none.
pub(crate) fn prompt_file(lua: &Lua) -> String {
    lua.app_data_ref::<PromptFile>()
        .map_or_else(|| UNNAMED_FILE.to_owned(), |held| held.file.clone())
}

/// Records `line` as this VM's call line, `None` included, so the line of
/// an earlier yield never outlives the one it belongs to.
pub(crate) fn set_call_line(lua: &Lua, line: Option<NonZeroU32>) {
    if let Some(mut held) = lua.app_data_mut::<PromptFile>() {
        held.call_line = line;
        return;
    }
    lua.set_app_data(PromptFile {
        file: UNNAMED_FILE.to_owned(),
        call_line: line,
    });
}

/// The author's line at the latest yield of the block this VM runs, which
/// holds while the block is suspended on that yield.
pub(crate) fn call_line(lua: &Lua) -> Option<NonZeroU32> {
    lua.app_data_ref::<PromptFile>()
        .and_then(|held| held.call_line)
}

/// The prompt line of the innermost frame on the running Lua stack whose
/// chunk is a prompt block, `line N`, skipping every other frame: a
/// shim's, a Pack prelude's, a Rust callback's. `None` when no such frame
/// runs, as when the caller renders an answer outside any Lua call.
pub(crate) fn author_line(lua: &Lua) -> Option<NonZeroU32> {
    (0..)
        .map_while(|level| {
            lua.inspect_stack(level, |frame| {
                let chunk_name = frame.source().source?;
                let line = u32::try_from(frame.current_line()?).ok()?;
                prompt_line(&chunk_name, line)
            })
        })
        .flatten()
        .next()
}

/// Whether a failure with `code` is one the prompt's Lua causes as it
/// runs, whose prose names what failed: a Lua failure, an exhausted quota,
/// the call-depth cap, a failed substitution, or a misused task.
fn is_authored(code: ErrorCode) -> bool {
    matches!(
        code,
        ErrorCode::LuaRuntime
            | ErrorCode::LuaCompile
            | ErrorCode::LuaQuota
            | ErrorCode::CallDepthExceeded
            | ErrorCode::Substitution
            | ErrorCode::TasksLive
            | ErrorCode::TaskNotOwned
            | ErrorCode::TaskConsumed
    )
}

/// The author prose of a failure with `code`: `{what}. {default}`, where
/// `what` is what failed and `default` the code's prose.
pub(crate) fn author_prose(what: &str, code: ErrorCode) -> String {
    format!("{what}. {}", code.prose())
}

/// Places `error` on `line` of `file` as [`place`] does. When the prompt's
/// Lua caused the failure and its prose is still its code's default, the
/// prose becomes the author prose for `what`.
pub(crate) fn authored_at(error: Error, file: &str, line: Option<NonZeroU32>, what: &str) -> Error {
    let mut error = place(error, file, line);
    if is_authored(error.code) && error.prose == error.code.prose() {
        error.prose = author_prose(what, error.code);
    }
    error
}

/// `error` as it crosses into this VM's Lua: placed in the VM's file on
/// the line of the innermost author frame running, or else on the call
/// line, with the author prose of its message's own text when the
/// prompt's Lua caused it.
pub(crate) fn authored(lua: &Lua, error: Error) -> Error {
    let line = author_line(lua).or_else(|| call_line(lua));
    let what = failure_text(&error.message).to_owned();
    authored_at(error, &prompt_file(lua), line, &what)
}

/// The heading Lua puts on its own line between a failure's text and its
/// traceback.
pub(crate) const TRACEBACK_HEADING: &str = "\nstack traceback:";

/// The label `mlua` puts in front of the text of a failure Lua raised.
const RUNTIME_LABEL: &str = "runtime error: ";

/// The text `mlua` renders `error` as, with each address [`reworded`]
/// finds taken out, in the shape `msvc` selects. Where `mlua` leads the
/// text with [`RUNTIME_LABEL`], the label stays and the rule reads what
/// follows it, which is what Lua raised.
fn rendered(error: &mlua::Error, msvc: bool) -> String {
    let text = error.to_string();
    let (label, raised) = match text.strip_prefix(RUNTIME_LABEL) {
        Some(raised) if leads_with_label(error) => (RUNTIME_LABEL, raised),
        _ => return reworded(&text, msvc).unwrap_or(text),
    };
    match reworded(raised, msvc) {
        Some(reworded) => format!("{label}{reworded}"),
        None => format!("{label}{raised}"),
    }
}

/// Whether the text `mlua` renders `error` as starts with the
/// [`RUNTIME_LABEL`] that `mlua` wrote: `error` is a failure Lua raised, or
/// a callback's failure whose cause is one, which `mlua` renders as the
/// cause and then the traceback. The same words at the start of any other
/// text are the author's.
fn leads_with_label(error: &mlua::Error) -> bool {
    match error {
        mlua::Error::RuntimeError(_) => true,
        mlua::Error::CallbackError { cause, .. } => leads_with_label(cause),
        _ => false,
    }
}

/// What [`without_address`] puts in the place of an address whose value
/// it cannot name.
const ADDRESS_STAND_IN: &str = "(address)";

/// The most hex digits a pointer of this platform prints as.
const ADDRESS_DIGITS: usize = 2 * size_of::<usize>();

/// `text` with each address [`without_address`] finds taken out, in the
/// shape `msvc` selects, or `None` when it finds none.
fn reworded(text: &str, msvc: bool) -> Option<String> {
    let mut out = String::new();
    let mut changed = false;
    let mut start = 0;
    let ends = text
        .match_indices(TRACEBACK_HEADING)
        .map(|(at, _)| at)
        .chain([text.len()]);
    for end in ends {
        let part = &text[start..end];
        match part
            .rsplit_once(": ")
            .filter(|(_, address)| is_address(address, msvc))
        {
            Some((front, _)) => {
                changed = true;
                let plain = !front.is_empty() && !front.contains(": ") && !front.contains('\n');
                if start == 0 && plain {
                    out.push_str("error object is a ");
                    out.push_str(front);
                    out.push_str(" value");
                } else {
                    out.push_str(front);
                    out.push_str(": ");
                    out.push_str(ADDRESS_STAND_IN);
                }
            }
            None => out.push_str(part),
        }
        start = end;
    }
    changed.then_some(out)
}

/// Whether `text` is an address as C's `%p` prints it, which Lua leaves to
/// the platform: `0x` and lowercase hex digits on Linux and macOS, and with
/// `msvc` (the C runtime every Windows target links) uppercase hex digits
/// padded to the pointer's width, with nothing in front. A pointer has
/// [`ADDRESS_DIGITS`] hex digits at most, so a longer number is no address
/// and stays.
fn is_address(text: &str, msvc: bool) -> bool {
    if msvc {
        return text.len() == ADDRESS_DIGITS
            && text
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'A'..=b'F').contains(&byte));
    }
    text.strip_prefix("0x").is_some_and(|digits| {
        (1..=ADDRESS_DIGITS).contains(&digits.len())
            && digits
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

/// The failure's own text in `message`: the text before the traceback,
/// without the `[string "..."]:N: ` positions Lua puts in front of it.
pub(crate) fn failure_text(message: &str) -> &str {
    let mut text = message
        .find(TRACEBACK_HEADING)
        .map_or(message, |at| &message[..at]);
    while let Some(rest) = text.strip_prefix(STRING_CHUNK_OPENING) {
        let Some(close) = rest.find("\"]:") else {
            break;
        };
        let after = &rest[close + 3..];
        let digits = after
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(after.len());
        match after[digits..].strip_prefix(": ") {
            Some(tail) if digits > 0 => text = tail,
            _ => break,
        }
    }
    text.trim_end()
}

/// Crate-internal result alias over [`Error`].
pub(crate) type Result<T> = std::result::Result<T, Error>;

/// Renders `tags` as the accepted-value list a parse error names: each tag
/// in backticks, joined by commas.
pub(crate) fn accepted_tags(tags: impl IntoIterator<Item = &'static str>) -> String {
    tags.into_iter()
        .map(|tag| format!("`{tag}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
#[path = "error-tests.rs"]
mod tests;
