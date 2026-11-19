//! HS2-H3WPPY: waiting on the local backend. Waiters are woken by a release signal or
//! sleep once to a known expiry; these tests pin that no waiter polls.

use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use hotsheet_resource_claims::{
    AcquireRequest, ClaimBackend, ClaimKey, LocalBackend, ManualClock, ResourceRef,
    StaticKeyProvider, SystemClock, WaitOutcome,
};

/// Long enough that a test which only passes by waiting for this lease has failed.
const LONG: Duration = Duration::from_secs(120);
/// A generous bound for "woken promptly" on a loaded machine.
const PROMPT: Duration = Duration::from_secs(20);

fn backend(root: &Path) -> Arc<LocalBackend> {
    Arc::new(LocalBackend::new(
        root.join("resource-claims"),
        Arc::new(SystemClock),
        Arc::new(StaticKeyProvider::new(ClaimKey::from_bytes([3; 32]))),
    ))
}

fn target(text: &str) -> ResourceRef {
    ResourceRef::parse(text).unwrap()
}

/// Block until `waiter` has inspected state `count` times, i.e. it has seen the holder
/// and is parked (or about to park; the registration order makes both safe).
fn until_inspected(waiter: &LocalBackend, count: u64) {
    let started = Instant::now();
    while waiter.inspections() < count {
        assert!(started.elapsed() < PROMPT, "waiter never inspected state");
        thread::yield_now();
    }
}

#[test]
fn hs2_h3wppy_waiter_wakes_on_release_with_one_reinspection() {
    let dir = tempfile::tempdir().unwrap();
    let holder = backend(dir.path());
    let waiter = backend(dir.path());
    let vm = target("vms/mac-1");
    let held = holder.acquire(&vm, &AcquireRequest::new(LONG)).unwrap();

    let (done, result) = mpsc::channel();
    let thread_waiter = waiter.clone();
    let thread_vm = vm.clone();
    let started = Instant::now();
    thread::spawn(move || {
        let grant = thread_waiter.acquire_wait(&thread_vm, &AcquireRequest::new(LONG), LONG);
        done.send(grant).unwrap();
    });
    until_inspected(&waiter, 1);
    holder.release(&held.renewal_token).unwrap();

    let grant = result.recv_timeout(PROMPT).unwrap().unwrap();
    assert!(
        started.elapsed() < PROMPT,
        "woken by the release, not the expiry"
    );
    assert_eq!(grant.claim.fencing_token, 2);
    // One refused attempt, then one successful attempt after the wake-up.
    assert_eq!(waiter.inspections(), 2);
    // The waiter withdrew its registration.
    let waiters = dir.path().join("resource-claims/namespaces/vms/waiters");
    assert_eq!(std::fs::read_dir(waiters).unwrap().count(), 0);
}

#[test]
fn hs2_h3wppy_waiter_wakes_on_expiry_with_one_sleep() {
    let dir = tempfile::tempdir().unwrap();
    let holder = backend(dir.path());
    let waiter = backend(dir.path());
    let vm = target("vms/mac-1");
    let lease = Duration::from_millis(600);
    let held = holder.acquire(&vm, &AcquireRequest::new(lease)).unwrap();

    let grant = waiter
        .acquire_wait(&vm, &AcquireRequest::new(LONG), LONG)
        .unwrap();
    assert!(grant.claim.acquired_at_ms >= held.claim.expires_at_ms);
    assert_eq!(grant.claim.fencing_token, 2);
    // No release ever signalled: one refused attempt, one sleep to the known expiry
    // instant, one successful attempt. A polling waiter would inspect many more times.
    assert_eq!(waiter.inspections(), 2);
    let renew = holder.renew(&held.renewal_token, lease);
    assert_eq!(renew.unwrap_err().code(), "not_holder");
}

#[test]
fn hs2_h3wppy_wait_times_out_naming_the_holder_without_polling() {
    let dir = tempfile::tempdir().unwrap();
    let holder = backend(dir.path());
    let waiter = backend(dir.path());
    let vm = target("vms/mac-1");
    let held = holder.acquire(&vm, &AcquireRequest::new(LONG)).unwrap();

    let started = Instant::now();
    let wait = Duration::from_millis(1_200);
    let error = waiter
        .acquire_wait(&vm, &AcquireRequest::new(LONG), wait)
        .unwrap_err();
    assert!(started.elapsed() >= wait);
    assert_eq!(error.code(), "wait_timeout");
    assert_eq!(error.holder().unwrap(), &held.claim);
    // Idle for 1.2 s: the attempt before the sleep and the attempt after it, nothing between.
    assert_eq!(waiter.inspections(), 2);

    // A zero wait is a try-now that reports as a timed-out wait.
    let error = waiter
        .acquire_wait(&vm, &AcquireRequest::new(LONG), Duration::ZERO)
        .unwrap_err();
    assert_eq!(error.code(), "wait_timeout");
    let too_long = Duration::from_secs(24 * 60 * 60 + 1);
    let error = waiter.acquire_wait(&vm, &AcquireRequest::new(LONG), too_long);
    assert_eq!(error.unwrap_err().code(), "invalid");
    assert_eq!(holder.get(&vm).unwrap(), Some(held.claim));
}

