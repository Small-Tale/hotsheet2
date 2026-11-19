//! The built-in local machine backend.
//!
//! State lives under `<HOTSHEET_HOME>/resource-claims/`: one bounded JSON file per
//! namespace, changed only under that namespace's OS file lock and published by atomic
//! rename. Every project on the machine sees the same claims. The backend's clock is the
//! machine's clock.

mod storage;
mod waiters;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::backend::ClaimBackend;
use crate::clock::{Clock, SystemClock};
use crate::codec::{FileKeyProvider, KeyProvider, MetadataCodecs};
use crate::error::ClaimError;
use crate::model::{
    AcquireRequest, Capabilities, Claim, Grant, Guarantee, MAX_LEASE, MAX_METADATA_BYTES,
    MAX_METADATA_ENTRIES, Namespace, ResourceRef, WaitMechanism, WaitOutcome, lease_ms, rfc3339,
    validate_metadata, wait_ms,
};
use crate::token;
use storage::{MAX_RESOURCES, NamespaceStore, Record, SCHEMA_VERSION, State, StoredClaim};
use waiters::Waiter;

/// The directory under the machine home that holds every machine-scoped namespace.
pub const LOCAL_ROOT_DIR: &str = "resource-claims";

pub struct LocalBackend {
    root: PathBuf,
    clock: Arc<dyn Clock>,
    keys: Arc<dyn KeyProvider>,
    inspections: AtomicU64,
}

impl std::fmt::Debug for LocalBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalBackend")
            .field("root", &self.root)
            .finish_non_exhaustive()
    }
}

impl LocalBackend {
    /// The machine backend rooted at `<home>/resource-claims`, on the system clock, with
    /// its key in `<home>/resource-claims/key` (owner-only).
    pub fn machine(home: &Path) -> Self {
        let root = home.join(LOCAL_ROOT_DIR);
        let keys = Arc::new(FileKeyProvider::new(root.join("key")));
        Self::new(root, Arc::new(SystemClock), keys)
    }

    /// A backend with an injected clock and key source.
    pub fn new(root: PathBuf, clock: Arc<dyn Clock>, keys: Arc<dyn KeyProvider>) -> Self {
        Self {
            root,
            clock,
            keys,
            inspections: AtomicU64::new(0),
        }
    }

    /// How many times this instance has read claim state. A waiter reads once per
    /// wake-up, so an idle wait leaves this unchanged; tests pin that.
    pub fn inspections(&self) -> u64 {
        self.inspections.load(Ordering::SeqCst)
    }

    /// Wake every waiter in a namespace so it re-reads state. Releases do this
    /// themselves; it is public for a caller that moved an injected clock.
    pub fn signal(&self, namespace: &Namespace) -> Result<(), ClaimError> {
        Ok(waiters::wake_all(
            &self.store(namespace).waiters_dir(),
            self.clock.now_ms(),
        )?)
    }

    fn store(&self, namespace: &Namespace) -> NamespaceStore {
        NamespaceStore::new(self.root.join("namespaces").join(namespace.as_str()))
    }

    fn read(&self, namespace: &Namespace) -> Result<State, ClaimError> {
        self.inspections.fetch_add(1, Ordering::SeqCst);
        self.store(namespace).read()
    }

    fn view(&self, target: &ResourceRef, record: &Record, stored: &StoredClaim) -> Claim {
        Claim {
            namespace: target.namespace().clone(),
            resource: target.resource().to_owned(),
            claim_id: stored.claim_id.clone(),
            fencing_token: record.fencing,
            guarantee: Guarantee::ExclusiveWithFencing,
            acquired_at_ms: stored.acquired_at_ms,
            expires_at_ms: stored.expires_at_ms,
            expires_at: rfc3339(stored.expires_at_ms),
            metadata: stored.metadata.clone(),
        }
    }

    fn live(&self, target: &ResourceRef, state: &State, now_ms: u64) -> Option<Claim> {
        let record = state.resources.get(target.resource())?;
        let stored = record.claim.as_ref()?;
        (stored.expires_at_ms > now_ms).then(|| self.view(target, record, stored))
    }

