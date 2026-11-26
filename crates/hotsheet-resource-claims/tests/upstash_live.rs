//! HS2-K9J1RK: the Upstash backend against a real database, local only. It runs when
//! `HOTSHEET_TEST_UPSTASH_URL` and `HOTSHEET_TEST_UPSTASH_TOKEN` name a dedicated, disposable
//! Upstash database, and otherwise returns at once, so CI never needs one. This is the only
//! test that runs the Lua scripts of `upstash::scripts` against real Redis semantics; run it
//! after any change to them (`docs/manual-test-plan.md`, "Upstash claim backend").
//!
//! Each run uses its own key prefix, so runs and developers never see each other's claims.
//! Claim keys expire on their own; the per-resource fencing counters and name sets of a run
//! remain until the database is flushed.

use std::sync::Arc;
use std::time::{Duration, Instant};

use hotsheet_resource_claims::conformance::{self, Fixture};
use hotsheet_resource_claims::upstash::UpstashBackend;
use hotsheet_resource_claims::upstash::transport::UreqTransport;
use hotsheet_resource_claims::{AcquireRequest, ClaimBackend, Namespace, ResourceRef};

fn live() -> Option<UpstashBackend> {
    let url = std::env::var("HOTSHEET_TEST_UPSTASH_URL").ok()?;
    let token = std::env::var("HOTSHEET_TEST_UPSTASH_TOKEN").ok()?;
    let prefix = format!("hs2live{}", ulid::Ulid::new().to_string().to_lowercase());
    Some(UpstashBackend::new(
        "live",
        &prefix,
        Arc::new(UreqTransport::new(&url, &token)),
        None,
    ))
}

#[test]
fn hs2_k9j1rk_live_the_conformance_suite_passes_against_a_real_database() {
    let Some(backend) = live() else {
        return;
    };
    conformance::run(&Fixture {
        backend: &backend,
        namespace: Namespace::parse("conformance").unwrap(),
        advance: &std::thread::sleep,
        short_lease: Duration::from_millis(1500),
    });
}

#[test]
fn hs2_k9j1rk_live_a_release_wakes_a_parked_waiter_and_fencing_grows() {
    let Some(backend) = live() else {
        return;
    };
    let target = ResourceRef::parse("vms/live-wait").unwrap();
    let held = backend
        .acquire(&target, &AcquireRequest::new(Duration::from_secs(60)))
        .unwrap();
    let started = Instant::now();
    let grant = std::thread::scope(|scope| {
        let waiter = scope.spawn(|| {
            backend.acquire_wait(
                &target,
                &AcquireRequest::new(Duration::from_secs(60)),
                Duration::from_secs(30),
            )
        });
        std::thread::sleep(Duration::from_secs(1));
        backend.release(held.renewal_token.expose()).unwrap();
        waiter.join().unwrap()
    })
    .unwrap();
    assert!(grant.claim.fencing_token > held.claim.fencing_token);
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "woken by the message, not the deadline"
    );
    backend.release(grant.renewal_token.expose()).unwrap();
}