#[test]
fn hs2_h3wppy_wait_free_reports_free_or_the_holder_without_acquiring() {
    let dir = tempfile::tempdir().unwrap();
    let holder = backend(dir.path());
    let watcher = backend(dir.path());
    let vm = target("vms/mac-1");

    // Never claimed: free at once, and the wait created nothing on disk.
    assert_eq!(watcher.wait_free(&vm, LONG).unwrap(), WaitOutcome::Free);
    assert!(!dir.path().join("resource-claims").exists());

    let held = holder.acquire(&vm, &AcquireRequest::new(LONG)).unwrap();
    let before = watcher.inspections();
    let started = Instant::now();
    let wait = Duration::from_millis(400);
    let outcome = watcher.wait_free(&vm, wait).unwrap();
    assert!(
        started.elapsed() >= wait,
        "HS2-K3ZXR5: never gives up early"
    );
    assert_eq!(
        outcome,
        WaitOutcome::TimedOut {
            holder: Box::new(held.claim.clone())
        }
    );
    assert_eq!(
        watcher.inspections() - before,
        3,
        "check, check, sleep, check"
    );

    let (done, result) = mpsc::channel();
    let thread_watcher = watcher.clone();
    let thread_vm = vm.clone();
    let before = watcher.inspections();
    thread::spawn(move || {
        done.send(thread_watcher.wait_free(&thread_vm, LONG))
            .unwrap()
    });
    until_inspected(&watcher, before + 2);
    holder.release(&held.renewal_token).unwrap();
    assert_eq!(
        result.recv_timeout(PROMPT).unwrap().unwrap(),
        WaitOutcome::Free
    );
    // Waiting never acquired: the resource is free and the next grant is only the second.
    assert_eq!(holder.get(&vm).unwrap(), None);
    let next = holder.acquire(&vm, &AcquireRequest::new(LONG)).unwrap();
    assert_eq!(next.claim.fencing_token, 2);

    // Free by expiry as well as by release.
    holder.release(&next.renewal_token).unwrap();
    holder
        .acquire(&vm, &AcquireRequest::new(Duration::from_millis(300)))
        .unwrap();
    assert_eq!(watcher.wait_free(&vm, LONG).unwrap(), WaitOutcome::Free);
}

#[test]
fn hs2_h3wppy_queued_waiters_each_get_a_turn_with_rising_fencing_tokens() {
    let dir = tempfile::tempdir().unwrap();
    let holder = backend(dir.path());
    let vm = target("vms/mac-1");
    let held = holder.acquire(&vm, &AcquireRequest::new(LONG)).unwrap();

    let (done, results) = mpsc::channel();
    let mut waiters = Vec::new();
    for _ in 0..4 {
        let waiter = backend(dir.path());
        waiters.push(waiter.clone());
        let done = done.clone();
        let vm = vm.clone();
        thread::spawn(move || {
            let grant = waiter
                .acquire_wait(&vm, &AcquireRequest::new(LONG), LONG)
                .unwrap();
            // Holding the resource: nobody else may be live on it right now.
            let seen = waiter.get(&vm).unwrap().unwrap();
            assert_eq!(seen.claim_id, grant.claim.claim_id);
            waiter.release(&grant.renewal_token).unwrap();
            done.send(grant.claim.fencing_token).unwrap();
        });
    }
    for waiter in &waiters {
        until_inspected(waiter, 1);
    }
    holder.release(&held.renewal_token).unwrap();

    let mut tokens: Vec<u64> = (0..4)
        .map(|_| results.recv_timeout(PROMPT).unwrap())
        .collect();
    tokens.sort_unstable();
    assert_eq!(tokens, [2, 3, 4, 5]);
    assert_eq!(holder.get(&vm).unwrap(), None);
}

