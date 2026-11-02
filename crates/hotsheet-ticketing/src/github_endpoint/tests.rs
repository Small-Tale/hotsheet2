use super::state::RESERVED;
use super::*;
use crate::{ProviderConfigRegistry, ProviderConnection};
use hotsheet_model::Ulid;
use serde_json::{Value, json};

struct Links(bool);
impl LinkObserver for Links {
    fn is_linked(&self, _: &str, _: &Link) -> Result<bool, EndpointError> {
        Ok(self.0)
    }
}

fn record(id: &str, endpoint: Endpoint) -> ProviderConnection {
    let mut record = ProviderConnection {
        id: id.into(),
        provider: "github".into(),
        locator: "synthetic/project".into(),
        name: Some("Synthetic source".into()),
        default: false,
        disabled: false,
        settings: json!({"credential":{"secret":"synthetic-account"},"attachments":{"repository":"synthetic/assets"}}),
    };
    endpoint.write(&mut record).unwrap();
    record
}
fn public() -> Endpoint {
    Endpoint::Public
}
fn custom() -> Endpoint {
    Endpoint::Url("https://ghe.test:443/Custom/API/v3/".into())
}
fn observed(site: &str) -> Observation {
    Observation::Available {
        credential_ref: "synthetic-account".into(),
        site: site.into(),
    }
}
fn inferred(endpoint: Endpoint, origin: &str) -> Intent {
    Intent::InferSite {
        credential_ref: "synthetic-account".into(),
        endpoint,
        origin: origin.into(),
    }
}
fn explicit(endpoint: Endpoint) -> Intent {
    Intent::Explicit {
        credential_ref: "synthetic-account".into(),
        endpoint,
    }
}
fn create(id: &str, endpoint: Endpoint, intent: Intent) -> Request {
    Request {
        operation_id: Ulid::new(),
        expected: None,
        connection: record(id, endpoint),
        intent,
        link: None,
    }
}
fn edit(current: &ProviderConnection, endpoint: Endpoint, intent: Intent) -> Request {
    let mut connection = current.clone();
    endpoint.write(&mut connection).unwrap();
    Request {
        operation_id: Ulid::new(),
        expected: Some(Expected::capture(current).unwrap()),
        connection,
        intent,
        link: None,
    }
}
fn registry(dir: &tempfile::TempDir) -> ProviderConfigRegistry {
    ProviderConfigRegistry::new(dir.path().join("providers.json"))
}
fn bytes(registry: &ProviderConfigRegistry) -> Option<Vec<u8>> {
    std::fs::read(registry.path()).ok()
}
fn state(record: &ProviderConnection) -> State {
    State::load(record).unwrap().unwrap()
}
fn current(registry: &ProviderConfigRegistry, id: &str) -> ProviderConnection {
    registry
        .load()
        .unwrap()
        .into_iter()
        .find(|record| record.id == id)
        .unwrap()
}

// HS2-6H8ZY6: arrange old/out-of-band corruption or byte copies in an owned fixture.
// Public save/update must not become an authority-injection bypass for these tests.
fn external_records(registry: &ProviderConfigRegistry, records: &[ProviderConnection]) {
    std::fs::write(
        registry.path(),
        serde_json::to_vec_pretty(&json!({"connections":records})).unwrap(),
    )
    .unwrap();
}

#[test]
fn hs2_6h8zy6_absence_is_legacy_but_present_malformed_or_newer_state_never_is() {
    let mut old = record("legacy", public());
    assert!(State::load(&old).unwrap().is_none());
    assert_eq!(admit_read(&old, &Observation::Unavailable), Ok(()));
    for value in [
        Value::Null,
        json!({}),
        json!({"version":2}),
        json!({"version":1,"decision":null}),
    ] {
        old.settings[RESERVED] = value;
        assert_eq!(State::load(&old), Err(EndpointError::Invalid));
        assert_eq!(
            admit_read(&old, &Observation::Unavailable),
            Err(EndpointError::Invalid)
        );
    }
}

#[test]
fn hs2_6h8zy6_fixed_endpoint_requires_explicit_null_and_retains_ports_path_and_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(&dir);
    let core = EndpointRegistry::new(&registry);
    for endpoint in [
        public(),
        custom(),
        Endpoint::Url("http://github.com:80/API/v%33".into()),
    ] {
        let request = create(
            &Ulid::new().to_string(),
            endpoint.clone(),
            explicit(endpoint.clone()),
        );
        let saved = core
            .apply(&request, &Observation::Unavailable, &Links(false))
            .unwrap()
            .connection;
        assert_eq!(state(&saved).decision.endpoint(), &endpoint);
        assert_eq!(Endpoint::from_record(&saved).unwrap(), endpoint);
        let mut missing = saved.clone();
        missing.settings[RESERVED]["decision"]
            .as_object_mut()
            .unwrap()
            .remove("endpoint");
        assert_eq!(State::load(&missing), Err(EndpointError::Invalid));
        let mut mismatch = saved.clone();
        mismatch.settings["api_base"] = Value::Null;
        assert_eq!(State::load(&mismatch), Err(EndpointError::Invalid));
        let mut foreign = saved.clone();
        foreign.settings[RESERVED]["unexpected"] = true.into();
        assert_eq!(State::load(&foreign), Err(EndpointError::Invalid));
    }
}

