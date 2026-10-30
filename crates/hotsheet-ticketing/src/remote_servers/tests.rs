use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use hotsheet_sync::LockExt;

use super::*;
use crate::{SecretError, SecretStore};

#[path = "transition_tests.rs"]
mod transitions;

const ID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const PEER: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAW";
const OTHER: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAX";

#[derive(Default)]
struct Memory {
    values: Mutex<HashMap<String, String>>,
    events: Mutex<Vec<&'static str>>,
    fail_set: AtomicBool,
    fail_delete: AtomicBool,
    crash_after_set: AtomicBool,
    inspect_set: Mutex<Option<Box<dyn Fn() + Send + Sync>>>,
}

impl SecretStore for Memory {
    fn set(&self, account: &str, secret: &str) -> Result<(), SecretError> {
        self.events.lock_or_recover().push("set");
        if let Some(inspect) = self.inspect_set.lock_or_recover().as_ref() {
            inspect();
        }
        if self.fail_set.load(Ordering::SeqCst) {
            return Err(SecretError::Backend("synthetic private detail".into()));
        }
        self.values
            .lock_or_recover()
            .insert(account.into(), secret.into());
        assert!(
            !self.crash_after_set.swap(false, Ordering::SeqCst),
            "synthetic interruption after secret persistence"
        );
        Ok(())
    }
    fn get(&self, account: &str) -> Result<Option<String>, SecretError> {
        self.events.lock_or_recover().push("get");
        Ok(self.values.lock_or_recover().get(account).cloned())
    }
    fn delete(&self, account: &str) -> Result<bool, SecretError> {
        self.events.lock_or_recover().push("delete");
        if self.fail_delete.load(Ordering::SeqCst) {
            return Err(SecretError::Backend("synthetic private detail".into()));
        }
        Ok(self.values.lock_or_recover().remove(account).is_some())
    }
}

struct Peer {
    result: Mutex<Result<RemoteSnapshot, RemoteFailure>>,
    calls: AtomicUsize,
    before: Mutex<Option<Box<dyn Fn() + Send + Sync>>>,
}

impl Default for Peer {
    fn default() -> Self {
        Self {
            result: Mutex::new(Ok(snapshot(PEER))),
            calls: AtomicUsize::new(0),
            before: Mutex::new(None),
        }
    }
}

impl RemoteTransport for Peer {
    fn probe(&self, _: &str, _: &RemoteCredentials) -> Result<RemoteSnapshot, RemoteFailure> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(before) = self.before.lock_or_recover().take() {
            before();
        }
        self.result.lock_or_recover().clone()
    }
}

fn snapshot(server_id: &str) -> RemoteSnapshot {
    RemoteSnapshot {
        identity: RemoteIdentity {
            generation: "hs2".into(),
            protocol: 1,
            server_id: server_id.into(),
        },
        checkouts: vec![RemoteCheckout {
            id: "synthetic-checkout".into(),
            name: "Synthetic".into(),
            sources: vec![RemoteSource {
                connection_id: "synthetic-source".into(),
                provider: "git".into(),
            }],
            state: RemoteCheckoutState::Available,
        }],
    }
}

fn request() -> RegisterRemote {
    RegisterRemote {
        id: ID.into(),
        label: "Synthetic".into(),
        origin: "http://127.0.0.1:8765".into(),
        expected_server_id: PEER.into(),
        credentials: RemoteCredentials {
            bearer_secret: "synthetic-bearer".into(),
            ca_pem: None,
            client_certificate_pem: None,
            client_key_pem: None,
        },
    }
}

fn review(remote: &RemoteView) -> ReviewRemote {
    let request = request();
    ReviewRemote {
        expected_revision: remote.revision,
        previous_identity: remote.identity.clone(),
        label: request.label,
        origin: request.origin,
        expected_server_id: request.expected_server_id,
        credentials: request.credentials,
    }
}

