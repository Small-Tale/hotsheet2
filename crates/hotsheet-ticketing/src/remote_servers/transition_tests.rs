//! HS2-1Y45MZ: bounded credential obligations and clock-independent probe ordering.

use std::sync::mpsc;
use std::time::Duration;

use super::*;
use crate::remote_servers::registry::{MAX_CREDENTIAL_OBLIGATIONS, credential_obligations};

fn seed_pending(registry: &RemoteRegistry, secrets: &Memory, count: usize) {
    let mut file = registry.read().unwrap();
    file.pending_cleanup = (1..=count)
        .map(|revision| format!("remote-{OTHER}-{revision}"))
        .collect();
    for reference in &file.pending_cleanup {
        secrets
            .values
            .lock_or_recover()
            .insert(reference.clone(), "synthetic-pending".into());
    }
    registry.write(&file).unwrap();
    secrets.fail_delete.store(true, Ordering::SeqCst);
}

fn set_count(secrets: &Memory) -> usize {
    secrets
        .events
        .lock_or_recover()
        .iter()
        .filter(|event| **event == "set")
        .count()
}

#[test]
fn hs2_1y45mz_global_obligation_budget_reserves_last_slot_and_never_blocks_revoke() {
    let home = tempfile::tempdir().unwrap();
    let registry = RemoteRegistry::new(home.path().join("remotes.json"));
    let secrets = Memory::default();
    let peer = Peer::default();
    registry.register(request(), &secrets, &peer, 1).unwrap();
    seed_pending(&registry, &secrets, MAX_CREDENTIAL_OBLIGATIONS - 2);
    let mut second = request();
    second.id = PEER.into();
    second.origin = "http://127.0.0.1:8766".into();
    registry.register(second, &secrets, &peer, 2).unwrap();
    assert_eq!(credential_obligations(&registry.read().unwrap()), 256);
    assert_eq!(registry.list().unwrap().len(), 2);
    let before = std::fs::read(registry.path()).unwrap();
    let sets = set_count(&secrets);
    let active = registry.list().unwrap().remove(0);
    assert!(
        registry
            .review(ID, review(&active), &secrets, &peer, 3)
            .is_err()
    );
    let mut third = request();
    third.id = "01ARZ3NDEKTSV4RRFFQ69G5FAY".into();
    third.origin = "http://127.0.0.1:8767".into();
    assert!(registry.register(third, &secrets, &peer, 3).is_err());
    assert_eq!(set_count(&secrets), sets);
    assert_eq!(std::fs::read(registry.path()).unwrap(), before);
    let first = registry.revoke(ID, 1, &secrets).unwrap();
    assert!(first.revoked && first.credential_cleanup_pending);
    assert_eq!(credential_obligations(&registry.read().unwrap()), 256);
    let second = registry.revoke(PEER, 1, &secrets).unwrap();
    assert!(second.revoked);
    assert_eq!(registry.read().unwrap().pending_cleanup.len(), 256);
    let calls = peer.calls.load(Ordering::SeqCst);
    assert!(
        registry
            .discover(ID, &secrets, &peer, 4)
            .unwrap()
            .checkouts
            .is_none()
    );
    assert_eq!(peer.calls.load(Ordering::SeqCst), calls);
    assert_eq!(registry.list().unwrap().len(), 2); // Reopen remains valid at capacity.
    secrets.fail_delete.store(false, Ordering::SeqCst);
    registry.revoke(ID, 1, &secrets).unwrap();
    assert!(registry.read().unwrap().pending_cleanup.is_empty());
    assert!(secrets.values.lock_or_recover().is_empty());
    let restored = registry
        .review(ID, review(&first), &secrets, &peer, 5)
        .unwrap();
    assert!(restored.checkouts.is_some());
    assert_eq!(credential_obligations(&registry.read().unwrap()), 1);
}

