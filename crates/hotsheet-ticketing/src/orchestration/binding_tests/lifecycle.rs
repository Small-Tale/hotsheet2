use super::*;

#[test]
fn hs2_4j6v46_provider_neutral_bind_failure_reconnect_and_cleanup_without_manifest() {
    let (root, home, flow, observer, revision) = setup();
    let clock = Clock(200);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let unbound = flow
        .resolve(&reference(), observer.as_ref(), &mut tx)
        .unwrap();
    assert_eq!(unbound.state, ResolutionState::Unbound);
    assert!(!unbound.usable);
    let bound = flow
        .bind(bind_request(&revision, 0, None), observer.as_ref(), &mut tx)
        .unwrap();
    assert!(bound.usable);
    assert_eq!(bound.binding.revision, 1);
    assert_eq!(bound.sources.unwrap()[0].provider, "synthetic");
    let registry = BindingRegistry::new(home.path());
    assert!(registry.get(&reference()).unwrap().stale);
    *observer.result.lock().unwrap() = Err(TargetFailure::Unauthorized);
    let failed = flow
        .resolve(&reference(), observer.as_ref(), &mut tx)
        .unwrap();
    assert_eq!(failed.state, ResolutionState::Unauthorized);
    assert!(failed.stale);
    assert!(!failed.usable);
    assert!(failed.sources.is_none());
    assert_eq!(failed.binding.last_confirmed_sources.len(), 1);
    *observer.result.lock().unwrap() = Ok(projection());
    assert!(
        flow.resolve(&reference(), observer.as_ref(), &mut tx)
            .unwrap()
            .usable
    );
    // Corrupt/newer shared data cannot trap a machine-local disable.
    std::fs::write(
        root.path().join(".hotsheet2/orchestration.json"),
        br#"{"schema_version":99}"#,
    )
    .unwrap();
    let disabled = registry
        .unbind(unbind_request(1, Some(destination())), &mut tx)
        .unwrap();
    assert!(disabled.destination.is_none());
    assert_eq!(disabled.revision, 2);
    let repeat = registry.unbind(unbind_request(2, None), &mut tx).unwrap();
    assert_eq!(repeat.revision, 2);
    assert_eq!(repeat.review_history.len(), 2);
    let wire = std::fs::read_to_string(home.path().join("orchestration-bindings.json")).unwrap();
    for forbidden in [
        "credential",
        "locator",
        "origin",
        "observation_generation\":null",
        "controller_server_id",
    ] {
        assert!(!wire.contains(forbidden));
    }
    let view = serde_json::to_value(repeat).unwrap();
    assert!(view.get("observation_generation").is_none());
    assert!(
        std::fs::read(root.path().join(".hotsheet2/orchestration.json"))
            .unwrap()
            .starts_with(br#"{"schema_version":99}"#)
    );
}

#[test]
fn hs2_4j6v46_source_free_enable_and_cross_owner_are_rejected_before_write() {
    let root = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let observer = Observer::new(root.path());
    observer.sources.store(0, Ordering::SeqCst);
    let flow = Orchestration::new(
        reference().owner_checkout_id,
        ManifestStore::new(root.path()),
        BindingRegistry::new(home.path()),
    )
    .unwrap();
    let clock = Clock(100);
    let mut rng = Rng(1);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    assert!(!flow.declarations().unwrap().enabled);
    assert!(matches!(
        flow.replace(
            ReplaceTargets {
                expected_revision: None,
                targets: Vec::new()
            },
            &observer,
            &mut tx
        ),
        Err(OrchestrationError::Observation(
            TargetFailure::SourceMissing
        ))
    ));
    assert!(!root.path().join(".hotsheet2").exists());
    assert!(!home.path().join("orchestration-bindings.json").exists());
    let mut other = reference();
    other.owner_checkout_id = "other-owner".into();
    assert!(flow.binding(&other).is_err());
}

#[test]
fn hs2_4j6v46_invalid_or_unavailable_admission_never_writes_binding_data() {
    for mode in 0..7 {
        let (root, home, flow, observer, revision) = setup();
        let path = home.path().join("orchestration-bindings.json");
        let mut request = bind_request(&revision, 0, None);
        match mode {
            0 => {
                if let TargetDestination::Local { checkout_id, .. } = &mut request.destination {
                    checkout_id.clear();
                }
            }
            1 => request.expected_manifest_revision = id(999),
            2 => observer.sources.store(0, Ordering::SeqCst),
            3 => std::fs::write(&path, b"corrupt").unwrap(),
            4 => std::fs::write(&path, br#"{"schema_version":99}"#).unwrap(),
            5 => std::fs::write(
                root.path().join(".hotsheet2/orchestration.json"),
                b"corrupt",
            )
            .unwrap(),
            _ => std::fs::write(
                root.path().join(".hotsheet2/orchestration.json"),
                br#"{"schema_version":99}"#,
            )
            .unwrap(),
        }
        let before = std::fs::read(&path).ok();
        let clock = Clock(0);
        let mut rng = Rng(100);
        assert!(
            flow.bind(
                request,
                observer.as_ref(),
                &mut Transaction {
                    clock: &clock,
                    rng: &mut rng
                }
            )
            .is_err()
        );
        assert_eq!(observer.observations.load(Ordering::SeqCst), 0);
        assert_eq!(std::fs::read(&path).ok(), before);
    }
}

#[test]
fn hs2_4j6v46_bound_empty_or_partial_missing_sources_report_source_missing_before_authority() {
    for empty in [true, false] {
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
        let mut unavailable = projection();
        if empty {
            unavailable.sources.clear();
        } else {
            unavailable.sources.push(TargetSource {
                connection_id: "missing-provider".into(),
                provider: "synthetic".into(),
                identity: None,
                state: SourceState::Missing,
                capabilities: None,
            });
        }
        // Missing config cannot reconstruct the previous complete authority. The
        // stable server/checkout ids still match; absence is not an observed retarget.
        unavailable.checkout_authority = checkout_authority(&[]).unwrap();
        *observer.result.lock().unwrap() = Ok(unavailable);
        let missing = flow
            .resolve(&reference(), observer.as_ref(), &mut tx)
            .unwrap();
        assert_eq!(missing.state, ResolutionState::SourceMissing);
        assert_eq!(missing.reason, Some(TargetFailure::SourceMissing));
        assert!(!missing.usable);
        assert!(missing.stale);
        assert!(missing.sources.is_none());
        assert_eq!(missing.binding.destination, bound.binding.destination);
        assert_eq!(missing.binding.revision, bound.binding.revision);
        assert_eq!(
            missing.binding.last_confirmed_sources,
            bound.binding.last_confirmed_sources
        );
        *observer.result.lock().unwrap() = Ok(projection());
        assert!(
            flow.resolve(&reference(), observer.as_ref(), &mut tx)
                .unwrap()
                .usable
        );
        let mut retargeted = projection();
        retargeted.sources[0].identity = Some("b".repeat(64));
        retargeted.checkout_authority = checkout_authority(&retargeted.sources).unwrap();
        *observer.result.lock().unwrap() = Ok(retargeted);
        let changed = flow
            .resolve(&reference(), observer.as_ref(), &mut tx)
            .unwrap();
        assert_eq!(changed.state, ResolutionState::IdentityChanged);
        assert_eq!(changed.reason, Some(TargetFailure::IdentityChanged));
        assert!(!changed.usable);
    }
}