#[test]
fn hs2_1y45mz_lifecycle_never_promotes_cached_or_changed_identity_to_live_authority() {
    let home = tempfile::tempdir().unwrap();
    let registry = RemoteRegistry::new(home.path().join("remotes.json"));
    let secrets = Memory::default();
    let peer = Peer::default();
    assert!(registry.list().unwrap().is_empty());
    let registered = registry.register(request(), &secrets, &peer, 1).unwrap();
    assert_eq!(registered.remote.revision, 1);
    assert!(!registered.remote.stale);
    assert_eq!(registered.checkouts.as_ref().unwrap().len(), 1);
    let repeated = registry.register(request(), &secrets, &peer, 2).unwrap();
    assert_eq!(repeated.remote.id, registered.remote.id);
    assert_eq!(repeated.remote.revision, 1);
    assert_eq!(secrets.values.lock_or_recover().len(), 1);
    let before_calls = peer.calls.load(Ordering::SeqCst);
    let listed = registry.list().unwrap();
    assert!(listed[0].stale);
    assert_eq!(peer.calls.load(Ordering::SeqCst), before_calls);
    *peer.result.lock_or_recover() = Err(RemoteFailure::Unreachable);
    let failed = registry.discover(ID, &secrets, &peer, 3).unwrap();
    assert!(failed.checkouts.is_none());
    assert!(failed.remote.stale);
    assert_eq!(failed.remote.confirmed_at_ms, Some(2));
    assert_eq!(failed.remote.last_failure, Some(RemoteFailure::Unreachable));
    assert_eq!(failed.remote.last_confirmed_checkouts.len(), 1);
    *peer.result.lock_or_recover() = Ok(snapshot(OTHER));
    let changed = registry.discover(ID, &secrets, &peer, 4).unwrap();
    assert!(changed.checkouts.is_none());
    assert_eq!(
        changed.remote.last_failure,
        Some(RemoteFailure::IdentityChanged)
    );
    assert_eq!(changed.remote.identity.server_id, PEER);
    let mut accepted = review(&changed.remote);
    accepted.expected_server_id = OTHER.into();
    let reviewed = registry.review(ID, accepted, &secrets, &peer, 5).unwrap();
    assert_eq!(reviewed.remote.identity.server_id, OTHER);
    assert_eq!(reviewed.remote.revision, 2);
    assert_eq!(reviewed.remote.review_history.len(), 2);
    assert!(reviewed.checkouts.is_some());
    assert_eq!(secrets.values.lock_or_recover().len(), 1);
    let before = peer.calls.load(Ordering::SeqCst);
    assert!(matches!(
        registry.review(ID, review(&registered.remote), &secrets, &peer, 6),
        Err(RemoteError::Conflict)
    ));
    assert_eq!(peer.calls.load(Ordering::SeqCst), before);
    let disk = std::fs::read_to_string(&registry.path).unwrap();
    assert!(!disk.contains("synthetic-bearer"));
    let public = serde_json::to_string(&reviewed).unwrap();
    assert!(!public.contains("credential_ref"));
    assert!(!public.contains("synthetic-bearer"));
}

#[test]
fn hs2_1y45mz_secret_intent_survives_interruption_before_activation_and_retry_cleans_it() {
    let home = tempfile::tempdir().unwrap();
    let registry = RemoteRegistry::new(home.path().join("remotes.json"));
    let secrets = Memory::default();
    let path = registry.path.clone();
    *secrets.inspect_set.lock_or_recover() = Some(Box::new(move || {
        let pending: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(pending["pending_cleanup"].as_array().unwrap().len(), 1);
        assert!(pending["servers"].as_array().unwrap().is_empty());
    }));
    secrets.crash_after_set.store(true, Ordering::SeqCst);
    let peer = Peer::default();
    let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        registry.register(request(), &secrets, &peer, 1)
    }));
    assert!(interrupted.is_err());
    assert!(registry.list().unwrap().is_empty());
    assert_eq!(registry.read().unwrap().pending_cleanup.len(), 1);
    assert_eq!(secrets.values.lock_or_recover().len(), 1);
    let result = registry.register(request(), &secrets, &peer, 2).unwrap();
    assert_eq!(result.remote.revision, 1);
    assert!(registry.read().unwrap().pending_cleanup.is_empty());
    assert_eq!(secrets.values.lock_or_recover().len(), 1);
    assert!(
        secrets
            .events
            .lock_or_recover()
            .windows(2)
            .any(|events| events == ["delete", "set"])
    );
}

#[test]
fn hs2_1y45mz_failed_secret_storage_and_failed_probe_never_activate_a_binding() {
    let home = tempfile::tempdir().unwrap();
    let registry = RemoteRegistry::new(home.path().join("remotes.json"));
    let secrets = Memory::default();
    let peer = Peer::default();
    secrets.fail_set.store(true, Ordering::SeqCst);
    let error = registry
        .register(request(), &secrets, &peer, 1)
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "remote credential storage is unavailable"
    );
    assert!(registry.list().unwrap().is_empty());
    assert!(registry.read().unwrap().pending_cleanup.is_empty());
    secrets.fail_set.store(false, Ordering::SeqCst);
    *peer.result.lock_or_recover() = Err(RemoteFailure::Unauthorized);
    let before = std::fs::read(&registry.path).unwrap();
    assert!(matches!(
        registry.register(request(), &secrets, &peer, 2),
        Err(RemoteError::Verification(RemoteFailure::Unauthorized))
    ));
    assert_eq!(before, std::fs::read(&registry.path).unwrap());
    assert!(secrets.values.lock_or_recover().is_empty());
}

