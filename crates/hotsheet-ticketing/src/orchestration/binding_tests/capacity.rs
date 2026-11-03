use super::super::bindings::{BindingFile, MAX_AUDIT, MAX_BINDING_BYTES, MAX_BINDINGS};
use super::*;

#[test]
fn hs2_4j6v46_audit_rotation_capacity_and_disable_need_no_new_binding_slot() {
    let (_root, home, flow, observer, revision) = setup();
    let clock = Clock(200);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    flow.bind(bind_request(&revision, 0, None), observer.as_ref(), &mut tx)
        .unwrap();
    let registry = BindingRegistry::new(home.path());
    let mut file = registry.read_file().unwrap();
    let seed = file.bindings[0].clone();
    for n in 1..MAX_BINDINGS {
        let mut b = seed.clone();
        b.target.target_id = id(n as u128 + 100);
        file.bindings.push(b);
    }
    registry.write_file(&file).unwrap();
    let disabled = registry
        .unbind(unbind_request(1, Some(destination())), &mut tx)
        .unwrap();
    assert!(disabled.destination.is_none());
    assert_eq!(registry.read_file().unwrap().bindings.len(), MAX_BINDINGS);
    let before = std::fs::read(home.path().join("orchestration-bindings.json")).unwrap();
    let mut over = registry.read_file().unwrap();
    let mut b = seed.clone();
    b.target.target_id = id(9999);
    over.bindings.push(b);
    assert!(matches!(
        registry.write_file(&over),
        Err(OrchestrationError::Capacity)
    ));
    assert_eq!(
        std::fs::read(home.path().join("orchestration-bindings.json")).unwrap(),
        before
    );
    // Rotate a single record's bounded identity audit without a clock ordering assumption.
    let mut b = seed;
    for n in 0..MAX_AUDIT + 4 {
        let previous = b.destination.take();
        b.audit(BindingAction::Unbind, previous, 1).unwrap();
        b.destination = Some(destination());
        b.audit(BindingAction::Bind, None, 0).unwrap();
        assert!(b.history.len() <= MAX_AUDIT);
        assert!(b.revision > n as u64);
    }
    let mut only = BindingFile::default();
    only.bindings.push(b);
    registry.write_file(&only).unwrap();
    assert_eq!(
        registry.read_file().unwrap().bindings[0].history.len(),
        MAX_AUDIT
    );
}

