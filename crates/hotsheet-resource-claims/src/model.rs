//! Names, claims, grants and capabilities: the values every backend exchanges.

use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

use hotsheet_sync::secret::SecretString;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::error::ClaimError;

/// Longest lease one acquire or renew may ask for. Long work renews.
pub const MAX_LEASE: Duration = Duration::from_secs(24 * 60 * 60);
/// Longest single wait. A caller that wants longer waits again.
pub const MAX_WAIT: Duration = Duration::from_secs(24 * 60 * 60);
/// Most metadata entries one claim carries.
pub const MAX_METADATA_ENTRIES: usize = 16;
/// Most bytes of metadata (keys plus stored values) one claim carries.
pub const MAX_METADATA_BYTES: usize = 4096;
const MAX_NAMESPACE_LEN: usize = 64;
const MAX_RESOURCE_LEN: usize = 128;
const MAX_METADATA_KEY_LEN: usize = 64;

/// A namespace of resources: lowercase, because it names a directory on file systems
/// that fold case.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Namespace(String);

impl Namespace {
    pub fn parse(value: &str) -> Result<Self, ClaimError> {
        let valid = !value.is_empty()
            && value.len() <= MAX_NAMESPACE_LEN
            && value.as_bytes()[0].is_ascii_alphanumeric()
            && value
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
            && !value.contains("..");
        if valid {
            Ok(Self(value.to_owned()))
        } else {
            Err(ClaimError::invalid(format!(
                "namespace must be 1-{MAX_NAMESPACE_LEN} characters of a-z, 0-9, '.', '_' or '-', starting with a letter or digit"
            )))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for Namespace {
    type Error = ClaimError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<Namespace> for String {
    fn from(value: Namespace) -> Self {
        value.0
    }
}

impl fmt::Display for Namespace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One named resource in a namespace, written `<namespace>/<resource>`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResourceRef {
    namespace: Namespace,
    resource: String,
}

impl ResourceRef {
    pub fn new(namespace: Namespace, resource: &str) -> Result<Self, ClaimError> {
        Ok(Self {
            namespace,
            resource: validate_resource(resource)?,
        })
    }

    /// Parse `<namespace>/<resource>`.
    pub fn parse(target: &str) -> Result<Self, ClaimError> {
        let (namespace, resource) = target
            .split_once('/')
            .ok_or_else(|| ClaimError::invalid("expected <namespace>/<resource>"))?;
        Self::new(Namespace::parse(namespace)?, resource)
    }

    pub fn namespace(&self) -> &Namespace {
        &self.namespace
    }

    pub fn resource(&self) -> &str {
        &self.resource
    }
}

impl fmt::Display for ResourceRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.namespace, self.resource)
    }
}

fn validate_resource(value: &str) -> Result<String, ClaimError> {
    let valid = !value.is_empty()
        && value.len() <= MAX_RESOURCE_LEN
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b));
    if valid {
        Ok(value.to_owned())
    } else {
        Err(ClaimError::invalid(format!(
            "resource must be 1-{MAX_RESOURCE_LEN} characters of letters, digits, '.', '_' or '-', starting with a letter or digit"
        )))
    }
}

/// What a backend promises about a claim it reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Guarantee {
    /// Atomic acquire and a monotonic fencing token: at most one live holder.
    ExclusiveWithFencing,
    /// A best-effort signal. Never treat it as mutual exclusion.
    AdvisoryOnly,
}

/// How a backend wakes a waiter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaitMechanism {
    /// The backend pushes a signal on release; expiry is one sleep to a known instant.
    PushSignal,
    /// The backend holds a request open until the resource changes.
    LongPoll,
    /// The backend cannot wait; only try-now acquire works.
    None,
}

/// What one backend offers, advertised before use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    pub backend: String,
    pub guarantee: Guarantee,
    pub wait: WaitMechanism,
    pub max_metadata_entries: usize,
    pub max_metadata_bytes: usize,
    pub max_lease_ms: u64,
}

/// A live claim as any reader sees it. It never carries the renewal token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    pub namespace: Namespace,
    pub resource: String,
    pub claim_id: String,
    /// Strictly greater than every earlier grant of this resource.
    pub fencing_token: u64,
    pub guarantee: Guarantee,
    pub acquired_at_ms: u64,
    /// Backend-clock expiry, in milliseconds since the Unix epoch.
    pub expires_at_ms: u64,
    /// The same instant as RFC 3339 UTC.
    pub expires_at: String,
    /// Stored opaquely; every value carries a codec prefix.
    pub metadata: BTreeMap<String, String>,
}

impl Claim {
    pub fn target(&self) -> String {
        format!("{}/{}", self.namespace, self.resource)
    }
}

