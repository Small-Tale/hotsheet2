//! HS2-4J6V46 shared-format and projection transitions; synthetic roots only.

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
fn id(value: u128) -> String {
    ulid::Ulid::from(value).to_string()
}
fn target(value: u128, label: &str) -> TargetDeclaration {
    TargetDeclaration {
        target_id: id(value),
        state: DeclarationState::Active,
        label: label.into(),
        expected_repository_identity: None,
    }
}

#[test]
fn hs2_4j6v46_absent_read_enable_noop_and_rename_back_keep_revision_authority() {
    let root = tempfile::tempdir().unwrap();
    let store = ManifestStore::new(root.path());
    let clock = Clock(100);
    let mut rng = Rng(1);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let absent = store.read().unwrap();
    assert!(!absent.enabled);
    assert!(absent.revision.is_none());
    assert!(!root.path().join(".hotsheet2").exists());
    let first = store.add(None, target(11, "First"), &mut tx).unwrap();
    let first_bytes = std::fs::read(root.path().join(".hotsheet2/orchestration.json")).unwrap();
    let repeat = store
        .add(first.revision.clone(), target(11, "First"), &mut tx)
        .unwrap();
    assert_eq!(repeat, first);
    assert_eq!(
        std::fs::read(root.path().join(".hotsheet2/orchestration.json")).unwrap(),
        first_bytes
    );
    assert!(matches!(
        store.add(None, target(11, "First"), &mut tx),
        Err(OrchestrationError::Conflict)
    ));
    let changed = store
        .replace(
            ReplaceTargets {
                expected_revision: first.revision.clone(),
                targets: vec![target(11, "Second")],
            },
            &mut tx,
        )
        .unwrap();
    let restored = store
        .replace(
            ReplaceTargets {
                expected_revision: changed.revision,
                targets: vec![target(11, "First")],
            },
            &mut tx,
        )
        .unwrap();
    assert_eq!(restored.targets, first.targets);
    assert_ne!(restored.revision, first.revision);
    assert!(matches!(
        store.replace(
            ReplaceTargets {
                expected_revision: first.revision,
                targets: vec![target(11, "Obsolete")]
            },
            &mut tx
        ),
        Err(OrchestrationError::Conflict)
    ));
    assert_eq!(store.read().unwrap(), restored);
}

#[test]
fn hs2_4j6v46_tombstones_survive_reorder_and_cannot_drop_or_resurrect() {
    let root = tempfile::tempdir().unwrap();
    let store = ManifestStore::new(root.path());
    let clock = Clock(100);
    let mut rng = Rng(1);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let a = store.add(None, target(11, "One"), &mut tx).unwrap();
    let b = store.add(a.revision, target(12, "Two"), &mut tx).unwrap();
    let removed = store.remove(b.revision.unwrap(), &id(11), &mut tx).unwrap();
    let repeated = store
        .remove(removed.revision.clone().unwrap(), &id(11), &mut tx)
        .unwrap();
    assert_eq!(repeated, removed);
    assert!(matches!(
        store.replace(
            ReplaceTargets {
                expected_revision: removed.revision.clone(),
                targets: vec![target(12, "Two")]
            },
            &mut tx
        ),
        Err(OrchestrationError::Invalid(_))
    ));
    assert!(matches!(
        store.replace(
            ReplaceTargets {
                expected_revision: removed.revision.clone(),
                targets: vec![target(11, "One"), target(12, "Two")]
            },
            &mut tx
        ),
        Err(OrchestrationError::Removed)
    ));
    assert!(matches!(
        store.add(removed.revision.clone(), target(11, "Replacement"), &mut tx),
        Err(OrchestrationError::Removed)
    ));
    let mut reordered = removed.targets.clone();
    reordered.reverse();
    let saved = store
        .replace(
            ReplaceTargets {
                expected_revision: removed.revision,
                targets: reordered.clone(),
            },
            &mut tx,
        )
        .unwrap();
    assert_eq!(saved.targets, reordered);
    assert_eq!(saved.targets[1].state, DeclarationState::Removed);
}

