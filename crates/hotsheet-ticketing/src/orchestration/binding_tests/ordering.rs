use super::*;

#[test]
fn hs2_4j6v46_repeated_unbind_generation_fences_delayed_reactivation_same_revision() {
    let (_root, home, flow, observer, revision) = setup();
    let clock = Clock(200);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    flow.bind(bind_request(&revision, 0, None), observer.as_ref(), &mut tx)
        .unwrap();
    flow.unbind(unbind_request(1, Some(destination())), &mut tx)
        .unwrap();
    let (ready_tx, ready_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let delayed = Delayed {
        inner: observer.clone(),
        ready: ready_tx,
        release: Mutex::new(release_rx),
    };
    let task_flow = flow.clone();
    let task_revision = revision.clone();
    let task = std::thread::spawn(move || {
        let clock = Clock(10);
        let mut rng = Rng(500);
        let mut tx = Transaction {
            clock: &clock,
            rng: &mut rng,
        };
        task_flow.bind(bind_request(&task_revision, 2, None), &delayed, &mut tx)
    });
    ready_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    flow.unbind(unbind_request(2, None), &mut tx).unwrap();
    let after = std::fs::read(home.path().join("orchestration-bindings.json")).unwrap();
    release_tx.send(()).unwrap();
    assert!(matches!(
        task.join().unwrap(),
        Err(OrchestrationError::Conflict)
    ));
    assert_eq!(
        std::fs::read(home.path().join("orchestration-bindings.json")).unwrap(),
        after
    );
    let fresh = flow
        .bind(bind_request(&revision, 2, None), observer.as_ref(), &mut tx)
        .unwrap();
    assert!(fresh.usable);
    assert_eq!(fresh.binding.revision, 3);
}

#[test]
fn hs2_4j6v46_late_success_cannot_replace_newer_failure_even_with_backward_clock() {
    let (_root, home, flow, observer, revision) = setup();
    let clock = Clock(200);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    flow.bind(bind_request(&revision, 0, None), observer.as_ref(), &mut tx)
        .unwrap();
    let (ready_tx, ready_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let task_flow = flow.clone();
    let delayed = Delayed {
        inner: observer.clone(),
        ready: ready_tx,
        release: Mutex::new(release_rx),
    };
    let task = std::thread::spawn(move || {
        let clock = Clock(50);
        let mut rng = Rng(500);
        let mut tx = Transaction {
            clock: &clock,
            rng: &mut rng,
        };
        task_flow.resolve(&reference(), &delayed, &mut tx)
    });
    ready_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    *observer.result.lock().unwrap() = Err(TargetFailure::Unreachable);
    let failure = flow
        .resolve(&reference(), observer.as_ref(), &mut tx)
        .unwrap();
    assert!(!failure.usable);
    let after = std::fs::read(home.path().join("orchestration-bindings.json")).unwrap();
    release_tx.send(()).unwrap();
    assert!(matches!(
        task.join().unwrap(),
        Err(OrchestrationError::Conflict)
    ));
    assert_eq!(
        std::fs::read(home.path().join("orchestration-bindings.json")).unwrap(),
        after
    );
    *observer.result.lock().unwrap() = Ok(projection());
    assert!(
        flow.resolve(&reference(), observer.as_ref(), &mut tx)
            .unwrap()
            .usable
    );
}

#[test]
fn hs2_4j6v46_removed_manifest_and_new_remote_generation_fence_inflight_bind() {
    for remove_declaration in [true, false] {
        let (root, home, flow, observer, revision) = setup();
        let (ready_tx, ready_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let delayed = Delayed {
            inner: observer.clone(),
            ready: ready_tx,
            release: Mutex::new(release_rx),
        };
        let task_flow = flow.clone();
        let task_revision = revision.clone();
        let task = std::thread::spawn(move || {
            let clock = Clock(50);
            let mut rng = Rng(500);
            let mut tx = Transaction {
                clock: &clock,
                rng: &mut rng,
            };
            task_flow.bind(bind_request(&task_revision, 0, None), &delayed, &mut tx)
        });
        ready_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        if remove_declaration {
            let clock = Clock(200);
            let mut rng = Rng(100);
            let mut tx = Transaction {
                clock: &clock,
                rng: &mut rng,
            };
            ManifestStore::new(root.path())
                .remove(revision, &id(1), &mut tx)
                .unwrap();
        } else {
            let _guard = observer.remote_lock.lock().unwrap();
            observer.generation.fetch_add(1, Ordering::SeqCst);
        }
        release_tx.send(()).unwrap();
        assert!(matches!(
            task.join().unwrap(),
            Err(OrchestrationError::Conflict)
        ));
        // Admission leaves only a safe intent; a stale finalization cannot erase
        // its private fence or turn it into a destination/observation.
        assert_safe_intent(home.path());
    }
}

struct HoldingObserver {
    inner: Arc<Observer>,
    home: PathBuf,
    committed: mpsc::Sender<()>,
    release: Mutex<mpsc::Receiver<()>>,
}
impl TargetObserver for HoldingObserver {
    type Proof = u64;
    fn observe(
        &self,
        target: &TargetRef,
        destination: &TargetDestination,
    ) -> Result<ObservedTarget<u64>, OrchestrationError> {
        self.inner.observe(target, destination)
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
        let _guard = self.inner.remote_lock.lock().unwrap();
        if *proof != self.inner.generation.load(Ordering::SeqCst) {
            return Err(OrchestrationError::Conflict);
        }
        let result = commit()?;
        // This fails if a future workflow validates the guard then writes outside it.
        assert!(
            BindingRegistry::new(&self.home)
                .get(&reference())
                .unwrap()
                .destination
                .is_some()
        );
        self.committed.send(()).unwrap();
        self.release
            .lock()
            .unwrap()
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        Ok(result)
    }
}

#[test]
fn hs2_4j6v46_remote_guard_linearizes_commit_before_concurrent_revoke() {
    let (_root, home, flow, observer, revision) = setup();
    let (committed_tx, committed_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let held = HoldingObserver {
        inner: observer.clone(),
        home: home.path().into(),
        committed: committed_tx,
        release: Mutex::new(release_rx),
    };
    let task_flow = flow.clone();
    let task = std::thread::spawn(move || {
        let clock = Clock(200);
        let mut rng = Rng(100);
        let mut tx = Transaction {
            clock: &clock,
            rng: &mut rng,
        };
        task_flow.bind(bind_request(&revision, 0, None), &held, &mut tx)
    });
    committed_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    assert!(observer.remote_lock.try_lock().is_err());
    let (trying_tx, trying_rx) = mpsc::channel();
    let (revoked_tx, revoked_rx) = mpsc::channel();
    let peer = observer.clone();
    let revoke = std::thread::spawn(move || {
        trying_tx.send(()).unwrap();
        let _guard = peer.remote_lock.lock().unwrap();
        peer.generation.fetch_add(1, Ordering::SeqCst);
        *peer.result.lock().unwrap() = Err(TargetFailure::Revoked);
        revoked_tx.send(()).unwrap();
    });
    trying_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    assert!(revoked_rx.try_recv().is_err());
    release_tx.send(()).unwrap();
    assert!(task.join().unwrap().unwrap().usable);
    revoked_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    revoke.join().unwrap();
    // A response linearized before revoke is not future authority: a fresh resolve must fail.
    let clock = Clock(300);
    let mut rng = Rng(300);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let fresh = flow
        .resolve(&reference(), observer.as_ref(), &mut tx)
        .unwrap();
    assert!(!fresh.usable);
    assert_eq!(fresh.reason, Some(TargetFailure::Revoked));
}

#[test]
fn hs2_4j6v46_same_identity_registry_review_allows_fresh_retry_without_target_rebind() {
    let (_root, _home, flow, observer, revision) = setup();
    let clock = Clock(200);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let bound = flow
        .bind(bind_request(&revision, 0, None), observer.as_ref(), &mut tx)
        .unwrap();
    let (ready_tx, ready_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let task_flow = flow.clone();
    let delayed = Delayed {
        inner: observer.clone(),
        ready: ready_tx,
        release: Mutex::new(release_rx),
    };
    let task = std::thread::spawn(move || {
        let clock = Clock(200);
        let mut rng = Rng(500);
        let mut tx = Transaction {
            clock: &clock,
            rng: &mut rng,
        };
        task_flow.resolve(&reference(), &delayed, &mut tx)
    });
    ready_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    {
        let _guard = observer.remote_lock.lock().unwrap();
        observer.generation.fetch_add(1, Ordering::SeqCst);
    }
    release_tx.send(()).unwrap();
    assert!(matches!(
        task.join().unwrap(),
        Err(OrchestrationError::Conflict)
    ));
    let fresh = flow
        .resolve(&reference(), observer.as_ref(), &mut tx)
        .unwrap();
    assert!(fresh.usable);
    assert_eq!(fresh.binding.revision, bound.binding.revision);
    assert_eq!(fresh.binding.destination, bound.binding.destination);
    assert_eq!(fresh.binding.review_history.len(), 1);
}

fn assert_safe_intent(home: &Path) {
    let file = BindingRegistry::new(home).read_file().unwrap();
    assert_eq!(file.bindings.len(), 1);
    let record = &file.bindings[0];
    assert_eq!(record.target, reference());
    assert_eq!(record.revision, 0);
    assert!(record.destination.is_none());
    assert!(record.checked_at_ms.is_none());
    assert!(record.confirmed_at_ms.is_none());
    assert!(record.last_failure.is_none());
    assert!(record.sources.is_empty());
    assert!(record.history.is_empty());
}

#[test]
fn hs2_4j6v46_absent_unbind_fences_initial_probe_without_public_revision() {
    let (_root, home, flow, observer, revision) = setup();
    let (ready, arrived) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let delayed = Delayed {
        inner: observer.clone(),
        ready,
        release: Mutex::new(wait),
    };
    let task_flow = flow.clone();
    let task_revision = revision.clone();
    let task = std::thread::spawn(move || {
        let clock = Clock(200);
        let mut rng = Rng(500);
        task_flow.bind(
            bind_request(&task_revision, 0, None),
            &delayed,
            &mut Transaction {
                clock: &clock,
                rng: &mut rng,
            },
        )
    });
    arrived
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    let clock = Clock(0);
    let mut rng = Rng(1000);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    assert_eq!(
        flow.unbind(unbind_request(0, None), &mut tx)
            .unwrap()
            .revision,
        0
    );
    assert_safe_intent(home.path());
    let after = std::fs::read(home.path().join("orchestration-bindings.json")).unwrap();
    release.send(()).unwrap();
    assert!(matches!(
        task.join().unwrap(),
        Err(OrchestrationError::Conflict)
    ));
    assert_eq!(
        std::fs::read(home.path().join("orchestration-bindings.json")).unwrap(),
        after
    );
    assert!(
        flow.bind(bind_request(&revision, 0, None), observer.as_ref(), &mut tx)
            .unwrap()
            .usable
    );
}

#[test]
fn hs2_4j6v46_latest_started_failed_bind_fences_initial_and_review_successes() {
    for existing in [false, true] {
        for failure_mode in 0..3 {
            let (_root, home, flow, observer, revision) = setup();
            let clock = Clock(200);
            let mut rng = Rng(100);
            let mut tx = Transaction {
                clock: &clock,
                rng: &mut rng,
            };
            if existing {
                flow.bind(bind_request(&revision, 0, None), observer.as_ref(), &mut tx)
                    .unwrap();
            }
            let before = flow.binding(&reference()).unwrap();
            // Candidate B differs from reviewed A. Failure of B must not poison A.
            let mut candidate_projection = projection();
            candidate_projection.checkout_id = "candidate-0123456789ab".into();
            let candidate = TargetDestination::Local {
                server_id: candidate_projection.server_id.clone(),
                checkout_id: candidate_projection.checkout_id.clone(),
                checkout_authority: candidate_projection.checkout_authority.clone(),
            };
            *observer.result.lock().unwrap() = Ok(candidate_projection);
            let mut request = bind_request(
                &revision,
                if existing { 1 } else { 0 },
                existing.then(destination),
            );
            request.destination = candidate;
            let (ready, arrived) = mpsc::channel();
            let (release, wait) = mpsc::channel();
            let delayed = Delayed {
                inner: observer.clone(),
                ready,
                release: Mutex::new(wait),
            };
            let task_flow = flow.clone();
            let old_request = request.clone();
            let task = std::thread::spawn(move || {
                let clock = Clock(200); // Equal time does not grant authority.
                let mut rng = Rng(500);
                task_flow.bind(
                    old_request,
                    &delayed,
                    &mut Transaction {
                        clock: &clock,
                        rng: &mut rng,
                    },
                )
            });
            arrived
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
            *observer.result.lock().unwrap() = if failure_mode == 1 {
                Ok(projection()) // Verified identity does not match candidate B.
            } else {
                Err(TargetFailure::Unauthorized)
            };
            observer
                .adapter_failure
                .store(usize::from(failure_mode == 2), Ordering::SeqCst);
            let backward_clock = Clock(0);
            let mut tx = Transaction {
                clock: &backward_clock,
                rng: &mut rng,
            };
            assert!(matches!(
                flow.bind(request, observer.as_ref(), &mut tx),
                Err(OrchestrationError::Observation(_))
            ));
            assert_eq!(flow.binding(&reference()).unwrap(), before);
            if !existing {
                assert_safe_intent(home.path());
            }
            let after = std::fs::read(home.path().join("orchestration-bindings.json")).unwrap();
            release.send(()).unwrap();
            assert!(matches!(
                task.join().unwrap(),
                Err(OrchestrationError::Conflict)
            ));
            assert_eq!(
                std::fs::read(home.path().join("orchestration-bindings.json")).unwrap(),
                after
            );
            observer.adapter_failure.store(0, Ordering::SeqCst);
            *observer.result.lock().unwrap() = Ok(projection());
            if existing {
                assert!(
                    flow.resolve(&reference(), observer.as_ref(), &mut tx)
                        .unwrap()
                        .usable
                );
            } else {
                assert!(
                    flow.bind(bind_request(&revision, 0, None), observer.as_ref(), &mut tx)
                        .unwrap()
                        .usable
                );
            }
        }
    }
}
