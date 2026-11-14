//! Store operations: the body of each `store.*` operation over a store
//! view, the line bounds of the two reads, and the model-facing message of
//! each store failure.

use std::fmt::Write as _;

use runvil_vfs::{Access, VfsError};

use crate::error::Error;
use crate::error_value::make_store_error;
use crate::protocol::{VfsOp, VfsOutcome};

/// The store failure `cause` with `message` in place of its own text.
#[must_use]
pub fn make_store_failure_error(cause: &VfsError, message: String) -> Error {
    let mut error = make_store_error(cause);
    error.message = message;
    error
}

/// Whether a store read prefixes each line with its absolute line number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineNumbers {
    /// The file's text verbatim.
    Omitted,
    /// Each line right-aligned behind its number and `"| "`.
    Prefixed,
}

/// The model-facing message for a failed store operation: what failed and,
/// when there is a fix, how to make it, so the text a model reads names
/// both. The operation supplies the wording its call surface promises (a
/// glob reports "invalid glob pattern", a path reports "invalid path"), and
/// the structured [`VfsError`] supplies the path, rule, anchor, and count.
#[must_use]
pub fn store_error_message(op: &VfsOp, error: &VfsError) -> String {
    match error {
        VfsError::NotFound { path } => format!("file not found in store: {path}"),
        VfsError::AlreadyExists { path } => format!("file already exists in store: {path}"),
        VfsError::NotADirectory { path } => format!("not a directory in store: {path}"),
        VfsError::IsADirectory { path } => format!("is a directory in store: {path}"),
        VfsError::DirectoryNotEmpty { path } => format!("directory not empty in store: {path}"),
        VfsError::NotUtf8 { path, valid_up_to } => {
            format!("file in store is not UTF-8 after byte {valid_up_to}: {path}")
        }
        VfsError::InvalidPath { path, reason } => {
            if matches!(op, VfsOp::Glob { .. }) {
                format!("invalid glob pattern {path:?}: {reason}")
            } else {
                format!("invalid path {path:?}: {reason}")
            }
        }
        VfsError::InvalidRange { path, reason } => {
            format!("invalid line range for {path}: {reason}")
        }
        VfsError::Anchor {
            path,
            anchor,
            count,
        } => {
            if anchor.is_empty() {
                format!("str_replace requires a non-empty anchor: {path}")
            } else if *count == 0 {
                format!("anchor {anchor:?} was not found in {path}, expected exactly one")
            } else {
                format!(
                    "anchor {anchor:?} occurs {count} times in {path}, expected exactly one; \
                     include more surrounding text so it matches once"
                )
            }
        }
        VfsError::PermissionDenied { path, reason } => {
            format!("permission denied for store path {path}: {reason}")
        }
        VfsError::ApprovalRequired { path, reason } => {
            format!("store path {path} needs approval, which this run cannot give: {reason}")
        }
        VfsError::ScopeEnded { path } => format!(
            "store access ended: the run or tool call that owned it has ended, so {path} \
             can no longer be touched through it"
        ),
        VfsError::ReadOnly { path } => format!("store path is read-only: {path}"),
        VfsError::OutsideRoot { path, detail } => {
            format!("store path {path} is outside the mounted root: {detail}")
        }
        VfsError::RootProtected { path } => {
            format!("the store's mounted root cannot be removed, renamed, or replaced: {path}")
        }
        VfsError::CrossMount { path, detail } => {
            format!("store operation on {path} spans two mounts: {detail}")
        }
        VfsError::Io { path, message, .. } => format!("store I/O failure on {path}: {message}"),
        VfsError::Unsupported { path, detail } => {
            format!("unsupported store operation on {path}: {detail}")
        }
        VfsError::Conflict { path, detail } => format!("store conflict on {path}: {detail}"),
        VfsError::Backend { message } => format!("store backend failure: {message}"),
        _ => "store operation failed".to_owned(),
    }
}

/// Executes one validated store operation against the store view `view`:
/// what the request a `store.*` call yields comes to once it is performed.
///
/// # Errors
/// Returns the [`VfsError`] the operation produces: path or pattern
/// validation, not-found, anchor, range, or conflict.
pub fn run_store_op(view: &Access, op: VfsOp) -> std::result::Result<VfsOutcome, VfsError> {
    match op {
        VfsOp::Write { path, contents } => view
            .write(&path, contents.as_bytes())
            .map(|()| VfsOutcome::Unit),
        VfsOp::Append { path, contents } => view
            .append(&path, contents.as_bytes())
            .map(|()| VfsOutcome::Unit),
        VfsOp::Read { path, start, end } => {
            read_store(view, &path, start, end).map(VfsOutcome::Text)
        }
        VfsOp::ReadNumbered { path, start, end } => {
            read_store_numbered(view, &path, start, end).map(VfsOutcome::Text)
        }
        VfsOp::StrReplace { path, old, new } => view
            .str_replace(&path, &old, &new)
            .map(|()| VfsOutcome::Unit),
        VfsOp::Delete { path } => view.remove(&path, false).map(|_| VfsOutcome::Unit),
        VfsOp::Glob { pattern } => view.glob(&pattern).map(VfsOutcome::Paths),
        VfsOp::Exists { path } => view.exists(&path).map(VfsOutcome::Bool),
    }
}

