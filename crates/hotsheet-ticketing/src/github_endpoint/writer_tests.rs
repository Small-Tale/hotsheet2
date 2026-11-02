//! HS2-6H8ZY6: actual ordinary registry owner boundaries; all data is synthetic.
use super::*;
use crate::{ProviderConfigRegistry, ProviderConnection, ProviderError};
use hotsheet_model::Ulid;
use serde_json::{Value, json};

struct Links;
impl LinkObserver for Links {
    fn is_linked(&self, _: &str, _: &Link) -> Result<bool, EndpointError> {
        Ok(false)
    }
}
fn registry(dir: &tempfile::TempDir) -> ProviderConfigRegistry {
    ProviderConfigRegistry::new(dir.path().join("providers.json"))
}
fn raw(id: &str) -> ProviderConnection {
    ProviderConnection {
        id: id.into(),
        provider: "github".into(),
        locator: "synthetic/project".into(),
        name: None,
        default: false,
        disabled: false,
        settings: json!({"credential":{"secret":"synthetic-account"}}),
    }
}
fn known(
    registry: &ProviderConfigRegistry,
    id: &str,
    pending: bool,
    default: bool,
) -> ProviderConnection {
    let mut connection = raw(id);
    connection.default = default;
    EndpointRegistry::new(registry)
        .apply(
            &Request {
                operation_id: Ulid::new(),
                expected: None,
                connection,
                intent: Intent::Explicit {
                    credential_ref: "synthetic-account".into(),
                    endpoint: Endpoint::Public,
                },
                link: pending.then(|| Link {
                    checkout_id: "synthetic-checkout".into(),
                    make_default: false,
                }),
            },
            &Observation::Unavailable,
            &Links,
        )
        .unwrap()
        .connection
}
fn bytes(registry: &ProviderConfigRegistry) -> Vec<u8> {
    std::fs::read(registry.path()).unwrap()
}
fn endpoint(error: ProviderError, expected: EndpointError) {
    assert!(matches!(error, ProviderError::Endpoint(actual) if actual == expected));
}
fn external(registry: &ProviderConfigRegistry, rows: &[ProviderConnection]) {
    // Simulate an older/external writer only in this owned fixture, never a public bypass.
    std::fs::write(
        registry.path(),
        serde_json::to_vec(&json!({"connections":rows})).unwrap(),
    )
    .unwrap();
}

#[test]
fn hs2_6h8zy6_ordinary_snapshot_checks_owner_whole_receipt_and_returns_normalized_disabled_metadata()
 {
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(&dir);
    let saved = known(&registry, "source", false, false);
    let owner = EndpointRegistry::new(&registry);
    let snapshot = owner.capture_ordinary("source").unwrap();
    let other = tempfile::tempdir().unwrap();
    let other_registry = ProviderConfigRegistry::new(other.path().join("providers.json"));
    endpoint(
        EndpointRegistry::new(&other_registry)
            .check_ordinary(&snapshot)
            .unwrap_err(),
        EndpointError::UnsupportedOwner,
    );
    let mut receipt_changed = saved.clone();
    let mut state = State::load(&receipt_changed).unwrap().unwrap();
    state.operation = None;
    state.write(&mut receipt_changed).unwrap();
    external(&registry, &[receipt_changed]);
    endpoint(
        owner.check_ordinary(&snapshot).unwrap_err(),
        EndpointError::Conflict,
    );
    let fresh = owner.capture_ordinary("source").unwrap();
    let returned = owner
        .update_ordinary(&fresh, |row| {
            row.disabled = true;
            row.name = Some("Metadata edit".into());
            row.settings
                .as_object_mut()
                .unwrap()
                .remove("github_endpoint");
            Ok(())
        })
        .unwrap();
    assert!(returned.disabled);
    assert_eq!(returned, registry.load().unwrap()[0]);
    assert!(State::load(&returned).unwrap().is_some());
    endpoint(
        owner.check_ordinary(&fresh).unwrap_err(),
        EndpointError::Conflict,
    );
    let disabled = owner.capture_ordinary("source").unwrap();
    let result = owner
        .update_ordinary(&disabled, |row| {
            row.settings["webhook"] = json!({"secret":"hook"});
            Ok(())
        })
        .unwrap();
    assert!(result.disabled);
    assert_eq!(result.settings["webhook"]["secret"], "hook");
}