#[test]
fn hs2_1y45mz_review_last_slot_crash_preserves_intent_then_recovers_capacity() {
    let home = tempfile::tempdir().unwrap();
    let registry = RemoteRegistry::new(home.path().join("remotes.json"));
    let secrets = Memory::default();
    let peer = Peer::default();
    let original = registry.register(request(), &secrets, &peer, 1).unwrap();
    seed_pending(&registry, &secrets, MAX_CREDENTIAL_OBLIGATIONS - 2);
    let replacement = review(&original.remote);
    secrets.crash_after_set.store(true, Ordering::SeqCst);
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            registry.review(ID, replacement.clone(), &secrets, &peer, 2)
        }))
        .is_err()
    );
    let interrupted = registry.read().unwrap();
    assert_eq!(credential_obligations(&interrupted), 256);
    assert_eq!(interrupted.servers[0].revision, 1);
    assert!(
        registry
            .discover(ID, &secrets, &peer, 3)
            .unwrap()
            .checkouts
            .is_some()
    );
    let sets = set_count(&secrets);
    assert!(
        registry
            .review(ID, replacement.clone(), &secrets, &peer, 4)
            .is_err()
    );
    assert_eq!(set_count(&secrets), sets);
    secrets.fail_delete.store(false, Ordering::SeqCst);
    let recovered = registry
        .review(ID, replacement, &secrets, &peer, 5)
        .unwrap();
    assert_eq!(recovered.remote.revision, 2);
    assert_eq!(credential_obligations(&registry.read().unwrap()), 1);
    assert_eq!(secrets.values.lock_or_recover().len(), 1);
}

#[test]
fn hs2_1y45mz_review_activation_fits_the_last_reserved_obligation() {
    let home = tempfile::tempdir().unwrap();
    let registry = RemoteRegistry::new(home.path().join("remotes.json"));
    let secrets = Memory::default();
    let peer = Peer::default();
    let original = registry.register(request(), &secrets, &peer, 1).unwrap();
    seed_pending(&registry, &secrets, MAX_CREDENTIAL_OBLIGATIONS - 2);
    let reviewed = registry
        .review(ID, review(&original.remote), &secrets, &peer, 2)
        .unwrap();
    assert_eq!(reviewed.remote.revision, 2);
    assert!(reviewed.remote.credential_cleanup_pending);
    assert_eq!(credential_obligations(&registry.read().unwrap()), 256);
    assert!(
        registry
            .discover(ID, &secrets, &peer, 3)
            .unwrap()
            .checkouts
            .is_some()
    );
    assert_eq!(registry.revoke(ID, 2, &secrets).unwrap().revision, 3);
    assert_eq!(registry.read().unwrap().pending_cleanup.len(), 256);
}

#[test]
fn hs2_1y45mz_read_and_write_share_budget_and_generation_validation() {
    let home = tempfile::tempdir().unwrap();
    let registry = RemoteRegistry::new(home.path().join("remotes.json"));
    let secrets = Memory::default();
    let peer = Peer::default();
    let registered = registry.register(request(), &secrets, &peer, 1).unwrap();
    assert!(
        !serde_json::to_string(&registered)
            .unwrap()
            .contains("observation_generation")
    );
    let original = registry.read().unwrap();
    let bytes = std::fs::read(registry.path()).unwrap();
    for malformed_generation in ["", "not-an-id", "01arz3ndektsv4rrffq69g5fav"] {
        let mut invalid = original.clone();
        invalid.servers[0].observation_generation = malformed_generation.into();
        assert!(registry.write(&invalid).is_err());
        assert_eq!(std::fs::read(registry.path()).unwrap(), bytes);
        std::fs::write(registry.path(), serde_json::to_vec(&invalid).unwrap()).unwrap();
        assert!(registry.read().is_err());
        std::fs::write(registry.path(), &bytes).unwrap();
    }
    let mut overlapping = original.clone();
    overlapping
        .pending_cleanup
        .push(overlapping.servers[0].credential_ref.clone().unwrap());
    assert!(registry.write(&overlapping).is_err());
    assert_eq!(std::fs::read(registry.path()).unwrap(), bytes);
    let mut invalid = original;
    invalid.pending_cleanup = (1..=MAX_CREDENTIAL_OBLIGATIONS)
        .map(|revision| format!("remote-{OTHER}-{revision}"))
        .collect();
    assert!(registry.write(&invalid).is_err()); // 256 pending + one active is invalid.
    assert_eq!(std::fs::read(registry.path()).unwrap(), bytes);
    std::fs::write(registry.path(), serde_json::to_vec(&invalid).unwrap()).unwrap();
    assert!(registry.list().is_err());
}

