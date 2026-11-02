//! HS2-6H8ZY6: called only by the existing capability-checked owned child.
use super::*;
use crate::routes::providers::admission_tests::{finish, isolate, request};
use hotsheet_ticketing::github_endpoint::{
    Endpoint, EndpointError, EndpointRegistry, Intent, Link, LinkObserver, Observation, Request,
};
use hotsheet_ticketing::{KeyRegistry, OsKeychain, SecretError, SecretStore};
use serde_json::{Value, json};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

const HOOK: &str = "synthetic-b2-hook";
const OLD: &str = "synthetic-b2-old-hook";
const ACCOUNT: &str = "synthetic-b2-provider";
struct Links;
impl LinkObserver for Links {
    fn is_linked(&self, _: &str, _: &Link) -> Result<bool, EndpointError> {
        Ok(false)
    }
}
#[derive(Clone, Default)]
struct Counted {
    calls: Arc<Mutex<Vec<String>>>,
    during_get: DuringGet,
}
type DuringGet = Arc<Mutex<Option<Box<dyn FnOnce() + Send>>>>;
impl SecretStore for Counted {
    fn get(&self, reference: &str) -> Result<Option<String>, SecretError> {
        self.calls.lock().unwrap().push(reference.into());
        let hook = self.during_get.lock().unwrap().take();
        if let Some(hook) = hook {
            hook();
        }
        Ok((reference == HOOK).then(|| "synthetic-hook-bytes".into()))
    }
    fn set(&self, _: &str, _: &str) -> Result<(), SecretError> {
        Ok(())
    }
    fn delete(&self, _: &str) -> Result<bool, SecretError> {
        panic!("webhook readiness does not delete")
    }
}
fn raw(id: &str) -> ProviderConnection {
    ProviderConnection {
        id: id.into(),
        provider: "github".into(),
        locator: "synthetic/project".into(),
        name: None,
        default: false,
        disabled: false,
        settings: json!({"credential":{"secret":ACCOUNT},"webhook":{"secret":HOOK}}),
    }
}
fn put(config: &ProviderConfigRegistry, row: ProviderConnection) {
    config
        .update(|rows| {
            rows.retain(|old| old.id != row.id);
            rows.push(row);
            Ok(())
        })
        .unwrap();
}
fn corrupt_owned(config: &ProviderConfigRegistry, row: ProviderConnection) {
    let mut rows = config.load().unwrap();
    rows.retain(|old| old.id != row.id);
    rows.push(row);
    // Explicit old/corrupt writer fixture, not a public authority bypass.
    std::fs::write(
        config.path(),
        serde_json::to_vec(&json!({"connections":rows})).unwrap(),
    )
    .unwrap();
}

pub(crate) async fn run(state: &AppState, config: &ProviderConfigRegistry, home: &Path) {
    assert!(hotsheet_ticketing::secrets::test_credentials_available());
    assert_eq!(
        std::env::var_os("HOTSHEET_HOME").as_deref(),
        Some(home.as_os_str())
    );
    for reference in [HOOK, OLD, ACCOUNT, "synthetic-b2-missing"] {
        assert!(std::env::var_os(hotsheet_ticketing::secrets::env_name(reference)).is_none());
    }
    hs2_6h8zy6_setup_watch_counted_matrix_keeps_six_fields(state, config, home);
    hs2_6h8zy6_hook_get_checks_original_read_after_real_link_lookup(state, config, home);
    hs2_6h8zy6_real_webhook_http_and_cli_configuration(state, config, home).await;
    hs2_6h8zy6_webhook_original_proof_get_mutation_preserves_effect_state(state, config).await;
}