/// Reads the file at `path` under the optional line bounds `store.read`
/// and `store.read_numbered` share, numbering lines as `numbers` says.
///
/// No `start` reads the whole file; a present `start` slices a 1-based
/// inclusive line range. A negative bound converts to 0, which the range
/// validation rejects with the same error a zero bound produces, and an
/// `end` without a `start` is refused rather than silently ignored.
fn read_store_bounded(
    view: &Access,
    path: &str,
    start: Option<i64>,
    end: Option<i64>,
    numbers: LineNumbers,
) -> std::result::Result<String, VfsError> {
    let render: fn(&[&str], usize) -> String = match numbers {
        LineNumbers::Prefixed => number_lines_from,
        LineNumbers::Omitted => |lines, _| lines.join("\n"),
    };
    match start {
        None if end.is_some() => Err(VfsError::InvalidRange {
            path: path.to_owned(),
            reason: "start is required when end is given",
        }),
        None if numbers == LineNumbers::Omitted => view.read_string(path),
        None => with_read_range(view, path, 1, None, render),
        Some(start) => {
            let start = usize::try_from(start).unwrap_or(0);
            let end = end.map(|line| usize::try_from(line).unwrap_or(0));
            with_read_range(view, path, start, end, render)
        }
    }
}

/// Runs the `store.read` operation.
fn read_store(
    view: &Access,
    path: &str,
    start: Option<i64>,
    end: Option<i64>,
) -> std::result::Result<String, VfsError> {
    read_store_bounded(view, path, start, end, LineNumbers::Omitted)
}

/// Runs the `store.read_numbered` operation.
fn read_store_numbered(
    view: &Access,
    path: &str,
    start: Option<i64>,
    end: Option<i64>,
) -> std::result::Result<String, VfsError> {
    read_store_bounded(view, path, start, end, LineNumbers::Prefixed)
}

/// Reads and resolves one line range while its owned contents remain live.
fn with_read_range(
    view: &Access,
    path: &str,
    start: usize,
    end: Option<usize>,
    render: impl FnOnce(&[&str], usize) -> String,
) -> std::result::Result<String, VfsError> {
    let contents = view.read_string(path)?;
    let lines: Vec<&str> = contents.lines().collect();
    let Some((start, end)) = resolve_line_range(path, lines.len(), start, end)? else {
        return Ok(String::new());
    };
    Ok(render(&lines[start - 1..end], start))
}

/// Resolves 1-based inclusive bounds against `line_count` into the
/// effective `(start, end)`, or `None` when the range falls entirely past
/// the last line. Evaluation order is fixed: a `start` below 1 is an
/// error; a `start` past the last line reads as empty; an omitted `end`
/// means the last line, and a given `end` clamps down to it; an `end`
/// before `start` at that point is an error.
fn resolve_line_range(
    path: &str,
    line_count: usize,
    start: usize,
    end: Option<usize>,
) -> std::result::Result<Option<(usize, usize)>, VfsError> {
    if start == 0 {
        return Err(VfsError::InvalidRange {
            path: path.to_owned(),
            reason: "start must be at least 1",
        });
    }
    if start > line_count {
        return Ok(None);
    }
    let end = end.unwrap_or(line_count).min(line_count);
    if end < start {
        return Err(VfsError::InvalidRange {
            path: path.to_owned(),
            reason: "end must not be before start",
        });
    }
    Ok(Some((start, end)))
}

/// Renders `lines` numbered absolutely from `start`, each number
/// right-aligned to the width of the largest emitted number, followed by
/// `"| "`; lines are joined with `"\n"` and there is no trailing newline.
fn number_lines_from(lines: &[&str], start: usize) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let last = start + lines.len() - 1;
    let width = last.to_string().len();
    let mut out = String::new();
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        let number = start + index;
        // Writing to a String is infallible, so the result is discarded.
        let _ = write!(out, "{number:>width$}| {line}");
    }
    out
}
