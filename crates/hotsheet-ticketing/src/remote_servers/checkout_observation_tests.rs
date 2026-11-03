use super::*;
use crate::orchestration::{
    CheckoutProjection, SourceState, TargetFailure, TargetSource, checkout_authority,
};

struct ResolutionPeer {
    peer: Peer,
    projection: Mutex<Result<CheckoutProjection, TargetFailure>>,
    metadata_calls: AtomicUsize,
}
impl Default for ResolutionPeer {
    fn default() -> Self {
        let peer = Peer::default();
        peer.result.lock_or_recover().as_mut().unwrap().checkouts[0].resolution_metadata_version =
            Some(1);
        let sources = vec![TargetSource {
            connection_id: "synthetic-source".into(),
            provider: "synthetic".into(),
            identity: Some("a".repeat(64)),
            state: SourceState::Available,
            capabilities: Some(crate::ProviderCapabilities::git()),
        }];
        Self {
            peer,
            projection: Mutex::new(Ok(CheckoutProjection {
                schema_version: 1,
                server_id: PEER.into(),
                checkout_id: "synthetic-checkout".into(),
                checkout_authority: checkout_authority(&sources).unwrap(),
                sources,
            })),
            metadata_calls: AtomicUsize::new(0),
        }
    }
}
impl RemoteTransport for ResolutionPeer {
    fn probe(
        &self,
        origin: &str,
        credentials: &RemoteCredentials,
    ) -> Result<RemoteSnapshot, RemoteFailure> {
        self.peer.probe(origin, credentials)
    }
}
impl RemoteCheckoutTransport for ResolutionPeer {
    fn checkout_projection(
        &self,
        _: &str,
        _: &RemoteCredentials,
        _: &str,
    ) -> Result<CheckoutProjection, TargetFailure> {
        self.metadata_calls.fetch_add(1, Ordering::SeqCst);
        self.projection.lock_or_recover().clone()
    }
}

#[test]
fn hs2_4j6v46_remote_proof_holds_commit_guard_and_revoke_never_returns_live_projection() {
    let temp = tempfile::tempdir().unwrap();
    let registry = RemoteRegistry::new(temp.path().join("remotes.json"));
    let secrets = Memory::default();
    let peer = ResolutionPeer::default();
    let identity = registry
        .register(request(), &secrets, &peer, 100)
        .unwrap()
        .remote
        .identity;
    let unlocked_path = registry.path().with_extension("json.lock");
    *peer.peer.before.lock_or_recover() = Some(Box::new(move || {
        let fd = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&unlocked_path)
            .unwrap();
        assert!(
            crate::FileLock::try_acquire(fd).unwrap().is_some(),
            "network held registry lock"
        );
    }));
    let (proof, result) = registry
        .observe_checkout(ID, &identity, "synthetic-checkout", &secrets, &peer)
        .unwrap();
    assert!(result.is_ok());
    let output = temp.path().join("local-commit");
    registry
        .with_checkout_observation(&proof, || {
            let fd = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(registry.path().with_extension("json.lock"))
                .unwrap();
            assert!(crate::FileLock::try_acquire(fd).unwrap().is_none());
            std::fs::write(&output, b"synthetic durable local commit").unwrap();
            assert!(output.exists());
        })
        .unwrap();
    registry.revoke(ID, 1, &secrets).unwrap();
    assert!(matches!(
        registry.with_checkout_observation(&proof, || panic!("revoked authority committed")),
        Err(RemoteError::Conflict)
    ));
    let calls = peer.metadata_calls.load(Ordering::SeqCst);
    let (revoked_proof, result) = registry
        .observe_checkout(ID, &identity, "synthetic-checkout", &secrets, &peer)
        .unwrap();
    assert_eq!(result.unwrap_err(), TargetFailure::Revoked);
    assert_eq!(peer.metadata_calls.load(Ordering::SeqCst), calls);
    // A current failure proof can record an unusable result, never a projection.
    assert!(
        !registry
            .with_checkout_observation(&revoked_proof, || false)
            .unwrap()
    );
    registry.revoke(ID, 2, &secrets).unwrap(); // Same public revision; private fence changes.
    assert!(matches!(
        registry
            .with_checkout_observation(&revoked_proof, || panic!("repeated revoke proof accepted")),
        Err(RemoteError::Conflict)
    ));
}

#[test]
fn hs2_4j6v46_remote_proof_captured_before_transport_cannot_borrow_newer_generation() {
    let temp = tempfile::tempdir().unwrap();
    let registry = RemoteRegistry::new(temp.path().join("remotes.json"));
    let secrets = Arc::new(Memory::default());
    let peer = ResolutionPeer::default();
    let identity = registry
        .register(request(), secrets.as_ref(), &peer, 100)
        .unwrap()
        .remote
        .identity;
    let changing = registry.clone();
    let changing_secrets = secrets.clone();
    *peer.peer.before.lock_or_recover() = Some(Box::new(move || {
        changing.revoke(ID, 1, changing_secrets.as_ref()).unwrap();
    }));
    let (proof, result) = registry
        .observe_checkout(ID, &identity, "synthetic-checkout", secrets.as_ref(), &peer)
        .unwrap();
    assert!(result.is_ok()); // Response is only as-of; final local guard still decides.
    assert!(matches!(
        registry.with_checkout_observation(&proof, || panic!("late response committed")),
        Err(RemoteError::Conflict)
    ));
}

#[test]
fn hs2_4j6v46_old_peer_missing_checkout_and_wrong_metadata_identity_are_explicit() {
    let temp = tempfile::tempdir().unwrap();
    let registry = RemoteRegistry::new(temp.path().join("remotes.json"));
    let secrets = Memory::default();
    let peer = ResolutionPeer::default();
    let identity = registry
        .register(request(), &secrets, &peer, 100)
        .unwrap()
        .remote
        .identity;
    peer.peer
        .result
        .lock_or_recover()
        .as_mut()
        .unwrap()
        .checkouts[0]
        .resolution_metadata_version = None;
    let (_, old) = registry
        .observe_checkout(ID, &identity, "synthetic-checkout", &secrets, &peer)
        .unwrap();
    assert_eq!(old.unwrap_err(), TargetFailure::UnsupportedMetadata);
    assert_eq!(peer.metadata_calls.load(Ordering::SeqCst), 0);
    let (_, missing) = registry
        .observe_checkout(ID, &identity, "different-checkout", &secrets, &peer)
        .unwrap();
    assert_eq!(missing.unwrap_err(), TargetFailure::SourceMissing);
    peer.peer
        .result
        .lock_or_recover()
        .as_mut()
        .unwrap()
        .checkouts[0]
        .resolution_metadata_version = Some(1);
    peer.projection
        .lock_or_recover()
        .as_mut()
        .unwrap()
        .server_id = OTHER.into();
    let (proof, wrong) = registry
        .observe_checkout(ID, &identity, "synthetic-checkout", &secrets, &peer)
        .unwrap();
    assert_eq!(wrong.unwrap_err(), TargetFailure::IdentityChanged);
    assert!(
        !registry
            .with_checkout_observation(&proof, || false)
            .unwrap()
    );
}