    /// Run one compare-and-swap on a namespace under its lock. `change` returns its
    /// result and whether waiters must be woken; an error publishes nothing.
    fn transact<T>(
        &self,
        namespace: &Namespace,
        create: bool,
        change: impl FnOnce(&mut State, u64) -> Result<(T, bool), ClaimError>,
    ) -> Result<T, ClaimError> {
        let store = self.store(namespace);
        // A token for a namespace that was never written cannot be valid.
        let Some(lock) = store.lock(create)? else {
            return Err(ClaimError::TokenInvalid);
        };
        self.inspections.fetch_add(1, Ordering::SeqCst);
        let mut state = store.read()?;
        let now_ms = self.clock.now_ms();
        let (value, wake) = change(&mut state, now_ms)?;
        state.schema_version = SCHEMA_VERSION;
        store.write(&state)?;
        drop(lock);
        if wake {
            // Publication already succeeded; a failed wake-up only delays a waiter to
            // the claim's former expiry, so it never fails the operation.
            let _ = waiters::wake_all(&store.waiters_dir(), now_ms);
        }
        Ok(value)
    }

    /// Check a renewal token against the stored claim it names.
    fn authorize<'state>(
        &self,
        state: &'state mut State,
        renewal_token: &str,
        claims: &token::RenewalTokenClaims,
    ) -> Result<&'state mut Record, ClaimError> {
        let record = state
            .resources
            .get_mut(claims.target.resource())
            .ok_or(ClaimError::TokenInvalid)?;
        // A released claim leaves no digest behind: its token is revoked.
        let stored = record.claim.as_ref().ok_or(ClaimError::TokenInvalid)?;
        if stored.claim_id != claims.claim_id {
            return Err(ClaimError::NotHolder);
        }
        let key = self.keys.key()?.token_key();
        if !token::matches(&key, renewal_token, &stored.token_digest) {
            return Err(ClaimError::TokenInvalid);
        }
        Ok(record)
    }

    fn register_waiter(
        &self,
        namespace: &Namespace,
        deadline_ms: u64,
    ) -> Result<Waiter, ClaimError> {
        let store = self.store(namespace);
        // Creating the namespace directory here keeps the lock file and the waiter
        // registration under one owner-only directory.
        drop(store.lock(true)?);
        Waiter::register(&store.waiters_dir(), deadline_ms)?.ok_or_else(|| {
            ClaimError::Capacity(format!(
                "namespace '{namespace}' already has {} waiters",
                waiters::MAX_WAITERS
            ))
        })
    }
}

/// The caller's own wait, measured on the monotonic clock (HS2-K3ZXR5).
///
/// A wait is a local duration, not a shared instant: deciding it from whole wall-clock
/// milliseconds let a waiter give up up to a millisecond early (both ends truncated),
/// and a stepped or slewed wall clock could end it sooner still. The holder's expiry
/// stays on the backend clock; only "has the caller waited long enough" is monotonic.
struct WaitDeadline {
    at: Instant,
    /// The same deadline on the backend clock, published in the waiter registration
    /// only so a crashed waiter's registration can be cleaned up later.
    backend_ms: u64,
}

impl WaitDeadline {
    fn new(clock: &dyn Clock, timeout: Duration) -> Result<Self, ClaimError> {
        let wait = wait_ms(timeout)?;
        Ok(Self {
            at: Instant::now() + timeout,
            // Round up so the registration never names an earlier instant than the wait.
            backend_ms: clock.now_ms().saturating_add(wait).saturating_add(1),
        })
    }

    /// What is left of the wait, or `None` once it has fully elapsed.
    fn remaining(&self) -> Option<Duration> {
        let left = self.at.saturating_duration_since(Instant::now());
        (!left.is_zero()).then_some(left)
    }
}

/// Round a sleep up to whole milliseconds. A socket timeout is truncated to whole
/// microseconds by the OS interface, so an exact sleep could wake a hair before the
/// deadline and cost a second inspection; rounding up wakes at or after it.
fn ceil_ms(duration: Duration) -> Duration {
    let millis = duration.as_nanos().div_ceil(1_000_000);
    Duration::from_millis(u64::try_from(millis).unwrap_or(u64::MAX))
}

