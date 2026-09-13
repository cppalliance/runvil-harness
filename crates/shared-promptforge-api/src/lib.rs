//! Small shared host-support primitives for the PromptForge runtime.
//!
//! [`untrusted`] wraps untrusted external data in a nonce-guarded envelope,
//! [`cancel`] is the cooperative cancellation handle and task-local scope a
//! run observes, [`observe`] is the report-only vocabulary a run reports its
//! progress through, and [`events`] is the canonical metrics and
//! runtime-event vocabulary with the read-side
//! [`EventLog`](events::EventLog) a host may supply as a run input.
//! [`models`] is the host-facing model vocabulary (identity, catalog,
//! descriptor) and [`wire`] the streaming delta a host's `on_delta`
//! callback observes. This
//! crate depends on no other promptforge crate, so every promptforge crate
//! may depend on it.

pub mod cancel;
pub mod events;
pub mod models;
pub mod observe;
pub mod untrusted;
pub mod wire;
