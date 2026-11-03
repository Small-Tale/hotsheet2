//! Synthetic injected owner compositions only; no native secrets, accounts or network.
use std::collections::HashMap;
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

use hotsheet_ticketing::checkouts::TicketSource;
use hotsheet_ticketing::orchestration::{
    DeclarationState, SourceState, TargetDeclaration, TargetDestination, TargetSource,
    checkout_authority,
};
use hotsheet_ticketing::remote_servers::{
    RegisterRemote, RemoteCheckout, RemoteCheckoutState, RemoteCredentials, RemoteFailure,
    RemoteIdentity, RemoteSnapshot, RemoteSource, RemoteTransport, ReviewRemote,
};
use hotsheet_ticketing::{ProviderCapabilities, SecretError};

use super::*;

fn id(n: u128) -> String {
    ulid::Ulid::from(n).to_string()
}
struct Clock;
impl hotsheet_ticketing::Clock for Clock {
    fn now_ms(&self) -> u64 {
        200
    }
}
struct Rng(u128);
impl hotsheet_ticketing::Rng for Rng {
    fn next_u128(&mut self) -> u128 {
        self.0 += 1;
        self.0
    }
}
#[derive(Default)]
struct Memory {
    values: Mutex<HashMap<String, String>>,
    gets: AtomicUsize,
}
impl SecretStore for Memory {
    fn get(&self, key: &str) -> Result<Option<String>, SecretError> {
        self.gets.fetch_add(1, Ordering::SeqCst);
        Ok(self
            .values
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(key)
            .cloned())
    }
    fn set(&self, key: &str, value: &str) -> Result<(), SecretError> {
        self.values
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(key.into(), value.into());
        Ok(())
    }
    fn delete(&self, key: &str) -> Result<bool, SecretError> {
        Ok(self
            .values
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(key)
            .is_some())
    }
}
struct Peer {
    calls: AtomicUsize,
    projection: CheckoutProjection,
    before_metadata: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}