/// Sleep once until the earlier of the holder's expiry and the caller's deadline, or
/// until a release wakes the waiter. `false` means the caller's deadline has passed.
fn park_until(
    waiter: &Waiter,
    clock: &dyn Clock,
    holder: &Claim,
    deadline: &WaitDeadline,
) -> Result<bool, ClaimError> {
    let Some(left) = deadline.remaining() else {
        return Ok(false);
    };
    let to_expiry = Duration::from_millis(holder.expires_at_ms.saturating_sub(clock.now_ms()));
    waiter.park(ceil_ms(left).min(to_expiry))?;
    Ok(true)
}

impl ClaimBackend for LocalBackend {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            backend: "local".into(),
            guarantee: Guarantee::ExclusiveWithFencing,
            wait: WaitMechanism::PushSignal,
            max_metadata_entries: MAX_METADATA_ENTRIES,
            max_metadata_bytes: MAX_METADATA_BYTES,
            max_lease_ms: u64::try_from(MAX_LEASE.as_millis()).unwrap_or(u64::MAX),
        }
    }

    fn codecs(&self) -> Result<MetadataCodecs, ClaimError> {
        MetadataCodecs::new(&self.keys.key()?)
    }

    fn acquire(&self, target: &ResourceRef, request: &AcquireRequest) -> Result<Grant, ClaimError> {
        let lease_ms = lease_ms(request.duration)?;
        validate_metadata(&request.metadata)?;
        let token_key = self.keys.key()?.token_key();
        self.transact(target.namespace(), true, |state, now_ms| {
            if let Some(holder) = self.live(target, state, now_ms) {
                return Err(ClaimError::HeldBy {
                    holder: Box::new(holder),
                });
            }
            if !state.resources.contains_key(target.resource())
                && state.resources.len() >= MAX_RESOURCES
            {
                return Err(ClaimError::Capacity(format!(
                    "namespace '{}' already tracks {MAX_RESOURCES} resources",
                    target.namespace()
                )));
            }
            let record = state
                .resources
                .entry(target.resource().to_owned())
                .or_default();
            record.fencing = record
                .fencing
                .checked_add(1)
                .ok_or_else(|| ClaimError::Capacity("fencing counter is exhausted".into()))?;
            let claim_id = ulid::Ulid::new().to_string();
            let renewal_token = token::mint(target, &claim_id)?;
            let stored = StoredClaim {
                claim_id,
                token_digest: token::digest(&token_key, &renewal_token),
                acquired_at_ms: now_ms,
                expires_at_ms: now_ms.saturating_add(lease_ms),
                metadata: request.metadata.clone(),
            };
            let claim = self.view(target, record, &stored);
            record.claim = Some(stored);
            Ok((
                Grant {
                    claim,
                    renewal_token,
                },
                false,
            ))
        })
    }

    fn acquire_wait(
        &self,
        target: &ResourceRef,
        request: &AcquireRequest,
        timeout: Duration,
    ) -> Result<Grant, ClaimError> {
        let deadline = WaitDeadline::new(self.clock.as_ref(), timeout)?;
        lease_ms(request.duration)?;
        validate_metadata(&request.metadata)?;
        // Register before the first attempt: a release that lands between the attempt
        // and the sleep must find this waiter.
        let waiter = self.register_waiter(target.namespace(), deadline.backend_ms)?;
        loop {
            let holder = match self.acquire(target, request) {
                Err(ClaimError::HeldBy { holder }) => holder,
                other => return other,
            };
            if !park_until(&waiter, self.clock.as_ref(), &holder, &deadline)? {
                return Err(ClaimError::WaitTimedOut { holder });
            }
        }
    }

    fn renew(&self, renewal_token: &str, duration: Duration) -> Result<Claim, ClaimError> {
        let claims = token::parse_renewal_token(renewal_token)?;
        let lease_ms = lease_ms(duration)?;
        self.transact(claims.target.namespace(), false, |state, now_ms| {
            let record = self.authorize(state, renewal_token, &claims)?;
            let Some(stored) = record.claim.as_mut() else {
                return Err(ClaimError::TokenInvalid);
            };
            // A lapsed lease is over even when nobody has taken the resource since.
            if stored.expires_at_ms <= now_ms {
                return Err(ClaimError::Expired);
            }
            let previous = stored.expires_at_ms;
            stored.expires_at_ms = now_ms.saturating_add(lease_ms);
            let shortened = stored.expires_at_ms < previous;
            let stored = stored.clone();
            Ok((self.view(&claims.target, record, &stored), shortened))
        })
    }

    fn release(&self, renewal_token: &str) -> Result<Claim, ClaimError> {
        let claims = token::parse_renewal_token(renewal_token)?;
        let (claim, expired) =
            self.transact(claims.target.namespace(), false, |state, now_ms| {
                let record = self.authorize(state, renewal_token, &claims)?;
                let Some(stored) = record.claim.take() else {
                    return Err(ClaimError::TokenInvalid);
                };
                let expired = stored.expires_at_ms <= now_ms;
                let claim = self.view(&claims.target, record, &stored);
                Ok(((claim, expired), !expired))
            })?;
        // Either way the slot is cleared and the token revoked; a lapsed lease still
        // reports that it had lapsed.
        if expired {
            Err(ClaimError::Expired)
        } else {
            Ok(claim)
        }
    }

    fn get(&self, target: &ResourceRef) -> Result<Option<Claim>, ClaimError> {
        let state = self.read(target.namespace())?;
        Ok(self.live(target, &state, self.clock.now_ms()))
    }

    fn list(&self, namespace: &Namespace) -> Result<Vec<Claim>, ClaimError> {
        let state = self.read(namespace)?;
        let now_ms = self.clock.now_ms();
        let mut claims = Vec::new();
        for resource in state.resources.keys() {
            let target = ResourceRef::new(namespace.clone(), resource)?;
            claims.extend(self.live(&target, &state, now_ms));
        }
        Ok(claims)
    }

    fn namespaces(&self) -> Result<Vec<Namespace>, ClaimError> {
        let entries = match std::fs::read_dir(self.root.join("namespaces")) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        let mut names = Vec::new();
        for entry in entries {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            // A directory that is not a namespace name was not written by this backend.
            if let Some(name) = entry
                .file_name()
                .to_str()
                .and_then(|name| Namespace::parse(name).ok())
            {
                names.push(name);
            }
        }
        names.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        Ok(names)
    }

    fn wait_free(
        &self,
        target: &ResourceRef,
        timeout: Duration,
    ) -> Result<WaitOutcome, ClaimError> {
        let deadline = WaitDeadline::new(self.clock.as_ref(), timeout)?;
        // Nothing to wait for: do not create a namespace or a waiter for a free resource.
        if self.get(target)?.is_none() {
            return Ok(WaitOutcome::Free);
        }
        let waiter = self.register_waiter(target.namespace(), deadline.backend_ms)?;
        loop {
            let Some(holder) = self.get(target)? else {
                return Ok(WaitOutcome::Free);
            };
            if !park_until(&waiter, self.clock.as_ref(), &holder, &deadline)? {
                return Ok(WaitOutcome::TimedOut {
                    holder: Box::new(holder),
                });
            }
        }
    }

    fn verify_fencing(&self, target: &ResourceRef, fencing_token: u64) -> Result<(), ClaimError> {
        let state = self.read(target.namespace())?;
        let record = state.resources.get(target.resource());
        let latest = record.map_or(0, |record| record.fencing);
        if fencing_token == 0 || fencing_token > latest {
            return Err(ClaimError::invalid("fencing token was never issued"));
        }
        if fencing_token < latest {
            return Err(ClaimError::NotHolder);
        }
        match self.live(target, &state, self.clock.now_ms()) {
            Some(_) => Ok(()),
            None => Err(ClaimError::Expired),
        }
    }
}