#[test]
fn hs2_6h8zy6_ordinary_capture_pending_and_malformed_refuse_before_callback() {
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(&dir);
    known(&registry, "pending", true, false);
    endpoint(
        EndpointRegistry::new(&registry)
            .capture_ordinary("pending")
            .unwrap_err(),
        EndpointError::Pending,
    );
    let mut broken = raw("broken");
    broken.settings["github_endpoint"] = Value::Null;
    external(&registry, &[broken]);
    endpoint(
        EndpointRegistry::new(&registry)
            .capture_ordinary("broken")
            .unwrap_err(),
        EndpointError::Invalid,
    );
}

#[test]
fn hs2_6h8zy6_public_save_keeps_empty_and_unchanged_atomic_publication() {
    use std::io::Read;
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(&dir);
    registry.save(&[]).unwrap();
    let before = bytes(&registry);
    let mut reader = std::fs::File::open(registry.path()).unwrap();
    #[cfg(unix)]
    let inode = {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        std::fs::set_permissions(registry.path(), std::fs::Permissions::from_mode(0o640)).unwrap();
        reader.metadata().unwrap().ino()
    };
    registry.save(&[]).unwrap();
    assert_eq!(bytes(&registry), before);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let metadata = std::fs::metadata(registry.path()).unwrap();
        assert_ne!(
            metadata.ino(),
            inode,
            "unchanged public save must still publish atomically"
        );
        assert_eq!(metadata.permissions().mode() & 0o777, 0o640);
    }
    registry.save(&[raw("source")]).unwrap();
    let mut old = Vec::new();
    reader.read_to_end(&mut old).unwrap();
    assert_eq!(old, before);
    #[cfg(unix)]
    let unchanged_reader = std::fs::File::open(registry.path()).unwrap();
    registry.update(|_| Ok(())).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let held = unchanged_reader.metadata().unwrap();
        let current = std::fs::metadata(registry.path()).unwrap();
        assert_eq!(
            (current.dev(), current.ino()),
            (held.dev(), held.ino()),
            "HS2-6H8ZY6: no-op public update must not publish a replacement inode"
        );
    }
    assert_eq!(registry.load().unwrap(), vec![raw("source")]);
}

#[test]
fn hs2_6h8zy6_raw_creates_preserve_unmarked_incomplete_and_other_provider_shapes() {
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(&dir);
    let mut rows = Vec::new();
    for (index, settings) in [
        Value::Null,
        json!({}),
        json!({"api_base":null}),
        json!({"api_base":"http:invalid"}),
        raw("unused").settings,
    ]
    .into_iter()
    .enumerate()
    {
        let mut row = raw(&format!("legacy-{index}"));
        row.settings = settings;
        rows.push(row);
    }
    let mut jira = raw("jira");
    jira.provider = "jira".into();
    jira.settings = json!({"github_endpoint":null});
    rows.push(jira);
    registry.save(&rows).unwrap();
    assert_eq!(registry.load().unwrap(), rows);
    assert!(
        rows.iter()
            .filter(|row| row.provider == "github")
            .all(|row| State::load(row).unwrap().is_none())
    );
    registry
        .update(|rows| {
            rows.last_mut().unwrap().settings["github_endpoint"] =
                json!({"opaque":"other-provider"});
            Ok(())
        })
        .unwrap();
    assert_eq!(
        registry.load().unwrap().last().unwrap().settings["github_endpoint"],
        json!({"opaque":"other-provider"})
    );
}