#[test]
fn hs2_6h8zy6_inferred_is_fixed_and_metadata_absence_is_not_public_match() {
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(&dir);
    let core = EndpointRegistry::new(&registry);
    let request = create(
        "inferred",
        public(),
        inferred(public(), "https://github.com"),
    );
    let saved = core
        .apply(&request, &observed("HTTPS://GITHUB.COM/"), &Links(false))
        .unwrap()
        .connection;
    assert_eq!(admit_read(&saved, &observed("https://github.com")), Ok(()));
    assert_eq!(
        admit_read(&saved, &observed("https://github.com:443")),
        Err(EndpointError::Drift)
    );
    assert_eq!(
        admit_read(&saved, &Observation::Unavailable),
        Err(EndpointError::Unavailable)
    );
    assert_eq!(
        admit_read(&saved, &observed("malformed")),
        Err(EndpointError::Unavailable)
    );
    let mut other_credential = saved.clone();
    other_credential.settings["credential"]["secret"] = "another".into();
    assert_eq!(
        admit_read(&other_credential, &observed("https://github.com")),
        Err(EndpointError::Invalid)
    );
    assert_eq!(
        validate_bundle_origin(&saved, &BundleOrigin::Known("https://other.test".into())),
        Err(EndpointError::Drift)
    );
    assert_eq!(
        validate_bundle_origin(&saved, &BundleOrigin::Invalid),
        Err(EndpointError::Unavailable)
    );
    assert_eq!(
        validate_bundle_origin(&saved, &BundleOrigin::Missing),
        Ok(())
    );
    assert_eq!(
        Endpoint::from_record(&saved).unwrap(),
        public(),
        "drift never re-derives the fixed endpoint"
    );
}

#[test]
fn hs2_6h8zy6_legacy_public_uppercase_recovery_requires_deliberate_choice() {
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(&dir);
    let core = EndpointRegistry::new(&registry);
    let endpoint = Endpoint::Url("HTTPS://GITHUB.COM/api/v3".into());
    let legacy = record("legacy", endpoint.clone());
    registry.save(std::slice::from_ref(&legacy)).unwrap();
    let before = bytes(&registry);
    let _expected = Expected::capture(&legacy).unwrap();
    assert_eq!(admit_read(&legacy, &observed("https://github.com")), Ok(()));
    assert_eq!(bytes(&registry), before);
    let kept = core
        .apply(
            &edit(&legacy, endpoint.clone(), Intent::Keep),
            &Observation::Unavailable,
            &Links(false),
        )
        .unwrap()
        .connection;
    assert!(matches!(state(&kept).decision, Decision::Explicit { .. }));
    assert_eq!(Endpoint::from_record(&kept).unwrap(), endpoint);
    let repaired = core
        .apply(
            &edit(&kept, public(), inferred(public(), "https://github.com")),
            &observed("https://github.com"),
            &Links(false),
        )
        .unwrap()
        .connection;
    assert!(repaired.settings.get("api_base").is_none());
    assert_eq!(
        repaired.settings["attachments"],
        legacy.settings["attachments"]
    );
    assert_eq!(
        repaired.settings["credential"],
        legacy.settings["credential"]
    );
}

#[test]
fn hs2_6h8zy6_same_looking_unknown_and_explicit_never_infer_provenance_from_url() {
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(&dir);
    let core = EndpointRegistry::new(&registry);
    let unknown = core
        .apply(
            &create("unknown", public(), Intent::Legacy),
            &Observation::Unavailable,
            &Links(false),
        )
        .unwrap()
        .connection;
    let known = core
        .apply(
            &create("explicit", public(), explicit(public())),
            &Observation::Unavailable,
            &Links(false),
        )
        .unwrap()
        .connection;
    assert!(matches!(
        state(&unknown).decision,
        Decision::LegacyUnknown { .. }
    ));
    assert!(matches!(state(&known).decision, Decision::Explicit { .. }));
    assert_ne!(state(&known).incarnation, state(&unknown).incarnation);
}

#[test]
fn hs2_6h8zy6_known_authority_cannot_downgrade_through_omitted_or_forged_settings() {
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(&dir);
    let core = EndpointRegistry::new(&registry);
    for intent in [explicit(public()), inferred(public(), "https://github.com")] {
        let saved = core
            .apply(
                &create(&Ulid::new().to_string(), public(), intent),
                &observed("https://github.com"),
                &Links(false),
            )
            .unwrap()
            .connection;
        let before = bytes(&registry);
        let mut changed = saved.clone();
        changed.settings.as_object_mut().unwrap().remove(RESERVED);
        changed.settings["credential"]["secret"] = "other".into();
        assert_eq!(
            core.ordinary(&Expected::capture(&saved).unwrap(), &changed),
            Err(EndpointError::Conflict)
        );
        changed = saved.clone();
        changed.settings.as_object_mut().unwrap().remove(RESERVED);
        custom().write(&mut changed).unwrap();
        assert_eq!(
            core.ordinary(&Expected::capture(&saved).unwrap(), &changed),
            Err(EndpointError::Conflict)
        );
        changed = saved.clone();
        changed.settings[RESERVED]["decision"]["kind"] = "legacy_unknown".into();
        assert_eq!(
            core.ordinary(&Expected::capture(&saved).unwrap(), &changed),
            Err(EndpointError::Invalid)
        );
        assert!(matches!(
            core.apply(
                &edit(&saved, public(), Intent::Legacy),
                &Observation::Unavailable,
                &Links(false)
            ),
            Err(EndpointError::Conflict)
        ));
        assert_eq!(bytes(&registry), before);
        let mut renamed = saved.clone();
        renamed.name = Some("New label".into());
        renamed.disabled = true;
        renamed.settings.as_object_mut().unwrap().remove(RESERVED);
        let renamed = core
            .ordinary(&Expected::capture(&saved).unwrap(), &renamed)
            .unwrap();
        assert_eq!(state(&renamed).decision, state(&saved).decision);
        assert_eq!(state(&renamed).incarnation, state(&saved).incarnation);
        assert_eq!(state(&renamed).revision, state(&saved).revision);
        assert!(state(&renamed).operation.is_none());
    }
}