#[test]
fn hs2_4j6v46_private_registry_versions_and_byte_limits_preserve_original_bytes() {
    let home = tempfile::tempdir().unwrap();
    let registry = BindingRegistry::new(home.path());
    assert_eq!(registry.get(&reference()).unwrap().revision, 0);
    assert!(!home.path().join("orchestration-bindings.json").exists());
    let path = home.path().join("orchestration-bindings.json");
    for bytes in [
        br#"{"schema_version":99}"#.as_slice(),
        b"invalid".as_slice(),
    ] {
        std::fs::write(&path, bytes).unwrap();
        assert!(registry.get(&reference()).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
    let large = vec![b' '; MAX_BINDING_BYTES + 1];
    std::fs::write(&path, &large).unwrap();
    assert!(matches!(
        registry.read_file(),
        Err(OrchestrationError::Corrupt)
    ));
    assert_eq!(std::fs::metadata(&path).unwrap().len(), large.len() as u64);
}

#[test]
fn hs2_4j6v46_maximum_bounded_records_reserve_disable_and_audit_capacity() {
    let home = tempfile::tempdir().unwrap();
    let registry = BindingRegistry::new(home.path());
    let sources = (0..MAX_TARGET_SOURCES)
        .map(|n| CachedTargetSource {
            connection_id: format!("{}{:03}", "c".repeat(157), n),
            provider: "p".repeat(80),
            identity: "a".repeat(64),
        })
        .collect::<Vec<_>>();
    let authority = super::super::contract::identity_authority(
        sources
            .iter()
            .map(|source| (source.provider.as_str(), source.identity.as_str()))
            .collect(),
    )
    .unwrap();
    let make_destination = |name: char| TargetDestination::Remote {
        registration_id: id(20),
        identity: crate::remote_servers::ReviewedRemoteIdentity {
            server_id: id(21),
            ca_fingerprint: Some("f".repeat(64)),
        },
        checkout_id: name.to_string().repeat(160),
        checkout_authority: authority.clone(),
    };
    let a = make_destination('x');
    let b = make_destination('y');
    let mut history = Vec::new();
    let mut previous = Some(b.clone());
    for n in 0..MAX_AUDIT {
        let destination = Some(if n % 2 == 0 { a.clone() } else { b.clone() });
        history.push(BindingAudit {
            revision: 100 + n as u64,
            action: BindingAction::Review,
            previous_destination: previous,
            destination: destination.clone(),
            at_ms: u64::MAX,
        });
        previous = destination;
    }
    let seed = super::super::bindings::BindingRecord {
        target: TargetRef {
            owner_checkout_id: "o".repeat(160),
            target_id: id(1),
        },
        revision: 100 + MAX_AUDIT as u64 - 1,
        destination: previous,
        observation_generation: id(30),
        checked_at_ms: Some(u64::MAX),
        confirmed_at_ms: Some(u64::MAX),
        last_failure: None,
        sources,
        history,
    };
    let mut file = BindingFile::default();
    for n in 0..MAX_BINDINGS {
        let mut binding = seed.clone();
        binding.target.target_id = id(n as u128 + 1);
        file.bindings.push(binding);
    }
    registry.write_file(&file).unwrap();
    let before = std::fs::read(home.path().join("orchestration-bindings.json")).unwrap();
    assert!(before.len() <= MAX_BINDING_BYTES);
    let clock = Clock(0);
    let mut rng = Rng(1000);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let target = file.bindings[0].target.clone();
    let disabled = registry
        .unbind(
            UnbindTarget {
                target,
                expected_binding_revision: seed.revision,
                previous_destination: seed.destination,
            },
            &mut tx,
        )
        .unwrap();
    assert!(disabled.destination.is_none());
    assert_eq!(disabled.review_history.len(), MAX_AUDIT);
    assert_eq!(registry.read_file().unwrap().bindings.len(), MAX_BINDINGS);
}

#[test]
fn hs2_4j6v46_revision_zero_and_first_audit_corruption_are_not_repaired() {
    let (_root, home, flow, observer, revision) = setup();
    let clock = Clock(200);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    flow.bind(bind_request(&revision, 0, None), observer.as_ref(), &mut tx)
        .unwrap();
    let registry = BindingRegistry::new(home.path());
    let valid = registry.read_file().unwrap();
    let path = home.path().join("orchestration-bindings.json");
    for mode in 0..5 {
        let mut corrupt = valid.clone();
        let record = &mut corrupt.bindings[0];
        match mode {
            0 => {
                record.history[0].action = BindingAction::Review;
                let mut previous = destination();
                if let TargetDestination::Local { checkout_id, .. } = &mut previous {
                    *checkout_id = "previous-0123456789ab".into();
                }
                record.history[0].previous_destination = Some(previous);
            }
            1 => {
                record.history[0].action = BindingAction::Unbind;
                record.history[0].previous_destination = record.destination.take();
                record.history[0].destination = None;
            }
            _ => {
                *record = super::super::bindings::BindingRecord::unbound(reference(), id(900));
                match mode {
                    2 => record.destination = Some(destination()),
                    3 => record.checked_at_ms = Some(0),
                    _ => record.sources = valid.bindings[0].sources.clone(),
                }
            }
        }
        let bytes = serde_json::to_vec(&corrupt).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        assert!(matches!(
            registry.read_file(),
            Err(OrchestrationError::Corrupt)
        ));
        assert!(matches!(
            registry.unbind(unbind_request(1, Some(destination())), &mut tx),
            Err(OrchestrationError::Corrupt)
        ));
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}

#[test]
fn hs2_4j6v46_full_registry_rejects_unknown_probe_before_network_or_write() {
    let (_root, home, flow, observer, revision) = setup();
    let registry = BindingRegistry::new(home.path());
    let mut file = BindingFile::default();
    for n in 0..MAX_BINDINGS {
        let mut target = reference();
        target.target_id = id(n as u128 + 100);
        file.bindings
            .push(super::super::bindings::BindingRecord::unbound(
                target,
                id(n as u128 + 1000),
            ));
    }
    registry.write_file(&file).unwrap();
    let path = home.path().join("orchestration-bindings.json");
    let before = std::fs::read(&path).unwrap();
    let clock = Clock(0);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    assert!(matches!(
        flow.bind(bind_request(&revision, 0, None), observer.as_ref(), &mut tx),
        Err(OrchestrationError::Capacity)
    ));
    assert!(matches!(
        flow.unbind(unbind_request(0, None), &mut tx),
        Err(OrchestrationError::Capacity)
    ));
    assert_eq!(observer.observations.load(Ordering::SeqCst), 0);
    assert_eq!(std::fs::read(&path).unwrap(), before);
    // Existing reservations consume no further slot and remain disableable at capacity.
    let existing = file.bindings[0].target.clone();
    assert_eq!(
        registry
            .unbind(
                UnbindTarget {
                    target: existing,
                    expected_binding_revision: 0,
                    previous_destination: None
                },
                &mut tx
            )
            .unwrap()
            .revision,
        0
    );
    assert_eq!(registry.read_file().unwrap().bindings.len(), MAX_BINDINGS);
}