struct Delayed {
    result: Result<RemoteSnapshot, RemoteFailure>,
    entered: mpsc::SyncSender<()>,
    release: Mutex<mpsc::Receiver<()>>,
}

impl RemoteTransport for Delayed {
    fn probe(&self, _: &str, _: &RemoteCredentials) -> Result<RemoteSnapshot, RemoteFailure> {
        self.entered.send(()).unwrap();
        self.release
            .lock_or_recover()
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        self.result.clone()
    }
}

fn delayed(
    result: Result<RemoteSnapshot, RemoteFailure>,
) -> (Delayed, mpsc::Receiver<()>, mpsc::SyncSender<()>) {
    let (entered, waiting) = mpsc::sync_channel(1);
    let (release, released) = mpsc::sync_channel(1);
    (
        Delayed {
            result,
            entered,
            release: Mutex::new(released),
        },
        waiting,
        release,
    )
}

#[test]
fn hs2_1y45mz_out_of_order_discovery_cannot_overwrite_newer_state_even_when_clocks_reverse() {
    for (late, current, old_time, new_time) in [
        (
            Ok(snapshot(PEER)),
            Err(RemoteFailure::Unreachable),
            100,
            200,
        ),
        (Ok(snapshot(PEER)), Ok(snapshot(OTHER)), 100, 100),
        (Err(RemoteFailure::Unauthorized), Ok(snapshot(PEER)), 100, 1),
    ] {
        let home = tempfile::tempdir().unwrap();
        let registry = RemoteRegistry::new(home.path().join("remotes.json"));
        let secrets = Arc::new(Memory::default());
        let peer = Peer::default();
        registry
            .register(request(), secrets.as_ref(), &peer, 0)
            .unwrap();
        let (transport, entered, release) = delayed(late);
        let owned = registry.clone();
        let stored = secrets.clone();
        let job =
            std::thread::spawn(move || owned.discover(ID, stored.as_ref(), &transport, old_time));
        entered.recv_timeout(Duration::from_secs(5)).unwrap();
        *peer.result.lock_or_recover() = current;
        let winner = registry
            .discover(ID, secrets.as_ref(), &peer, new_time)
            .unwrap();
        let bytes = std::fs::read(registry.path()).unwrap();
        release.send(()).unwrap();
        assert!(matches!(job.join().unwrap(), Err(RemoteError::Conflict)));
        assert_eq!(std::fs::read(registry.path()).unwrap(), bytes);
        assert_eq!(registry.list().unwrap()[0].checked_at_ms, Some(new_time));
        *peer.result.lock_or_recover() = Ok(snapshot(PEER));
        let retry = registry.discover(ID, secrets.as_ref(), &peer, 0).unwrap();
        assert!(retry.checkouts.is_some());
        assert_eq!(retry.remote.revision, winner.remote.revision);
    }
}

#[test]
fn hs2_1y45mz_idempotent_register_and_discover_fence_each_others_late_probe() {
    for late_register in [true, false] {
        let home = tempfile::tempdir().unwrap();
        let registry = RemoteRegistry::new(home.path().join("remotes.json"));
        let secrets = Arc::new(Memory::default());
        let peer = Peer::default();
        registry
            .register(request(), secrets.as_ref(), &peer, 1)
            .unwrap();
        let (transport, entered, release) = delayed(Ok(snapshot(PEER)));
        let owned = registry.clone();
        let stored = secrets.clone();
        let job = std::thread::spawn(move || {
            if late_register {
                owned.register(request(), stored.as_ref(), &transport, 100)
            } else {
                owned.discover(ID, stored.as_ref(), &transport, 100)
            }
        });
        entered.recv_timeout(Duration::from_secs(5)).unwrap();
        if late_register {
            *peer.result.lock_or_recover() = Err(RemoteFailure::Unreachable);
            registry.discover(ID, secrets.as_ref(), &peer, 100).unwrap();
        } else {
            registry
                .register(request(), secrets.as_ref(), &peer, 0)
                .unwrap();
        }
        let bytes = std::fs::read(registry.path()).unwrap();
        release.send(()).unwrap();
        assert!(matches!(job.join().unwrap(), Err(RemoteError::Conflict)));
        assert_eq!(std::fs::read(registry.path()).unwrap(), bytes);
        assert_eq!(set_count(secrets.as_ref()), 1);
    }
}