#[test]
fn hs2_6h8zy6_unmarked_legacy_nonmaterial_invalid_endpoint_is_not_silently_migrated() {
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(&dir);
    let core = EndpointRegistry::new(&registry);
    let mut old = record("legacy", public());
    old.settings["api_base"] = Value::Null;
    registry.save(std::slice::from_ref(&old)).unwrap();
    let mut incoming = old.clone();
    incoming.name = Some("Only a label".into());
    let saved = core
        .ordinary(&Expected::capture(&old).unwrap(), &incoming)
        .unwrap();
    assert!(State::load(&saved).unwrap().is_none());
    assert_eq!(saved.settings["api_base"], Value::Null);
}

#[test]
fn hs2_6h8zy6_ordinary_unknown_material_write_gets_revision_but_not_explicit_authority() {
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(&dir);
    let core = EndpointRegistry::new(&registry);
    let old = record("legacy", public());
    registry.save(std::slice::from_ref(&old)).unwrap();
    let mut changed = old.clone();
    custom().write(&mut changed).unwrap();
    let saved = core
        .ordinary(&Expected::capture(&old).unwrap(), &changed)
        .unwrap();
    assert!(matches!(
        state(&saved).decision,
        Decision::LegacyUnknown { .. }
    ));
    assert_ne!(
        Expected::capture(&saved).unwrap(),
        Expected::capture(&old).unwrap()
    );
}

#[test]
fn hs2_6h8zy6_replay_binds_exact_request_result_and_retained_operation_only() {
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(&dir);
    let core = EndpointRegistry::new(&registry);
    let request = create("source", custom(), explicit(custom()));
    let first = core
        .apply(&request, &Observation::Unavailable, &Links(false))
        .unwrap();
    let before = bytes(&registry);
    let replay = core
        .apply(&request, &Observation::Unavailable, &Links(false))
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.connection, first.connection);
    assert_eq!(bytes(&registry), before);
    let mut other_payload = request.clone();
    other_payload.connection.name = Some("Different payload".into());
    assert!(matches!(
        core.apply(&other_payload, &Observation::Unavailable, &Links(false)),
        Err(EndpointError::Conflict)
    ));
    let newer = core
        .apply(
            &edit(&first.connection, public(), explicit(public())),
            &Observation::Unavailable,
            &Links(false),
        )
        .unwrap();
    assert!(matches!(
        core.apply(&request, &Observation::Unavailable, &Links(false)),
        Err(EndpointError::Conflict)
    ));
    assert_eq!(current(&registry, "source"), newer.connection);
}

#[test]
fn hs2_6h8zy6_stale_review_cannot_return_after_endpoint_aba_or_delete_recreate() {
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(&dir);
    let core = EndpointRegistry::new(&registry);
    let first = core
        .apply(
            &create("source", public(), Intent::Legacy),
            &Observation::Unavailable,
            &Links(false),
        )
        .unwrap()
        .connection;
    let stale = edit(&first, public(), Intent::Keep);
    let second = core
        .apply(
            &edit(&first, custom(), explicit(custom())),
            &Observation::Unavailable,
            &Links(false),
        )
        .unwrap()
        .connection;
    let third = core
        .apply(
            &edit(&second, public(), explicit(public())),
            &Observation::Unavailable,
            &Links(false),
        )
        .unwrap()
        .connection;
    assert_ne!(state(&first).revision, state(&third).revision);
    assert!(matches!(
        core.apply(&stale, &Observation::Unavailable, &Links(false)),
        Err(EndpointError::Conflict)
    ));
    registry
        .update(|records| {
            records.clear();
            Ok(())
        })
        .unwrap(); // Actual delete adapter adoption remains pending.
    let recreated = core
        .apply(
            &create("source", public(), Intent::Legacy),
            &Observation::Unavailable,
            &Links(false),
        )
        .unwrap()
        .connection;
    assert_ne!(state(&first).incarnation, state(&recreated).incarnation);
    assert!(matches!(
        core.apply(&stale, &Observation::Unavailable, &Links(false)),
        Err(EndpointError::Conflict)
    ));
}

#[test]
fn hs2_6h8zy6_bad_inference_fails_before_provider_file_creation_and_secret_ports_do_not_exist() {
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(&dir);
    let core = EndpointRegistry::new(&registry);
    for observation in [
        Observation::Unavailable,
        observed("https://other.test"),
        observed("invalid"),
    ] {
        assert!(
            core.apply(
                &create("source", public(), inferred(public(), "https://github.com")),
                &observation,
                &Links(false)
            )
            .is_err()
        );
        assert!(bytes(&registry).is_none());
    }
    let mismatch = create("source", custom(), inferred(custom(), "https://github.com"));
    assert!(matches!(
        core.apply(&mismatch, &observed("https://github.com"), &Links(false)),
        Err(EndpointError::Invalid)
    ));
    assert!(bytes(&registry).is_none());
}

#[test]
fn hs2_6h8zy6_owner_derived_copy_preserves_known_or_genuine_unknown_without_laundering() {
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(&dir);
    let core = EndpointRegistry::new(&registry);
    for (id, intent, unknown) in [
        ("legacy", Intent::Legacy, true),
        ("explicit", explicit(public()), false),
        ("inferred", inferred(public(), "https://github.com"), false),
    ] {
        let saved = core
            .apply(
                &create(id, public(), intent),
                &observed("https://github.com"),
                &Links(false),
            )
            .unwrap()
            .connection;
        let proof = core
            .capture_source(
                id,
                "synthetic-account",
                &public(),
                &observed("https://github.com"),
            )
            .unwrap();
        let request = create(
            &format!("copy-{id}"),
            public(),
            Intent::Copy {
                proof: proof.clone(),
                unknown,
            },
        );
        let copied = core
            .apply(&request, &observed("https://github.com"), &Links(false))
            .unwrap()
            .connection;
        assert_eq!(state(&copied).decision, state(&saved).decision);
        assert_ne!(state(&copied).incarnation, state(&saved).incarnation);
        let wrong_kind = create(
            &format!("wrong-{id}"),
            public(),
            Intent::Copy {
                proof,
                unknown: !unknown,
            },
        );
        assert!(matches!(
            core.apply(&wrong_kind, &observed("https://github.com"), &Links(false)),
            Err(EndpointError::Conflict)
        ));
    }
    assert!(
        core.capture_source(
            "unused-account-has-no-source",
            "synthetic-account",
            &public(),
            &Observation::Unavailable
        )
        .is_err()
    );
}

