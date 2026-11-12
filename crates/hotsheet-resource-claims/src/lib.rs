//! Resource claims (HS2-H3WPPY): lease a named resource for a duration, renew it while
//! work continues, release it early. See `docs/36-resource-claims.md`.
//!
//! [`ClaimBackend`] is the contract; [`LocalBackend`] is the built-in machine-scoped
//! implementation. A grant carries a claim id, a monotonic fencing token, a backend-clock
//! expiry, bounded metadata, and a renewal token that can renew or release that one
//! claim and nothing else.

mod backend;
mod clock;
mod codec;
mod error;
mod local;
mod model;
mod token;

pub use backend::ClaimBackend;
pub use clock::{Clock, ManualClock, SystemClock};
pub use codec::{
    ClaimKey, ENC_PREFIX, FileKeyProvider, HMAC_PREFIX, KeyProvider, MetadataCodec, MetadataCodecs,
    MetadataValue, PLAIN_PREFIX, StaticKeyProvider,
};
pub use error::ClaimError;
pub use local::{LOCAL_ROOT_DIR, LocalBackend};
pub use model::{
    AcquireRequest, Capabilities, Claim, Grant, Guarantee, MAX_LEASE, MAX_METADATA_BYTES,
    MAX_METADATA_ENTRIES, MAX_WAIT, Namespace, ResourceRef, WaitMechanism, WaitOutcome,
    validate_metadata,
};
pub use token::{MAX_RENEWAL_TOKEN_LEN, RenewalTokenClaims, parse_renewal_token};
