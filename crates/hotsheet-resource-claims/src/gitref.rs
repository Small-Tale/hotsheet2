//! The built-in Git-ref claim backend (HS2-WJF0D8, `docs/36` §36.14): resource claims
//! kept as marker refs `refs/hotsheet/resource-claims/<namespace>/<resource>` on a Git
//! remote, through `hotsheet_ticketing::gitref_cas`. The remote serializes every change,
//! so acquire is a real compare-and-swap. It cannot wait (`wait: none`), and its clock is
//! the claimer's.
//!
//! While the remote is unreachable a `fail_closed` configuration answers
//! `backend_unreachable`. A `provisional` one grants advisory claims from a local
//! fallback under `<HOTSHEET_HOME>/resource-claims/provisional/<configuration>`; they
//! report `advisory_only`, never exclusivity.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use hotsheet_ticketing::gitref_cas::{
    Acquired, CasError, GitRefCas, RESOURCE_REF_PREFIX, RefMarker,
};

use crate::backend::ClaimBackend;
use crate::clock::{Clock, SystemClock};
use crate::codec::{FileKeyProvider, KeyProvider, MetadataCodecs};
use crate::config::OfflinePolicy;
use crate::error::ClaimError;
use crate::local::{LOCAL_ROOT_DIR, LocalBackend};
use crate::model::{
    AcquireRequest, Capabilities, Claim, Grant, Guarantee, MAX_LEASE, MAX_METADATA_BYTES,
    MAX_METADATA_ENTRIES, Namespace, ResourceRef, WaitMechanism, WaitOutcome, lease_ms, rfc3339,
    validate_metadata,
};
use crate::token::{self, parse_renewal_token};

pub struct GitRefBackend {
    cas: GitRefCas,
    configuration: String,
    offline: OfflinePolicy,
    keys: Option<Arc<dyn KeyProvider>>,
    fallback: LocalBackend,
    clock: Arc<dyn Clock>,
}

impl std::fmt::Debug for GitRefBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GitRefBackend")
            .field("configuration", &self.configuration)
            .field("offline", &self.offline)
            .finish_non_exhaustive()
    }
}

fn token_digest(token: &str) -> String {
    token::sha256_hex(token)
}

impl GitRefBackend {
    /// A backend over `repository`'s `remote`. `keys` keys the metadata codecs; without
    /// it only `plain:` metadata is accepted.
    pub fn new(
        home: &Path,
        configuration: &str,
        repository: &Path,
        remote: &str,
        offline: OfflinePolicy,
        keys: Option<Arc<dyn KeyProvider>>,
    ) -> Self {
        let root = home
            .join(LOCAL_ROOT_DIR)
            .join("provisional")
            .join(configuration);
        let fallback_keys = Arc::new(FileKeyProvider::new(root.join("key")));
        Self {
            cas: GitRefCas::new(repository, remote),
            configuration: configuration.to_owned(),
            offline,
            keys,
            fallback: LocalBackend::new(root, Arc::new(SystemClock), fallback_keys),
            clock: Arc::new(SystemClock),
        }
    }

    fn refname(target: &ResourceRef) -> String {
        format!(
            "{RESOURCE_REF_PREFIX}{}/{}",
            target.namespace(),
            target.resource()
        )
    }

    fn view(target: &ResourceRef, marker: &RefMarker) -> Claim {
        let expires = marker.expiry_ms();
        Claim {
            namespace: target.namespace().clone(),
            resource: target.resource().to_owned(),
            claim_id: marker.claim_id.clone(),
            fencing_token: marker.fencing,
            guarantee: Guarantee::ExclusiveWithFencing,
            acquired_at_ms: marker.acquired_at_ms,
            expires_at_ms: expires,
            expires_at: rfc3339(expires),
            metadata: marker.metadata.clone(),
        }
    }

    fn unreachable(&self, error: &CasError) -> ClaimError {
        ClaimError::BackendUnreachable(format!("configuration '{}': {error}", self.configuration))
    }

    fn provisional(&self) -> bool {
        self.offline == OfflinePolicy::Provisional
    }

    fn advisory(mut claim: Claim) -> Claim {
        claim.guarantee = Guarantee::AdvisoryOnly;
        claim
    }