#[test]
fn hs2_1y45mz_concurrent_creation_does_not_reuse_an_earlier_registration_probe() {
    let home = tempfile::tempdir().unwrap();
    let registry = RemoteRegistry::new(home.path().join("remotes.json"));
    let secrets = Arc::new(Memory::default());
    let (transport, entered, release) = delayed(Ok(snapshot(PEER)));
    let owned = registry.clone();
    let stored = secrets.clone();
    let job =
        std::thread::spawn(move || owned.register(request(), stored.as_ref(), &transport, 100));
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    registry
        .register(request(), secrets.as_ref(), &Peer::default(), 1)
        .unwrap();
    let bytes = std::fs::read(registry.path()).unwrap();
    release.send(()).unwrap();
    assert!(matches!(job.join().unwrap(), Err(RemoteError::Conflict)));
    assert_eq!(std::fs::read(registry.path()).unwrap(), bytes);
    assert_eq!(set_count(secrets.as_ref()), 1);
}

#[test]
fn hs2_1y45mz_delayed_review_cannot_overwrite_newer_failure_or_success() {
    for (current, new_time) in [
        (Err(RemoteFailure::Unreachable), 200),
        (Ok(snapshot(OTHER)), 100),
        (Ok(snapshot(PEER)), 0),
    ] {
        let home = tempfile::tempdir().unwrap();
        let registry = RemoteRegistry::new(home.path().join("remotes.json"));
        let secrets = Arc::new(Memory::default());
        let peer = Peer::default();
        let original = registry
            .register(request(), secrets.as_ref(), &peer, 1)
            .unwrap();
        let mut intended = review(&original.remote);
        intended.credentials.bearer_secret = "synthetic-reviewed-replacement".into();
        let retry_request = intended.clone();
        let (transport, entered, release) = delayed(Ok(snapshot(PEER)));
        let owned = registry.clone();
        let stored = secrets.clone();
        let job = std::thread::spawn(move || {
            owned.review(ID, intended, stored.as_ref(), &transport, 100)
        });
        entered.recv_timeout(Duration::from_secs(5)).unwrap();
        *peer.result.lock_or_recover() = current.clone();
        let latest = registry
            .discover(ID, secrets.as_ref(), &peer, new_time)
            .unwrap();
        assert_eq!(latest.remote.revision, original.remote.revision);
        let bytes = std::fs::read(registry.path()).unwrap();
        let values = secrets.values.lock_or_recover().clone();
        release.send(()).unwrap();
        assert!(matches!(job.join().unwrap(), Err(RemoteError::Conflict)));
        assert_eq!(std::fs::read(registry.path()).unwrap(), bytes);
        assert_eq!(*secrets.values.lock_or_recover(), values);
        assert_eq!(set_count(secrets.as_ref()), 1);
        // Fresh intent recaptures the internal token. A changed server is named
        // explicitly, while the same public revision/previous identity remains valid.
        let mut retry = retry_request;
        if let Ok(snapshot) = current {
            retry.expected_server_id = snapshot.identity.server_id;
        } else {
            *peer.result.lock_or_recover() = Ok(snapshot(PEER));
        }
        assert_eq!(retry.expected_revision, original.remote.revision);
        let reviewed = registry
            .review(ID, retry, secrets.as_ref(), &peer, 0)
            .unwrap();
        assert_eq!(reviewed.remote.revision, original.remote.revision + 1);
        assert!(reviewed.checkouts.is_some());
        assert_eq!(secrets.values.lock_or_recover().len(), 1);
        assert_eq!(set_count(secrets.as_ref()), 2);
    }
}