impl Default for Peer {
    fn default() -> Self {
        let sources = vec![TargetSource {
            connection_id: "remote-source".into(),
            provider: "synthetic".into(),
            identity: Some("b".repeat(64)),
            state: SourceState::Available,
            capabilities: Some(ProviderCapabilities::git()),
        }];
        Self {
            calls: AtomicUsize::new(0),
            before_metadata: Mutex::new(None),
            projection: CheckoutProjection {
                schema_version: 1,
                server_id: id(99),
                checkout_id: "remote-0123456789ab".into(),
                checkout_authority: checkout_authority(&sources).unwrap(),
                sources,
            },
        }
    }
}
impl RemoteTransport for Peer {
    fn probe(&self, _: &str, _: &RemoteCredentials) -> Result<RemoteSnapshot, RemoteFailure> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(RemoteSnapshot {
            identity: RemoteIdentity {
                generation: "hs2".into(),
                protocol: 1,
                server_id: id(99),
            },
            checkouts: vec![RemoteCheckout {
                id: self.projection.checkout_id.clone(),
                name: "Synthetic remote".into(),
                sources: vec![RemoteSource {
                    connection_id: "remote-source".into(),
                    provider: "synthetic".into(),
                }],
                state: RemoteCheckoutState::Available,
                resolution_metadata_version: Some(1),
            }],
        })
    }
}
impl RemoteCheckoutTransport for Peer {
    fn checkout_projection(
        &self,
        _: &str,
        _: &RemoteCredentials,
        _: &str,
    ) -> Result<CheckoutProjection, TargetFailure> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(before) = self
            .before_metadata
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            before();
        }
        Ok(self.projection.clone())
    }
}
struct Fixture {
    _root: tempfile::TempDir,
    home: PathBuf,
    runtime: OrchestrationRuntime,
    owner: Checkout,
    target: Checkout,
    peer: Arc<Peer>,
    secrets: Arc<Memory>,
}
fn fixture() -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let checkouts = CheckoutRegistry::new(home.join("checkouts.json"));
    let providers = ProviderConfigRegistry::new(root.path().join("providers.json"));
    providers
        .save(&[ProviderConnection {
            id: "synthetic-source".into(),
            provider: "github".into(),
            locator: "synthetic/repo".into(),
            name: None,
            default: false,
            settings: serde_json::Value::Null,
            disabled: false,
        }])
        .unwrap();
    let mut projects = Vec::new();
    for name in ["owner", "destination"] {
        let path = root.path().join(name);
        std::fs::create_dir(&path).unwrap();
        projects.push(
            checkouts
                .register_registered_sources(
                    &providers,
                    &path,
                    None,
                    None,
                    vec![TicketSource {
                        connection_id: "synthetic-source".into(),
                        provider: "github".into(),
                        locator: "synthetic/repo".into(),
                    }],
                    None,
                )
                .unwrap(),
        );
    }
    let peer = Arc::new(Peer::default());
    let secrets = Arc::new(Memory::default());
    let runtime = OrchestrationRuntime::new(
        home.clone(),
        checkouts,
        providers,
        secrets.clone(),
        peer.clone(),
        id(10),
    )
    .unwrap();
    Fixture {
        _root: root,
        home,
        runtime,
        owner: projects.remove(0),
        target: projects.remove(0),
        peer,
        secrets,
    }
}
fn declaration(f: &Fixture, tx: &mut Transaction<'_>) -> String {
    f.runtime
        .replace(
            &f.owner.id,
            ReplaceTargets {
                expected_revision: None,
                targets: vec![TargetDeclaration {
                    target_id: id(1),
                    state: DeclarationState::Active,
                    label: "Synthetic target".into(),
                    expected_repository_identity: None,
                }],
            },
            tx,
        )
        .unwrap()
        .revision
        .unwrap()
}
fn reference(f: &Fixture) -> TargetRef {
    TargetRef {
        owner_checkout_id: f.owner.id.clone(),
        target_id: id(1),
    }
}
fn request(f: &Fixture, revision: String, destination: TargetDestination) -> BindTarget {
    BindTarget {
        target: reference(f),
        expected_manifest_revision: revision,
        expected_binding_revision: 0,
        previous_destination: None,
        destination,
    }
}
fn credentials() -> RemoteCredentials {
    RemoteCredentials {
        bearer_secret: "synthetic-bearer".into(),
        ca_pem: None,
        client_certificate_pem: None,
        client_key_pem: None,
    }
}
fn register(f: &Fixture) -> TargetDestination {
    let registered = f
        .runtime
        .remotes
        .register(
            RegisterRemote {
                id: id(20),
                label: "Synthetic peer".into(),
                origin: "http://127.0.0.1:8765".into(),
                expected_server_id: id(99),
                credentials: credentials(),
            },
            f.secrets.as_ref(),
            f.peer.as_ref(),
            100,
        )
        .unwrap();
    TargetDestination::Remote {
        registration_id: id(20),
        identity: registered.remote.identity,
        checkout_id: f.peer.projection.checkout_id.clone(),
        checkout_authority: f.peer.projection.checkout_authority.clone(),
    }
}

