use std::collections::HashSet;
use std::io::BufReader;
use std::net::IpAddr;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::RemoteError;

pub const REMOTE_PROTOCOL_VERSION: u32 = 1;
pub const MAX_REMOTE_CHECKOUTS: usize = 1_000;
pub const MAX_REMOTE_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
pub(crate) const MAX_REGISTRY_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteCredentials {
    pub bearer_secret: String,
    #[serde(default)]
    pub ca_pem: Option<String>,
    #[serde(default)]
    pub client_certificate_pem: Option<String>,
    #[serde(default)]
    pub client_key_pem: Option<String>,
}

impl std::fmt::Debug for RemoteCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RemoteCredentials(<redacted>)")
    }
}

impl RemoteCredentials {
    pub fn trust_fingerprint(&self, origin: &str) -> Result<Option<String>, RemoteError> {
        if self.bearer_secret.is_empty()
            || self.bearer_secret.len() > 8_192
            || !self
                .bearer_secret
                .bytes()
                .all(|byte| byte.is_ascii_graphic())
        {
            return Err(RemoteError::Invalid(
                "bearer must be nonempty printable ASCII",
            ));
        }
        if origin.starts_with("http://") {
            if self.ca_pem.is_some()
                || self.client_certificate_pem.is_some()
                || self.client_key_pem.is_some()
            {
                return Err(RemoteError::Invalid(
                    "plaintext loopback does not take TLS material",
                ));
            }
            return Ok(None);
        }
        let ca = self
            .ca_pem
            .as_deref()
            .ok_or(RemoteError::Invalid("CA is required"))?;
        for value in [
            Some(ca),
            self.client_certificate_pem.as_deref(),
            self.client_key_pem.as_deref(),
        ] {
            if value.is_none_or(|value| value.is_empty() || value.len() > 128 * 1024) {
                return Err(RemoteError::Invalid(
                    "bounded CA, device certificate and key are required",
                ));
            }
        }
        let certs = rustls_pemfile::certs(&mut BufReader::new(ca.as_bytes()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| RemoteError::Invalid("invalid CA PEM"))?;
        let [certificate] = certs.as_slice() else {
            return Err(RemoteError::Invalid(
                "exactly one explicit CA certificate is required",
            ));
        };
        Ok(Some(format!("{:x}", Sha256::digest(certificate.as_ref()))))
    }
}

pub fn normalize_remote_origin(value: &str) -> Result<String, RemoteError> {
    if value != value.trim()
        || value
            .chars()
            .any(|character| character.is_control() || character == '\\')
    {
        return Err(RemoteError::Invalid(
            "origin contains whitespace or backslashes",
        ));
    }
    let url = url::Url::parse(value)
        .map_err(|_| RemoteError::Invalid("origin must be an absolute URL"))?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
        || url.host_str().is_none()
    {
        return Err(RemoteError::Invalid(
            "origin must contain only HTTP(S) scheme, host and port",
        ));
    }
    if url.scheme() == "http"
        && !matches!(url.host(), Some(url::Host::Ipv4(ip)) if IpAddr::V4(ip).is_loopback())
        && !matches!(url.host(), Some(url::Host::Ipv6(ip)) if IpAddr::V6(ip).is_loopback())
    {
        return Err(RemoteError::Invalid(
            "off-box origins require HTTPS and mutual TLS",
        ));
    }
    Ok(url.origin().ascii_serialization())
}

pub fn remote_registry_origin_is_loopback(origin: &str) -> bool {
    normalize_remote_origin(origin)
        .ok()
        .and_then(|value| url::Url::parse(&value).ok())
        .is_some_and(|url| match url.host() {
            Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
            Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
            _ => false,
        })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteIdentity {
    pub generation: String,
    pub protocol: u32,
    pub server_id: String,
}

impl RemoteIdentity {
    pub fn validate(&self) -> Result<(), RemoteFailure> {
        if self.generation != "hs2" || self.protocol != REMOTE_PROTOCOL_VERSION {
            return Err(RemoteFailure::Unsupported);
        }
        valid_id(&self.server_id).map_err(|_| RemoteFailure::Unsupported)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteSource {
    pub connection_id: String,
    pub provider: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteCheckout {
    pub id: String,
    pub name: String,
    pub sources: Vec<RemoteSource>,
    pub state: RemoteCheckoutState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteCheckoutState {
    Available,
    Inaccessible,
    IdentityChanged,
}

pub fn validate_remote_checkouts(checkouts: &[RemoteCheckout]) -> Result<(), RemoteFailure> {
    let mut ids = HashSet::new();
    if checkouts.len() > MAX_REMOTE_CHECKOUTS {
        return Err(RemoteFailure::ResponseLimit);
    }
    for checkout in checkouts {
        if !bounded_text(&checkout.id, 160)
            || !bounded_text(&checkout.name, 256)
            || !ids.insert(&checkout.id)
            || checkout.sources.len() > 64
        {
            return Err(RemoteFailure::InvalidResponse);
        }
        let mut sources = HashSet::new();
        for source in &checkout.sources {
            if !bounded_text(&source.connection_id, 160)
                || !bounded_text(&source.provider, 80)
                || !sources.insert(&source.connection_id)
            {
                return Err(RemoteFailure::InvalidResponse);
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteFailure {
    Unreachable,
    Unauthorized,
    MissingCredentials,
    Unsupported,
    IdentityChanged,
    TrustFailed,
    Redirect,
    ResponseLimit,
    InvalidResponse,
}

impl std::fmt::Display for RemoteFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Unreachable => "unreachable",
            Self::Unauthorized => "unauthorized",
            Self::MissingCredentials => "missing_credentials",
            Self::Unsupported => "unsupported",
            Self::IdentityChanged => "identity_changed",
            Self::TrustFailed => "trust_failed",
            Self::Redirect => "redirect_refused",
            Self::ResponseLimit => "response_limit",
            Self::InvalidResponse => "invalid_response",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewedRemoteIdentity {
    pub server_id: String,
    pub ca_fingerprint: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RemoteSnapshot {
    pub identity: RemoteIdentity,
    pub checkouts: Vec<RemoteCheckout>,
}

/// Probe only this origin with these supplied credentials. Implementations must authenticate
/// TLS/hostname and the bearer, refuse redirects, bound input, and confirm identity across discovery.
pub trait RemoteTransport {
    fn probe(
        &self,
        origin: &str,
        credentials: &RemoteCredentials,
    ) -> Result<RemoteSnapshot, RemoteFailure>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteReview {
    pub revision: u64,
    pub origin: String,
    pub identity: ReviewedRemoteIdentity,
    pub reviewed_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteView {
    pub id: String,
    pub label: String,
    pub origin: String,
    pub identity: ReviewedRemoteIdentity,
    pub revision: u64,
    pub revoked: bool,
    pub last_failure: Option<RemoteFailure>,
    pub checked_at_ms: Option<u64>,
    pub confirmed_at_ms: Option<u64>,
    pub last_confirmed_checkouts: Vec<RemoteCheckout>,
    /// Cached observations returned by list are always stale; only discover yields live checkouts.
    pub stale: bool,
    pub credential_cleanup_pending: bool,
    pub review_history: Vec<RemoteReview>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteDiscovery {
    pub remote: RemoteView,
    /// None for revoked, inaccessible, unsupported or changed identities, even with cached data.
    pub checkouts: Option<Vec<RemoteCheckout>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegisterRemote {
    pub id: String,
    pub label: String,
    pub origin: String,
    pub expected_server_id: String,
    pub credentials: RemoteCredentials,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewRemote {
    pub expected_revision: u64,
    pub previous_identity: ReviewedRemoteIdentity,
    pub label: String,
    pub origin: String,
    pub expected_server_id: String,
    pub credentials: RemoteCredentials,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevokeRemote {
    pub expected_revision: u64,
}

pub(crate) fn valid_id(value: &str) -> Result<(), RemoteError> {
    value
        .parse::<ulid::Ulid>()
        .map_err(|_| RemoteError::Invalid("id must be a ULID"))?;
    if value.to_ascii_uppercase() != value {
        return Err(RemoteError::Invalid("id must use canonical uppercase"));
    }
    Ok(())
}

pub(crate) fn bounded_text(value: &str, limit: usize) -> bool {
    !value.trim().is_empty() && value.len() <= limit && !value.chars().any(char::is_control)
}
