//! Called only from the capability-checked private credential child; all values synthetic.
use super::*;
use hotsheet_ticketing::github_endpoint::Expected;
use hotsheet_ticketing::{SecretError, SecretStore};
use serde_json::{Value, json};
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

const HTTP_ACCOUNT: &str = "github-app-synthetic-http-coverage";
const CACHE_ACCOUNT: &str = "github-app-synthetic-cache-coverage";

#[derive(Clone, Default)]
struct Counted {
    value: Arc<Mutex<String>>,
    reads: Arc<AtomicUsize>,
    writes: Arc<AtomicUsize>,
}
impl SecretStore for Counted {
    fn get(&self, _: &str) -> Result<Option<String>, SecretError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        Ok(Some(self.value.lock().unwrap().clone()))
    }
    fn set(&self, _: &str, value: &str) -> Result<(), SecretError> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        *self.value.lock().unwrap() = value.into();
        Ok(())
    }
    fn delete(&self, _: &str) -> Result<bool, SecretError> {
        panic!("account read does not delete credentials")
    }
}
fn bundle(origin: &str, due: bool, token: &str) -> String {
    json!({"kind":"github_app","client_id":"IvSynthetic12345","web_base":origin,"obtained_at":0,
        "token":{"access_token":token,"refresh_token":"synthetic-refresh","expires_in":if due { json!(1) } else { Value::Null }}}).to_string()
}
struct Upstream {
    origin: String,
    calls: Arc<Mutex<Vec<String>>>,
    refreshes: Arc<AtomicUsize>,
    task: Option<tokio::task::JoinHandle<()>>,
}
impl Drop for Upstream {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}
impl Upstream {
    async fn start(refresh_failure: bool, repository_failure: bool) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let calls = Arc::new(Mutex::new(Vec::new()));
        let seen = calls.clone();
        let refreshes = Arc::new(AtomicUsize::new(0));
        let refreshed = refreshes.clone();
        let app = axum::Router::new().fallback(axum::routing::any(move |request: axum::http::Request<axum::body::Body>| {
            let seen = seen.clone(); let refreshed = refreshed.clone();
            async move {
                let path = request.uri().path().to_string();
                seen.lock().unwrap().push(format!("{} {path}", request.method()));
                if path == "/login/oauth/access_token" {
                    assert_eq!(request.method(), "POST");
                    let body = axum::body::to_bytes(request.into_body(), 16_384).await.unwrap();
                    let body: Value = serde_json::from_slice(&body).unwrap();
                    assert_eq!(body["client_id"], "IvSynthetic12345");
                    assert_eq!(body["refresh_token"], "synthetic-refresh");
                    assert_eq!(body["grant_type"], "refresh_token");
                    refreshed.fetch_add(1, Ordering::SeqCst);
                    return if refresh_failure { (StatusCode::BAD_REQUEST, axum::Json(json!({"error":"synthetic_refresh_refused"}))) }
                    else { (StatusCode::OK, axum::Json(json!({"access_token":"synthetic-new-token","token_type":"bearer"}))) };
                }
                assert_eq!(request.method(), "GET");
                assert_eq!(request.headers()["authorization"], if refreshed.load(Ordering::SeqCst) == 0 { "Bearer synthetic-old-token" } else { "Bearer synthetic-new-token" });
                if repository_failure { return (StatusCode::BAD_GATEWAY, axum::Json(json!({"message":"synthetic repository failure"}))); }
                let body = match path.as_str() {
                    "/api/v3/user/installations" => json!({"installations":[{"id":7,"app_slug":"synthetic-app","account":{"login":"synthetic"},"repository_selection":"selected"}]}),
                    "/api/v3/user/installations/7/repositories" => json!({"repositories":[{"full_name":"synthetic/repository"}]}),
                    _ => panic!("unexpected owned endpoint"),
                };
                (StatusCode::OK, axum::Json(body))
            }
        }));
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self {
            origin,
            calls,
            refreshes,
            task: Some(task),
        }
    }
    async fn stop(mut self) {
        let task = self.task.take().unwrap();
        task.abort();
        let result = tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap();
        assert!(result.is_err_and(|error| error.is_cancelled()));
    }
    fn assert_calls(&self, due: bool, repetitions: usize) {
        let mut expected = Vec::new();
        if due {
            expected.push("POST /login/oauth/access_token".to_string());
        }
        for _ in 0..repetitions {
            expected.extend([
                "GET /api/v3/user/installations".into(),
                "GET /api/v3/user/installations/7/repositories".into(),
            ]);
        }
        assert_eq!(*self.calls.lock().unwrap(), expected);
    }
}