#[test]
fn hs2_1y45mz_delayed_review_cannot_restore_a_concurrently_revoked_binding() {
    let home = tempfile::tempdir().unwrap();
    let registry = RemoteRegistry::new(home.path().join("remotes.json"));
    let secrets = Arc::new(Memory::default());
    let peer = Peer::default();
    let original = registry
        .register(request(), secrets.as_ref(), &peer, 1)
        .unwrap();
    let intended = review(&original.remote);
    let stale_retry = intended.clone();
    let (transport, entered, release) = delayed(Ok(snapshot(PEER)));
    let owned = registry.clone();
    let stored = secrets.clone();
    let job =
        std::thread::spawn(move || owned.review(ID, intended, stored.as_ref(), &transport, 100));
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    let revoked = registry.revoke(ID, 1, secrets.as_ref()).unwrap();
    let bytes = std::fs::read(registry.path()).unwrap();
    release.send(()).unwrap();
    assert!(matches!(job.join().unwrap(), Err(RemoteError::Conflict)));
    assert_eq!(std::fs::read(registry.path()).unwrap(), bytes);
    assert!(secrets.values.lock_or_recover().is_empty());
    assert_eq!(set_count(secrets.as_ref()), 1);
    let calls = peer.calls.load(Ordering::SeqCst);
    assert!(matches!(
        registry.review(ID, stale_retry, secrets.as_ref(), &peer, 0),
        Err(RemoteError::Conflict)
    ));
    assert_eq!(peer.calls.load(Ordering::SeqCst), calls);
    let restored = registry
        .review(ID, review(&revoked), secrets.as_ref(), &peer, 0)
        .unwrap();
    assert_eq!(restored.remote.revision, revoked.revision + 1);
    assert!(restored.checkouts.is_some());
}

#[test]
fn hs2_1y45mz_repeated_revoke_fences_reactivation_without_changing_public_revision() {
    let home = tempfile::tempdir().unwrap();
    let registry = RemoteRegistry::new(home.path().join("remotes.json"));
    let secrets = Arc::new(Memory::default());
    let peer = Peer::default();
    registry
        .register(request(), secrets.as_ref(), &peer, 1)
        .unwrap();
    let revoked = registry.revoke(ID, 1, secrets.as_ref()).unwrap();
    let generation = registry.read().unwrap().servers[0]
        .observation_generation
        .clone();
    let intended = review(&revoked);
    let fresh_retry = intended.clone();
    let (transport, entered, release) = delayed(Ok(snapshot(PEER)));
    let owned = registry.clone();
    let stored = secrets.clone();
    let job =
        std::thread::spawn(move || owned.review(ID, intended, stored.as_ref(), &transport, 100));
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    let repeated = registry
        .revoke(ID, revoked.revision, secrets.as_ref())
        .unwrap();
    assert_eq!(repeated.revision, revoked.revision);
    assert_ne!(
        registry.read().unwrap().servers[0].observation_generation,
        generation
    );
    let bytes = std::fs::read(registry.path()).unwrap();
    release.send(()).unwrap();
    assert!(matches!(job.join().unwrap(), Err(RemoteError::Conflict)));
    assert_eq!(std::fs::read(registry.path()).unwrap(), bytes);
    assert!(registry.list().unwrap()[0].revoked);
    assert!(secrets.values.lock_or_recover().is_empty());
    assert_eq!(set_count(secrets.as_ref()), 1);
    // The same public review request becomes valid only as a fresh invocation
    // that captures the repeated revoke's new private generation.
    assert_eq!(fresh_retry.expected_revision, repeated.revision);
    let restored = registry
        .review(ID, fresh_retry, secrets.as_ref(), &peer, 0)
        .unwrap();
    assert_eq!(restored.remote.revision, repeated.revision + 1);
    assert!(restored.checkouts.is_some());
    assert_eq!(set_count(secrets.as_ref()), 2);
    assert_eq!(secrets.values.lock_or_recover().len(), 1);
}