#[test]
fn hs2_1y45mz_revoke_disables_before_failed_cleanup_and_can_retry_or_be_explicitly_reviewed() {
    let home = tempfile::tempdir().unwrap();
    let registry = RemoteRegistry::new(home.path().join("remotes.json"));
    let secrets = Memory::default();
    let peer = Peer::default();
    registry.register(request(), &secrets, &peer, 1).unwrap();
    secrets.fail_delete.store(true, Ordering::SeqCst);
    let revoked = registry.revoke(ID, 1, &secrets).unwrap();
    assert!(revoked.revoked && revoked.credential_cleanup_pending);
    assert_eq!(revoked.revision, 2);
    let before = peer.calls.load(Ordering::SeqCst);
    assert!(
        registry
            .discover(ID, &secrets, &peer, 2)
            .unwrap()
            .checkouts
            .is_none()
    );
    assert_eq!(peer.calls.load(Ordering::SeqCst), before);
    secrets.fail_delete.store(false, Ordering::SeqCst);
    let repeated = registry.revoke(ID, 1, &secrets).unwrap();
    assert_eq!(repeated.revision, 2);
    assert!(!repeated.credential_cleanup_pending);
    assert!(secrets.values.lock_or_recover().is_empty());
    assert!(matches!(
        registry.register(request(), &secrets, &peer, 3),
        Err(RemoteError::Revoked)
    ));
    let restored = registry
        .review(ID, review(&repeated), &secrets, &peer, 4)
        .unwrap();
    assert!(!restored.remote.revoked);
    assert_eq!(restored.remote.revision, 3);
    assert!(restored.checkouts.is_some());
}

#[test]
fn hs2_1y45mz_revoke_during_probe_rejects_the_late_discovery_result() {
    let home = tempfile::tempdir().unwrap();
    let registry = RemoteRegistry::new(home.path().join("remotes.json"));
    let secrets = Arc::new(Memory::default());
    let peer = Peer::default();
    registry
        .register(request(), secrets.as_ref(), &peer, 1)
        .unwrap();
    let concurrent_registry = registry.clone();
    let concurrent_secrets = secrets.clone();
    *peer.before.lock_or_recover() = Some(Box::new(move || {
        concurrent_registry
            .revoke(ID, 1, concurrent_secrets.as_ref())
            .unwrap();
    }));
    assert!(matches!(
        registry.discover(ID, secrets.as_ref(), &peer, 2),
        Err(RemoteError::Conflict)
    ));
    assert!(registry.list().unwrap()[0].revoked);
}

#[test]
fn hs2_1y45mz_concurrent_origin_registration_has_one_authoritative_winner() {
    let home = tempfile::tempdir().unwrap();
    let registry = RemoteRegistry::new(home.path().join("remotes.json"));
    let secrets = Arc::new(Memory::default());
    let peer = Arc::new(Peer::default());
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let mut jobs = Vec::new();
    for id in [ID, OTHER] {
        let (registry, secrets, peer, barrier) = (
            registry.clone(),
            secrets.clone(),
            peer.clone(),
            barrier.clone(),
        );
        jobs.push(std::thread::spawn(move || {
            barrier.wait();
            let mut request = request();
            request.id = id.into();
            registry.register(request, secrets.as_ref(), peer.as_ref(), 1)
        }));
    }
    assert_eq!(
        jobs.into_iter()
            .map(|job| job.join().unwrap().is_ok())
            .filter(|success| *success)
            .count(),
        1
    );
    assert_eq!(registry.list().unwrap().len(), 1);
    assert_eq!(secrets.values.lock_or_recover().len(), 1);
}

#[test]
fn hs2_1y45mz_invalid_origins_and_bounded_protocol_errors_never_touch_credentials() {
    let home = tempfile::tempdir().unwrap();
    let registry = RemoteRegistry::new(home.path().join("remotes.json"));
    let secrets = Memory::default();
    let peer = Peer::default();
    for invalid in [
        "http://remote.example.test",
        "http://localhost",
        "https://host.test/path",
        "https://user@host.test",
        "https://host.test?query",
        "https://host.test#fragment",
        "https://host.test\\path",
        " https://host.test",
    ] {
        let mut request = request();
        request.origin = invalid.into();
        assert!(registry.register(request, &secrets, &peer, 1).is_err());
    }
    assert_eq!(peer.calls.load(Ordering::SeqCst), 0);
    assert!(secrets.events.lock_or_recover().is_empty());
    assert_eq!(
        normalize_remote_origin("HTTPS://EXAMPLE.TEST:443/").unwrap(),
        "https://example.test"
    );
    assert!(normalize_remote_origin("http://[::1]:80").is_ok());
    assert!(remote_registry_origin_is_loopback("https://127.0.0.1:1234"));
    assert!(!remote_registry_origin_is_loopback(
        "https://localhost:1234"
    ));
    let mut invalid = snapshot(PEER);
    invalid.checkouts = vec![invalid.checkouts[0].clone(); MAX_REMOTE_CHECKOUTS + 1];
    *peer.result.lock_or_recover() = Ok(invalid);
    assert!(registry.register(request(), &secrets, &peer, 1).is_err());
    assert!(!registry.path.exists());
    assert!(secrets.events.lock_or_recover().is_empty());
}