#[test]
fn hs2_4j6v46_descriptive_hint_can_change_without_recycling_target() {
    let root = tempfile::tempdir().unwrap();
    let store = ManifestStore::new(root.path());
    let clock = Clock(100);
    let mut rng = Rng(1);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let mut declaration = target(11, "One");
    declaration.expected_repository_identity = Some("https://PEER.invalid/repository".into());
    let first = store.add(None, declaration.clone(), &mut tx).unwrap();
    assert_eq!(
        first.targets[0].expected_repository_identity.as_deref(),
        Some("https://peer.invalid/repository")
    );
    let repeated = store
        .add(first.revision.clone(), declaration, &mut tx)
        .unwrap();
    assert_eq!(first, repeated);
    let mut corrected = first.targets.clone();
    corrected[0].expected_repository_identity = Some("https://peer.invalid/corrected".into());
    let next = store
        .replace(
            ReplaceTargets {
                expected_revision: first.revision,
                targets: corrected,
            },
            &mut tx,
        )
        .unwrap();
    assert_eq!(next.targets[0].target_id, id(11));
    assert!(next.revision.is_some());
}

#[test]
fn hs2_4j6v46_newer_corrupt_and_untrusted_shared_data_are_not_overwritten() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(".hotsheet2")).unwrap();
    let path = root.path().join(".hotsheet2/orchestration.json");
    let store = ManifestStore::new(root.path());
    let clock = Clock(100);
    let mut rng = Rng(1);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    for bytes in [
        b"{\"schema_version\":99,\"future\":true}".as_slice(),
        b"{\"schema_version\":1,\"targets\":[]}".as_slice(),
        b"not json".as_slice(),
    ] {
        std::fs::write(&path, bytes).unwrap();
        assert!(store.read().is_err());
        assert!(store.add(None, target(11, "One"), &mut tx).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
    let mut value = serde_json::to_value(TargetManifest {
        schema_version: 1,
        revision: id(21),
        targets: vec![target(11, "One")],
    })
    .unwrap();
    value["targets"][0]["origin"] = serde_json::json!("https://peer.invalid");
    let bytes = serde_json::to_vec(&value).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    assert!(matches!(store.read(), Err(OrchestrationError::Corrupt)));
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}

#[test]
fn hs2_4j6v46_manifest_rejects_duplicate_ids_capacity_and_credential_hints() {
    let root = tempfile::tempdir().unwrap();
    let store = ManifestStore::new(root.path());
    let clock = Clock(100);
    let mut rng = Rng(1);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    assert!(
        store
            .replace(
                ReplaceTargets {
                    expected_revision: None,
                    targets: vec![target(11, "One"), target(11, "Two")]
                },
                &mut tx
            )
            .is_err()
    );
    let over = (0..=MAX_TARGETS)
        .map(|n| target(n as u128 + 1, "Synthetic"))
        .collect();
    assert!(matches!(
        store.replace(
            ReplaceTargets {
                expected_revision: None,
                targets: over
            },
            &mut tx
        ),
        Err(OrchestrationError::Capacity)
    ));
    for uri in [
        "file:///not-a-repository",
        "git@peer.invalid:repository",
        "https://synthetic:credential@peer.invalid/repository",
        "https://peer.invalid/repository?token=synthetic",
        "https://peer.invalid/repository#fragment",
    ] {
        assert!(normalize_repository_hint(uri).is_err(), "{uri}");
    }
    assert!(!root.path().join(".hotsheet2/orchestration.json").exists());
}

#[test]
fn hs2_4j6v46_projection_is_path_free_checked_and_not_a_grant() {
    let source = TargetSource {
        connection_id: "provider-main".into(),
        provider: "git".into(),
        identity: Some("a".repeat(64)),
        state: SourceState::Available,
        capabilities: Some(crate::ProviderCapabilities::git()),
    };
    let authority = checkout_authority(std::slice::from_ref(&source)).unwrap();
    let mut projection = CheckoutProjection {
        schema_version: 1,
        server_id: id(10),
        checkout_id: "synthetic-0123456789ab".into(),
        checkout_authority: authority.clone(),
        sources: vec![source.clone()],
    };
    let destination = TargetDestination::Local {
        server_id: id(10),
        checkout_id: projection.checkout_id.clone(),
        checkout_authority: authority,
    };
    projection.verified_for(&destination).unwrap();
    let wire = serde_json::to_value(&projection).unwrap();
    for forbidden in ["root", "locator", "origin", "credential_ref", "grant"] {
        assert!(wire.get(forbidden).is_none());
        assert!(wire["sources"][0].get(forbidden).is_none());
    }
    projection.sources[0].connection_id = "renamed-source".into();
    assert_eq!(
        checkout_authority(&projection.sources).unwrap(),
        destination.authority()
    );
    projection.verified_for(&destination).unwrap();
    projection.sources[0].identity = Some("b".repeat(64));
    assert!(projection.validate().is_err());
    projection.sources = vec![source];
    projection.sources[0].capabilities = None;
    assert_eq!(
        projection.verified_for(&destination),
        Err(TargetFailure::InvalidResponse)
    );
    projection.sources[0].state = SourceState::Unsupported;
    assert_eq!(
        projection.verified_for(&destination),
        Err(TargetFailure::UnsupportedMetadata)
    );
}

#[cfg(unix)]
#[test]
fn hs2_4j6v46_shared_symlink_and_special_paths_fail_without_following() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    symlink(elsewhere.path(), root.path().join(".hotsheet2")).unwrap();
    let store = ManifestStore::new(root.path());
    assert!(store.read().is_err());
    let clock = Clock(100);
    let mut rng = Rng(1);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    assert!(store.add(None, target(11, "One"), &mut tx).is_err());
    assert_eq!(std::fs::read_dir(elsewhere.path()).unwrap().count(), 0);
    std::fs::remove_file(root.path().join(".hotsheet2")).unwrap();
    std::fs::create_dir(root.path().join(".hotsheet2")).unwrap();
    std::fs::create_dir(root.path().join(".hotsheet2/orchestration.json")).unwrap();
    assert!(store.read().is_err());
}

