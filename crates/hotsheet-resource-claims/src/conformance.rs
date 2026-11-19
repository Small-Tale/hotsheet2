//! The conformance suite every claim backend must pass, built-in or external
//! (HS2-322ZSZ, `docs/36` §36.16). A backend's own test builds a [`Fixture`] and calls
//! [`run`]; a failure panics naming the check and what the backend did instead.
//!
//! The checks that depend on exclusivity run only for a backend that advertises
//! [`Guarantee::ExclusiveWithFencing`]; waiting checks follow its advertised
//! [`WaitMechanism`]. Nothing here reaches into a backend's storage.

use std::collections::BTreeMap;
use std::sync::{Barrier, Mutex};
use std::time::Duration;

use crate::backend::ClaimBackend;
use crate::error::ClaimError;
use crate::model::{AcquireRequest, Guarantee, MAX_LEASE, Namespace, ResourceRef, WaitMechanism};
use crate::token::parse_renewal_token;

/// What a backend's test supplies.
pub struct Fixture<'a> {
    pub backend: &'a dyn ClaimBackend,
    /// A namespace no other test uses, so runs never see each other's claims.
    pub namespace: Namespace,
    /// Let `duration` pass at the backend's clock: advance a manual clock, or sleep.
    pub advance: &'a (dyn Fn(Duration) + Sync),
    /// A lease short enough to expire inside the test, long enough to survive one call.
    pub short_lease: Duration,
}

const LEASE: Duration = Duration::from_secs(60);

impl Fixture<'_> {
    fn target(&self, name: &str) -> ResourceRef {
        ResourceRef::new(self.namespace.clone(), name).expect("conformance resource name")
    }

    fn exclusive(&self) -> bool {
        self.backend.capabilities().guarantee == Guarantee::ExclusiveWithFencing
    }
}

fn fail(check: &str, what: impl std::fmt::Display) -> ! {
    panic!("claim backend conformance '{check}': {what}")
}

#[track_caller]
fn expect_code<T: std::fmt::Debug>(check: &str, got: Result<T, ClaimError>, codes: &[&str]) {
    match got {
        Err(error) if codes.contains(&error.code()) => {}
        Err(error) => fail(check, format!("expected {codes:?}, got {}", error.code())),
        Ok(value) => fail(check, format!("expected {codes:?}, got Ok({value:?})")),
    }
}

/// Run every check against one backend.
pub fn run(f: &Fixture<'_>) {
    capabilities_are_sane(f);
    acquire_grants_a_claim_and_a_scoped_token(f);
    a_held_resource_refuses_a_second_acquire(f);
    readers_see_the_claim_without_its_token(f);
    metadata_round_trips(f);
    renew_extends_and_rejects_foreign_tokens(f);
    release_frees_the_resource_and_revokes_the_token(f);
    fencing_tokens_only_grow(f);
    a_lapsed_claim_expires_for_every_reader(f);
    verify_fencing_follows_the_live_claim(f);
    malformed_requests_are_refused(f);
    concurrent_acquirers_have_one_winner(f);
    waiting_follows_the_advertised_mechanism(f);
}

pub fn capabilities_are_sane(f: &Fixture<'_>) {
    let caps = f.backend.capabilities();
    if caps.backend.is_empty() || caps.max_lease_ms == 0 || caps.max_metadata_entries == 0 {
        fail("capabilities", format!("{caps:?}"));
    }
    if caps.max_lease_ms > u64::try_from(MAX_LEASE.as_millis()).unwrap_or(u64::MAX) {
        fail(
            "capabilities",
            "max_lease_ms exceeds Hot Sheet's 24 hour bound",
        );
    }
}

pub fn acquire_grants_a_claim_and_a_scoped_token(f: &Fixture<'_>) {
    let target = f.target("grant");
    let grant = f
        .backend
        .acquire(&target, &AcquireRequest::new(LEASE))
        .unwrap_or_else(|e| fail("acquire", e));
    if grant.claim.claim_id.is_empty() || grant.claim.expires_at_ms <= grant.claim.acquired_at_ms {
        fail(
            "acquire",
            format!("a grant needs an id and a future expiry: {grant:?}"),
        );
    }
    if f.exclusive() && grant.claim.fencing_token == 0 {
        fail(
            "acquire",
            "an exclusive backend grants a fencing token above 0",
        );
    }
    let named = parse_renewal_token(&grant.renewal_token)
        .unwrap_or_else(|e| fail("token", format!("the token must name its claim: {e}")));
    if named.target != target || named.claim_id != grant.claim.claim_id {
        fail("token", "the token names a different claim than the grant");
    }
    f.backend
        .release(&grant.renewal_token)
        .unwrap_or_else(|e| fail("release", e));
}