fn hs2_6h8zy6_setup_watch_counted_matrix_keeps_six_fields(
    state: &AppState,
    config: &ProviderConfigRegistry,
    home: &Path,
) {
    let counted = Counted::default();
    let keys = KeyRegistry::new(home.join("b2-counted"), counted.clone());
    keys.set(HOOK, "synthetic-hook-bytes").unwrap();
    keys.set(ACCOUNT, "synthetic-provider-bytes").unwrap();
    keys.record_site(ACCOUNT, "https://github.com").unwrap();
    let project = home.join("b2-counted-checkout");
    std::fs::create_dir_all(&project).unwrap();
    let row = raw("b2-counted-source");
    put(config, row.clone());
    state
        .checkout_registry
        .register_registered_sources(
            config,
            &project,
            Some("b2-counted"),
            None,
            vec![hotsheet_ticketing::checkouts::TicketSource {
                connection_id: row.id.clone(),
                provider: row.provider.clone(),
                locator: row.locator.clone(),
            }],
            None,
        )
        .unwrap();
    for condition in [
        "empty",
        "absent",
        "unlinked",
        "disabled",
        "missing-key",
        "valid",
        "malformed",
        "pending",
        "drift",
    ] {
        let mut row = row.clone();
        match condition {
            "empty" => row.settings["webhook"]["secret"] = "".into(),
            "absent" => {
                row.settings.as_object_mut().unwrap().remove("webhook");
            }
            "unlinked" => row.locator = "synthetic/other".into(),
            "disabled" => row.disabled = true,
            "missing-key" => row.settings["webhook"]["secret"] = "synthetic-b2-missing".into(),
            "malformed" => row.settings["github_endpoint"] = Value::Null,
            _ => {}
        }
        corrupt_owned(config, row.clone());
        if matches!(condition, "pending" | "drift") {
            // Replace this owned raw row through the real typed owner, not forged authority.
            config
                .update(|rows| {
                    rows.retain(|old| old.id != row.id);
                    Ok(())
                })
                .unwrap();
            row = EndpointRegistry::new(config)
                .apply(
                    &Request {
                        operation_id: ulid::Ulid::new(),
                        expected: None,
                        connection: row,
                        intent: Intent::InferSite {
                            credential_ref: ACCOUNT.into(),
                            endpoint: Endpoint::Public,
                            origin: "https://github.com".into(),
                        },
                        link: (condition == "pending").then(|| Link {
                            checkout_id: "synthetic-pending".into(),
                            make_default: false,
                        }),
                    },
                    &Observation::Available {
                        credential_ref: ACCOUNT.into(),
                        site: "https://github.com".into(),
                    },
                    &Links,
                )
                .unwrap()
                .connection;
            if condition == "drift" {
                keys.record_site(ACCOUNT, "https://other.test").unwrap();
            }
        }
        counted.calls.lock().unwrap().clear();
        let setup = setup_from_connection(state, config, &keys, &row);
        if matches!(condition, "malformed" | "pending" | "drift") {
            assert_eq!(setup.err().unwrap().status, StatusCode::CONFLICT);
        } else {
            let setup = setup.unwrap();
            let fields = serde_json::to_value(&setup).unwrap();
            assert_eq!(fields.as_object().unwrap().len(), 6);
            assert_eq!(setup.configured, condition == "valid");
            assert!(!setup.active);
        }
        let expected = if matches!(condition, "valid" | "missing-key") {
            1
        } else {
            0
        };
        assert_eq!(
            counted.calls.lock().unwrap().len(),
            expected,
            "setup {condition}"
        );
        counted.calls.lock().unwrap().clear();
        assert!(!watch_ready_with(state, &row, config, &keys));
        assert_eq!(
            counted.calls.lock().unwrap().len(),
            expected,
            "watch {condition}"
        );
        if condition == "valid" {
            state.github_deliveries.mark_verified(
                &format!("{}:{}:{HOOK}", row.id, row.locator),
                b"synthetic-hook-bytes",
            );
            assert!(watch_ready_with(state, &row, config, &keys));
            state.github_deliveries.unverify_connection(&row.id);
        }
    }
    corrupt_owned(config, raw("b2-counted-source"));
}