    /// The live marker of a token's claim, after checking the token's digest.
    fn own_marker(
        &self,
        renewal_token: &str,
    ) -> Result<Result<(ResourceRef, RefMarker), CasError>, ClaimError> {
        let parsed = parse_renewal_token(renewal_token)?;
        let refname = Self::refname(&parsed.target);
        let marker = match self.cas.read(&refname) {
            Ok(marker) => marker,
            Err(error) => return Ok(Err(error)),
        };
        match marker {
            None => Err(ClaimError::TokenInvalid),
            Some((_, marker)) if marker.claim_id != parsed.claim_id => Err(ClaimError::NotHolder),
            Some((_, marker)) => {
                if marker.token_digest.as_deref() != Some(token_digest(renewal_token).as_str()) {
                    return Err(ClaimError::TokenInvalid);
                }
                Ok(Ok((parsed.target, marker)))
            }
        }
    }

    fn cas_error(error: CasError) -> ClaimError {
        match error {
            CasError::Unreachable(message) => ClaimError::BackendUnreachable(message),
            CasError::Git(message) => ClaimError::Storage(std::io::Error::other(message)),
        }
    }
}

impl ClaimBackend for GitRefBackend {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            backend: "git_ref".into(),
            guarantee: Guarantee::ExclusiveWithFencing,
            wait: WaitMechanism::None,
            max_metadata_entries: MAX_METADATA_ENTRIES,
            max_metadata_bytes: MAX_METADATA_BYTES,
            max_lease_ms: u64::try_from(MAX_LEASE.as_millis()).unwrap_or(u64::MAX),
        }
    }

    fn codecs(&self) -> Result<MetadataCodecs, ClaimError> {
        let keys = self.keys.as_ref().ok_or_else(|| {
            ClaimError::Unsupported(format!(
                "configuration '{}' has no credential, so only plain: metadata is accepted",
                self.configuration
            ))
        })?;
        Ok(MetadataCodecs::new(&keys.key()?)?)
    }

    fn acquire(&self, target: &ResourceRef, request: &AcquireRequest) -> Result<Grant, ClaimError> {
        validate_metadata(&request.metadata)?;
        let now = self.clock.now_ms();
        let expires = now.saturating_add(lease_ms(request.duration)?);
        let claim_id = ulid::Ulid::new().to_string();
        let renewal_token = token::mint(target, &claim_id)?;
        let mut marker = RefMarker::new("resource-claim", &claim_id, now, expires);
        marker.token_digest = Some(token_digest(&renewal_token));
        marker.metadata.clone_from(&request.metadata);
        match self.cas.acquire(&Self::refname(target), marker, now) {
            Ok(Acquired::Won(marker)) => Ok(Grant {
                claim: Self::view(target, &marker),
                renewal_token: renewal_token.into(),
            }),
            Ok(Acquired::Held(holder)) => Err(ClaimError::HeldBy {
                holder: Box::new(Self::view(target, &holder)),
            }),
            Err(error @ CasError::Unreachable(_)) if self.provisional() => {
                let _ = error;
                let mut grant = self.fallback.acquire(target, request)?;
                grant.claim = Self::advisory(grant.claim);
                Ok(grant)
            }
            Err(error @ CasError::Unreachable(_)) => Err(self.unreachable(&error)),
            Err(error) => Err(Self::cas_error(error)),
        }
    }

    fn acquire_wait(
        &self,
        target: &ResourceRef,
        request: &AcquireRequest,
        timeout: Duration,
    ) -> Result<Grant, ClaimError> {
        match self.acquire(target, request) {
            Err(ClaimError::HeldBy { holder }) if timeout.is_zero() => {
                Err(ClaimError::WaitTimedOut { holder })
            }
            Err(ClaimError::HeldBy { .. }) => Err(ClaimError::Unsupported(
                "the git-ref backend cannot wait: a Git remote cannot signal a release. Acquire again later, or bind this namespace to a backend that can wait (local, upstash-redis, or an external plugin with a blocking wait; docs/36 section 36.14)".into(),
            )),
            other => other,
        }
    }

    fn renew(&self, renewal_token: &str, duration: Duration) -> Result<Claim, ClaimError> {
        let lease = lease_ms(duration)?;
        match self.own_marker(renewal_token) {
            Ok(Ok((target, marker))) => {
                let now = self.clock.now_ms();
                if !marker.is_live(now) {
                    return Err(ClaimError::Expired);
                }
                match self.cas.renew(
                    &Self::refname(&target),
                    &marker.claim_id,
                    now.saturating_add(lease),
                ) {
                    Ok(Ok(renewed)) => Ok(Self::view(&target, &renewed)),
                    Ok(Err(Some(_))) => Err(ClaimError::NotHolder),
                    Ok(Err(None)) => Err(ClaimError::TokenInvalid),
                    Err(error) => Err(Self::cas_error(error)),
                }
            }
            // A provisional token, or any token while the remote is down.
            Ok(Err(_)) | Err(ClaimError::TokenInvalid) if self.provisional() => self
                .fallback
                .renew(renewal_token, duration)
                .map(Self::advisory),
            Ok(Err(error)) => Err(self.unreachable(&error)),
            Err(error) => Err(error),
        }
    }

    fn release(&self, renewal_token: &str) -> Result<Claim, ClaimError> {
        match self.own_marker(renewal_token) {
            Ok(Ok((target, marker))) => {
                match self.cas.release(&Self::refname(&target), &marker.claim_id) {
                    Ok(true) => Ok(Self::view(&target, &marker)),
                    Ok(false) => Err(ClaimError::NotHolder),
                    Err(error) => Err(Self::cas_error(error)),
                }
            }
            Ok(Err(_)) | Err(ClaimError::TokenInvalid) if self.provisional() => {
                self.fallback.release(renewal_token).map(Self::advisory)
            }
            Ok(Err(error)) => Err(self.unreachable(&error)),
            Err(error) => Err(error),
        }
    }

    fn get(&self, target: &ResourceRef) -> Result<Option<Claim>, ClaimError> {
        match self.cas.read(&Self::refname(target)) {
            Ok(marker) => Ok(marker
                .filter(|(_, m)| m.is_live(self.clock.now_ms()))
                .map(|(_, m)| Self::view(target, &m))),
            Err(CasError::Unreachable(_)) if self.provisional() => {
                Ok(self.fallback.get(target)?.map(Self::advisory))
            }
            Err(error) => Err(Self::cas_error(error)),
        }
    }

    fn list(&self, namespace: &Namespace) -> Result<Vec<Claim>, ClaimError> {
        let prefix = format!("{RESOURCE_REF_PREFIX}{namespace}/");
        let now = self.clock.now_ms();
        let markers = self.cas.list(&prefix).map_err(Self::cas_error)?;
        let mut claims = Vec::new();
        for (refname, marker) in markers {
            let Some(resource) = refname.strip_prefix(&prefix) else {
                continue;
            };
            if let Ok(target) = ResourceRef::new(namespace.clone(), resource)
                && marker.is_live(now)
            {
                claims.push(Self::view(&target, &marker));
            }
        }
        Ok(claims)
    }

    fn namespaces(&self) -> Result<Vec<Namespace>, ClaimError> {
        let markers = self
            .cas
            .list(RESOURCE_REF_PREFIX)
            .map_err(Self::cas_error)?;
        let mut names: Vec<Namespace> = markers
            .iter()
            .filter_map(|(refname, _)| {
                let rest = refname.strip_prefix(RESOURCE_REF_PREFIX)?;
                Namespace::parse(rest.split('/').next()?).ok()
            })
            .collect();
        names.sort();
        names.dedup();
        Ok(names)
    }

    fn wait_free(
        &self,
        target: &ResourceRef,
        timeout: Duration,
    ) -> Result<WaitOutcome, ClaimError> {
        match self.get(target)? {
            None => Ok(WaitOutcome::Free),
            Some(holder) if timeout.is_zero() => Ok(WaitOutcome::TimedOut {
                holder: Box::new(holder),
            }),
            Some(_) => Err(ClaimError::Unsupported(
                "the git-ref backend cannot wait: a Git remote cannot signal a release. Check again later, or bind this namespace to a backend that can wait (docs/36 section 36.14)".into(),
            )),
        }
    }

    fn verify_fencing(&self, target: &ResourceRef, fencing_token: u64) -> Result<(), ClaimError> {
        let marker = self
            .cas
            .read(&Self::refname(target))
            .map_err(Self::cas_error)?;
        match marker {
            Some((_, m)) if m.fencing == fencing_token && m.is_live(self.clock.now_ms()) => Ok(()),
            Some((_, m)) if m.fencing > fencing_token => Err(ClaimError::NotHolder),
            Some((_, m)) if m.fencing == fencing_token => Err(ClaimError::Expired),
            _ => Err(ClaimError::Invalid("unknown fencing token".into())),
        }
    }
}

#[cfg(test)]
#[path = "gitref_tests.rs"]
mod tests;
