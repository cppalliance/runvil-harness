//! The store: [`Store`], an empty set of files in memory under a claims
//! ledger of its own, and a view of one, with its strict logical-path
//! rules and the re-spelling of error paths into the caller's logical
//! form.

use std::fmt;
use std::sync::{Arc, Mutex};

use super::forward::StoreScoped;
use super::{Access, VfsRef};
use crate::error::{PathReason, VfsError};
use crate::memory::MemoryBackend;
use crate::traits::AcquireContext;

/// Re-spells a namespace-absolute result relative to `/`, by dropping
/// its leading `/`, or `None` when the result has none (never, for a
/// canonical path).
pub(super) fn strip_root(matched: &str) -> Option<String> {
    matched.strip_prefix('/').map(str::to_owned)
}

/// The largest logical store path, in bytes, accepted by the store
/// view, so a store path stays a bounded denial-of-service lever.
const MAX_STORE_PATH_BYTES: usize = 1024;

/// The store's strict logical-path rules, applied by the store view
/// before a path reaches canonicalization, in the store contract's
/// check order: the first rule broken is the one reported, with the
/// path exactly as supplied.
pub(super) fn validate_store_path(raw: &str) -> Result<(), VfsError> {
    let reject = |reason| {
        Err(VfsError::InvalidPath {
            path: raw.to_owned(),
            reason,
        })
    };
    if raw.is_empty() {
        return reject(PathReason::Empty);
    }
    if raw.len() > MAX_STORE_PATH_BYTES {
        return reject(PathReason::TooLong);
    }
    if raw.starts_with('/') {
        return reject(PathReason::Absolute);
    }
    if raw.bytes().any(|b| b < 0x20 || b == 0x7f) {
        return reject(PathReason::Control);
    }
    // A backslash is a separator on some backends and a literal on
    // others; refuse it so a canonical `/`-separated path cannot be
    // reinterpreted.
    if raw.contains('\\') {
        return reject(PathReason::Backslash);
    }
    // A colon names a drive or an alternate data stream on Windows, so
    // the stored file would not be the one named.
    if raw.contains(':') {
        return reject(PathReason::ReservedName);
    }
    for segment in raw.split('/') {
        if segment.is_empty() {
            return reject(PathReason::EmptySegment);
        }
        if segment == "." || segment == ".." {
            return reject(PathReason::Traversal);
        }
        // Trailing `.`/space are stripped by some backends, so the
        // stored name would not round-trip.
        if segment.ends_with('.') || segment.ends_with(' ') {
            return reject(PathReason::UnsafeSuffix);
        }
        if is_reserved_device_name(segment) {
            return reject(PathReason::ReservedName);
        }
    }
    Ok(())
}

/// Whether `segment` is a platform-reserved device name. Windows
/// treats names like `CON`, `NUL`, `COM1`, and `LPT1` as devices even
/// with an extension (`con.txt`), so the base name before the first
/// `.` is checked case-insensitively.
fn is_reserved_device_name(segment: &str) -> bool {
    let base = segment.split('.').next().unwrap_or(segment);
    let upper = base.to_ascii_uppercase();
    matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || is_numbered_device(&upper, "COM")
        || is_numbered_device(&upper, "LPT")
}

/// Whether `name` is `<prefix>N` for one device digit: `0` through `9`,
/// or the superscript `¹`, `²`, or `³`, which Windows also reserves.
fn is_numbered_device(name: &str, prefix: &str) -> bool {
    name.strip_prefix(prefix).is_some_and(|rest| {
        let mut digits = rest.chars();
        matches!(
            (digits.next(), digits.next()),
            (Some('0'..='9' | '\u{b9}' | '\u{b2}' | '\u{b3}'), None)
        )
    })
}

/// Re-spells an error's canonical paths for a store view's caller: a
/// path without its leading `/` is the logical form the caller
/// supplied. A logical path is never absolute, so one already in that
/// form passes unchanged.
pub(super) fn relativize_error(err: VfsError) -> VfsError {
    respell_error(err, |path| {
        strip_root(path).unwrap_or_else(|| path.to_owned())
    })
}

