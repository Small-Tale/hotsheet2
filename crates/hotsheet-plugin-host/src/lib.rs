//! The out-of-process plugin host (docs/38-plugin-protocol.md, HS2-XY0VTN): code-bearing
//! manifests ([`manifest`]), the trust gate ([`trust`]), and the supervisor
//! ([`PluginHost`]) that starts, multiplexes, restarts and stops plugin processes for the
//! CLI and the server alike. Capability adapters (ticket sources, AI drives, notifiers)
//! sit on top of [`PluginHost::call`] and [`PluginHost::stream`].

mod connection;
mod host;
pub mod manifest;
mod tree;
pub mod trust;

pub use host::{ALLOWED_ENV, CallStream, HostConfig, PLUGIN_DATA_DIR, PluginHost, PluginStatus};
pub use manifest::CodePluginManifest;
pub use trust::{TrustError, TrustStore};