pub fn a_held_resource_refuses_a_second_acquire(f: &Fixture<'_>) {
    if !f.exclusive() {
        return;
    }
    let target = f.target("held");
    let first = f
        .backend
        .acquire(&target, &AcquireRequest::new(LEASE))
        .unwrap();
    match f.backend.acquire(&target, &AcquireRequest::new(LEASE)) {
        Err(ClaimError::HeldBy { holder }) => {
            if holder.claim_id != first.claim.claim_id {
                fail("held_by", "the conflict must name the live holder");
            }
        }
        other => fail("held_by", format!("expected held_by, got {other:?}")),
    }
    f.backend.release(&first.renewal_token).unwrap();
}

pub fn readers_see_the_claim_without_its_token(f: &Fixture<'_>) {
    let target = f.target("reader");
    if f.backend
        .get(&target)
        .unwrap_or_else(|e| fail("get", e))
        .is_some()
    {
        fail("get", "a never-claimed resource reads as free");
    }
    let grant = f
        .backend
        .acquire(&target, &AcquireRequest::new(LEASE))
        .unwrap();
    let live = f
        .backend
        .get(&target)
        .unwrap_or_else(|e| fail("get", e))
        .unwrap_or_else(|| fail("get", "a granted resource reads as held"));
    if live.claim_id != grant.claim.claim_id || live.fencing_token != grant.claim.fencing_token {
        fail("get", "the reader sees a different claim than the grant");
    }
    let listed = f
        .backend
        .list(&f.namespace)
        .unwrap_or_else(|e| fail("list", e));
    if !listed.iter().any(|c| c.claim_id == grant.claim.claim_id) {
        fail("list", "the namespace listing omits a live claim");
    }
    let namespaces = f
        .backend
        .namespaces()
        .unwrap_or_else(|e| fail("namespaces", e));
    if !namespaces.contains(&f.namespace) {
        fail("namespaces", "a namespace holding a live claim is listed");
    }
    f.backend.release(&grant.renewal_token).unwrap();
    if f.backend.get(&target).unwrap().is_some() {
        fail("get", "a released resource reads as free");
    }
}

pub fn metadata_round_trips(f: &Fixture<'_>) {
    let target = f.target("meta");
    let mut metadata = BTreeMap::new();
    metadata.insert("owner".to_owned(), "plain:synthetic-owner".to_owned());
    metadata.insert("purpose".to_owned(), "plain:synthetic purpose".to_owned());
    let grant = f
        .backend
        .acquire(
            &target,
            &AcquireRequest::new(LEASE).with_metadata(metadata.clone()),
        )
        .unwrap_or_else(|e| fail("metadata", e));
    let live = f.backend.get(&target).unwrap().unwrap();
    if live.metadata != metadata || grant.claim.metadata != metadata {
        fail(
            "metadata",
            "stored metadata must come back exactly as written",
        );
    }
    f.backend.release(&grant.renewal_token).unwrap();
}

pub fn renew_extends_and_rejects_foreign_tokens(f: &Fixture<'_>) {
    let target = f.target("renew");
    let grant = f
        .backend
        .acquire(&target, &AcquireRequest::new(Duration::from_secs(30)))
        .unwrap();
    let renewed = f
        .backend
        .renew(&grant.renewal_token, Duration::from_secs(300))
        .unwrap_or_else(|e| fail("renew", e));
    if renewed.claim_id != grant.claim.claim_id
        || renewed.expires_at_ms <= grant.claim.expires_at_ms
    {
        fail(
            "renew",
            "a renewal keeps the claim and moves its expiry later",
        );
    }
    if renewed.fencing_token != grant.claim.fencing_token {
        fail("renew", "a renewal keeps the fencing token");
    }
    expect_code(
        "garbage token",
        f.backend.renew("not-a-token", LEASE),
        &["token_invalid"],
    );
    expect_code(
        "garbage token",
        f.backend.release("hsrc1.x.y.z"),
        &["token_invalid"],
    );
    // A token of an earlier claim never renews a later one on the same resource.
    f.backend.release(&grant.renewal_token).unwrap();
    let later = f
        .backend
        .acquire(&target, &AcquireRequest::new(LEASE))
        .unwrap();
    expect_code(
        "foreign token",
        f.backend.renew(&grant.renewal_token, LEASE),
        &["not_holder", "token_invalid"],
    );
    expect_code(
        "foreign token",
        f.backend.release(&grant.renewal_token),
        &["not_holder", "token_invalid"],
    );
    f.backend.release(&later.renewal_token).unwrap();
}