#[test]
fn hs2_6h8zy6_new_reserved_authority_refuses_even_when_null_or_copied() {
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(&dir);
    let saved = known(&registry, "known", false, false);
    let before = bytes(&registry);
    for value in [
        Value::Null,
        json!({}),
        saved.settings["github_endpoint"].clone(),
    ] {
        let mut forged = raw("new");
        forged.settings["github_endpoint"] = value;
        endpoint(
            registry.save(&[saved.clone(), forged]).unwrap_err(),
            EndpointError::Invalid,
        );
        assert_eq!(bytes(&registry), before);
    }
}

#[test]
fn hs2_6h8zy6_known_material_and_forged_updates_never_publish() {
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(&dir);
    let saved = known(&registry, "known", false, false);
    let before = bytes(&registry);
    for mutation in ["endpoint", "credential", "provider", "reserved"] {
        let mut changed = saved.clone();
        match mutation {
            "endpoint" => changed.settings["api_base"] = "https://other.test/API/".into(),
            "credential" => changed.settings["credential"]["secret"] = "other".into(),
            "provider" => changed.provider = "jira".into(),
            "reserved" => changed.settings["github_endpoint"] = Value::Null,
            _ => unreachable!(),
        }
        assert!(matches!(
            registry.save(&[changed]),
            Err(ProviderError::Endpoint(_))
        ));
        assert_eq!(bytes(&registry), before);
    }
}

#[test]
fn hs2_6h8zy6_normalized_target_is_returned_under_owner_and_retires_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(&dir);
    let saved = known(&registry, "known", false, false);
    let before = State::load(&saved).unwrap().unwrap();
    assert!(before.operation.is_some());
    let normalized = registry
        .update_connection("known", |rows| {
            rows[0]
                .settings
                .as_object_mut()
                .unwrap()
                .remove("github_endpoint");
            rows[0].name = Some("Further label".into());
            Ok(())
        })
        .unwrap()
        .unwrap();
    assert_eq!(normalized, registry.load().unwrap()[0]);
    let after = State::load(&normalized).unwrap().unwrap();
    assert_eq!(after.revision, before.revision);
    assert_eq!(after.decision, before.decision);
    assert!(after.operation.is_none());
    let disabled = registry.set_disabled("known", true).unwrap().unwrap();
    assert!(disabled.disabled);
    assert_eq!(disabled, registry.load().unwrap()[0]);
    assert!(
        !registry
            .set_disabled("known", false)
            .unwrap()
            .unwrap()
            .disabled
    );
}

#[test]
fn hs2_6h8zy6_legacy_material_edits_initialize_unknown_then_refresh_revision() {
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(&dir);
    let mut old = raw("legacy");
    old.settings["api_base"] = Value::Null;
    registry.save(&[old]).unwrap();
    registry
        .update(|rows| {
            rows[0].name = Some("Historical label".into());
            Ok(())
        })
        .unwrap();
    assert!(State::load(&registry.load().unwrap()[0]).unwrap().is_none());
    let first = registry
        .update_connection("legacy", |rows| {
            rows[0].settings["api_base"] = "https://ghe.test:443/Exact/API/".into();
            Ok(())
        })
        .unwrap()
        .unwrap();
    let state = State::load(&first).unwrap().unwrap();
    assert!(matches!(state.decision, Decision::LegacyUnknown { .. }));
    let next = registry
        .update_connection("legacy", |rows| {
            rows[0].settings["credential"]["secret"] = "replacement".into();
            Ok(())
        })
        .unwrap()
        .unwrap();
    let next_state = State::load(&next).unwrap().unwrap();
    assert_ne!(state.revision, next_state.revision);
    assert!(matches!(
        next_state.decision,
        Decision::LegacyUnknown { .. }
    ));
    registry.save(std::slice::from_ref(&next)).unwrap();
    assert_eq!(registry.load().unwrap()[0], next);
}

