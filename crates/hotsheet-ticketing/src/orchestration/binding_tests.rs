//! HS2-4J6V46 injected observations; no transport, provider account or native secret access.
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, AtomicUsize, Ordering},
    mpsc,
};

use super::*;

struct Clock(u64);
impl crate::Clock for Clock {
    fn now_ms(&self) -> u64 {
        self.0
    }
}
struct Rng(u128);
impl crate::Rng for Rng {
    fn next_u128(&mut self) -> u128 {
        self.0 += 1;
        self.0
    }
}
fn id(n: u128) -> String {
    ulid::Ulid::from(n).to_string()
}
fn reference() -> TargetRef {
    TargetRef {
        owner_checkout_id: "owner-0123456789ab".into(),
        target_id: id(1),
    }
}
fn projection() -> CheckoutProjection {
    let sources = vec![TargetSource {
        connection_id: "source-main".into(),
        provider: "synthetic".into(),
        identity: Some("a".repeat(64)),
        state: SourceState::Available,
        capabilities: Some(crate::ProviderCapabilities::git()),
    }];
    CheckoutProjection {
        schema_version: 1,
        server_id: id(10),
        checkout_id: "target-0123456789ab".into(),
        checkout_authority: checkout_authority(&sources).unwrap(),
        sources,
    }
}
fn destination() -> TargetDestination {
    let p = projection();
    TargetDestination::Local {
        server_id: p.server_id,
        checkout_id: p.checkout_id,
        checkout_authority: p.checkout_authority,
    }
}

struct Observer {
    owner: String,
    root: PathBuf,
    sources: AtomicUsize,
    observations: AtomicUsize,
    adapter_failure: AtomicUsize,
    generation: AtomicU64,
    source_lock: Mutex<()>,
    remote_lock: Mutex<()>,
    result: Mutex<Result<CheckoutProjection, TargetFailure>>,
}
impl Observer {
    fn new(root: &Path) -> Self {
        Self {
            owner: reference().owner_checkout_id,
            root: root.into(),
            sources: AtomicUsize::new(1),
            observations: AtomicUsize::new(0),
            adapter_failure: AtomicUsize::new(0),
            generation: AtomicU64::new(1),
            source_lock: Mutex::new(()),
            remote_lock: Mutex::new(()),
            result: Mutex::new(Ok(projection())),
        }
    }
}
impl TargetObserver for Observer {
    type Proof = u64;
    fn observe(
        &self,
        _: &TargetRef,
        _: &TargetDestination,
    ) -> Result<ObservedTarget<u64>, OrchestrationError> {
        self.observations.fetch_add(1, Ordering::SeqCst);
        if self.adapter_failure.load(Ordering::SeqCst) != 0 {
            return Err(OrchestrationError::Observation(TargetFailure::Unreachable));
        }
        Ok(ObservedTarget::new(
            self.generation.load(Ordering::SeqCst),
            self.result.lock().unwrap().clone(),
        ))
    }
    fn with_source_guard<T>(
        &self,
        owner: &str,
        root: &Path,
        proof: Option<&u64>,
        commit: impl FnOnce() -> Result<T, OrchestrationError>,
    ) -> Result<T, OrchestrationError> {
        let _guard = self.source_lock.lock().unwrap();
        if owner != self.owner || root != self.root || self.sources.load(Ordering::SeqCst) == 0 {
            return Err(OrchestrationError::Observation(
                TargetFailure::SourceMissing,
            ));
        }
        if proof.is_some_and(|proof| *proof != self.generation.load(Ordering::SeqCst)) {
            return Err(OrchestrationError::Conflict);
        }
        commit()
    }
    fn with_remote_guard<T>(
        &self,
        proof: &u64,
        commit: impl FnOnce() -> Result<T, OrchestrationError>,
    ) -> Result<T, OrchestrationError> {
        let _guard = self.remote_lock.lock().unwrap();
        if *proof != self.generation.load(Ordering::SeqCst) {
            return Err(OrchestrationError::Conflict);
        }
        commit()
    }
}
struct Delayed {
    inner: Arc<Observer>,
    ready: mpsc::Sender<()>,
    release: Mutex<mpsc::Receiver<()>>,
}
impl TargetObserver for Delayed {
    type Proof = u64;
    fn observe(
        &self,
        target: &TargetRef,
        destination: &TargetDestination,
    ) -> Result<ObservedTarget<u64>, OrchestrationError> {
        let result = self.inner.observe(target, destination)?;
        self.ready.send(()).unwrap();
        self.release
            .lock()
            .unwrap()
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        Ok(result)
    }
    fn with_source_guard<T>(
        &self,
        owner: &str,
        root: &Path,
        proof: Option<&u64>,
        commit: impl FnOnce() -> Result<T, OrchestrationError>,
    ) -> Result<T, OrchestrationError> {
        self.inner.with_source_guard(owner, root, proof, commit)
    }
    fn with_remote_guard<T>(
        &self,
        proof: &u64,
        commit: impl FnOnce() -> Result<T, OrchestrationError>,
    ) -> Result<T, OrchestrationError> {
        self.inner.with_remote_guard(proof, commit)
    }
}
fn setup() -> (
    tempfile::TempDir,
    tempfile::TempDir,
    Arc<Orchestration>,
    Arc<Observer>,
    String,
) {
    let root = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let manifest = ManifestStore::new(root.path());
    let mut rng = Rng(1);
    let clock = Clock(100);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let view = manifest
        .add(
            None,
            TargetDeclaration {
                target_id: id(1),
                state: DeclarationState::Active,
                label: "Synthetic target".into(),
                expected_repository_identity: None,
            },
            &mut tx,
        )
        .unwrap();
    let flow = Arc::new(
        Orchestration::new(
            reference().owner_checkout_id,
            manifest,
            BindingRegistry::new(home.path()),
        )
        .unwrap(),
    );
    let observer = Arc::new(Observer::new(root.path()));
    (root, home, flow, observer, view.revision.unwrap())
}
fn bind_request(
    revision: &str,
    binding_revision: u64,
    previous: Option<TargetDestination>,
) -> BindTarget {
    BindTarget {
        target: reference(),
        expected_manifest_revision: revision.into(),
        expected_binding_revision: binding_revision,
        previous_destination: previous,
        destination: destination(),
    }
}
fn unbind_request(revision: u64, previous: Option<TargetDestination>) -> UnbindTarget {
    UnbindTarget {
        target: reference(),
        expected_binding_revision: revision,
        previous_destination: previous,
    }
}

mod capacity;
mod lifecycle;
mod ordering;
