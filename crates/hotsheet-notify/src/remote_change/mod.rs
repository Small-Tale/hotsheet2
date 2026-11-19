//! Inbound remote-change hints (HS2-8X2S08, `docs/37-remote-change-notifications.md`): an
//! optional way for an external event service to say "a Git remote may have advanced", so a
//! Hot Sheet server can check it now instead of waiting for its next scheduled sync.
//!
//! Git stays authoritative. A hint is untrusted: it is validated against the source it is
//! bound to, deduplicated, coalesced, and only ever asks a consumer to *check*. The adapter
//! contract is [`model::ChangeAdapter`]; connections and bindings are [`registry`]; routing
//! and running them is [`hub`]; the idempotent headless workflow the CLI and server share is
//! [`workflow`]; the first adapter is [`ably`] (HS2-BWJWWX).

pub mod ably;
pub mod github;
pub mod hub;
pub mod model;
pub mod registry;
pub mod workflow;

/// The adapters this build ships: Ably Realtime (HS2-BWJWWX).
pub fn builtin_adapters() -> Vec<std::sync::Arc<dyn ChangeAdapter>> {
    vec![std::sync::Arc::new(ably::AblyAdapter::new())]
}

pub use hub::{Backoff, ChangeListener, Hub, Ignored, RecheckReason, RemoteChange, Router};
pub use model::{
    AdapterDescriptor, AdapterError, CONTRACT_VERSION, ChangeAdapter, ChangeHint,
    ConnectionContext, Event, Health, RepoIdentity, SettingSpec,
};
pub use registry::{Binding, Connection, Registry, RegistryError};

#[cfg(test)]
mod tests;