/// A fresh claim plus the one copy of its renewal token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grant {
    #[serde(flatten)]
    pub claim: Claim,
    /// Bearer capability for renewing and releasing this claim only. Shown once.
    pub renewal_token: SecretString,
}

/// What an acquirer asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcquireRequest {
    pub duration: Duration,
    /// Already-encoded values (see [`crate::MetadataCodecs`]).
    pub metadata: BTreeMap<String, String>,
}

impl AcquireRequest {
    pub fn new(duration: Duration) -> Self {
        Self {
            duration,
            metadata: BTreeMap::new(),
        }
    }

    pub fn with_metadata(mut self, metadata: BTreeMap<String, String>) -> Self {
        self.metadata = metadata;
        self
    }
}

/// How a wait for a resource to become free ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WaitOutcome {
    /// No live claim holds the resource at the backend's clock.
    Free,
    /// The wait ended first; this claim still holds it.
    TimedOut { holder: Box<Claim> },
}

pub(crate) fn rfc3339(ms: u64) -> String {
    let nanos = i128::from(ms) * 1_000_000;
    OffsetDateTime::from_unix_timestamp_nanos(nanos)
        .ok()
        .and_then(|instant| instant.format(&Rfc3339).ok())
        .unwrap_or_else(|| ms.to_string())
}

pub(crate) fn lease_ms(duration: Duration) -> Result<u64, ClaimError> {
    if duration.is_zero() || duration > MAX_LEASE {
        return Err(ClaimError::invalid(format!(
            "duration must be between 1 ms and {} hours",
            MAX_LEASE.as_secs() / 3600
        )));
    }
    Ok(u64::try_from(duration.as_millis())
        .unwrap_or(u64::MAX)
        .max(1))
}

pub(crate) fn wait_ms(timeout: Duration) -> Result<u64, ClaimError> {
    if timeout > MAX_WAIT {
        return Err(ClaimError::invalid(format!(
            "wait must be at most {} hours",
            MAX_WAIT.as_secs() / 3600
        )));
    }
    Ok(u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX))
}

/// Check the bounded metadata map: key shape, a known codec prefix, and total size.
pub fn validate_metadata(metadata: &BTreeMap<String, String>) -> Result<(), ClaimError> {
    if metadata.len() > MAX_METADATA_ENTRIES {
        return Err(ClaimError::invalid(format!(
            "at most {MAX_METADATA_ENTRIES} metadata entries"
        )));
    }
    let mut bytes = 0usize;
    for (key, value) in metadata {
        let key_ok = !key.is_empty()
            && key.len() <= MAX_METADATA_KEY_LEN
            && key
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b));
        if !key_ok {
            return Err(ClaimError::invalid(format!(
                "metadata key must be 1-{MAX_METADATA_KEY_LEN} characters of a-z, 0-9, '.', '_' or '-'"
            )));
        }
        if crate::codec::MetadataCodec::of_stored(value).is_none() {
            return Err(ClaimError::invalid(format!(
                "metadata value for '{key}' needs a plain:, hmac-sha256: or enc:v1: prefix"
            )));
        }
        if value.chars().any(char::is_control) {
            return Err(ClaimError::invalid(format!(
                "metadata value for '{key}' contains a control character"
            )));
        }
        bytes += key.len() + value.len();
    }
    if bytes > MAX_METADATA_BYTES {
        return Err(ClaimError::invalid(format!(
            "metadata is {bytes} bytes; the limit is {MAX_METADATA_BYTES}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod hs2_9zmc7n_tests {
    use super::Grant;

    /// HS2-9ZMC7N: the renewal token is a bearer capability, so `Debug` redacts it while the
    /// flattened grant JSON every backend and the service return keeps its exact shape.
    #[test]
    fn hs2_9zmc7n_grant_renewal_token_is_redacted_and_the_wire_json_is_unchanged() {
        let json = r#"{"namespace":"vm","resource":"macos","claim_id":"c1","fencing_token":3,"guarantee":"exclusive_with_fencing","acquired_at_ms":1,"expires_at_ms":2,"expires_at":"1970-01-01T00:00:00.002Z","metadata":{},"renewal_token":"hsrc1.synthetic"}"#;
        let grant: Grant = serde_json::from_str(json).unwrap();
        assert_eq!(grant.renewal_token.expose(), "hsrc1.synthetic");
        assert_eq!(grant.claim.fencing_token, 3);
        assert_eq!(serde_json::to_string(&grant).unwrap(), json);
        let printed = format!("{grant:?} {grant:#?}");
        assert!(!printed.contains("synthetic"), "{printed}");
        assert!(printed.contains("renewal_token: <redacted>"), "{printed}");
    }
}