fn hs2_6h8zy6_hook_get_checks_original_read_after_real_link_lookup(
    state: &AppState,
    config: &ProviderConfigRegistry,
    home: &Path,
) {
    let counted = Counted::default();
    let keys = KeyRegistry::new(home.join("b2-hook-boundary"), counted.clone());
    keys.set(HOOK, "synthetic-hook-bytes").unwrap();
    let row = raw("b2-hook-boundary");
    put(config, row.clone());
    let project = home.join("b2-hook-boundary-checkout");
    std::fs::create_dir_all(&project).unwrap();
    state
        .checkout_registry
        .register_registered_sources(
            config,
            &project,
            Some("b2-hook-boundary"),
            None,
            vec![hotsheet_ticketing::checkouts::TicketSource {
                connection_id: row.id.clone(),
                provider: row.provider.clone(),
                locator: row.locator.clone(),
            }],
            None,
        )
        .unwrap();
    let cursor = state.event_cursor();
    let before = {
        let recent = state.github_deliveries.0.lock_or_recover();
        (
            recent.seen.clone(),
            recent.revisions.clone(),
            recent.verified_sources.clone(),
        )
    };
    let read = hotsheet_extsync::ConnectionRead::capture(config, &row, &keys).unwrap();
    read.check().unwrap();
    assert!(
        linked_checkout(state, &row).unwrap(),
        "actual checkout read must complete successfully first"
    );
    config
        .update_connection(&row.id, |rows| {
            rows.iter_mut().find(|item| item.id == row.id).unwrap().name =
                Some("Changed after lookup".into());
            Ok(())
        })
        .unwrap();
    let error = hook_secret(&read, &keys, HOOK).unwrap_err();
    assert_eq!(error.status, StatusCode::CONFLICT);
    assert!(
        counted.calls.lock().unwrap().is_empty(),
        "stale proof must refuse BEFORE actual get"
    );

    let current = config
        .load()
        .unwrap()
        .into_iter()
        .find(|item| item.id == row.id)
        .unwrap();
    let fresh = hotsheet_extsync::ConnectionRead::capture(config, &current, &keys).unwrap();
    assert!(linked_checkout(state, &current).unwrap());
    assert_eq!(
        hook_secret(&fresh, &keys, HOOK).unwrap().as_deref(),
        Some("synthetic-hook-bytes")
    );
    assert_eq!(
        *counted.calls.lock().unwrap(),
        [HOOK],
        "unchanged control resolves exactly once"
    );

    let changed = config.clone();
    let id = row.id.clone();
    *counted.during_get.lock().unwrap() = Some(Box::new(move || {
        changed
            .update_connection(&id, |rows| {
                rows.iter_mut().find(|item| item.id == id).unwrap().disabled = true;
                Ok(())
            })
            .unwrap();
    }));
    assert!(linked_checkout(state, &current).unwrap());
    assert_eq!(
        hook_secret(&fresh, &keys, HOOK).unwrap_err().status,
        StatusCode::CONFLICT
    );
    assert_eq!(*counted.calls.lock().unwrap(), [HOOK, HOOK]);
    assert!(
        config
            .load()
            .unwrap()
            .iter()
            .find(|item| item.id == row.id)
            .unwrap()
            .disabled
    );
    assert_eq!(state.event_cursor(), cursor);
    let recent = state.github_deliveries.0.lock_or_recover();
    assert_eq!(
        (
            recent.seen.clone(),
            recent.revisions.clone(),
            recent.verified_sources.clone()
        ),
        before
    );
}

async fn hs2_6h8zy6_real_webhook_http_and_cli_configuration(
    state: &AppState,
    config: &ProviderConfigRegistry,
    home: &Path,
) {
    let keys = KeyRegistry::new(home, OsKeychain);
    keys.set(HOOK, "synthetic-hook-bytes").unwrap();
    let mut row = raw("b2-parent-source");
    row.disabled = true;
    row.settings["webhook"]["secret"] = OLD.into();
    put(config, row.clone());
    let uri = format!("/providers/{}/webhook", row.id);
    // OLD is deliberately absent. Metadata-only previous-ref inspection must not need it.
    let (status, body) = request(state, "PUT", &uri, json!({"secret_ref":HOOK})).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["secret_ref"], HOOK);
    assert_eq!(body["configured"], false);
    assert_eq!(body["active"], false);
    assert_eq!(body.as_object().unwrap().len(), 6);
    let before = std::fs::read(config.path()).unwrap();
    let cli = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join(format!("hotsheet-cli{}", std::env::consts::EXE_SUFFIX));
    let mut command = hotsheet_ticketing::git::launch(cli);
    isolate(&mut command, home);
    command
        .args(["--test-fixture-credentials", "-C"])
        .arg(state.store.root())
        .args(["github-webhook-setup", &row.id, "--secret-ref", HOOK]);
    let (ok, stdout, stderr) = finish(command, home, "b2-webhook-cli");
    assert!(ok, "{}", String::from_utf8_lossy(&stderr));
    assert!(String::from_utf8_lossy(&stdout).contains("Configured GitHub webhook"));
    assert_eq!(std::fs::read(config.path()).unwrap(), before);
    let (status, body) = request(state, "GET", &uri, Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["secret_ref"], HOOK);
    let mut broken = config
        .load()
        .unwrap()
        .into_iter()
        .find(|row| row.id == "b2-parent-source")
        .unwrap();
    broken.settings["github_endpoint"] = Value::Null;
    corrupt_owned(config, broken);
    let before = std::fs::read(config.path()).unwrap();
    let (status, _) = request(state, "PUT", &uri, json!({"secret_ref":HOOK})).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(std::fs::read(config.path()).unwrap(), before);
}

