//! Resource claims (HS2-H3WPPY): lease a named resource for a duration, renew it while
//! work continues, release it early. See `docs/36-resource-claims.md`.
//!
//! [`ClaimBackend`] is the contract; [`LocalBackend`] is the built-in machine-scoped
//! implementation. [`ClaimService`] is the one workflow the CLI, MCP and server share
//! (HS2-FMHH9Y), and [`held`] / [`wait_until_free`] let another crate treat a held
//! resource as unavailable capacity without acquiring it. A grant carries a claim id, a monotonic fencing token, a backend-clock
//! expiry, bounded metadata, and a renewal token that can renew or release that one
//! claim and nothing else.

mod backend;
mod clock;
mod codec;
mod config;
mod error;
mod gitref;
mod local;
mod model;
mod service;
mod token;

pub use backend::ClaimBackend;
pub use clock::{Clock, ManualClock, SystemClock};
pub use codec::{
    ClaimKey, ENC_PREFIX, FileKeyProvider, HMAC_PREFIX, KeyProvider, MetadataCodec, MetadataCodecs,
    MetadataValue, PLAIN_PREFIX, StaticKeyProvider,
};
pub use config::{
    CredentialKeyProvider, OfflinePolicy, PROJECTS_DIR, ResolvedBackend, TestReport, backend_for,
    configuration_json, export_key, generate_key, git_remote, git_repository, import_key,
    key_next_step, offline_policy, project_state_root, resolve, test_configuration,
};
pub use error::ClaimError;
pub use gitref::GitRefBackend;
pub use local::{LOCAL_ROOT_DIR, LocalBackend};
pub use model::{
    AcquireRequest, Capabilities, Claim, Grant, Guarantee, MAX_LEASE, MAX_METADATA_BYTES,
    MAX_METADATA_ENTRIES, MAX_WAIT, Namespace, ResourceRef, WaitMechanism, WaitOutcome,
    validate_metadata,
};
pub use service::{
    ClaimService, ROUTE_PREFIX, TOKEN_ROUTE_PREFIX, held, held_namespaces, http_status, is_path,
    parse_duration, resolve_backend, wait_until_free,
};
pub use token::{MAX_RENEWAL_TOKEN_LEN, RenewalTokenClaims, parse_renewal_token};