#[test]
fn hs2_6h8zy6_copy_refuses_stale_summary_known_drift_membership_and_unsupported_owner() {
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(&dir);
    let core = EndpointRegistry::new(&registry);
    let saved = core
        .apply(
            &create("source", public(), inferred(public(), "https://github.com")),
            &observed("https://github.com"),
            &Links(false),
        )
        .unwrap()
        .connection;
    assert!(
        core.capture_source(
            "source",
            "other",
            &public(),
            &observed("https://github.com")
        )
        .is_err()
    );
    assert!(
        core.capture_source(
            "source",
            "synthetic-account",
            &public(),
            &Observation::Unavailable
        )
        .is_err()
    );
    let proof = core
        .capture_source(
            "source",
            "synthetic-account",
            &public(),
            &observed("https://github.com"),
        )
        .unwrap();
    let request = create(
        "copy",
        public(),
        Intent::Copy {
            proof: proof.clone(),
            unknown: false,
        },
    );
    let before = bytes(&registry);
    assert!(matches!(
        core.apply(&request, &observed("https://other.test"), &Links(false)),
        Err(EndpointError::Drift)
    ));
    assert_eq!(bytes(&registry), before);
    let other_dir = tempfile::tempdir().unwrap();
    let other = ProviderConfigRegistry::new(other_dir.path().join("providers.json"));
    assert!(matches!(
        EndpointRegistry::new(&other).apply(
            &request,
            &observed("https://github.com"),
            &Links(false)
        ),
        Err(EndpointError::UnsupportedOwner)
    ));
    core.apply(
        &edit(&saved, public(), explicit(public())),
        &Observation::Unavailable,
        &Links(false),
    )
    .unwrap();
    assert!(matches!(
        core.apply(&request, &observed("https://github.com"), &Links(false)),
        Err(EndpointError::Conflict)
    ));
    assert!(
        !registry
            .load()
            .unwrap()
            .iter()
            .any(|record| record.id == "copy")
    );
}

#[test]
fn hs2_6h8zy6_pending_all_kinds_refuse_reads_copy_new_operation_and_every_ordinary_edit() {
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(&dir);
    let core = EndpointRegistry::new(&registry);
    for (id, intent) in [
        ("legacy", Intent::Legacy),
        ("explicit", explicit(public())),
        ("inferred", inferred(public(), "https://github.com")),
    ] {
        let mut request = create(id, public(), intent);
        request.link = Some(Link {
            checkout_id: "synthetic-checkout".into(),
            make_default: false,
        });
        let saved = core
            .apply(&request, &observed("https://github.com"), &Links(false))
            .unwrap()
            .connection;
        let before = bytes(&registry);
        assert_eq!(
            admit_read(&saved, &Observation::Unavailable),
            Err(EndpointError::Pending)
        );
        assert_eq!(
            validate_bundle_origin(&saved, &BundleOrigin::Missing),
            Err(EndpointError::Pending)
        );
        assert!(matches!(
            core.capture_source(
                id,
                "synthetic-account",
                &public(),
                &observed("https://github.com")
            ),
            Err(EndpointError::Pending)
        ));
        assert!(matches!(
            core.apply(
                &edit(&saved, public(), explicit(public())),
                &observed("https://github.com"),
                &Links(true)
            ),
            Err(EndpointError::Pending)
        ));
        for field in [
            "unchanged",
            "name",
            "disabled",
            "default",
            "webhook",
            "attachments",
        ] {
            let mut changed = saved.clone();
            match field {
                "name" => changed.name = Some("rename".into()),
                "disabled" => changed.disabled = true,
                "default" => changed.default = true,
                "webhook" => changed.settings["webhook"] = json!({"secret":"synthetic-ref"}),
                "attachments" => changed.settings["attachments"] = json!({}),
                _ => {}
            }
            assert_eq!(
                core.ordinary(&Expected::capture(&saved).unwrap(), &changed),
                Err(EndpointError::Pending)
            );
        }
        assert_eq!(bytes(&registry), before);
    }
}

#[test]
fn hs2_6h8zy6_matching_recovery_finalizes_only_after_observed_link_and_replays_actual_result() {
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(&dir);
    let core = EndpointRegistry::new(&registry);
    let mut request = create("source", custom(), explicit(custom()));
    request.link = Some(Link {
        checkout_id: "synthetic-checkout".into(),
        make_default: true,
    });
    let mutation = core
        .apply(&request, &Observation::Unavailable, &Links(false))
        .unwrap();
    assert!(!mutation.is_complete());
    let proof = mutation.recovery.unwrap();
    let before = bytes(&registry);
    assert!(matches!(
        core.apply(&request, &Observation::Unavailable, &Links(true)),
        Err(EndpointError::Pending)
    ));
    assert_eq!(
        core.finalize(&proof, &Links(false)),
        Err(EndpointError::Pending)
    );
    assert_eq!(bytes(&registry), before);
    let operation = state(&mutation.connection).operation.unwrap();
    assert!(
        core.pending("source", operation.id, &"0".repeat(64))
            .is_err()
    );
    let resumed = core
        .pending("source", operation.id, &operation.request_sha256)
        .unwrap();
    let saved = core.finalize(&resumed, &Links(true)).unwrap();
    assert_eq!(
        state(&saved).operation.unwrap().state,
        OperationState::Completed
    );
    assert_eq!(core.finalize(&resumed, &Links(true)).unwrap(), saved);
    let replay = core
        .apply(&request, &Observation::Unavailable, &Links(true))
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.connection, saved);
    assert!(matches!(
        core.apply(&request, &Observation::Unavailable, &Links(false)),
        Err(EndpointError::Conflict)
    ));
}