#[test]
fn hs2_4j6v46_shared_provider_only_local_flow_and_orphan_unbind_never_use_credentials_or_transport()
{
    let f = fixture();
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let revision = declaration(&f, &mut tx);
    let projection = f.runtime.checkout_projection(&f.target.id).unwrap();
    let destination = TargetDestination::Local {
        server_id: projection.server_id,
        checkout_id: projection.checkout_id,
        checkout_authority: projection.checkout_authority,
    };
    assert!(
        f.runtime
            .bind(request(&f, revision, destination.clone()), &mut tx)
            .unwrap()
            .usable
    );
    assert!(f.runtime.resolve(&reference(&f), &mut tx).unwrap().usable);
    f.runtime
        .providers
        .set_disabled("synthetic-source", true)
        .unwrap();
    let failed = f.runtime.resolve(&reference(&f), &mut tx).unwrap();
    assert_eq!(failed.reason, Some(TargetFailure::DisabledSource));
    assert!(!failed.usable);
    assert!(failed.sources.is_none());
    f.runtime
        .providers
        .set_disabled("synthetic-source", false)
        .unwrap();
    assert!(f.runtime.resolve(&reference(&f), &mut tx).unwrap().usable);
    assert!(f.runtime.bindings(&f.owner.id).unwrap()[0].stale);
    assert_eq!(
        f.runtime.declarations(&f.owner.id).unwrap().targets.len(),
        1
    );
    assert_eq!(f.peer.calls.load(Ordering::SeqCst), 0);
    assert_eq!(f.secrets.gets.load(Ordering::SeqCst), 0);
    std::fs::remove_file(f.runtime.checkouts.path()).unwrap();
    std::fs::write(
        Path::new(&f.owner.root).join(".hotsheet2/orchestration.json"),
        b"corrupt",
    )
    .unwrap();
    std::fs::write(f.runtime.providers.path(), b"corrupt").unwrap();
    std::fs::write(f.home.join("server-identity.json"), b"corrupt").unwrap();
    assert_eq!(
        OrchestrationRuntime::binding_at(&f.home, &reference(&f))
            .unwrap()
            .revision,
        1
    );
    let disabled = OrchestrationRuntime::unbind_at(
        &f.home,
        UnbindTarget {
            target: reference(&f),
            expected_binding_revision: 1,
            previous_destination: Some(destination),
        },
        &mut tx,
    )
    .unwrap();
    assert_eq!(disabled.revision, 2);
    assert!(disabled.destination.is_none());
    assert_eq!(f.peer.calls.load(Ordering::SeqCst), 0);
    assert_eq!(f.secrets.gets.load(Ordering::SeqCst), 0);
}

#[test]
fn hs2_4j6v46_shared_remote_flow_accepts_fresh_same_identity_address_review_and_records_revoked_failure()
 {
    let f = fixture();
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let revision = declaration(&f, &mut tx);
    let destination = register(&f);
    let bound = f
        .runtime
        .bind(request(&f, revision, destination.clone()), &mut tx)
        .unwrap();
    assert!(bound.usable);
    let TargetDestination::Remote { identity, .. } = &destination else {
        unreachable!()
    };
    f.runtime
        .remotes
        .review(
            &id(20),
            ReviewRemote {
                expected_revision: 1,
                previous_identity: identity.clone(),
                label: "Synthetic peer".into(),
                origin: "http://127.0.0.1:8766".into(),
                expected_server_id: id(99),
                credentials: credentials(),
            },
            f.secrets.as_ref(),
            f.peer.as_ref(),
            0,
        )
        .unwrap();
    let moved = f.runtime.resolve(&reference(&f), &mut tx).unwrap();
    assert!(moved.usable);
    assert_eq!(moved.binding.revision, bound.binding.revision);
    assert_eq!(moved.binding.destination, Some(destination));
    assert_eq!(moved.binding.review_history, bound.binding.review_history);
    let calls = f.peer.calls.load(Ordering::SeqCst);
    let gets = f.secrets.gets.load(Ordering::SeqCst);
    f.runtime.bindings(&f.owner.id).unwrap();
    f.runtime.declarations(&f.owner.id).unwrap();
    assert_eq!(f.peer.calls.load(Ordering::SeqCst), calls);
    assert_eq!(f.secrets.gets.load(Ordering::SeqCst), gets);
    f.runtime
        .remotes
        .revoke(&id(20), 2, f.secrets.as_ref())
        .unwrap();
    let revoked = f.runtime.resolve(&reference(&f), &mut tx).unwrap();
    assert_eq!(revoked.reason, Some(TargetFailure::Revoked));
    assert!(!revoked.usable);
    assert!(revoked.stale);
    assert!(revoked.sources.is_none());
}