#[test]
fn hs2_h3wppy_shortening_a_lease_wakes_its_waiter() {
    let dir = tempfile::tempdir().unwrap();
    let holder = backend(dir.path());
    let waiter = backend(dir.path());
    let vm = target("vms/mac-1");
    let held = holder.acquire(&vm, &AcquireRequest::new(LONG)).unwrap();

    let (done, result) = mpsc::channel();
    let thread_waiter = waiter.clone();
    let thread_vm = vm.clone();
    thread::spawn(move || {
        let grant = thread_waiter.acquire_wait(&thread_vm, &AcquireRequest::new(LONG), LONG);
        done.send(grant).unwrap();
    });
    until_inspected(&waiter, 1);
    // The waiter planned to sleep for two minutes; the shorter lease must reach it.
    holder
        .renew(&held.renewal_token, Duration::from_millis(300))
        .unwrap();
    let grant = result.recv_timeout(PROMPT).unwrap().unwrap();
    assert_eq!(grant.claim.fencing_token, 2);
    // Refused, re-planned after the renewal signal, granted at the new expiry.
    assert_eq!(waiter.inspections(), 3);
}

#[test]
fn hs2_h3wppy_an_injected_clock_decides_expiry_for_waiters() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(ManualClock::new(5_000_000));
    let keys = Arc::new(StaticKeyProvider::new(ClaimKey::from_bytes([3; 32])));
    let make = || {
        Arc::new(LocalBackend::new(
            dir.path().join("resource-claims"),
            clock.clone(),
            keys.clone(),
        ))
    };
    let (holder, waiter) = (make(), make());
    let vm = target("vms/mac-1");
    holder.acquire(&vm, &AcquireRequest::new(LONG)).unwrap();

    let (done, result) = mpsc::channel();
    let thread_waiter = waiter.clone();
    let thread_vm = vm.clone();
    thread::spawn(move || {
        let grant = thread_waiter.acquire_wait(&thread_vm, &AcquireRequest::new(LONG), LONG * 2);
        done.send(grant).unwrap();
    });
    until_inspected(&waiter, 1);
    // The backend clock passes the expiry; the signal makes the waiter look again.
    clock.advance_ms(120_000);
    holder.signal(vm.namespace()).unwrap();
    let grant = result.recv_timeout(PROMPT).unwrap().unwrap();
    assert_eq!(grant.claim.acquired_at_ms, 5_120_000);
    assert_eq!(grant.claim.fencing_token, 2);
}

/// HS2-K3ZXR5: the caller's wait is measured on the monotonic clock. A backend clock
/// that stands still or jumps ahead decides the holder's expiry, never when the caller
/// has waited long enough.
#[test]
fn hs2_k3zxr5_a_wait_ends_on_monotonic_time_whatever_the_backend_clock_does() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(ManualClock::new(5_000_000));
    let keys = Arc::new(StaticKeyProvider::new(ClaimKey::from_bytes([3; 32])));
    let make = || {
        Arc::new(LocalBackend::new(
            dir.path().join("resource-claims"),
            clock.clone(),
            keys.clone(),
        ))
    };
    let (holder, waiter) = (make(), make());
    let vm = target("vms/mac-1");
    let lease = Duration::from_secs(60 * 60);
    let held = holder.acquire(&vm, &AcquireRequest::new(lease)).unwrap();
    let wait = Duration::from_millis(400);

    // A clock that stands still: the wait still ends, after one sleep, not early. (On the
    // backend clock it never ended: the waiter re-slept forever.)
    let (done, result) = mpsc::channel();
    let thread_waiter = waiter.clone();
    let thread_vm = vm.clone();
    let started = Instant::now();
    thread::spawn(move || {
        let outcome = thread_waiter.acquire_wait(&thread_vm, &AcquireRequest::new(LONG), wait);
        done.send((outcome, started.elapsed())).unwrap();
    });
    let (outcome, elapsed) = result
        .recv_timeout(PROMPT)
        .expect("a frozen backend clock must not stall the wait");
    assert!(elapsed >= wait);
    assert_eq!(outcome.unwrap_err().code(), "wait_timeout");
    assert_eq!(waiter.inspections(), 2);

    // A clock that jumps ahead mid-wait (well past the wait, still inside the holder's
    // lease and the registration's cleanup grace) does not end the wait: the signal
    // costs one re-inspection and the waiter sleeps out the rest.
    let (done, result) = mpsc::channel();
    let thread_waiter = waiter.clone();
    let thread_vm = vm.clone();
    let started = Instant::now();
    thread::spawn(move || {
        let outcome = thread_waiter.acquire_wait(&thread_vm, &AcquireRequest::new(LONG), wait);
        done.send((outcome, started.elapsed())).unwrap();
    });
    until_inspected(&waiter, 3);
    clock.advance_ms(10_000);
    holder.signal(vm.namespace()).unwrap();
    let (outcome, elapsed) = result.recv_timeout(PROMPT).unwrap();
    assert_eq!(outcome.unwrap_err().code(), "wait_timeout");
    assert!(
        elapsed >= wait,
        "a stepped clock must not end the wait early"
    );
    assert_eq!(holder.get(&vm).unwrap(), Some(held.claim));
}