#[test]
fn hs2_1y45mz_newer_corrupt_and_symlinked_registries_are_never_overwritten() {
    let home = tempfile::tempdir().unwrap();
    let registry = RemoteRegistry::new(home.path().join("remotes.json"));
    for contents in [
        "not json",
        r#"{"version":2,"servers":[]}"#,
        r#"{"version":1,"servers":[],"pending_cleanup":["invalid"]}"#,
    ] {
        std::fs::write(&registry.path, contents).unwrap();
        assert!(registry.list().is_err());
        assert_eq!(std::fs::read_to_string(&registry.path).unwrap(), contents);
    }
    #[cfg(unix)]
    {
        std::fs::remove_file(&registry.path).unwrap();
        let target = home.path().join("sentinel");
        std::fs::write(&target, "synthetic sentinel").unwrap();
        std::os::unix::fs::symlink(&target, &registry.path).unwrap();
        assert!(registry.list().is_err());
        assert_eq!(
            std::fs::read_to_string(target).unwrap(),
            "synthetic sentinel"
        );
    }
}

#[test]
fn hs2_1y45mz_server_identity_is_stable_per_home_and_corruption_does_not_remint() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    assert_eq!(
        server_identity(first.path(), || PEER.into())
            .unwrap()
            .server_id,
        PEER
    );
    assert_eq!(
        server_identity(first.path(), || OTHER.into())
            .unwrap()
            .server_id,
        PEER
    );
    assert_eq!(
        server_identity(second.path(), || OTHER.into())
            .unwrap()
            .server_id,
        OTHER
    );
    std::fs::write(first.path().join("server-identity.json"), "not json").unwrap();
    assert!(server_identity(first.path(), || ID.into()).is_err());
    assert_eq!(
        std::fs::read_to_string(first.path().join("server-identity.json")).unwrap(),
        "not json"
    );
}

#[test]
fn hs2_1y45mz_failed_and_interrupted_review_preserves_the_prior_active_credential() {
    let home = tempfile::tempdir().unwrap();
    let registry = RemoteRegistry::new(home.path().join("remotes.json"));
    let secrets = Memory::default();
    let peer = Peer::default();
    let original = registry.register(request(), &secrets, &peer, 1).unwrap();
    let mut replacement = review(&original.remote);
    replacement.credentials.bearer_secret = "synthetic-replacement-bearer".into();
    let before = std::fs::read(registry.path()).unwrap();
    *peer.result.lock_or_recover() = Err(RemoteFailure::Unauthorized);
    assert!(
        registry
            .review(ID, replacement.clone(), &secrets, &peer, 2)
            .is_err()
    );
    assert_eq!(before, std::fs::read(registry.path()).unwrap());
    *peer.result.lock_or_recover() = Ok(snapshot(PEER));
    secrets.fail_set.store(true, Ordering::SeqCst);
    assert!(matches!(
        registry.review(ID, replacement.clone(), &secrets, &peer, 3),
        Err(RemoteError::Credentials)
    ));
    assert_eq!(registry.list().unwrap()[0].revision, 1);
    assert_eq!(secrets.values.lock_or_recover().len(), 1);
    secrets.fail_set.store(false, Ordering::SeqCst);
    secrets.crash_after_set.store(true, Ordering::SeqCst);
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            registry.review(ID, replacement.clone(), &secrets, &peer, 4)
        }))
        .is_err()
    );
    assert_eq!(registry.list().unwrap()[0].revision, 1);
    assert_eq!(registry.read().unwrap().pending_cleanup.len(), 1);
    assert_eq!(secrets.values.lock_or_recover().len(), 2);
    let old = registry.discover(ID, &secrets, &peer, 5).unwrap();
    assert!(old.checkouts.is_some());
    assert_eq!(old.remote.revision, 1);
    let recovered = registry
        .review(ID, replacement, &secrets, &peer, 6)
        .unwrap();
    assert_eq!(recovered.remote.revision, 2);
    assert!(registry.read().unwrap().pending_cleanup.is_empty());
    let values = secrets.values.lock_or_recover();
    assert_eq!(values.len(), 1);
    assert!(
        values
            .values()
            .next()
            .unwrap()
            .contains("synthetic-replacement-bearer")
    );
}