#[test]
fn hs2_6h8zy6_compensation_restores_semantics_with_fresh_revision_or_removes_exact_new_candidate() {
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(&dir);
    let core = EndpointRegistry::new(&registry);
    let initial = core
        .apply(
            &create("source", public(), explicit(public())),
            &Observation::Unavailable,
            &Links(false),
        )
        .unwrap()
        .connection;
    let mut request = edit(&initial, custom(), explicit(custom()));
    request.link = Some(Link {
        checkout_id: "synthetic-checkout".into(),
        make_default: false,
    });
    let pending = core
        .apply(&request, &Observation::Unavailable, &Links(false))
        .unwrap();
    let proof = pending.recovery.unwrap();
    let Compensation::Restored(restored) = core.compensate(&proof).unwrap() else {
        panic!("expected semantic restore");
    };
    assert_eq!(Endpoint::from_record(&restored).unwrap(), public());
    assert_eq!(state(&restored).incarnation, state(&initial).incarnation);
    assert_ne!(state(&restored).revision, state(&initial).revision);
    assert_ne!(
        state(&restored).revision,
        state(&pending.connection).revision
    );
    assert!(state(&restored).operation.is_none());
    assert!(matches!(
        core.apply(&request, &Observation::Unavailable, &Links(true)),
        Err(EndpointError::Conflict)
    ));
    assert!(matches!(
        core.compensate(&proof).unwrap(),
        Compensation::Retained(Some(_))
    ));
    let mut new = create("new", public(), Intent::Legacy);
    new.link = request.link;
    let pending = core
        .apply(&new, &Observation::Unavailable, &Links(false))
        .unwrap();
    assert!(matches!(
        core.compensate(&pending.recovery.unwrap()).unwrap(),
        Compensation::Removed
    ));
    assert!(
        !registry
            .load()
            .unwrap()
            .iter()
            .any(|record| record.id == "new")
    );
}

#[test]
fn hs2_6h8zy6_interleaved_out_of_band_writer_is_retained_on_conditional_compensation() {
    use std::sync::{Arc, Barrier};
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(&dir);
    let core = EndpointRegistry::new(&registry);
    let mut request = create("source", public(), Intent::Legacy);
    request.link = Some(Link {
        checkout_id: "synthetic-checkout".into(),
        make_default: false,
    });
    let pending = core
        .apply(&request, &Observation::Unavailable, &Links(false))
        .unwrap();
    let proof = pending.recovery.unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let path = registry.path().to_path_buf();
    let worker_barrier = barrier.clone();
    let worker = std::thread::spawn(move || {
        worker_barrier.wait();
        // Explicit old/out-of-band control, not an ordinary protected writer.
        let registry = ProviderConfigRegistry::new(path);
        let mut records = registry.load().unwrap();
        {
            let record = &mut records[0];
            let mut state = state(record);
            record.name = Some("Intervening writer".into());
            custom().write(record).unwrap();
            state.revision = Ulid::new();
            state.decision = Decision::Explicit {
                credential_ref: "synthetic-account".into(),
                endpoint: custom(),
            };
            state.operation = None;
            state.write(record).unwrap();
        }
        external_records(&registry, &records);
    });
    // Join reports a worker panic; there is no second barrier to strand this thread.
    barrier.wait();
    worker.join().unwrap();
    let before = bytes(&registry);
    let Compensation::Retained(Some(retained)) = core.compensate(&proof).unwrap() else {
        panic!("must retain later writer");
    };
    assert_eq!(retained.name.as_deref(), Some("Intervening writer"));
    assert_eq!(Endpoint::from_record(&retained).unwrap(), custom());
    assert!(matches!(
        state(&retained).decision,
        Decision::Explicit { .. }
    ));
    assert_eq!(bytes(&registry), before);
    assert!(core.finalize(&proof, &Links(true)).is_err());
}

#[test]
fn hs2_6h8zy6_pending_before_image_cannot_be_stripped_or_promoted_as_ready() {
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(&dir);
    let core = EndpointRegistry::new(&registry);
    let mut request = create("source", public(), Intent::Legacy);
    request.link = Some(Link {
        checkout_id: "synthetic-checkout".into(),
        make_default: false,
    });
    let pending = core
        .apply(&request, &Observation::Unavailable, &Links(false))
        .unwrap();
    let mut forged = pending.connection.clone();
    forged.settings[RESERVED]["operation"]["before"] =
        serde_json::to_value(&pending.connection).unwrap();
    assert_eq!(State::load(&forged), Err(EndpointError::Invalid));
    let mut ordinary = pending.connection.clone();
    ordinary.settings.as_object_mut().unwrap().remove(RESERVED);
    assert_eq!(
        guard_ordinary(&pending.connection, &mut ordinary),
        Err(EndpointError::Pending)
    );
}

