//! HS2-2FG91X: how orchestration reads resource claims, and how a release wakes the
//! scheduler.
//!
//! The scheduler never claims. A machine reads its own claim backend for the
//! namespaces ready work names (the hosted half of a capacity read), and the
//! orchestrating side treats what it is told as unavailable capacity. A session
//! acquires what its work needs itself, with the claim service's own primitives
//! (`docs/36-resource-claims.md`).
//!
//! Nothing here polls. A deferred resource is waited on with the claim service's
//! wait primitive, on one thread per resource, and the wait ending writes the
//! scheduler's wake marker.
use std::collections::BTreeSet;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use hotsheet_resource_claims::{ClaimService, MAX_WAIT, Namespace, ResourceRef, WaitOutcome};
use hotsheet_ticketing::orchestration::{HeldClaim, OfflinePolicy, ResourceState, Schedule};

use super::*;

/// Resources one process waits on at a time, per machine home.
pub const MAX_CLAIM_WATCHES: usize = 16;

/// A namespace's backend could not be read, with what its configuration says to do
/// meanwhile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClaimsUnreachable {
    pub offline: OfflinePolicy,
}

/// Where this machine's live resource claims come from. The default reads the claim
/// service; tests script one.
pub trait ClaimSource: Send + Sync {
    /// The live claims of one namespace: a namespace, a resource and an expiry each,
    /// never a holder, a token or metadata.
    fn held(&self, namespace: &str) -> Result<Vec<HeldClaim>, ClaimsUnreachable>;
    /// Block until the resource is free or the timeout passes, woken by the
    /// backend's own signal. True when it is free.
    fn wait_free(&self, namespace: &str, resource: &str, timeout: Duration) -> bool;
}

/// The claim service of this machine (`hotsheet_resource_claims`). Backend
/// configurations and their offline policy are HS2-JJWESD; until then every
/// namespace is served by the local machine backend, which is never offline, and a
/// namespace that cannot be read fails closed.
pub struct MachineClaims {
    home: PathBuf,
}
impl MachineClaims {
    pub fn new(home: impl Into<PathBuf>) -> Self {
        Self { home: home.into() }
    }
}
impl ClaimSource for MachineClaims {
    fn held(&self, namespace: &str) -> Result<Vec<HeldClaim>, ClaimsUnreachable> {
        let closed = ClaimsUnreachable {
            offline: OfflinePolicy::FailClosed,
        };
        let parsed = Namespace::parse(namespace).map_err(|_| closed)?;
        Ok(ClaimService::new(&self.home)
            .list(&parsed)
            .map_err(|_| closed)?
            .into_iter()
            .map(|claim| HeldClaim {
                namespace: namespace.to_owned(),
                resource: claim.resource,
                expires_at_ms: claim.expires_at_ms,
            })
            .collect())
    }

    fn wait_free(&self, namespace: &str, resource: &str, timeout: Duration) -> bool {
        ResourceRef::parse(&format!("{namespace}/{resource}"))
            .and_then(|target| {
                hotsheet_resource_claims::wait_until_free(&self.home, &target, timeout)
            })
            .is_ok_and(|outcome| outcome == WaitOutcome::Free)
    }
}

type Watch = (PathBuf, String, String);
static WATCHES: OnceLock<Mutex<BTreeSet<Watch>>> = OnceLock::new();
fn watches() -> std::sync::MutexGuard<'static, BTreeSet<Watch>> {
    WATCHES
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl OrchestrationRuntime {
    /// Replace where this machine's resource claims are read (tests, and the
    /// configured backends of HS2-JJWESD, HS2-KCEHVR).
    #[must_use]
    pub fn with_claim_source(mut self, claims: Arc<dyn ClaimSource>) -> Self {
        self.claims = claims;
        self
    }

    /// Resources of this machine that a release is being waited on for.
    pub fn claim_watches(&self) -> Vec<(String, String)> {
        watches()
            .iter()
            .filter(|(home, ..)| home == &self.home)
            .map(|(_, namespace, resource)| (namespace.clone(), resource.clone()))
            .collect()
    }

    /// Wait for every held resource of this machine that deferred work in a pass,
    /// and wake the scheduler when one is released or lapses. A resource is waited
    /// on once however many passes defer work for it. A claim on another machine's
    /// backend has no wait from here; its expiry is the schedule's retry time
    /// (HS2-G1PKK2).
    pub(super) fn watch_releases(&self, schedule: &Schedule) {
        for entry in &schedule.contention {
            if entry.state != ResourceState::Held
                || entry.server_id.as_deref() != Some(self.server_id.as_str())
            {
                continue;
            }
            let key = (
                self.home.clone(),
                entry.namespace.clone(),
                entry.resource.clone(),
            );
            {
                let mut watched = watches();
                let here = watched
                    .iter()
                    .filter(|(home, ..)| home == &self.home)
                    .count();
                if here >= MAX_CLAIM_WATCHES || !watched.insert(key.clone()) {
                    continue;
                }
            }
            let claims = self.claims.clone();
            let thread_key = key.clone();
            let spawned = std::thread::Builder::new()
                .name("orchestration-claim-wait".into())
                .spawn(move || {
                    let (home, namespace, resource) = &thread_key;
                    // Free or not, the next pass reads the claim again and, when it
                    // is still held, waits again.
                    claims.wait_free(namespace, resource, MAX_WAIT);
                    watches().remove(&thread_key);
                    Self::scheduler_wake_at(home);
                });
            if spawned.is_err() {
                watches().remove(&key);
            }
        }
    }
}