pub fn release_frees_the_resource_and_revokes_the_token(f: &Fixture<'_>) {
    let target = f.target("release");
    let grant = f
        .backend
        .acquire(&target, &AcquireRequest::new(LEASE))
        .unwrap();
    let ended = f
        .backend
        .release(&grant.renewal_token)
        .unwrap_or_else(|e| fail("release", e));
    if ended.claim_id != grant.claim.claim_id {
        fail("release", "release reports the claim it ended");
    }
    expect_code(
        "revoked token",
        f.backend.renew(&grant.renewal_token, LEASE),
        &["token_invalid", "not_holder", "expired"],
    );
    expect_code(
        "release twice",
        f.backend.release(&grant.renewal_token),
        &["token_invalid", "not_holder", "expired"],
    );
    let again = f
        .backend
        .acquire(&target, &AcquireRequest::new(LEASE))
        .unwrap_or_else(|e| fail("reacquire", e));
    f.backend.release(&again.renewal_token).unwrap();
}

pub fn fencing_tokens_only_grow(f: &Fixture<'_>) {
    if !f.exclusive() {
        return;
    }
    let target = f.target("fence");
    let mut last = 0;
    for round in 0..3 {
        let grant = f
            .backend
            .acquire(&target, &AcquireRequest::new(LEASE))
            .unwrap();
        if grant.claim.fencing_token <= last {
            fail(
                "fencing",
                format!(
                    "round {round}: token {} not above {last}",
                    grant.claim.fencing_token
                ),
            );
        }
        last = grant.claim.fencing_token;
        f.backend.release(&grant.renewal_token).unwrap();
    }
    // And across an expiry, not only a release.
    let short = f
        .backend
        .acquire(&target, &AcquireRequest::new(f.short_lease))
        .unwrap();
    (f.advance)(f.short_lease + Duration::from_millis(50));
    let after = f
        .backend
        .acquire(&target, &AcquireRequest::new(LEASE))
        .unwrap();
    if after.claim.fencing_token <= short.claim.fencing_token {
        fail(
            "fencing",
            "a grant after an expiry still carries a larger token",
        );
    }
    f.backend.release(&after.renewal_token).unwrap();
}

pub fn a_lapsed_claim_expires_for_every_reader(f: &Fixture<'_>) {
    let target = f.target("lapse");
    let grant = f
        .backend
        .acquire(&target, &AcquireRequest::new(f.short_lease))
        .unwrap();
    (f.advance)(f.short_lease + Duration::from_millis(50));
    if f.backend.get(&target).unwrap().is_some() {
        fail("expiry", "a lapsed claim reads as free");
    }
    expect_code(
        "expiry",
        f.backend.renew(&grant.renewal_token, LEASE),
        &["expired"],
    );
    let taken = f
        .backend
        .acquire(&target, &AcquireRequest::new(LEASE))
        .unwrap_or_else(|e| fail("expiry", format!("a lapsed resource is acquirable: {e}")));
    f.backend.release(&taken.renewal_token).unwrap();
}

pub fn verify_fencing_follows_the_live_claim(f: &Fixture<'_>) {
    if !f.exclusive() {
        return;
    }
    let target = f.target("verify");
    let first = f
        .backend
        .acquire(&target, &AcquireRequest::new(LEASE))
        .unwrap();
    f.backend
        .verify_fencing(&target, first.claim.fencing_token)
        .unwrap_or_else(|e| fail("verify_fencing", e));
    f.backend.release(&first.renewal_token).unwrap();
    let second = f
        .backend
        .acquire(&target, &AcquireRequest::new(LEASE))
        .unwrap();
    expect_code(
        "verify_fencing",
        f.backend.verify_fencing(&target, first.claim.fencing_token),
        &["not_holder"],
    );
    f.backend
        .verify_fencing(&target, second.claim.fencing_token)
        .unwrap_or_else(|e| fail("verify_fencing", e));
    f.backend.release(&second.renewal_token).unwrap();
}