#[test]
fn hs2_6h8zy6_pending_target_noop_and_batch_default_delete_refuse_but_unrelated_edits_work() {
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(&dir);
    let pending = known(&registry, "pending", true, true);
    registry.save(&[pending.clone(), raw("other")]).unwrap();
    registry
        .update_connection("other", |rows| {
            rows[1].name = Some("Independent".into());
            Ok(())
        })
        .unwrap();
    let before = bytes(&registry);
    endpoint(
        registry.set_disabled("pending", false).unwrap_err(),
        EndpointError::Pending,
    );
    let mut called = false;
    endpoint(
        registry
            .update_connection("pending", |_| {
                called = true;
                Ok(())
            })
            .unwrap_err(),
        EndpointError::Pending,
    );
    assert!(!called);
    endpoint(
        registry
            .update(|rows| {
                rows.retain(|row| row.id != "pending");
                Ok(())
            })
            .unwrap_err(),
        EndpointError::Pending,
    );
    endpoint(
        registry
            .update_connection("other", |rows| {
                for row in rows {
                    row.default = row.id == "other";
                }
                Ok(())
            })
            .unwrap_err(),
        EndpointError::Pending,
    );
    assert_eq!(bytes(&registry), before);
    registry.save(&registry.load().unwrap()).unwrap();
    assert_eq!(bytes(&registry), before);
}

#[test]
fn hs2_6h8zy6_malformed_existing_target_refuses_change_or_delete_without_global_ban() {
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(&dir);
    let mut broken = raw("broken");
    broken.settings["github_endpoint"] = Value::Null;
    external(&registry, &[broken, raw("other")]);
    registry
        .update_connection("other", |rows| {
            rows[1].name = Some("Allowed".into());
            Ok(())
        })
        .unwrap();
    let before = bytes(&registry);
    endpoint(
        registry.set_disabled("broken", false).unwrap_err(),
        EndpointError::Invalid,
    );
    endpoint(
        registry
            .update(|rows| {
                rows[0].name = Some("Refused".into());
                Ok(())
            })
            .unwrap_err(),
        EndpointError::Invalid,
    );
    endpoint(
        registry
            .update(|rows| {
                rows.remove(0);
                Ok(())
            })
            .unwrap_err(),
        EndpointError::Invalid,
    );
    assert_eq!(bytes(&registry), before);
}

#[test]
fn hs2_6h8zy6_exact_id_creation_deletion_and_after_closure_target_semantics() {
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(&dir);
    let _saved = known(&registry, "old", false, false);
    let created = registry
        .update_connection("new", |rows| {
            rows.clear();
            rows.push(raw("new"));
            Ok(())
        })
        .unwrap()
        .unwrap();
    assert_eq!(created, raw("new"));
    assert!(State::load(&created).unwrap().is_none());
    assert!(
        registry
            .update_connection("new", |rows| {
                rows.clear();
                Ok(())
            })
            .unwrap()
            .is_none()
    );
    let mut called = false;
    assert!(
        registry
            .update_connection("absent", |_| {
                called = true;
                Ok(())
            })
            .unwrap()
            .is_none()
    );
    assert!(called);
    let mut jira = raw("jira");
    jira.provider = "jira".into();
    registry.save(&[jira]).unwrap();
    let before = bytes(&registry);
    endpoint(
        registry
            .update(|rows| {
                rows[0].provider = "github".into();
                Ok(())
            })
            .unwrap_err(),
        EndpointError::Conflict,
    );
    assert_eq!(bytes(&registry), before);
}

#[test]
fn hs2_6h8zy6_typed_provider_endpoint_errors_roundtrip_without_ticket_staleness() {
    for expected in [
        EndpointError::Invalid,
        EndpointError::Pending,
        EndpointError::Conflict,
        EndpointError::Storage,
    ] {
        let provider = ProviderError::from(expected);
        assert!(!provider.is_changed_since_read());
        assert_eq!(provider.to_string(), expected.to_string());
        assert_eq!(EndpointError::from(provider), expected);
    }
}