/// Re-spells every path in `err` through `logical`; prose fields that
/// embed the path (a conflict's diagnosis, a denial's reason) follow.
fn respell_error(err: VfsError, logical: impl Fn(&str) -> String) -> VfsError {
    match err {
        VfsError::NotFound { path } => VfsError::NotFound {
            path: logical(&path),
        },
        VfsError::AlreadyExists { path } => VfsError::AlreadyExists {
            path: logical(&path),
        },
        VfsError::NotADirectory { path } => VfsError::NotADirectory {
            path: logical(&path),
        },
        VfsError::IsADirectory { path } => VfsError::IsADirectory {
            path: logical(&path),
        },
        VfsError::DirectoryNotEmpty { path } => VfsError::DirectoryNotEmpty {
            path: logical(&path),
        },
        VfsError::NotUtf8 { path, valid_up_to } => VfsError::NotUtf8 {
            path: logical(&path),
            valid_up_to,
        },
        VfsError::InvalidPath { path, reason } => VfsError::InvalidPath {
            path: logical(&path),
            reason,
        },
        VfsError::InvalidRange { path, reason } => VfsError::InvalidRange {
            path: logical(&path),
            reason,
        },
        VfsError::Anchor {
            path,
            anchor,
            count,
        } => VfsError::Anchor {
            path: logical(&path),
            anchor,
            count,
        },
        VfsError::PermissionDenied { path, reason } => VfsError::PermissionDenied {
            reason: reason.replace(&path, &logical(&path)),
            path: logical(&path),
        },
        VfsError::ApprovalRequired { path, reason } => VfsError::ApprovalRequired {
            reason: reason.replace(&path, &logical(&path)),
            path: logical(&path),
        },
        VfsError::ScopeEnded { path } => VfsError::ScopeEnded {
            path: logical(&path),
        },
        VfsError::ReadOnly { path } => VfsError::ReadOnly {
            path: logical(&path),
        },
        VfsError::OutsideRoot { path, detail } => VfsError::OutsideRoot {
            detail: detail.replace(&path, &logical(&path)),
            path: logical(&path),
        },
        VfsError::RootProtected { path } => VfsError::RootProtected {
            path: logical(&path),
        },
        VfsError::CrossMount { path, detail } => VfsError::CrossMount {
            detail: detail.replace(&path, &logical(&path)),
            path: logical(&path),
        },
        VfsError::Io {
            path,
            kind,
            raw_os_error,
            message,
        } => VfsError::Io {
            path: logical(&path),
            kind,
            raw_os_error,
            message,
        },
        VfsError::Unsupported { path, detail } => VfsError::Unsupported {
            detail: detail.replace(&path, &logical(&path)),
            path: logical(&path),
        },
        VfsError::Conflict { path, detail } => VfsError::Conflict {
            detail: detail.replace(&path, &logical(&path)),
            path: logical(&path),
        },
        VfsError::Backend { message } => VfsError::Backend { message },
    }
}

/// A store: an empty set of files in memory, under a claims ledger of its
/// own.
///
/// A store sits outside every [`VfsRef`]. No path of a handle reaches a
/// store's files, and a view of a store reaches nothing else. Each store
/// has its own files and its own claims, so two stores share no path and
/// no claim, even when one identity views both. A clone is the same
/// store.
///
/// A store has no policy and no operation observer: every operation that
/// the claims rules admit runs.
#[derive(Clone)]
pub struct Store(VfsRef);

impl fmt::Debug for Store {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Store").finish_non_exhaustive()
    }
}

impl Default for Store {
    fn default() -> Store {
        Store::new()
    }
}

impl Store {
    /// Returns a new, empty store.
    #[must_use]
    pub fn new() -> Store {
        Store(VfsRef::builder().mount("/", MemoryBackend::new()).build())
    }

    /// Returns a view of this store under `access`'s identity and scope.
    ///
    /// The view is an [`Access`] whose operations reach this store alone.
    /// It holds `access`'s identity, so the forks, joins, and ends of
    /// `access`'s scope order the store's operations as they order the
    /// operations of `access` itself, and one identity never conflicts
    /// with itself through its views. The view records its claims in this
    /// store's ledger, never in the ledger of the handle `access` came
    /// from.
    ///
    /// Paths are relative to the store and follow strict rules. A path
    /// must be 1 to 1024 bytes long and free of control characters,
    /// backslashes, and colons. No segment may be empty, `.`, or `..`,
    /// end in a dot or a space, or be a reserved device name such as
    /// `CON` or `COM0`. Error paths come back in the form the caller
    /// supplied.
    ///
    /// # Errors
    /// Returns [`VfsError::ScopeEnded`] when `access`'s scope has ended,
    /// or when the tool call that owned `access` has.
    pub fn view(&self, access: &Access) -> Result<Access, VfsError> {
        let cx = AcquireContext::new(access.id, Arc::clone(&access.scope));
        // The view holds one more reference to the identity, like a mount
        // forward: the identity, and its scope, ends with its last access,
        // the view included.
        let scope = self.0.join_scope(&cx)?;
        let inner = match self.0.backend().acquire(&cx) {
            Ok(inner) => inner,
            Err(err) => {
                scope.release(cx.id());
                return Err(err);
            }
        };
        Ok(Access {
            id: access.id,
            origin: access.origin.clone(),
            store_view: true,
            volume: Arc::clone(&self.0.volume),
            policy: Arc::clone(&self.0.policy),
            scope,
            inner: Mutex::new(Box::new(StoreScoped { inner })),
            released: false,
        })
    }
}