#[test]
fn hs2_4j6v46_atomic_publish_failure_leaves_exact_old_or_complete_new_bytes() {
    use super::storage::{PublishStage, Storage};
    let root = tempfile::tempdir().unwrap();
    let store = Storage::shared(root.path());
    let path = root.path().join(".hotsheet2/orchestration.json");
    let old = br#"{"schema_version":1,"synthetic":"old"}"#;
    let new = br#"{"schema_version":1,"synthetic":"new"}"#;
    store.write(old, 4096).unwrap();
    let failed = store.write_with_hook(new, 4096, |stage| {
        if matches!(stage, PublishStage::Staged) {
            Err(std::io::Error::other("synthetic pre-publication fault"))
        } else {
            Ok(())
        }
    });
    assert!(failed.is_err());
    assert_eq!(std::fs::read(&path).unwrap(), old);
    let failed = store.write_with_hook(new, 4096, |stage| {
        if matches!(stage, PublishStage::Published) {
            Err(std::io::Error::other("synthetic post-publication fault"))
        } else {
            Ok(())
        }
    });
    assert!(failed.is_err());
    assert_eq!(std::fs::read(&path).unwrap(), new);
    assert!(serde_json::from_slice::<serde_json::Value>(&std::fs::read(&path).unwrap()).is_ok());
    let before = std::fs::read(&path).unwrap();
    assert!(matches!(
        store.write(&vec![0; 4097], 4096),
        Err(OrchestrationError::Capacity)
    ));
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(
        std::fs::read_dir(root.path().join(".hotsheet2"))
            .unwrap()
            .count(),
        1,
        "temporary publication files must be cleaned up"
    );
}

