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
//! scheduler's wake marker. HS2-G1PKK2: a resource held on another machine is waited
//! on with that machine's own wait, a genuine long poll over the reviewed target
//! transport (`target-resource-wait`).
use std::collections::BTreeSet;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use hotsheet_resource_claims::{ClaimService, MAX_WAIT, Namespace, ResourceRef, WaitOutcome};
use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::{
    DeclarationState, HeldClaim, OfflinePolicy, ResourceNeed, ResourceState, Schedule,
    TargetAction, TargetDestination,
};
use serde::{Deserialize, Serialize};

use super::tickets::encode;
use super::*;

/// HS2-G1PKK2: the longest one hosted resource wait parks, under the target
/// transport's read timeout. A caller with a later deadline asks again at once.
pub const MAX_RESOURCE_WAIT_MS: u64 = 8_000;
/// HS2-G1PKK2: a remote watch passes the gate again after this long.
const REAUTHORIZE_MS: u64 = 10 * 60 * 1000;

/// HS2-G1PKK2: what a hosted resource wait found when it ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceWait {
    /// Released or lapsed; false when it was still held when the wait ended.
    pub free: bool,
}

/// HS2-G1PKK2: how a watch of a resource on another machine ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteRelease {
    /// The peer answered free: the scheduler was woken.
    Released,
    /// No wake: the deadline passed, the gate refused, the peer could not be asked or
    /// could not wait. The schedule's retry time is the backstop, and the next pass
    /// arms the watch again. The reason is a stable code.
    Ended(String),
}

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