pub fn malformed_requests_are_refused(f: &Fixture<'_>) {
    let target = f.target("malformed");
    expect_code(
        "zero lease",
        f.backend
            .acquire(&target, &AcquireRequest::new(Duration::ZERO)),
        &["invalid"],
    );
    expect_code(
        "oversized lease",
        f.backend.acquire(
            &target,
            &AcquireRequest::new(MAX_LEASE + Duration::from_secs(1)),
        ),
        &["invalid"],
    );
    let mut bad = BTreeMap::new();
    bad.insert("owner".to_owned(), "no codec prefix".to_owned());
    expect_code(
        "unprefixed metadata",
        f.backend
            .acquire(&target, &AcquireRequest::new(LEASE).with_metadata(bad)),
        &["invalid"],
    );
    if f.backend.get(&target).unwrap().is_some() {
        fail("malformed", "a refused request must leave nothing held");
    }
}

pub fn concurrent_acquirers_have_one_winner(f: &Fixture<'_>) {
    if !f.exclusive() {
        return;
    }
    const RACERS: usize = 6;
    let target = f.target("race");
    let barrier = Barrier::new(RACERS);
    let outcomes = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..RACERS {
            scope.spawn(|| {
                barrier.wait();
                let outcome = f.backend.acquire(&target, &AcquireRequest::new(LEASE));
                outcomes.lock().unwrap().push(outcome);
            });
        }
    });
    let outcomes = outcomes.into_inner().unwrap();
    let winners: Vec<_> = outcomes.iter().filter_map(|o| o.as_ref().ok()).collect();
    if winners.len() != 1 {
        fail(
            "concurrency",
            format!("{} winners of {RACERS}", winners.len()),
        );
    }
    for outcome in &outcomes {
        if let Err(error) = outcome
            && error.code() != "held_by"
        {
            fail(
                "concurrency",
                format!("a loser must see held_by, saw {}", error.code()),
            );
        }
    }
    f.backend.release(&winners[0].renewal_token).unwrap();
}

pub fn waiting_follows_the_advertised_mechanism(f: &Fixture<'_>) {
    let target = f.target("wait");
    let holder = f
        .backend
        .acquire(&target, &AcquireRequest::new(LEASE))
        .unwrap();
    // A zero wait is a try-now that names the holder, whatever the mechanism.
    match f
        .backend
        .acquire_wait(&target, &AcquireRequest::new(LEASE), Duration::ZERO)
    {
        Err(ClaimError::WaitTimedOut { holder: named }) if f.exclusive() => {
            if named.claim_id != holder.claim.claim_id {
                fail("wait", "a timed-out wait names the live holder");
            }
        }
        Err(ClaimError::WaitTimedOut { .. }) => {}
        other if !f.exclusive() => drop(other),
        other => fail(
            "wait",
            format!("a zero wait times out naming the holder: {other:?}"),
        ),
    }
    if !f.exclusive() {
        f.backend.release(&holder.renewal_token).ok();
        return;
    }
    match f.backend.capabilities().wait {
        WaitMechanism::None => {
            expect_code(
                "wait unsupported",
                f.backend.acquire_wait(
                    &target,
                    &AcquireRequest::new(LEASE),
                    Duration::from_millis(200),
                ),
                &["unsupported"],
            );
            f.backend.release(&holder.renewal_token).unwrap();
        }
        WaitMechanism::PushSignal | WaitMechanism::LongPoll => {
            // Held: the wait times out naming the holder.
            match f.backend.acquire_wait(
                &target,
                &AcquireRequest::new(LEASE),
                Duration::from_millis(300),
            ) {
                Err(ClaimError::WaitTimedOut { holder: named })
                    if named.claim_id == holder.claim.claim_id => {}
                other => fail(
                    "wait",
                    format!("a held wait times out naming the holder: {other:?}"),
                ),
            }
            // A release wakes the waiter, which then holds the resource.
            let waiter = std::thread::scope(|scope| {
                let handle = scope.spawn(|| {
                    f.backend.acquire_wait(
                        &target,
                        &AcquireRequest::new(LEASE),
                        Duration::from_secs(20),
                    )
                });
                std::thread::sleep(Duration::from_millis(250));
                f.backend.release(&holder.renewal_token).unwrap();
                handle.join().unwrap()
            });
            let grant =
                waiter.unwrap_or_else(|e| fail("wait", format!("a release wakes the waiter: {e}")));
            if grant.claim.fencing_token <= holder.claim.fencing_token {
                fail("wait", "the waiter's grant carries a larger fencing token");
            }
            f.backend.release(&grant.renewal_token).unwrap();
        }
    }
}