#[test]
fn hs2_4j6v46_shared_composition_holds_no_owner_locks_over_transport_and_rejects_raced_proofs() {
    for revoke in [true, false] {
        let f = fixture();
        let mut rng = Rng(100);
        let mut tx = Transaction {
            clock: &Clock,
            rng: &mut rng,
        };
        let revision = declaration(&f, &mut tx);
        let destination = register(&f);
        let checkout_lock = f.runtime.checkouts.path().with_extension("json.lock");
        // Capture a configuration proof so the stable provider lock exists.
        f.runtime.providers.observe_configuration().unwrap();
        let provider_lock = f.runtime.providers.path().with_extension("json.lock");
        let remote_lock = f.runtime.remotes.path().with_extension("json.lock");
        let remotes = f.runtime.remotes.clone();
        let secrets = f.secrets.clone();
        let providers = f.runtime.providers.clone();
        *f.peer
            .before_metadata
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Box::new(move || {
            for path in [&checkout_lock, &provider_lock, &remote_lock] {
                let fd = std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(path)
                    .unwrap();
                assert!(
                    hotsheet_ticketing::FileLock::try_acquire(fd)
                        .unwrap()
                        .is_some(),
                    "transport retained owner lock"
                );
            }
            if revoke {
                remotes.revoke(&id(20), 1, secrets.as_ref()).unwrap();
            } else {
                providers.set_disabled("synthetic-source", true).unwrap();
            }
        }));
        assert!(matches!(
            f.runtime.bind(request(&f, revision, destination), &mut tx),
            Err(OrchestrationError::Conflict)
        ));
        let intent = &f.runtime.bindings(&f.owner.id).unwrap()[0];
        assert_eq!(intent.revision, 0);
        assert!(intent.destination.is_none());
        assert!(intent.checked_at_ms.is_none());
        assert!(intent.confirmed_at_ms.is_none());
        assert!(intent.last_failure.is_none());
        assert!(intent.review_history.is_empty());
        assert!(intent.last_confirmed_sources.is_empty());
    }
}

#[test]
fn hs2_4j6v46_shared_same_owner_destination_composition_has_no_recursive_lock() {
    let (send, receive) = std::sync::mpsc::channel();
    let task = std::thread::spawn(move || {
        let f = fixture();
        let mut rng = Rng(100);
        let mut tx = Transaction {
            clock: &Clock,
            rng: &mut rng,
        };
        let revision = declaration(&f, &mut tx);
        let projection = f.runtime.checkout_projection(&f.owner.id).unwrap();
        let destination = TargetDestination::Local {
            server_id: projection.server_id,
            checkout_id: projection.checkout_id,
            checkout_authority: projection.checkout_authority,
        };
        assert!(
            f.runtime
                .bind(request(&f, revision, destination), &mut tx)
                .unwrap()
                .usable
        );
        assert!(f.runtime.resolve(&reference(&f), &mut tx).unwrap().usable);
        send.send((
            f.peer.calls.load(Ordering::SeqCst),
            f.secrets.gets.load(Ordering::SeqCst),
        ))
        .unwrap();
    });
    assert_eq!(
        receive
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("same-owner composition blocked"),
        (0, 0)
    );
    task.join().unwrap();
}

#[test]
fn hs2_4j6v46_shared_observer_retains_all_real_owner_guards_through_commit_callback() {
    use hotsheet_ticketing::orchestration::TargetObserver;
    let f = fixture();
    let destination = register(&f);
    let observer = RuntimeObserver::new(&f.runtime, &f.owner);
    let (proof, projection) = observer.capture(&reference(&f), &destination).unwrap();
    assert!(projection.is_ok());
    let output = f.home.join("synthetic-guarded-commit");
    observer
        .with_source_guard(&f.owner.id, Path::new(&f.owner.root), Some(&proof), || {
            observer.with_remote_guard(&proof, || {
                for path in [
                    f.runtime.checkouts.path().with_extension("json.lock"),
                    f.runtime.providers.path().with_extension("json.lock"),
                    f.runtime.remotes.path().with_extension("json.lock"),
                ] {
                    let fd = std::fs::OpenOptions::new()
                        .read(true)
                        .write(true)
                        .open(path)
                        .unwrap();
                    assert!(
                        hotsheet_ticketing::FileLock::try_acquire(fd)
                            .unwrap()
                            .is_none()
                    );
                }
                std::fs::write(&output, b"synthetic durable callback commit").unwrap();
                assert!(output.exists());
                Ok(())
            })
        })
        .unwrap();
    for path in [
        f.runtime.checkouts.path().with_extension("json.lock"),
        f.runtime.providers.path().with_extension("json.lock"),
        f.runtime.remotes.path().with_extension("json.lock"),
    ] {
        let fd = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .unwrap();
        assert!(
            hotsheet_ticketing::FileLock::try_acquire(fd)
                .unwrap()
                .is_some()
        );
    }
}