#[test]
fn hs2_6h8zy6_recovery_cannot_cross_owner_even_with_identical_record_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(&dir);
    let core = EndpointRegistry::new(&registry);
    let mut request = create("source", public(), Intent::Legacy);
    request.link = Some(Link {
        checkout_id: "synthetic-checkout".into(),
        make_default: false,
    });
    let pending = core
        .apply(&request, &Observation::Unavailable, &Links(false))
        .unwrap();
    let proof = pending.recovery.unwrap();
    let other_dir = tempfile::tempdir().unwrap();
    let other = ProviderConfigRegistry::new(other_dir.path().join("providers.json"));
    external_records(&other, &[pending.connection]);
    let other = EndpointRegistry::new(&other);
    assert_eq!(
        other.finalize(&proof, &Links(true)),
        Err(EndpointError::UnsupportedOwner)
    );
    assert!(matches!(
        other.compensate(&proof),
        Err(EndpointError::UnsupportedOwner)
    ));
}

#[test]
fn hs2_6h8zy6_errors_do_not_display_input_credentials_paths_or_provider_parse_chains() {
    for error in [
        EndpointError::Invalid,
        EndpointError::Conflict,
        EndpointError::Pending,
        EndpointError::Drift,
        EndpointError::Unavailable,
        EndpointError::UnsupportedOwner,
        EndpointError::Storage,
    ] {
        let text = error.to_string();
        assert!(!text.contains("synthetic-secret"));
        assert!(!text.contains("providers.json"));
    }
    for raw in [
        "https://user:synthetic-secret@ghe.test/api",
        "https://ghe.test/path?token=synthetic-secret",
        "https://ghe.test/\napi",
    ] {
        assert_eq!(
            Endpoint::Url(raw.into()).validate(),
            Err(EndpointError::Invalid)
        );
    }
}

#[test]
fn hs2_6h8zy6_absent_source_proof_cannot_survive_promotion_or_fall_back_after_drift() {
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(&dir);
    let core = EndpointRegistry::new(&registry);
    let old = record("source", public());
    registry.save(std::slice::from_ref(&old)).unwrap();
    let proof = core
        .capture_source(
            "source",
            "synthetic-account",
            &public(),
            &Observation::Unavailable,
        )
        .unwrap();
    let request = create(
        "destination",
        public(),
        Intent::Copy {
            proof,
            unknown: true,
        },
    );
    let known = core
        .apply(
            &edit(&old, public(), inferred(public(), "https://github.com")),
            &observed("https://github.com"),
            &Links(false),
        )
        .unwrap()
        .connection;
    let before = bytes(&registry);
    assert!(matches!(
        core.apply(&request, &observed("https://github.com"), &Links(false)),
        Err(EndpointError::Conflict)
    ));
    assert_eq!(bytes(&registry), before);
    let proof = core
        .capture_source(
            "source",
            "synthetic-account",
            &public(),
            &observed("https://github.com"),
        )
        .unwrap();
    let request = create(
        "destination",
        public(),
        Intent::Copy {
            proof,
            unknown: false,
        },
    );
    assert!(matches!(
        core.apply(&request, &Observation::Unavailable, &Links(false)),
        Err(EndpointError::Unavailable)
    ));
    assert_eq!(current(&registry, "source"), known);
    assert_eq!(bytes(&registry), before);
    assert!(
        !registry
            .load()
            .unwrap()
            .iter()
            .any(|record| record.id == "destination")
    );
}

#[test]
fn hs2_6h8zy6_pending_receipt_missing_expected_link_or_before_image_is_invalid_not_ready() {
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(&dir);
    let core = EndpointRegistry::new(&registry);
    let initial = core
        .apply(
            &create("source", public(), explicit(public())),
            &Observation::Unavailable,
            &Links(false),
        )
        .unwrap()
        .connection;
    let mut request = edit(&initial, custom(), explicit(custom()));
    request.link = Some(Link {
        checkout_id: "synthetic-checkout".into(),
        make_default: false,
    });
    let mutation = core
        .apply(&request, &Observation::Unavailable, &Links(false))
        .unwrap();
    let proof = mutation.recovery.unwrap();
    let saved = mutation.connection;
    for field in [
        "expected",
        "link",
        "before",
        "before_sha256",
        "state",
        "request_sha256",
    ] {
        let mut broken = saved.clone();
        broken.settings[RESERVED]["operation"]
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert_eq!(
            State::load(&broken),
            Err(EndpointError::Invalid),
            "missing {field}"
        );
    }
    let mut new_record_lie = saved.clone();
    new_record_lie.settings[RESERVED]["operation"]["expected"] = Value::Null;
    new_record_lie.settings[RESERVED]["operation"]
        .as_object_mut()
        .unwrap()
        .remove("before");
    new_record_lie.settings[RESERVED]["operation"]
        .as_object_mut()
        .unwrap()
        .remove("before_sha256");
    assert_eq!(State::load(&new_record_lie), Err(EndpointError::Invalid));
    external_records(&registry, &[new_record_lie]);
    assert!(core.finalize(&proof, &Links(true)).is_err());
}

#[test]
fn hs2_6h8zy6_invalid_legacy_before_image_refuses_pending_write_but_deliberate_direct_repair_works()
{
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(&dir);
    let core = EndpointRegistry::new(&registry);
    let mut old = record("source", public());
    old.settings["api_base"] = Value::Null;
    registry.save(std::slice::from_ref(&old)).unwrap();
    let mut request = edit(&old, public(), explicit(public()));
    request.link = Some(Link {
        checkout_id: "synthetic-checkout".into(),
        make_default: false,
    });
    let before = bytes(&registry);
    assert!(matches!(
        core.apply(&request, &Observation::Unavailable, &Links(false)),
        Err(EndpointError::Invalid)
    ));
    assert_eq!(bytes(&registry), before);
    request.link = None;
    let saved = core
        .apply(&request, &Observation::Unavailable, &Links(false))
        .unwrap()
        .connection;
    assert!(matches!(state(&saved).decision, Decision::Explicit { .. }));
    assert!(saved.settings.get("api_base").is_none());
}

