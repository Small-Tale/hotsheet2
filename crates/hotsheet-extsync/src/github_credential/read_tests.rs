use super::*;
use hotsheet_model::Ulid;
use hotsheet_ticketing::github_endpoint::{
    Endpoint, Expected, Intent, Link, LinkObserver, Request,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

type Hook = Arc<Mutex<Option<Box<dyn FnOnce() + Send>>>>;

#[derive(Clone, Default)]
struct Secrets {
    value: Arc<Mutex<String>>,
    gets: Arc<AtomicUsize>,
    sets: Arc<AtomicUsize>,
    after_get: Hook,
    after_set: Hook,
}
impl SecretStore for Secrets {
    fn get(&self, _: &str) -> Result<Option<String>, SecretError> {
        self.gets.fetch_add(1, Ordering::SeqCst);
        let value = self.value.lock().unwrap().clone();
        let hook = self.after_get.lock().unwrap().take();
        if let Some(hook) = hook {
            hook();
        }
        Ok(Some(value))
    }
    fn set(&self, _: &str, value: &str) -> Result<(), SecretError> {
        self.sets.fetch_add(1, Ordering::SeqCst);
        *self.value.lock().unwrap() = value.into();
        let hook = self.after_set.lock().unwrap().take();
        if let Some(hook) = hook {
            hook();
        }
        Ok(())
    }
    fn delete(&self, _: &str) -> Result<bool, SecretError> {
        panic!("read flow must not delete a credential")
    }
}
struct Links;
impl LinkObserver for Links {
    fn is_linked(&self, _: &str, _: &Link) -> Result<bool, EndpointError> {
        Ok(false)
    }
}

fn token(access: &str) -> GitHubTokenBundle {
    GitHubTokenBundle {
        access_token: access.into(),
        refresh_token: Some("refresh".into()),
        expires_in: Some(120),
        refresh_token_expires_in: None,
        token_type: None,
        scope: None,
    }
}
fn raw(site: Value) -> String {
    json!({"kind":"github_app","client_id":"IvTest12345678","web_base":site,"obtained_at":100,"token":token("old")}).to_string()
}
struct Fixture {
    _home: tempfile::TempDir,
    registry: ProviderConfigRegistry,
    keys: KeyRegistry<Secrets>,
    secrets: Secrets,
    source: ProviderConnection,
}
impl Fixture {
    fn new(intent: &str) -> Self {
        let home = tempfile::tempdir().unwrap();
        let registry = ProviderConfigRegistry::new(home.path().join("providers.json"));
        let secrets = Secrets::default();
        let keys = KeyRegistry::new(home.path(), secrets.clone());
        keys.set("synthetic-read-account", &raw(json!("https://github.com")))
            .unwrap();
        keys.record_site("synthetic-read-account", "https://github.com")
            .unwrap();
        let source = ProviderConnection {
            id: "source".into(),
            provider: "github".into(),
            locator: "synthetic/project".into(),
            name: None,
            default: false,
            disabled: false,
            settings: json!({"credential":{"secret":"synthetic-read-account"}}),
        };
        let intent = match intent {
            "inferred" => Intent::InferSite {
                credential_ref: "synthetic-read-account".into(),
                endpoint: Endpoint::Public,
                origin: "https://github.com".into(),
            },
            "explicit" => Intent::Explicit {
                credential_ref: "synthetic-read-account".into(),
                endpoint: Endpoint::Public,
            },
            "legacy" => Intent::Legacy,
            _ => unreachable!(),
        };
        let source = EndpointRegistry::new(&registry)
            .apply(
                &Request {
                    operation_id: Ulid::new(),
                    expected: None,
                    connection: source,
                    intent,
                    link: None,
                },
                &Observation::Available {
                    credential_ref: "synthetic-read-account".into(),
                    site: "https://github.com".into(),
                },
                &Links,
            )
            .unwrap()
            .connection;
        Self {
            _home: home,
            registry,
            keys,
            secrets,
            source,
        }
    }
    fn read(&self) -> ConnectionRead<'_, Secrets> {
        ConnectionRead::capture(&self.registry, &self.source, &self.keys).unwrap()
    }
}