#[test]
fn hs2_4j6v46_shared_request_seven_operations_preserve_revisions_and_inspect_writes_no_binding() {
    let f = fixture();
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let prefix = format!("/checkouts/{}/orchestration/targets", f.owner.id);
    let inspect = f
        .runtime
        .request(
            "POST",
            &format!("/checkouts/{}/orchestration/inspect", f.owner.id),
            &serde_json::json!({"destination":{"kind":"local","checkout_id":f.target.id}}),
            &mut tx,
        )
        .unwrap();
    assert_eq!(inspect["usable"], false);
    assert_eq!(inspect["stale"], false);
    assert!(!f.home.join("orchestration-bindings.json").exists());
    assert!(
        !Path::new(&f.owner.root)
            .join(".hotsheet2/orchestration.json")
            .exists()
    );
    let add =
        serde_json::json!({"expected_revision":null,"target_id":id(1),"label":"Synthetic target"});
    let created = f.runtime.request("POST", &prefix, &add, &mut tx).unwrap();
    let revision = created["revision"].as_str().unwrap();
    assert!(matches!(
        f.runtime.request("POST", &prefix, &add, &mut tx),
        Err(OrchestrationError::Conflict)
    ));
    let mut retry = add;
    retry["expected_revision"] = revision.into();
    assert_eq!(
        f.runtime.request("POST", &prefix, &retry, &mut tx).unwrap(),
        created
    );
    let target_path = format!("{prefix}/{}", id(1));
    let renamed = f
        .runtime
        .request(
            "PATCH",
            &target_path,
            &serde_json::json!({"expected_revision":revision,"label":"Changed label"}),
            &mut tx,
        )
        .unwrap();
    assert_eq!(renamed["targets"][0]["target_id"], id(1));
    let binding_path = format!("{target_path}/binding");
    let bind = serde_json::json!({"target":reference(&f),"expected_manifest_revision":renamed["revision"],
        "expected_binding_revision":0,"previous_destination":null,"destination":inspect["destination"]});
    assert_eq!(
        f.runtime
            .request("PUT", &binding_path, &bind, &mut tx)
            .unwrap()["usable"],
        true
    );
    assert_eq!(
        f.runtime
            .request(
                "GET",
                &format!("{target_path}/resolve"),
                &serde_json::Value::Null,
                &mut tx
            )
            .unwrap()["usable"],
        true
    );
    assert_eq!(
        f.runtime
            .request("GET", &prefix, &serde_json::Value::Null, &mut tx)
            .unwrap(),
        renamed
    );
    assert_eq!(
        f.runtime
            .request("GET", &binding_path, &serde_json::Value::Null, &mut tx)
            .unwrap()["stale"],
        true
    );
    let removed = f
        .runtime
        .request(
            "DELETE",
            &target_path,
            &serde_json::json!({"expected_revision":renamed["revision"]}),
            &mut tx,
        )
        .unwrap();
    assert_eq!(removed["targets"][0]["state"], "removed");
    assert_eq!(
        f.runtime
            .request(
                "GET",
                &format!("{target_path}/resolve"),
                &serde_json::Value::Null,
                &mut tx
            )
            .unwrap()["usable"],
        false
    );
    let disabled = f.runtime.request("DELETE", &binding_path, &serde_json::json!({"target":reference(&f),"expected_binding_revision":1,"previous_destination":inspect["destination"]}), &mut tx).unwrap();
    assert_eq!(disabled["revision"], 2);
    assert!(disabled["destination"].is_null());
    assert_eq!(f.peer.calls.load(Ordering::SeqCst), 0);
    assert_eq!(f.secrets.gets.load(Ordering::SeqCst), 0);
}