pub(super) async fn run(state: &AppState, config: &ProviderConfigRegistry, home: &Path) {
    hs2_6h8zy6_actual_registry_http_normalization_and_refusal(state, config).await;
    hs2_6h8zy6_account_parent_and_counted_routine(state, config, home).await;
    hs2_6h8zy6_managed_secret_cache_observations(state, config, home);
    hs2_6h8zy6_actual_cached_return_source_transitions(state, config, home);
    hs2_6h8zy6_actual_removal_http_and_cli_owner_boundaries(state, config, home).await;
    crate::github_webhooks::admission_tests::run(state, config, home).await;
}

async fn hs2_6h8zy6_actual_registry_http_normalization_and_refusal(
    state: &AppState,
    config: &ProviderConfigRegistry,
) {
    let raw = ProviderConnection {
        id: "registry-http-boundary".into(),
        provider: "github".into(),
        locator: "synthetic/project".into(),
        name: Some("Before".into()),
        default: false,
        disabled: false,
        settings: json!({"credential":{"secret":ACCOUNT},"api_base":"https://ghe.synthetic.test:443/Exact/API/"}),
    };
    let (status, created) = request(
        state,
        "POST",
        "/provider-connections",
        serde_json::to_value(&raw).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let saved: ProviderConnection = serde_json::from_value(created).unwrap();
    assert_eq!(saved.settings["api_base"], raw.settings["api_base"]);
    assert!(matches!(
        hotsheet_ticketing::github_endpoint::State::load(&saved)
            .unwrap()
            .unwrap()
            .decision,
        hotsheet_ticketing::github_endpoint::Decision::Explicit { .. }
    ));
    let uri = format!("/provider-connections/{}", saved.id);
    let mut edited = raw;
    edited.name = Some("After".into());
    let (status, returned) =
        request(state, "PATCH", &uri, serde_json::to_value(&edited).unwrap()).await;
    assert_eq!(status, StatusCode::OK, "{returned}");
    let actual = config
        .load()
        .unwrap()
        .into_iter()
        .find(|row| row.id == saved.id)
        .unwrap();
    assert_eq!(returned, serde_json::to_value(&actual).unwrap());
    assert!(
        hotsheet_ticketing::github_endpoint::State::load(&actual)
            .unwrap()
            .unwrap()
            .operation
            .is_none()
    );
    let before = std::fs::read(config.path()).unwrap();
    edited.settings["api_base"] = "https://other.synthetic.test/API/".into();
    let (status, refusal) =
        request(state, "PATCH", &uri, serde_json::to_value(&edited).unwrap()).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(refusal["error"], EndpointError::Conflict.to_string());
    assert!(refusal.get("code").is_none());
    assert_eq!(std::fs::read(config.path()).unwrap(), before);
    EndpointRegistry::new(config)
        .apply(
            &Request {
                operation_id: ulid::Ulid::new(),
                expected: Some(Expected::capture(&actual).unwrap()),
                connection: actual.clone(),
                intent: Intent::Keep,
                link: Some(Link {
                    checkout_id: "synthetic-checkout".into(),
                    make_default: false,
                }),
            },
            &Observation::Unavailable,
            &NoLinks,
        )
        .unwrap();
    let before = std::fs::read(config.path()).unwrap();
    let (status, refusal) = request(
        state,
        "PUT",
        &format!("{uri}/disabled"),
        json!({"disabled":false}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(refusal["error"], EndpointError::Pending.to_string());
    assert_eq!(std::fs::read(config.path()).unwrap(), before);
}

fn inferred(
    config: &ProviderConfigRegistry,
    id: &str,
    account: &str,
    origin: &str,
) -> ProviderConnection {
    let api = hotsheet_ticketing::github_origin::api_base_for_web_origin(origin).unwrap();
    let endpoint = api.clone().map_or(Endpoint::Public, Endpoint::Url);
    let mut settings = json!({"credential":{"secret":account}});
    if let Some(api) = api {
        settings["api_base"] = api.into();
    }
    let source = ProviderConnection {
        id: id.into(),
        provider: "github".into(),
        locator: "synthetic/project".into(),
        name: None,
        default: false,
        disabled: false,
        settings,
    };
    EndpointRegistry::new(config)
        .apply(
            &Request {
                operation_id: ulid::Ulid::new(),
                expected: None,
                connection: source,
                intent: Intent::InferSite {
                    credential_ref: account.into(),
                    endpoint,
                    origin: origin.into(),
                },
                link: None,
            },
            &Observation::Available {
                credential_ref: account.into(),
                site: origin.into(),
            },
            &NoLinks,
        )
        .unwrap()
        .connection
}

async fn hs2_6h8zy6_account_parent_and_counted_routine(
    state: &AppState,
    config: &ProviderConfigRegistry,
    home: &Path,
) {
    let keys = KeyRegistry::new(home, OsKeychain);
    for due in [false, true] {
        let upstream = Upstream::start(false, false).await;
        keys.set(
            HTTP_ACCOUNT,
            &bundle(&upstream.origin, due, "synthetic-old-token"),
        )
        .unwrap();
        keys.record_site(HTTP_ACCOUNT, &upstream.origin).unwrap();
        keys.record_identity(HTTP_ACCOUNT, &format!("synthetic-http-{due}"))
            .unwrap();
        let source = inferred(
            config,
            &format!("old-http-source-{due}"),
            HTTP_ACCOUNT,
            "https://old.synthetic.test",
        );
        let before = std::fs::read(config.path()).unwrap();
        assert!(
            provider_for(state, &source.id).is_err(),
            "unrelated fixed inference must refuse drift"
        );
        let uri = format!("/accounts/{HTTP_ACCOUNT}/github-repositories");
        for _ in 0..2 {
            let (status, body) = request(state, "GET", &uri, Value::Null).await;
            assert_eq!(status, StatusCode::OK, "{body}");
            assert_eq!(body["repositories"], json!(["synthetic/repository"]));
            assert_eq!(body["installations"][0]["account"], "synthetic");
            assert_eq!(
                body["install_url"],
                format!("{}/apps/synthetic-app/installations/new", upstream.origin)
            );
        }
        upstream.assert_calls(due, 2);
        assert_eq!(upstream.refreshes.load(Ordering::SeqCst), usize::from(due));
        let saved: Value = serde_json::from_str(&keys.get(HTTP_ACCOUNT).unwrap()).unwrap();
        assert_eq!(
            saved["token"]["access_token"],
            if due {
                "synthetic-new-token"
            } else {
                "synthetic-old-token"
            }
        );
        assert_eq!(saved["token"]["refresh_token"], "synthetic-refresh");
        assert_eq!(std::fs::read(config.path()).unwrap(), before);
        assert!(provider_for(state, &source.id).is_err());
        upstream.stop().await;

        // Exact domain get count: SAME production async routine, without cache hiding a second get.
        let upstream = Upstream::start(false, false).await;
        let counted = Counted::default();
        *counted.value.lock().unwrap() = bundle(&upstream.origin, due, "synthetic-old-token");
        let keys = KeyRegistry::new(home.join(format!("counted-{due}")), counted.clone());
        let result = super::super::super::accounts::account_repositories_from_keys(
            keys,
            "synthetic-counted-account".into(),
        )
        .await
        .unwrap();
        assert_eq!(
            serde_json::to_value(result.0).unwrap()["repositories"],
            json!(["synthetic/repository"])
        );
        assert_eq!(counted.reads.load(Ordering::SeqCst), 1);
        assert_eq!(counted.writes.load(Ordering::SeqCst), usize::from(due));
        upstream.assert_calls(due, 1);
        upstream.stop().await;
    }
    let (status, _) = request(
        state,
        "GET",
        "/accounts/synthetic-missing-account/github-repositories",
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    keys.set(HTTP_ACCOUNT, "synthetic-personal-token").unwrap();
    // Change a nonsecret field so the actual managed cache must reload the fixture item.
    keys.record_identity(HTTP_ACCOUNT, "synthetic-nonbundle")
        .unwrap();
    let (status, _) = request(
        state,
        "GET",
        &format!("/accounts/{HTTP_ACCOUNT}/github-repositories"),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    for refresh_failure in [true, false] {
        let upstream = Upstream::start(refresh_failure, !refresh_failure).await;
        keys.set(
            HTTP_ACCOUNT,
            &bundle(&upstream.origin, refresh_failure, "synthetic-old-token"),
        )
        .unwrap();
        keys.record_site(HTTP_ACCOUNT, &upstream.origin).unwrap();
        keys.record_identity(HTTP_ACCOUNT, &format!("synthetic-error-{refresh_failure}"))
            .unwrap();
        let (status, body) = request(
            state,
            "GET",
            &format!("/accounts/{HTTP_ACCOUNT}/github-repositories"),
            Value::Null,
        )
        .await;
        assert_eq!(
            status,
            if refresh_failure {
                StatusCode::UNAUTHORIZED
            } else {
                StatusCode::BAD_GATEWAY
            }
        );
        assert_eq!(
            *upstream.calls.lock().unwrap(),
            vec![if refresh_failure {
                "POST /login/oauth/access_token"
            } else {
                "GET /api/v3/user/installations"
            }]
        );
        if refresh_failure {
            assert!(
                body["error"]
                    .as_str()
                    .unwrap()
                    .contains("could not be refreshed")
            );
        }
        assert!(
            body["error"]
                .as_str()
                .unwrap()
                .contains(if refresh_failure {
                    "synthetic_refresh_refused"
                } else {
                    "synthetic repository failure"
                }),
            "must observe the intended upstream refusal, not an arbitrary transport error"
        );
        upstream.stop().await;
    }
}

fn hs2_6h8zy6_managed_secret_cache_observations(
    state: &AppState,
    config: &ProviderConfigRegistry,
    home: &Path,
) {
    let keys = KeyRegistry::new(home, OsKeychain);
    keys.set(
        CACHE_ACCOUNT,
        &bundle("https://github.com", false, "synthetic-old-token"),
    )
    .unwrap();
    keys.record_site(CACHE_ACCOUNT, "https://github.com")
        .unwrap();
    let source = inferred(
        config,
        "managed-cache-source",
        CACHE_ACCOUNT,
        "https://github.com",
    );
    let first = provider_for(state, &source.id).unwrap();
    let metadata = std::fs::read(home.join("keys.json")).unwrap();
    OsKeychain.delete(CACHE_ACCOUNT).unwrap();
    assert!(matches!(
        keys.get(CACHE_ACCOUNT),
        Err(SecretError::NotFound(_))
    ));
    assert!(
        Arc::ptr_eq(&first, &provider_for(state, &source.id).unwrap()),
        "native item absent: managed fast path must supply the same bundle"
    );
    keys.record_site(CACHE_ACCOUNT, "https://other.synthetic.test")
        .unwrap();
    assert!(provider_for(state, &source.id).is_err());
    keys.record_site(CACHE_ACCOUNT, "https://github.com")
        .unwrap();
    assert_eq!(
        std::fs::read(home.join("keys.json")).unwrap(),
        metadata,
        "require exact prior bytes before expecting the surviving cache revision"
    );
    assert!(Arc::ptr_eq(
        &first,
        &provider_for(state, &source.id).unwrap()
    ));
    keys.set(
        CACHE_ACCOUNT,
        &bundle("https://github.com", false, "synthetic-new-token"),
    )
    .unwrap();
    keys.record_identity(CACHE_ACCOUNT, "synthetic-changed-identity")
        .unwrap();
    assert_ne!(std::fs::read(home.join("keys.json")).unwrap(), metadata);
    let next = provider_for(state, &source.id).unwrap();
    assert!(
        !Arc::ptr_eq(&first, &next),
        "metadata hash change must expose fresh token/fingerprint"
    );
}

fn hs2_6h8zy6_actual_cached_return_source_transitions(
    state: &AppState,
    config: &ProviderConfigRegistry,
    home: &Path,
) {
    let keys = state.key_registry();
    for change in ["review", "pending", "recreate"] {
        let id = format!("cached-return-{change}");
        let source = inferred(config, &id, CACHE_ACCOUNT, "https://github.com");
        let first = provider_for(state, &id).unwrap();
        let fingerprint = state
            .live_providers
            .lock_or_recover()
            .get(&id)
            .unwrap()
            .0
            .clone();
        let read = hotsheet_extsync::ConnectionRead::capture(config, &source, &keys).unwrap();
        assert!(Arc::ptr_eq(
            &first,
            &cached_provider(state, &id, &fingerprint, &read)
                .unwrap()
                .unwrap()
        ));
        let owner = EndpointRegistry::new(config);
        let observation = Observation::Available {
            credential_ref: CACHE_ACCOUNT.into(),
            site: "https://github.com".into(),
        };
        if change == "recreate" {
            config
                .update(|records| {
                    records.retain(|record| record.id != id);
                    Ok(())
                })
                .unwrap();
            let replaced = inferred(config, &id, CACHE_ACCOUNT, "https://github.com");
            assert_ne!(
                Expected::capture(&replaced).unwrap(),
                Expected::capture(&source).unwrap()
            );
        } else {
            let mutation = owner
                .apply(
                    &Request {
                        operation_id: ulid::Ulid::new(),
                        expected: Some(Expected::capture(&source).unwrap()),
                        connection: source.clone(),
                        intent: Intent::Keep,
                        link: (change == "pending").then(|| Link {
                            checkout_id: "synthetic-checkout".into(),
                            make_default: false,
                        }),
                    },
                    &observation,
                    &NoLinks,
                )
                .unwrap();
            if change == "pending" {
                assert!(cached_provider(state, &id, &fingerprint, &read).is_err());
                assert!(provider_for(state, &id).is_err());
                // Exact existing core compensation arranges a fresh allowed read; no link adapter is claimed.
                owner
                    .compensate(mutation.recovery.as_ref().unwrap())
                    .unwrap();
            }
        }
        assert!(
            cached_provider(state, &id, &fingerprint, &read).is_err(),
            "the actual finisher must refuse this SAME stale proof"
        );
        assert!(
            provider_from_read(state, &read).is_err(),
            "webhook resolution must not recapture and upgrade the original read"
        );
        let fresh = provider_for(state, &id).unwrap();
        assert!(!Arc::ptr_eq(&first, &fresh));
        assert!(
            read.check().is_err(),
            "fresh resolution must not upgrade the old proof"
        );
    }
    assert_eq!(
        std::env::var_os("HOTSHEET_HOME").as_deref(),
        Some(home.as_os_str())
    );
}

async fn hs2_6h8zy6_actual_removal_http_and_cli_owner_boundaries(
    state: &AppState,
    config: &ProviderConfigRegistry,
    home: &Path,
) {
    let cli = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join(format!("hotsheet-cli{}", std::env::consts::EXE_SUFFIX));
    for malformed in [false, true] {
        let id = format!("removal-parent-{malformed}");
        let root = home.join(&id);
        std::fs::create_dir(&root).unwrap();
        let source = ProviderConnection {
            id: id.clone(),
            provider: "github".into(),
            locator: "synthetic/project".into(),
            name: None,
            default: false,
            disabled: false,
            settings: json!({"credential":{"secret":ACCOUNT}}),
        };
        let source = EndpointRegistry::new(config)
            .apply(
                &Request {
                    operation_id: ulid::Ulid::new(),
                    expected: None,
                    connection: source,
                    intent: Intent::Explicit {
                        credential_ref: ACCOUNT.into(),
                        endpoint: Endpoint::Public,
                    },
                    link: Some(Link {
                        checkout_id: "synthetic-pending-parent".into(),
                        make_default: false,
                    }),
                },
                &Observation::Unavailable,
                &NoLinks,
            )
            .unwrap()
            .connection;
        if malformed {
            let mut rows = config.load().unwrap();
            rows.iter_mut().find(|row| row.id == id).unwrap().settings["github_endpoint"] =
                Value::Null;
            // Owned older/corrupt writer fixture, not public authority injection.
            std::fs::write(
                config.path(),
                serde_json::to_vec(&json!({"connections":rows})).unwrap(),
            )
            .unwrap();
        }
        let before = std::fs::read(config.path()).unwrap();
        // No checkout membership: matching eligible removal reaches typed endpoint refusal.
        let uri = format!("/accounts/{ACCOUNT}/sources/{id}");
        let (status, body) = request(state, "DELETE", &uri, Value::Null).await;
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        assert_eq!(
            body["error"],
            (if malformed {
                EndpointError::Invalid
            } else {
                EndpointError::Pending
            })
            .to_string()
        );
        assert!(body.get("code").is_none());
        let (status, _) = request(
            state,
            "DELETE",
            &format!("/accounts/wrong-account/sources/{id}"),
            Value::Null,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let checkout = state
            .checkout_registry
            .register_sources(&root, Some(&id), None, vec![], None)
            .unwrap();
        // A historical link can outlive a later pending or malformed endpoint.
        // Seed that persisted shape directly; the public registration path must
        // refuse this new membership once the endpoint is no longer ordinary.
        let mut saved: Value =
            serde_json::from_slice(&std::fs::read(state.checkout_registry.path()).unwrap())
                .unwrap();
        let entry = saved["checkouts"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|entry| entry["id"] == checkout.id)
            .unwrap();
        entry["sources"] = json!([{
            "connection_id": id,
            "provider": "github",
            "locator": source.locator,
        }]);
        entry["default_source"] = json!(id);
        std::fs::write(
            state.checkout_registry.path(),
            serde_json::to_vec(&saved).unwrap(),
        )
        .unwrap();
        let checkout_bytes = std::fs::read(state.checkout_registry.path()).unwrap();
        let (status, body) = request(state, "DELETE", &uri, Value::Null).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert!(
            body["error"].as_str().unwrap().contains("still used"),
            "eligibility must precede pending/malformed refusal"
        );
        for uri in [
            format!("/provider-connections/{id}"),
            format!("/checkouts/{}/sources/{id}", checkout.id),
        ] {
            let (status, body) = request(state, "DELETE", &uri, Value::Null).await;
            assert_eq!(status, StatusCode::CONFLICT, "{body}");
            assert_eq!(
                body["error"],
                (if malformed {
                    EndpointError::Invalid
                } else {
                    EndpointError::Pending
                })
                .to_string()
            );
        }
        for global in [true, false] {
            let mut command = hotsheet_ticketing::git::launch(&cli);
            isolate(&mut command, home);
            command
                .args(["--test-fixture-credentials", "-C"])
                .arg(state.store.root());
            if global {
                command.args(["provider-remove", &id, "--json"]);
            } else {
                command.args(["checkout", "remove-source", &checkout.id, &id, "--json"]);
            }
            let (ok, _, stderr) = finish(command, home, &format!("b1-refuse-{malformed}-{global}"));
            assert!(!ok);
            assert!(
                String::from_utf8_lossy(&stderr).contains(
                    &(if malformed {
                        EndpointError::Invalid
                    } else {
                        EndpointError::Pending
                    })
                    .to_string()
                )
            );
        }
        assert_eq!(std::fs::read(config.path()).unwrap(), before);
        assert_eq!(
            std::fs::read(state.checkout_registry.path()).unwrap(),
            checkout_bytes
        );
    }
    // Valid controls use the same authenticated parent and headless commands.
    for mode in ["http", "cli-global", "cli-detach", "account"] {
        let id = format!("removal-valid-{mode}");
        let source = ProviderConnection {
            id: id.clone(),
            provider: "github".into(),
            locator: "synthetic/project".into(),
            name: None,
            default: false,
            disabled: false,
            settings: json!({"credential":{"secret":ACCOUNT}}),
        };
        config
            .update(|rows| {
                rows.push(source.clone());
                Ok(())
            })
            .unwrap();
        if mode == "account" {
            let (status, _) = request(
                state,
                "DELETE",
                &format!("/accounts/{ACCOUNT}/sources/{id}"),
                Value::Null,
            )
            .await;
            assert_eq!(status, StatusCode::NO_CONTENT);
        } else {
            let root = home.join(&id);
            std::fs::create_dir(&root).unwrap();
            let checkout = state
                .checkout_registry
                .register_registered_sources(
                    config,
                    &root,
                    Some(&id),
                    None,
                    vec![hotsheet_ticketing::checkouts::TicketSource {
                        connection_id: id.clone(),
                        provider: "github".into(),
                        locator: source.locator.clone(),
                    }],
                    Some(id.clone()),
                )
                .unwrap();
            if mode == "http" {
                let (status, body) = request(
                    state,
                    "DELETE",
                    &format!("/provider-connections/{id}"),
                    Value::Null,
                )
                .await;
                assert_eq!(status, StatusCode::OK);
                assert_eq!(body["removed_connection"], true);
            } else {
                let mut command = hotsheet_ticketing::git::launch(&cli);
                isolate(&mut command, home);
                command
                    .args(["--test-fixture-credentials", "-C"])
                    .arg(state.store.root());
                if mode == "cli-global" {
                    command.args(["provider-remove", &id, "--json"]);
                } else {
                    command.args(["checkout", "remove-source", &checkout.id, &id, "--json"]);
                }
                let (ok, stdout, stderr) = finish(command, home, mode);
                assert!(ok, "{}", String::from_utf8_lossy(&stderr));
                let report: Value = serde_json::from_slice(&stdout).unwrap();
                assert_eq!(report["removed_connection"], true);
            }
            assert!(
                state
                    .checkout_registry
                    .resolve(&checkout.id)
                    .unwrap()
                    .source(&id)
                    .is_none()
            );
        }
        assert!(!config.load().unwrap().iter().any(|row| row.id == id));
    }
}