#[test]
fn hs2_6h8zy6_selected_read_refuses_site_failure_before_any_secret_get() {
    for site in [None, Some("https://other.test"), Some("malformed")] {
        let f = Fixture::new("inferred");
        let read = f.read();
        match site {
            Some(site) => f.keys.record_site("synthetic-read-account", site).unwrap(),
            None => std::fs::remove_file(f._home.path().join("keys.json")).unwrap(),
        }
        assert!(matches!(
            connection_token_with(&read, 1000, |_, _, _| panic!("no refresh on refusal")),
            Err(GitHubCredentialError::Endpoint(_))
        ));
        assert_eq!(f.secrets.gets.load(Ordering::SeqCst), 0);
        assert_eq!(f.secrets.sets.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn hs2_6h8zy6_pending_all_decision_kinds_refuse_existing_and_new_read_before_get() {
    for kind in ["inferred", "explicit", "legacy"] {
        let f = Fixture::new(kind);
        let read = f.read();
        let intent = match kind {
            "inferred" => Intent::InferSite {
                credential_ref: "synthetic-read-account".into(),
                endpoint: Endpoint::Public,
                origin: "https://github.com".into(),
            },
            "explicit" => Intent::Explicit {
                credential_ref: "synthetic-read-account".into(),
                endpoint: Endpoint::Public,
            },
            "legacy" => Intent::Legacy,
            _ => unreachable!(),
        };
        let pending = EndpointRegistry::new(&f.registry)
            .apply(
                &Request {
                    operation_id: Ulid::new(),
                    expected: Some(Expected::capture(&f.source).unwrap()),
                    connection: f.source.clone(),
                    intent,
                    link: Some(Link {
                        checkout_id: "synthetic-checkout".into(),
                        make_default: false,
                    }),
                },
                &Observation::Available {
                    credential_ref: "synthetic-read-account".into(),
                    site: "https://github.com".into(),
                },
                &Links,
            )
            .unwrap();
        assert!(matches!(
            connection_token_with(&read, 1000, |_, _, _| panic!("pending cannot refresh")),
            Err(GitHubCredentialError::Endpoint(EndpointError::Pending))
        ));
        assert!(matches!(
            ConnectionRead::capture(&f.registry, &pending.connection, &f.keys),
            Err(GitHubCredentialError::Endpoint(EndpointError::Pending))
        ));
        assert_eq!(f.secrets.gets.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn hs2_6h8zy6_selected_bundle_origin_invalid_or_changed_never_refreshes() {
    for site in [
        Value::Null,
        json!(17),
        json!("malformed"),
        json!("https://other.test"),
    ] {
        let f = Fixture::new("inferred");
        *f.secrets.value.lock().unwrap() = raw(site);
        assert!(matches!(
            connection_token_with(&f.read(), 1000, |_, _, _| panic!(
                "no refresh on bundle mismatch"
            )),
            Err(GitHubCredentialError::Endpoint(_))
        ));
        assert_eq!(f.secrets.gets.load(Ordering::SeqCst), 1);
        assert_eq!(f.secrets.sets.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn hs2_6h8zy6_source_change_during_real_get_refuses_old_proof_and_fresh_read_can_succeed() {
    let f = Fixture::new("inferred");
    let read = f.read();
    let registry = f.registry.clone();
    *f.secrets.after_get.lock().unwrap() = Some(Box::new(move || {
        registry
            .update(|sources| {
                sources[0].name = Some("Changed source".into());
                Ok(())
            })
            .unwrap();
    }));
    assert!(matches!(
        connection_token_with(&read, 159, |_, _, _| panic!("not due")),
        Err(GitHubCredentialError::Endpoint(EndpointError::Conflict))
    ));
    let changed = f.registry.load().unwrap().remove(0);
    let fresh = ConnectionRead::capture(&f.registry, &changed, &f.keys).unwrap();
    assert_eq!(
        connection_token_with(&fresh, 159, |_, _, _| panic!("not due")).unwrap(),
        "old"
    );
    assert!(
        read.check().is_err(),
        "a new read cannot upgrade the old proof"
    );
}

#[test]
fn hs2_6h8zy6_site_change_during_get_refuses_token_without_backfilling_metadata() {
    let f = Fixture::new("inferred");
    let home = f._home.path().to_path_buf();
    let secrets = f.secrets.clone();
    *f.secrets.after_get.lock().unwrap() = Some(Box::new(move || {
        KeyRegistry::new(home, secrets)
            .record_site("synthetic-read-account", "https://other.test")
            .unwrap();
    }));
    assert!(matches!(
        connection_token_with(&f.read(), 159, |_, _, _| panic!("not due")),
        Err(GitHubCredentialError::Endpoint(EndpointError::Drift))
    ));
    assert_eq!(
        f.keys.list().unwrap()[0].site.as_deref(),
        Some("https://other.test")
    );
    assert_eq!(f.secrets.gets.load(Ordering::SeqCst), 1);
}

#[test]
fn hs2_6h8zy6_source_change_during_refresh_refuses_save_and_token() {
    let f = Fixture::new("inferred");
    let refreshed = AtomicUsize::new(0);
    let result = connection_token_with(&f.read(), 1000, |_, _, _| {
        refreshed.fetch_add(1, Ordering::SeqCst);
        f.registry
            .update(|sources| {
                sources.clear();
                Ok(())
            })
            .unwrap();
        Ok(token("new"))
    });
    assert!(matches!(result, Err(GitHubCredentialError::Endpoint(_))));
    assert_eq!(
        refreshed.load(Ordering::SeqCst),
        1,
        "the synthetic refresh already happened"
    );
    assert_eq!(
        f.secrets.sets.load(Ordering::SeqCst),
        1,
        "no later secret write"
    );
}

#[test]
fn hs2_6h8zy6_source_change_during_secret_save_refuses_return_after_performed_write() {
    let f = Fixture::new("inferred");
    let registry = f.registry.clone();
    *f.secrets.after_set.lock().unwrap() = Some(Box::new(move || {
        registry
            .update(|sources| {
                sources[0].disabled = true;
                Ok(())
            })
            .unwrap();
    }));
    assert!(matches!(
        connection_token_with(&f.read(), 1000, |_, _, _| Ok(token("new"))),
        Err(GitHubCredentialError::Endpoint(_))
    ));
    assert_eq!(
        f.secrets.sets.load(Ordering::SeqCst),
        2,
        "write preceded final observed refusal; no atomicity claim"
    );
}

#[test]
fn hs2_6h8zy6_fixed_explicit_and_legacy_reads_do_not_need_site_metadata_and_pat_still_does() {
    for kind in ["explicit", "legacy"] {
        let f = Fixture::new(kind);
        std::fs::write(f._home.path().join("keys.json"), "invalid").unwrap();
        assert_eq!(
            connection_token_with(&f.read(), 159, |_, _, _| panic!("not due")).unwrap(),
            "old"
        );
    }
    let f = Fixture::new("inferred");
    *f.secrets.value.lock().unwrap() = "synthetic-personal-token".into();
    assert_eq!(
        connection_token_with(&f.read(), 159, |_, _, _| panic!("PAT never refreshes")).unwrap(),
        "synthetic-personal-token"
    );
    f.keys
        .record_site("synthetic-read-account", "https://other.test")
        .unwrap();
    assert!(
        connection_token_with(&f.read(), 159, |_, _, _| panic!("PAT never refreshes")).is_err()
    );
}

#[test]
fn hs2_6h8zy6_account_raw_resolver_does_not_guess_unrelated_source_authority() {
    let f = Fixture::new("inferred");
    f.keys
        .record_site("synthetic-read-account", "https://other.test")
        .unwrap();
    let already_read = raw(json!("https://github.com"));
    assert_eq!(
        access_token_from_raw(&already_read, &f.keys, "synthetic-read-account", 159).unwrap(),
        "old"
    );
    assert_eq!(
        f.secrets.gets.load(Ordering::SeqCst),
        0,
        "already-read account bundle is not read twice"
    );
    assert!(
        f.read().check().is_err(),
        "selected source still refuses drift"
    );
    // Public mutation Expected remains separate from the opaque read proof.
    assert!(Expected::capture(&f.source).is_ok());
}