#[test]
fn hs2_4j6v46_inspected_candidate_is_not_binding_authority_after_retarget_or_revoke() {
    use hotsheet_ticketing::orchestration::{InspectDestination, InspectTarget};
    for remote in [false, true] {
        let f = fixture();
        let mut rng = Rng(100);
        let mut tx = Transaction {
            clock: &Clock,
            rng: &mut rng,
        };
        let revision = declaration(&f, &mut tx);
        if remote {
            register(&f);
        }
        let selection = if remote {
            InspectDestination::Remote {
                registration_id: id(20),
                checkout_id: f.peer.projection.checkout_id.clone(),
            }
        } else {
            InspectDestination::Local {
                checkout_id: f.target.id.clone(),
            }
        };
        let manifest =
            std::fs::read(Path::new(&f.owner.root).join(".hotsheet2/orchestration.json")).unwrap();
        let candidate = f
            .runtime
            .inspect(
                &f.owner.id,
                InspectTarget {
                    destination: selection.clone(),
                },
            )
            .unwrap();
        assert!(!candidate.usable);
        assert!(!candidate.stale);
        assert!(!f.home.join("orchestration-bindings.json").exists());
        assert_eq!(
            std::fs::read(Path::new(&f.owner.root).join(".hotsheet2/orchestration.json")).unwrap(),
            manifest
        );
        if remote {
            f.runtime
                .remotes
                .revoke(&id(20), 1, f.secrets.as_ref())
                .unwrap();
        } else {
            f.runtime
                .providers
                .update(|connections| {
                    connections[0].locator = "different/repo".into();
                    Ok(())
                })
                .unwrap();
        }
        let expected = if remote {
            TargetFailure::Revoked
        } else {
            TargetFailure::IdentityChanged
        };
        assert!(
            matches!(f.runtime.bind(request(&f,revision.clone(),candidate.destination),&mut tx),Err(OrchestrationError::Observation(failure)) if failure == expected)
        );
        let path = f.home.join("orchestration-bindings.json");
        let before = std::fs::read(&path).unwrap();
        let next = f.runtime.inspect(
            &f.owner.id,
            InspectTarget {
                destination: selection,
            },
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
        if remote {
            assert!(matches!(
                next,
                Err(OrchestrationError::Observation(TargetFailure::Revoked))
            ));
        } else {
            assert!(
                f.runtime
                    .bind(request(&f, revision, next.unwrap().destination), &mut tx)
                    .unwrap()
                    .usable
            );
        }
    }
}

#[test]
fn hs2_4j6v46_revisioned_rename_corrects_or_clears_hint_without_changing_identity() {
    let f = fixture();
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let revision = declaration(&f, &mut tx);
    let path = format!("/checkouts/{}/orchestration/targets/{}", f.owner.id, id(1));
    let changed=f.runtime.request("PATCH",&path,&serde_json::json!({"expected_revision":revision,"label":"Synthetic target","expected_repository_identity":"https://repo.example.test/group/project"}),&mut tx).unwrap();
    assert_eq!(changed["targets"][0]["target_id"], id(1));
    assert_eq!(
        changed["targets"][0]["expected_repository_identity"],
        "https://repo.example.test/group/project"
    );
    let file = Path::new(&f.owner.root).join(".hotsheet2/orchestration.json");
    let before = std::fs::read(&file).unwrap();
    assert!(f.runtime.request("PATCH",&path,&serde_json::json!({"expected_revision":changed["revision"],"label":"Synthetic target","expected_repository_identity":"https://repo.example.test/other","clear_expected_repository_identity":true}),&mut tx).is_err());
    assert_eq!(std::fs::read(&file).unwrap(), before);
    let cleared=f.runtime.request("PATCH",&path,&serde_json::json!({"expected_revision":changed["revision"],"label":"Synthetic target","clear_expected_repository_identity":true}),&mut tx).unwrap();
    assert!(
        cleared["targets"][0]
            .get("expected_repository_identity")
            .is_none()
    );
    assert_ne!(cleared["revision"], changed["revision"]);
    assert!(!f.home.join("orchestration-bindings.json").exists());
}
