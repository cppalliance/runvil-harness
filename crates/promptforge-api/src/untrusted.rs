//! Guard-wrapping for untrusted external data.
//!
//! The implementation lives in the `shared-promptforge-api` crate and is
//! re-exported here unchanged, so existing `promptforge_api::untrusted::*`
//! paths keep working.

pub(crate) use shared_promptforge_api::untrusted::GuardNonce;