#[test]
fn hs2_6h8zy6_request_binding_is_stable_for_object_order_but_not_changed_intent() {
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(&dir);
    let core = EndpointRegistry::new(&registry);
    let request = create("source", custom(), explicit(custom()));
    let saved = core
        .apply(&request, &Observation::Unavailable, &Links(false))
        .unwrap()
        .connection;
    let mut reordered = request.clone();
    let entries = reordered
        .connection
        .settings
        .as_object()
        .unwrap()
        .iter()
        .rev()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    reordered.connection.settings = Value::Object(entries);
    assert!(
        core.apply(&reordered, &Observation::Unavailable, &Links(false))
            .unwrap()
            .replayed
    );
    let mut intent_changed = request.clone();
    intent_changed.intent = Intent::Legacy;
    assert!(matches!(
        core.apply(&intent_changed, &Observation::Unavailable, &Links(false)),
        Err(EndpointError::Conflict)
    ));
    assert_eq!(current(&registry, "source"), saved);
}

#[test]
fn hs2_6h8zy6_link_observation_errors_never_complete_or_replay_as_empty_success() {
    struct Unavailable;
    impl LinkObserver for Unavailable {
        fn is_linked(&self, _: &str, _: &Link) -> Result<bool, EndpointError> {
            Err(EndpointError::Unavailable)
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(&dir);
    let core = EndpointRegistry::new(&registry);
    let mut request = create("source", public(), Intent::Legacy);
    request.link = Some(Link {
        checkout_id: "synthetic-checkout".into(),
        make_default: false,
    });
    let mutation = core
        .apply(&request, &Observation::Unavailable, &Links(false))
        .unwrap();
    let proof = mutation.recovery.unwrap();
    let before = bytes(&registry);
    assert_eq!(
        core.finalize(&proof, &Unavailable),
        Err(EndpointError::Unavailable)
    );
    assert_eq!(bytes(&registry), before);
    core.finalize(&proof, &Links(true)).unwrap();
    let before = bytes(&registry);
    assert!(matches!(
        core.apply(&request, &Observation::Unavailable, &Unavailable),
        Err(EndpointError::Unavailable)
    ));
    assert_eq!(bytes(&registry), before);
}

#[test]
fn hs2_6h8zy6_corrupt_provider_file_is_content_free_refusal_without_rewrite() {
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(&dir);
    let core = EndpointRegistry::new(&registry);
    std::fs::write(registry.path(), "not JSON: synthetic-secret").unwrap();
    let before = bytes(&registry);
    assert!(matches!(
        core.apply(
            &create("source", public(), Intent::Legacy),
            &Observation::Unavailable,
            &Links(false)
        ),
        Err(EndpointError::Storage)
    ));
    assert!(matches!(
        core.capture_source(
            "source",
            "synthetic-account",
            &public(),
            &Observation::Unavailable
        ),
        Err(EndpointError::Storage)
    ));
    assert_eq!(bytes(&registry), before);
    assert_eq!(
        EndpointError::Storage.to_string(),
        "Provider configuration cannot be read or published"
    );
}

#[test]
fn hs2_6h8zy6_link_observation_receives_exact_connection_checkout_and_default() {
    struct Exact {
        id: String,
        link: Link,
    }
    impl LinkObserver for Exact {
        fn is_linked(&self, connection_id: &str, link: &Link) -> Result<bool, EndpointError> {
            Ok(connection_id == self.id && *link == self.link)
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(&dir);
    let core = EndpointRegistry::new(&registry);
    let link = Link {
        checkout_id: "synthetic-checkout".into(),
        make_default: true,
    };
    let mut request = create("source", public(), Intent::Legacy);
    request.link = Some(link.clone());
    let pending = core
        .apply(&request, &Observation::Unavailable, &Links(false))
        .unwrap();
    let proof = pending.recovery.unwrap();
    assert_eq!(
        core.finalize(
            &proof,
            &Exact {
                id: "other-source".into(),
                link: link.clone()
            }
        ),
        Err(EndpointError::Pending)
    );
    let different = Link {
        make_default: false,
        ..link.clone()
    };
    assert_eq!(
        core.finalize(
            &proof,
            &Exact {
                id: "source".into(),
                link: different
            }
        ),
        Err(EndpointError::Pending)
    );
    assert!(
        core.finalize(
            &proof,
            &Exact {
                id: "source".into(),
                link
            }
        )
        .is_ok()
    );
}

#[test]
fn hs2_6h8zy6_raw_authority_normalization_forms_refuse_create_and_review_without_data_writes() {
    let invalid = [
        "https://@ghe.test/Custom/API/v3/",
        "https://:@ghe.test/Custom/API/v3/",
        "https:ghe.test/Custom/API/v3/",
        "https:/ghe.test/Custom/API/v3/",
        "https:///ghe.test/Custom/API/v3/",
        "https:////ghe.test/Custom/API/v3/",
        "http:ghe.test/API",
        "http:/ghe.test/API",
        "http:///ghe.test/API",
        "//ghe.test/API",
        "ghe.test/API",
        "https//ghe.test/API",
        "https://",
        "https://ghe.test:/Custom/API/v3/",
        "http://[::1]:/API/v3",
        "https://:443/API",
    ];
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(&dir);
    let core = EndpointRegistry::new(&registry);
    for raw in invalid {
        let endpoint = Endpoint::Url(raw.into());
        assert_eq!(endpoint.validate(), Err(EndpointError::Invalid));
        assert!(matches!(
            core.apply(
                &create("source", endpoint.clone(), explicit(endpoint)),
                &Observation::Unavailable,
                &Links(false)
            ),
            Err(EndpointError::Invalid)
        ));
        assert!(
            bytes(&registry).is_none(),
            "invalid creation must not publish provider data"
        );
    }
    let saved = core
        .apply(
            &create("source", public(), explicit(public())),
            &Observation::Unavailable,
            &Links(false),
        )
        .unwrap()
        .connection;
    let before = bytes(&registry);
    for raw in invalid {
        let endpoint = Endpoint::Url(raw.into());
        let mut request = edit(&saved, public(), explicit(endpoint.clone()));
        request.connection.settings["api_base"] = Value::String(raw.into());
        assert!(matches!(
            core.apply(&request, &Observation::Unavailable, &Links(false)),
            Err(EndpointError::Invalid)
        ));
        assert_eq!(
            bytes(&registry),
            before,
            "invalid review must preserve exact provider bytes"
        );
    }
}

#[test]
fn hs2_6h8zy6_raw_complete_endpoint_controls_preserve_valid_ports_ipv6_path_at_and_bytes() {
    let valid = [
        "HTTPS://GHE.TEST:443/Custom/API/v3/",
        "HTTP://GHE.TEST:80/Case/API/",
        "https://ghe.test:8443/Custom/API/v3",
        "https://github.com:443/api/v3",
        "https://[::1]/Custom/API/v3/",
        "http://[::1]:8443/Custom/API/v3/",
        "https://ghe.test/Path/@segment/API/v3/",
        "https://ghe.test/Path/%40segment/v%33/",
        "https://ghe.test//Custom/API/v3/",
        "https://internal/Custom/API/v3/",
    ];
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(&dir);
    let core = EndpointRegistry::new(&registry);
    for raw in valid {
        let endpoint = Endpoint::Url(raw.into());
        assert_eq!(endpoint.validate(), Ok(()));
        let request = create(
            &Ulid::new().to_string(),
            endpoint.clone(),
            explicit(endpoint.clone()),
        );
        let saved = core
            .apply(&request, &Observation::Unavailable, &Links(false))
            .unwrap()
            .connection;
        assert_eq!(saved.settings["api_base"].as_str(), Some(raw));
        assert_eq!(state(&saved).decision.endpoint(), &endpoint);
    }
}

#[test]
fn hs2_6h8zy6_read_proof_refuses_current_source_changes_and_cannot_upgrade_itself() {
    for mutation in [
        "delete",
        "disabled",
        "provider",
        "credential",
        "endpoint",
        "name",
        "recreated",
        "pending",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let registry = registry(&dir);
        let core = EndpointRegistry::new(&registry);
        let saved = core
            .apply(
                &create("source", public(), explicit(public())),
                &Observation::Unavailable,
                &Links(false),
            )
            .unwrap()
            .connection;
        let proof = core.capture_read(&saved).unwrap();
        assert_eq!(core.check_current(&proof), Ok(()));
        if mutation == "pending" {
            let mut request = edit(&saved, public(), explicit(public()));
            request.link = Some(Link {
                checkout_id: "synthetic-checkout".into(),
                make_default: true,
            });
            core.apply(&request, &Observation::Unavailable, &Links(false))
                .unwrap();
        } else {
            let mut records = registry.load().unwrap();
            // This case deliberately simulates external mutation between proof checks.
            {
                match mutation {
                    "delete" => records.clear(),
                    "disabled" => records[0].disabled = true,
                    "provider" => records[0].provider = "gitlab".into(),
                    "credential" => records[0].settings["credential"]["secret"] = "other".into(),
                    "endpoint" => records[0].settings["api_base"] = "https://other.test/API".into(),
                    "name" => records[0].name = Some("Changed label".into()),
                    "recreated" => {
                        let mut next = state(&records[0]);
                        next.incarnation = Ulid::new();
                        next.revision = Ulid::new();
                        next.operation = None;
                        records[0].settings[RESERVED] = serde_json::to_value(next).unwrap();
                    }
                    _ => unreachable!(),
                }
            }
            external_records(&registry, &records);
        }
        assert!(
            core.check_read(&proof, &Observation::Unavailable).is_err(),
            "{mutation}"
        );
        assert!(
            core.capture_read(&saved).is_err(),
            "old snapshot must not recapture: {mutation}"
        );
    }
}

#[test]
fn hs2_6h8zy6_read_proof_binds_receipt_and_owner_and_metadata_sandwich() {
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(&dir);
    let core = EndpointRegistry::new(&registry);
    let saved = core
        .apply(
            &create("source", public(), inferred(public(), "https://github.com")),
            &observed("https://github.com"),
            &Links(false),
        )
        .unwrap()
        .connection;
    let proof = core.capture_read(&saved).unwrap();
    assert_eq!(
        core.check_read(&proof, &observed("https://github.com")),
        Ok(())
    );
    let other = ProviderConfigRegistry::new(dir.path().join("other.json"));
    external_records(&other, std::slice::from_ref(&saved));
    assert_eq!(
        EndpointRegistry::new(&other).check_read(&proof, &observed("https://github.com")),
        Err(EndpointError::UnsupportedOwner)
    );
    assert_eq!(core.check_current(&proof), Ok(()));
    let mut records = registry.load().unwrap();
    records[0].settings[RESERVED]
        .as_object_mut()
        .unwrap()
        .remove("operation");
    external_records(&registry, &records);
    let changed = current(&registry, "source");
    assert_eq!(
        Expected::capture(&changed).unwrap(),
        Expected::capture(&saved).unwrap(),
        "mutation Expected excludes receipts intentionally"
    );
    assert!(
        core.check_read(&proof, &observed("https://github.com"))
            .is_err(),
        "whole read proof must include receipt changes between observations"
    );
    let fresh = core.capture_read(&changed).unwrap();
    assert_eq!(
        core.check_read(&fresh, &observed("https://github.com")),
        Ok(())
    );
    assert_eq!(
        core.check_read(&fresh, &observed("https://other.test")),
        Err(EndpointError::Drift)
    );
}