#[test]
fn hs2_4j6v46_shared_byte_limit_and_removed_metadata_mutation_fail_without_write() {
    use super::contract::MAX_MANIFEST_BYTES;
    let root = tempfile::tempdir().unwrap();
    let store = ManifestStore::new(root.path());
    let clock = Clock(100);
    let mut rng = Rng(1);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let view = store.add(None, target(11, "One"), &mut tx).unwrap();
    let removed = store
        .remove(view.revision.unwrap(), &id(11), &mut tx)
        .unwrap();
    let mut changed = removed.targets.clone();
    changed[0].label = "Edited tombstone".into();
    let path = root.path().join(".hotsheet2/orchestration.json");
    let before = std::fs::read(&path).unwrap();
    assert!(matches!(
        store.replace(
            ReplaceTargets {
                expected_revision: removed.revision,
                targets: changed
            },
            &mut tx
        ),
        Err(OrchestrationError::Removed)
    ));
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let large = vec![b' '; MAX_MANIFEST_BYTES + 1];
    std::fs::write(&path, &large).unwrap();
    assert!(matches!(store.read(), Err(OrchestrationError::Corrupt)));
    assert_eq!(std::fs::metadata(&path).unwrap().len(), large.len() as u64);
    let mut invalid = target(99, "New tombstone");
    invalid.state = DeclarationState::Removed;
    assert!(
        ManifestStore::new(tempfile::tempdir().unwrap().path())
            .add(None, invalid, &mut tx)
            .is_err()
    );
}

#[test]
fn hs2_4j6v46_concurrent_manifest_writers_have_one_cas_winner() {
    let root = tempfile::tempdir().unwrap();
    let store = std::sync::Arc::new(ManifestStore::new(root.path()));
    let clock = Clock(100);
    let mut rng = Rng(1);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let view = store.add(None, target(11, "One"), &mut tx).unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let mut tasks = Vec::new();
    for (label, seed) in [("First editor", 100), ("Second editor", 200)] {
        let store = store.clone();
        let barrier = barrier.clone();
        let revision = view.revision.clone();
        tasks.push(std::thread::spawn(move || {
            let clock = Clock(100);
            let mut rng = Rng(seed);
            let mut tx = Transaction {
                clock: &clock,
                rng: &mut rng,
            };
            barrier.wait();
            store.replace(
                ReplaceTargets {
                    expected_revision: revision,
                    targets: vec![target(11, label)],
                },
                &mut tx,
            )
        }));
    }
    barrier.wait();
    let results = tasks
        .into_iter()
        .map(|task| task.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(OrchestrationError::Conflict)))
            .count(),
        1
    );
    let saved = store.read().unwrap();
    assert_eq!(saved.targets.len(), 1);
    assert_eq!(saved.targets[0].target_id, id(11));
    assert_ne!(saved.revision, view.revision);
}

#[test]
fn hs2_4j6v46_public_error_display_and_codes_omit_internal_details() {
    let error =
        OrchestrationError::Storage(std::io::Error::other("synthetic private backend detail"));
    assert_eq!(error.code(), "storage_unavailable");
    assert_eq!(error.to_string(), "orchestration storage is unavailable");
    assert!(!error.to_string().contains("synthetic"));
    let failure = OrchestrationError::Observation(TargetFailure::UnsupportedMetadata);
    assert_eq!(failure.code(), "unsupported_metadata");
}

#[cfg(unix)]
#[test]
fn hs2_4j6v46_fifo_data_and_lock_paths_are_rejected_without_opening_streams() {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(".hotsheet2")).unwrap();
    let data = root.path().join(".hotsheet2/orchestration.json");
    let fifo = |path: &std::path::Path| {
        let cpath = CString::new(path.as_os_str().as_bytes()).unwrap();
        // SAFETY: NUL-terminated path inside this owned synthetic temporary directory.
        assert_eq!(unsafe { libc::mkfifo(cpath.as_ptr(), 0o600) }, 0);
    };
    fifo(&data);
    let store = ManifestStore::new(root.path());
    assert!(store.read().is_err());
    std::fs::remove_file(&data).unwrap();
    fifo(&root.path().join(".hotsheet2/orchestration.json.lock"));
    let clock = Clock(100);
    let mut rng = Rng(1);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    assert!(store.add(None, target(11, "One"), &mut tx).is_err());
    assert!(!data.exists());
}