async fn hs2_6h8zy6_webhook_original_proof_get_mutation_preserves_effect_state(
    state: &AppState,
    config: &ProviderConfigRegistry,
) {
    use tower::ServiceExt;
    struct Transport {
        config: ProviderConfigRegistry,
        calls: Arc<AtomicUsize>,
    }
    impl hotsheet_extsync::GitHubTransport for Transport {
        fn request(
            &self,
            _: &str,
            url: &str,
            _: &[(&str, String)],
            _: Option<&Value>,
        ) -> Result<hotsheet_extsync::HttpResponse, String> {
            if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                self.config
                    .update(|rows| {
                        rows.iter_mut()
                            .find(|row| row.id == "b2-receive-source")
                            .unwrap()
                            .name = Some("Changed during get".into());
                        Ok(())
                    })
                    .unwrap();
            }
            let body = if url.contains("/comments") {
                json!([])
            } else {
                json!({"number":7,"title":"Synthetic webhook issue","body":"","state":"open","state_reason":null,"html_url":"https://synthetic.test/issues/7",
                    "created_at":"2026-10-31T00:00:00Z","updated_at":"2026-10-31T00:00:00Z","closed_at":null,"labels":[],"assignees":[],"pull_request":null})
            };
            Ok(hotsheet_extsync::HttpResponse {
                status: 200,
                headers: HashMap::new(),
                body: body.to_string(),
            })
        }
    }
    let row = raw("b2-receive-source");
    put(config, row.clone());
    let checkout = tempfile::tempdir().unwrap();
    state
        .checkout_registry
        .register_registered_sources(
            config,
            checkout.path(),
            Some("b2-receive"),
            None,
            vec![hotsheet_ticketing::checkouts::TicketSource {
                connection_id: row.id.clone(),
                provider: "github".into(),
                locator: row.locator.clone(),
            }],
            None,
        )
        .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let provider = hotsheet_extsync::GitHubProvider::new(
        hotsheet_extsync::GitHubConfig::from_connection(&row, "synthetic").unwrap(),
        Arc::new(Transport {
            config: config.clone(),
            calls: calls.clone(),
        }),
    );
    let selected = state.clone().with_ticket_provider(Arc::new(provider));
    let cursor = state.event_cursor();
    let before = {
        let recent = state.github_deliveries.0.lock_or_recover();
        (recent.revisions.clone(), recent.verified_sources.clone())
    };
    let body = json!({"action":"edited","repository":{"full_name":"synthetic/project"},"issue":{"number":7,"updated_at":"2026-10-31T00:00:00Z"}}).to_string();
    let mut key = [0_u8; 64];
    key[..b"synthetic-hook-bytes".len()].copy_from_slice(b"synthetic-hook-bytes");
    let mut inner = Sha256::new();
    for byte in key {
        inner.update([byte ^ 0x36]);
    }
    inner.update(body.as_bytes());
    let mut outer = Sha256::new();
    for byte in key {
        outer.update([byte ^ 0x5c]);
    }
    outer.update(inner.finalize());
    let signature = format!("sha256={:x}", outer.finalize());
    let response = crate::app(selected)
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/webhooks/github/b2-receive-source")
                .header("x-hub-signature-256", signature)
                .header("x-github-event", "issues")
                .header("x-github-delivery", "synthetic-b2-delivery")
                .body(axum::body::Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "actual issue and comment reads complete before same-proof refusal"
    );
    assert_eq!(state.event_cursor(), cursor);
    let recent = state.github_deliveries.0.lock_or_recover();
    assert_eq!(
        (recent.revisions.clone(), recent.verified_sources.clone()),
        before
    );
    assert!(
        !recent
            .seen
            .contains_key("b2-receive-source:synthetic-b2-delivery")
    );
}
