//! Cooperative cancellation for long-running execute paths.
//!
//! The implementation lives in the `shared-promptforge-api` crate and is
//! re-exported here unchanged, so existing `promptforge_api::cancel::*` paths
//! keep working.

pub(crate) use shared_promptforge_api::cancel::{
    CancelHandle, current, is_cancelled, maybe_scope, wait_cancelled,
};
