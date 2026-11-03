use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::OrchestrationError;
use crate::ProviderCapabilities;
use crate::remote_servers::ReviewedRemoteIdentity;

mod binding;
pub use binding::*;

pub const ORCHESTRATION_VERSION: u32 = 1;
pub const MAX_TARGETS: usize = 256;
pub const MAX_TARGET_SOURCES: usize = 64;
pub const MAX_PROJECTION_BYTES: usize = 256 * 1024;
pub(crate) const MAX_MANIFEST_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetRef {
    pub owner_checkout_id: String,
    pub target_id: String,
}

impl TargetRef {
    pub fn validate(&self) -> Result<(), OrchestrationError> {
        valid_identifier(&self.owner_checkout_id)?;
        valid_token(&self.target_id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeclarationState {
    Active,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetDeclaration {
    pub target_id: String,
    pub state: DeclarationState,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_repository_identity: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetManifest {
    pub schema_version: u32,
    pub revision: String,
    pub targets: Vec<TargetDeclaration>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestView {
    pub enabled: bool,
    pub revision: Option<String>,
    pub targets: Vec<TargetDeclaration>,
}

impl From<Option<TargetManifest>> for ManifestView {
    fn from(document: Option<TargetManifest>) -> Self {
        match document {
            Some(document) => Self {
                enabled: true,
                revision: Some(document.revision),
                targets: document.targets,
            },
            None => Self {
                enabled: false,
                revision: None,
                targets: Vec::new(),
            },
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplaceTargets {
    pub expected_revision: Option<String>,
    pub targets: Vec<TargetDeclaration>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddTarget {
    pub expected_revision: Option<String>,
    #[serde(default)]
    pub target_id: Option<String>,
    pub label: String,
    #[serde(default)]
    pub expected_repository_identity: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenameTarget {
    pub expected_revision: String,
    pub label: String,
    #[serde(default)]
    pub expected_repository_identity: Option<String>,
    #[serde(default)]
    pub clear_expected_repository_identity: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoveTarget {
    pub expected_revision: String,
}

/// Local/remote identity is independent of a remote's mutable address or review revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TargetDestination {
    Local {
        server_id: String,
        checkout_id: String,
        checkout_authority: String,
    },
    Remote {
        registration_id: String,
        identity: ReviewedRemoteIdentity,
        checkout_id: String,
        checkout_authority: String,
    },
}

impl TargetDestination {
    pub fn checkout_id(&self) -> &str {
        match self {
            Self::Local { checkout_id, .. } | Self::Remote { checkout_id, .. } => checkout_id,
        }
    }

    pub fn authority(&self) -> &str {
        match self {
            Self::Local {
                checkout_authority, ..
            }
            | Self::Remote {
                checkout_authority, ..
            } => checkout_authority,
        }
    }

    pub fn server_id(&self) -> &str {
        match self {
            Self::Local { server_id, .. } => server_id,
            Self::Remote { identity, .. } => &identity.server_id,
        }
    }

    pub fn validate(&self) -> Result<(), OrchestrationError> {
        valid_token(self.server_id())?;
        valid_identifier(self.checkout_id())?;
        valid_digest(self.authority())?;
        if let Self::Remote {
            registration_id,
            identity,
            ..
        } = self
        {
            valid_token(registration_id)?;
            if let Some(fingerprint) = &identity.ca_fingerprint {
                valid_digest(fingerprint)?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceState {
    Available,
    Missing,
    Disabled,
    ReviewRequired,
    IdentityChanged,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetSource {
    pub connection_id: String,
    pub provider: String,
    pub identity: Option<String>,
    pub state: SourceState,
    /// None is unknown; an advertisement is never a delegated grant.
    pub capabilities: Option<ProviderCapabilities>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckoutProjection {
    pub schema_version: u32,
    pub server_id: String,
    pub checkout_id: String,
    pub checkout_authority: String,
    pub sources: Vec<TargetSource>,
}

impl CheckoutProjection {
    pub fn validate(&self) -> Result<(), OrchestrationError> {
        if self.schema_version != ORCHESTRATION_VERSION {
            return Err(OrchestrationError::Observation(
                TargetFailure::UnsupportedMetadata,
            ));
        }
        valid_token(&self.server_id)?;
        valid_identifier(&self.checkout_id)?;
        valid_digest(&self.checkout_authority)?;
        if self.sources.len() > MAX_TARGET_SOURCES {
            return Err(OrchestrationError::Capacity);
        }
        let mut ids = HashSet::new();
        for source in &self.sources {
            valid_identifier(&source.connection_id)?;
            valid_identifier(&source.provider)?;
            if source.provider.len() > 80 {
                return Err(OrchestrationError::Invalid("provider name is too long"));
            }
            if !ids.insert(&source.connection_id) {
                return Err(OrchestrationError::Invalid("duplicate source"));
            }
            if let Some(identity) = &source.identity {
                valid_digest(identity)?;
            }
            if source.state == SourceState::Available
                && (source.identity.is_none() || source.capabilities.is_none())
            {
                return Err(OrchestrationError::Invalid(
                    "available source needs verified identity and capabilities",
                ));
            }
            if let Some(capabilities) = &source.capabilities {
                if capabilities.query_fields.len() > 32
                    || capabilities
                        .query_fields
                        .iter()
                        .any(|field| !bounded_text(field, 80))
                {
                    return Err(OrchestrationError::Invalid(
                        "capability field bounds exceeded",
                    ));
                }
            }
        }
        let bytes = serde_json::to_vec(self).map_err(|_| OrchestrationError::Corrupt)?;
        if bytes.len() > MAX_PROJECTION_BYTES {
            return Err(OrchestrationError::Capacity);
        }
        if !self.sources.is_empty()
            && self
                .sources
                .iter()
                .all(|source| source.state == SourceState::Available)
            && checkout_authority(&self.sources)? != self.checkout_authority
        {
            return Err(OrchestrationError::Invalid(
                "checkout authority does not match its sources",
            ));
        }
        Ok(())
    }

    /// Metadata is as-of the bounded observation, not a distributed transaction or grant.
    /// The workflow must additionally hold its local observation guard through commit.
    pub fn verified_for(&self, destination: &TargetDestination) -> Result<(), TargetFailure> {
        self.validate().map_err(|error| match error {
            OrchestrationError::Observation(failure) => failure,
            OrchestrationError::Capacity => TargetFailure::ResponseLimit,
            _ => TargetFailure::InvalidResponse,
        })?;
        if self.server_id != destination.server_id()
            || self.checkout_id != destination.checkout_id()
        {
            return Err(TargetFailure::IdentityChanged);
        }
        // Availability failures cannot reconstruct a complete source authority.
        // Prefer their explicit reason after stable ids, before comparing the
        // authority of an entirely available projection (HS2-4J6V46).
        if self.sources.is_empty() {
            return Err(TargetFailure::SourceMissing);
        }
        for source in &self.sources {
            match source.state {
                SourceState::Available => {}
                SourceState::Missing => return Err(TargetFailure::SourceMissing),
                SourceState::Disabled => return Err(TargetFailure::DisabledSource),
                SourceState::ReviewRequired => return Err(TargetFailure::ReviewRequired),
                SourceState::IdentityChanged => return Err(TargetFailure::IdentityChanged),
                SourceState::Unsupported => return Err(TargetFailure::UnsupportedMetadata),
            }
        }
        if self.checkout_authority != destination.authority() {
            return Err(TargetFailure::IdentityChanged);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum InspectDestination {
    Local {
        checkout_id: String,
    },
    Remote {
        registration_id: String,
        checkout_id: String,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InspectTarget {
    pub destination: InspectDestination,
}
impl InspectTarget {
    pub fn validate(&self) -> Result<(), OrchestrationError> {
        match &self.destination {
            InspectDestination::Local { checkout_id } => valid_identifier(checkout_id),
            InspectDestination::Remote {
                registration_id,
                checkout_id,
            } => {
                valid_token(registration_id)?;
                valid_identifier(checkout_id)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetInspection {
    pub destination: TargetDestination,
    pub projection: CheckoutProjection,
    /// Fresh as-of metadata only; inspect never establishes a binding or grant.
    pub stale: bool,
    pub usable: bool,
}
impl TargetInspection {
    pub fn verified(
        destination: TargetDestination,
        projection: CheckoutProjection,
    ) -> Result<Self, OrchestrationError> {
        projection
            .verified_for(&destination)
            .map_err(OrchestrationError::Observation)?;
        Ok(Self {
            destination,
            projection,
            stale: false,
            usable: false,
        })
    }
}

/// Stable digest of provider-owned nonsecret identities; labels, ids and defaults are excluded.
pub fn checkout_authority(sources: &[TargetSource]) -> Result<String, OrchestrationError> {
    if sources.len() > MAX_TARGET_SOURCES {
        return Err(OrchestrationError::Capacity);
    }
    let identities = sources
        .iter()
        .map(|source| {
            let identity = source
                .identity
                .as_deref()
                .ok_or(OrchestrationError::Observation(
                    TargetFailure::SourceMissing,
                ))?;
            valid_identifier(&source.provider)?;
            valid_digest(identity)?;
            Ok((source.provider.as_str(), identity))
        })
        .collect::<Result<Vec<_>, OrchestrationError>>()?;
    identity_authority(identities)
}

pub(crate) fn identity_authority(
    mut identities: Vec<(&str, &str)>,
) -> Result<String, OrchestrationError> {
    identities.sort_unstable();
    let encoded = serde_json::to_vec(&identities).map_err(|_| OrchestrationError::Corrupt)?;
    let mut hash = Sha256::new();
    hash.update(b"hs2-checkout-authority-v1\0");
    hash.update(encoded);
    Ok(format!("{:x}", hash.finalize()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetFailure {
    Unreachable,
    Unauthorized,
    SourceMissing,
    IdentityChanged,
    UnsupportedMetadata,
    MissingCredentials,
    Revoked,
    TrustFailed,
    Redirect,
    ResponseLimit,
    InvalidResponse,
    DisabledSource,
    ReviewRequired,
    TargetRemoved,
}

impl std::fmt::Display for TargetFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code())
    }
}

impl TargetFailure {
    pub fn code(self) -> &'static str {
        match self {
            Self::Unreachable => "unreachable",
            Self::Unauthorized => "unauthorized",
            Self::SourceMissing => "source_missing",
            Self::IdentityChanged => "identity_changed",
            Self::UnsupportedMetadata => "unsupported_metadata",
            Self::MissingCredentials => "missing_credentials",
            Self::Revoked => "revoked",
            Self::TrustFailed => "trust_failed",
            Self::Redirect => "redirect_refused",
            Self::ResponseLimit => "response_limit",
            Self::InvalidResponse => "invalid_response",
            Self::DisabledSource => "disabled_source",
            Self::ReviewRequired => "review_required",
            Self::TargetRemoved => "target_removed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionState {
    Unbound,
    Connecting,
    Reachable,
    Unreachable,
    Unauthorized,
    SourceMissing,
    IdentityChanged,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BindTarget {
    pub target: TargetRef,
    pub expected_manifest_revision: String,
    pub expected_binding_revision: u64,
    pub previous_destination: Option<TargetDestination>,
    pub destination: TargetDestination,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnbindTarget {
    pub target: TargetRef,
    pub expected_binding_revision: u64,
    pub previous_destination: Option<TargetDestination>,
}

pub(crate) fn valid_token(value: &str) -> Result<(), OrchestrationError> {
    let parsed = value
        .parse::<ulid::Ulid>()
        .map_err(|_| OrchestrationError::Invalid("canonical ULID required"))?;
    if parsed.to_string() != value {
        return Err(OrchestrationError::Invalid("canonical ULID required"));
    }
    Ok(())
}

pub(crate) fn valid_identifier(value: &str) -> Result<(), OrchestrationError> {
    if value.is_empty()
        || value.len() > 160
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    {
        return Err(OrchestrationError::Invalid(
            "bounded qualified identifier required",
        ));
    }
    Ok(())
}

pub(crate) fn valid_digest(value: &str) -> Result<(), OrchestrationError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(OrchestrationError::Invalid(
            "canonical identity digest required",
        ));
    }
    Ok(())
}

pub(crate) fn bounded_text(value: &str, limit: usize) -> bool {
    !value.trim().is_empty() && value.len() <= limit && !value.chars().any(char::is_control)
}

pub fn normalize_repository_hint(value: &str) -> Result<String, OrchestrationError> {
    if value.len() > 2048
        || value
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || c == '\\')
    {
        return Err(OrchestrationError::Invalid(
            "bounded credential-free repository URI required",
        ));
    }
    let url = url::Url::parse(value)
        .map_err(|_| OrchestrationError::Invalid("absolute repository URI required"))?;
    if !matches!(url.scheme(), "http" | "https" | "ssh" | "git")
        || url.host_str().is_none()
        || url.path() == "/"
        || url.path().is_empty()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(OrchestrationError::Invalid(
            "repository URI must omit credentials and local paths",
        ));
    }
    Ok(url.to_string())
}

impl From<crate::remote_servers::RemoteFailure> for TargetFailure {
    fn from(failure: crate::remote_servers::RemoteFailure) -> Self {
        use crate::remote_servers::RemoteFailure as Remote;
        match failure {
            Remote::Unreachable => Self::Unreachable,
            Remote::Unauthorized => Self::Unauthorized,
            Remote::MissingCredentials => Self::MissingCredentials,
            Remote::Unsupported => Self::UnsupportedMetadata,
            Remote::IdentityChanged => Self::IdentityChanged,
            Remote::TrustFailed => Self::TrustFailed,
            Remote::Redirect => Self::Redirect,
            Remote::ResponseLimit => Self::ResponseLimit,
            Remote::InvalidResponse => Self::InvalidResponse,
        }
    }
}

/// Validate an owner-defined HTTP API identity field without rewriting explicit
/// ports or otherwise borrowing remote-origin identity semantics. The provider's
/// typed configuration owns canonical field values (HS2-4J6V46).
pub fn validate_source_identity_endpoint(value: &str) -> Result<(), OrchestrationError> {
    let parsed = url::Url::parse(value)
        .map_err(|_| OrchestrationError::Invalid("invalid source identity endpoint"))?;
    if value != value.trim()
        || value.len() > 2048
        || value
            .chars()
            .any(|character| character.is_control() || character == '\\')
        || !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(OrchestrationError::Invalid(
            "invalid source identity endpoint",
        ));
    }
    Ok(())
}