/// A watch is keyed by machine home, the machine that holds the claim (empty for
/// this one), namespace and resource.
type Watch = (PathBuf, String, String, String);
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

    /// Resources a release is being waited on for, on this machine or another.
    pub fn claim_watches(&self) -> Vec<(String, String)> {
        watches()
            .iter()
            .filter(|(home, ..)| home == &self.home)
            .map(|(_, _, namespace, resource)| (namespace.clone(), resource.clone()))
            .collect()
    }

    /// HS2-G1PKK2 hosted half: wait on this machine's claim backend until a resource
    /// is free or `wait_ms` (at most [`MAX_RESOURCE_WAIT_MS`]) passes. Woken by the
    /// backend's own signal; a lapse counts as free. The checkout must be one this
    /// machine hosts.
    pub fn hosted_resource_wait(
        &self,
        checkout_id: &str,
        namespace: &str,
        resource: &str,
        wait_ms: u64,
    ) -> Result<ResourceWait, OrchestrationError> {
        self.owner(checkout_id)?;
        ResourceRef::parse(&format!("{namespace}/{resource}"))
            .map_err(|_| OrchestrationError::Invalid("a namespace and resource are required"))?;
        Ok(ResourceWait {
            free: self.claims.wait_free(
                namespace,
                resource,
                Duration::from_millis(wait_ms.min(MAX_RESOURCE_WAIT_MS)),
            ),
        })
    }

    /// HS2-G1PKK2: the owner's active target bound to a checkout on `server_id`, the
    /// machine whose backend holds a claim. Read from this machine's bindings alone.
    fn target_on_server(&self, owner_id: &str, server_id: &str) -> Option<TargetRef> {
        let declarations = self.declarations(owner_id).ok()?;
        declarations
            .targets
            .into_iter()
            .filter(|declaration| declaration.state == DeclarationState::Active)
            .filter_map(|declaration| Self::target_ref(owner_id, &declaration.target_id).ok())
            .find(|target| {
                Self::binding_at(&self.home, target).is_ok_and(|binding| {
                    matches!(
                        binding.destination,
                        Some(TargetDestination::Remote { ref identity, .. })
                            if identity.server_id == server_id
                    )
                })
            })
    }

    /// HS2-G1PKK2: follow one resource held on another machine until it is free, the
    /// deadline passes, or the peer cannot be asked. Each request is a long poll on the
    /// peer's own wait; a held answer asks again at once. Wakes the scheduler only on
    /// free.
    pub fn follow_remote_release(
        &self,
        owner_id: &str,
        target: &TargetRef,
        actor: &MutationActor,
        held: &ResourceNeed,
        deadline: Instant,
        tx: &mut Transaction<'_>,
    ) -> RemoteRelease {
        let (namespace, resource) = (&held.namespace, &held.resource);
        let query = format!(
            "?namespace={}&resource={}&wait_ms={MAX_RESOURCE_WAIT_MS}",
            encode(namespace),
            encode(resource)
        );
        let mut authorized: Option<(Authorized, u64)> = None;
        loop {
            if Instant::now() >= deadline {
                return RemoteRelease::Ended("deadline".into());
            }
            let now_ms = tx.clock.now_ms();
            let stale = authorized
                .as_ref()
                .is_none_or(|(_, at)| now_ms.saturating_sub(*at) >= REAUTHORIZE_MS);
            if stale {
                // Observing a target never asks a person; a refusal ends the watch.
                match self.authorize(
                    target,
                    Some(actor),
                    ActionRequest::new(TargetAction::GatesObserve),
                    tx,
                ) {
                    Ok(fresh) => authorized = Some((fresh, now_ms)),
                    Err(error) => return RemoteRelease::Ended(error.code().into()),
                }
            }
            let Some((current, _)) = &authorized else {
                return RemoteRelease::Ended("gate_refused".into());
            };
            let asked = Instant::now();
            let reply = Self::destination(current)
                .and_then(|destination| {
                    self.hosted_call_at(
                        destination,
                        "/target-resource-wait",
                        "GET",
                        &query,
                        None,
                        tx,
                    )
                })
                .and_then(|reply| {
                    serde_json::from_value::<ResourceWait>(reply).map_err(|_| {
                        OrchestrationError::Observation(TargetFailure::InvalidResponse)
                    })
                });
            match reply {
                Ok(ResourceWait { free: true }) => {
                    self.scheduler_wake(owner_id);
                    return RemoteRelease::Released;
                }
                // A peer that answers held long before the wait it was asked for is
                // not waiting: asking it again at once would spin.
                Ok(ResourceWait { free: false })
                    if asked.elapsed() < Duration::from_millis(MAX_RESOURCE_WAIT_MS / 2) =>
                {
                    return RemoteRelease::Ended("peer_did_not_wait".into());
                }
                Ok(ResourceWait { free: false }) => {}
                Err(error) => return RemoteRelease::Ended(error.code().into()),
            }
        }
    }

    /// Wait for every held resource that deferred work in a pass, and wake the
    /// scheduler when one is released or lapses. A resource is waited on once however
    /// many passes defer work for it. A claim on this machine's backend is waited on
    /// with the claim service's wait; HS2-G1PKK2: a claim on another machine's backend
    /// is waited on with that machine's wait over the reviewed target transport, as
    /// `actor`. Its expiry stays the schedule's retry time either way.
    pub(super) fn watch_releases(
        &self,
        owner_id: &str,
        actor: &MutationActor,
        schedule: &Schedule,
    ) {
        for entry in &schedule.contention {
            if entry.state != ResourceState::Held {
                continue;
            }
            let Some(server_id) = entry.server_id.as_deref() else {
                continue;
            };
            let remote = (server_id != self.server_id.as_str())
                .then(|| self.target_on_server(owner_id, server_id))
                .map(|target| target.map(|target| (server_id.to_owned(), target)));
            // A claim on a machine no bound target of this owner reaches is left to
            // its retry time.
            if matches!(remote, Some(None)) {
                continue;
            }
            let remote = remote.flatten();
            let key = (
                self.home.clone(),
                remote
                    .as_ref()
                    .map(|(server, _)| server.clone())
                    .unwrap_or_default(),
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
            let thread_key = key.clone();
            let spawned = match remote {
                None => {
                    let claims = self.claims.clone();
                    std::thread::Builder::new()
                        .name("orchestration-claim-wait".into())
                        .spawn(move || {
                            let (home, _, namespace, resource) = &thread_key;
                            // Free or not, the next pass reads the claim again and,
                            // when it is still held, waits again.
                            claims.wait_free(namespace, resource, MAX_WAIT);
                            watches().remove(&thread_key);
                            Self::scheduler_wake_at(home);
                        })
                }
                Some((_, target)) => {
                    let runtime = self.clone();
                    let owner = owner_id.to_owned();
                    let actor = actor.clone();
                    std::thread::Builder::new()
                        .name("orchestration-remote-claim-wait".into())
                        .spawn(move || {
                            let (_, _, namespace, resource) = &thread_key;
                            let mut rng = super::request::Rng;
                            runtime.follow_remote_release(
                                &owner,
                                &target,
                                &actor,
                                &ResourceNeed {
                                    namespace: namespace.clone(),
                                    resource: resource.clone(),
                                },
                                Instant::now() + MAX_WAIT,
                                &mut Transaction {
                                    clock: &super::request::Clock,
                                    rng: &mut rng,
                                },
                            );
                            watches().remove(&thread_key);
                        })
                }
            };
            if spawned.is_err() {
                watches().remove(&key);
            }
        }
    }
}
