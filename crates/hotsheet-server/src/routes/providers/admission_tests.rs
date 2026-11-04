//! HS2-6H8ZY6 actual owner/HTTP/cache/CLI composition in one explicitly isolated child.
use super::*;
use hotsheet_ticketing::github_endpoint::{
    Endpoint, EndpointError, EndpointRegistry, Intent, Link, LinkObserver, Observation, Request,
};
use hotsheet_ticketing::{KeyRegistry, OsKeychain, StoreMetadata};
use std::{
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

mod checkout_links;
mod read_coverage;

const ACCOUNT: &str = "synthetic-read-admission-account";
const CHILD: &str = "HOTSHEET_6H_READ_ADMISSION_CHILD";

// HS2-081PXN/HS2-6H8ZY6: a project ticket store owns a real Git baseline.
fn init_store(path: &Path, prefix: &str) -> FsStore {
    std::fs::create_dir_all(path).unwrap();
    for arguments in [
        vec!["init", "-q"],
        vec!["config", "user.name", "Synthetic Fixture"],
        vec!["config", "user.email", "fixture@example.test"],
        vec!["config", "commit.gpgsign", "false"],
    ] {
        let result = hotsheet_ticketing::git::command_in(path)
            .args(arguments)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    let store = FsStore::init(path, &StoreMetadata::new(prefix)).unwrap();
    for arguments in [
        vec!["add", "hotsheet-store.json", ".gitignore"],
        vec!["commit", "-q", "-m", "Initialize synthetic ticket store"],
        vec!["rev-parse", "--verify", "HEAD"],
    ] {
        let result = hotsheet_ticketing::git::command_in(path)
            .args(arguments)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    let owner = hotsheet_ticketing::git::command_in(path)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .unwrap();
    assert!(owner.status.success());
    assert_eq!(
        std::fs::canonicalize(String::from_utf8(owner.stdout).unwrap().trim()).unwrap(),
        std::fs::canonicalize(path).unwrap()
    );
    store
}

pub(crate) async fn request(
    state: &AppState,
    method: &str,
    uri: &str,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    use tower::ServiceExt;
    let response = crate::app(state.clone())
        .oneshot(
            axum::http::Request::builder()
                .method(method)
                .uri(uri)
                .header("x-hotsheet-secret", "synthetic-secret")
                .header("Content-Type", "application/json")
                .body(axum::body::Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let body = if status == StatusCode::NO_CONTENT {
        assert!(
            bytes.is_empty(),
            "204 must not hide an unexpected response body"
        );
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or_else(|error| {
            panic!("{method} {uri} returned {status} with invalid JSON: {error}")
        })
    };
    (status, body)
}

struct Owned(Option<Child>);
impl Drop for Owned {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
pub(crate) fn finish(mut command: Command, home: &Path, name: &str) -> (bool, Vec<u8>, Vec<u8>) {
    let stdout = home.join(format!("{name}.stdout"));
    let stderr = home.join(format!("{name}.stderr"));
    command
        .stdin(Stdio::null())
        .stdout(std::fs::File::create(&stdout).unwrap())
        .stderr(std::fs::File::create(&stderr).unwrap());
    let mut owned = Owned(Some(command.spawn().unwrap()));
    let start = Instant::now();
    let status = loop {
        if let Some(status) = owned.0.as_mut().unwrap().try_wait().unwrap() {
            break status;
        }
        let bound = if std::env::var_os(CHILD).is_some() {
            10
        } else {
            60
        };
        assert!(
            start.elapsed() < Duration::from_secs(bound),
            "owned child exceeded bound; Drop kills and reaps"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    owned.0.take();
    (
        status.success(),
        std::fs::read(stdout).unwrap(),
        std::fs::read(stderr).unwrap(),
    )
}
pub(crate) fn isolate(command: &mut Command, home: &Path) {
    for directory in ["user", "codex", "config", "data", "state", "cache", "tmp"] {
        std::fs::create_dir_all(home.join(directory)).unwrap();
    }
    command
        .current_dir(home)
        .env("HOTSHEET_HOME", home)
        .env("HOME", home.join("user"))
        .env("CODEX_HOME", home.join("codex"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_STATE_HOME", home.join("state"))
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("HOTSHEET_CACHE_DIR", home.join("cache"))
        .env("TMPDIR", home.join("tmp"))
        .env("HOTSHEET_AI_TOOL_PROBES", "off")
        .env_remove("HOTSHEET_STORE")
        .env_remove("HOTSHEET_SERVER")
        .env_remove("HOTSHEET_SECRET")
        .env_remove(hotsheet_ticketing::secrets::env_name(ACCOUNT));
    for account in [
        "github-app-synthetic-http-coverage",
        "github-app-synthetic-cache-coverage",
        "synthetic-counted-account",
        "synthetic-missing-account",
    ] {
        command.env_remove(hotsheet_ticketing::secrets::env_name(account));
    }
}

#[test]
fn hs2_6h8zy6_owned_server_cli_read_admission_composition() {
    let home = tempfile::tempdir().unwrap();
    let mut child = hotsheet_ticketing::git::launch(std::env::current_exe().unwrap());
    isolate(&mut child, home.path());
    child.env(CHILD, "1").args([
        "--exact",
        "routes::providers::admission_tests::hs2_6h8zy6_owned_read_admission_child",
        "--ignored",
        "--nocapture",
    ]);
    let (ok, stdout, stderr) = finish(child, home.path(), "child");
    assert!(
        ok,
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&stdout),
        String::from_utf8_lossy(&stderr)
    );
}

struct NoLinks;
impl LinkObserver for NoLinks {
    fn is_linked(&self, _: &str, _: &Link) -> Result<bool, EndpointError> {
        Ok(false)
    }
}
struct NeverTransport;
impl hotsheet_extsync::GitHubTransport for NeverTransport {
    fn request(
        &self,
        _: &str,
        _: &str,
        _: &[(&str, String)],
        _: Option<&serde_json::Value>,
    ) -> Result<hotsheet_extsync::HttpResponse, String> {
        panic!("refused source must not reach the injected provider transport")
    }
}

#[test]
#[ignore = "owned fixture entry point; run only through the parent isolation test"]
fn hs2_6h8zy6_owned_read_admission_child() {
    assert_eq!(std::env::var(CHILD).as_deref(), Ok("1"));
    assert!(hotsheet_aitools::test_permission_vault_available());
    assert!(hotsheet_ticketing::secrets::test_credentials_available());
    let home = std::path::PathBuf::from(std::env::var_os("HOTSHEET_HOME").unwrap());
    let cli = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join(format!("hotsheet-cli{}", std::env::consts::EXE_SUFFIX));
    let mut capability = hotsheet_ticketing::git::launch(&cli);
    isolate(&mut capability, &home);
    capability.arg("--test-fixture-capabilities");
    let (ok, stdout, _) = finish(capability, &home, "capability");
    assert!(ok);
    let checked: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(checked["isolated_native_credentials"], true);
    hotsheet_aitools::enable_test_permission_vault();
    hotsheet_ticketing::secrets::enable_test_credentials().unwrap();
    let store = init_store(&home.join("store"), "HS");
    let config = ProviderConfigRegistry::new(store.root().join("providers.json"));
    let keys = KeyRegistry::new(&home, OsKeychain);
    keys.set(ACCOUNT, "synthetic-read-token").unwrap();
    keys.record_site(ACCOUNT, "https://github.com").unwrap();
    let source = ProviderConnection {
        id: "github-source".into(),
        provider: "github".into(),
        locator: "synthetic/project".into(),
        name: None,
        default: false,
        disabled: false,
        settings: serde_json::json!({"credential":{"secret":ACCOUNT}}),
    };
    let source = EndpointRegistry::new(&config)
        .apply(
            &Request {
                operation_id: ulid::Ulid::new(),
                expected: None,
                connection: source,
                intent: Intent::InferSite {
                    credential_ref: ACCOUNT.into(),
                    endpoint: Endpoint::Public,
                    origin: "https://github.com".into(),
                },
                link: None,
            },
            &Observation::Available {
                credential_ref: ACCOUNT.into(),
                site: "https://github.com".into(),
            },
            &NoLinks,
        )
        .unwrap()
        .connection;
    let state = AppState::new(store, "synthetic-secret".into())
        .unwrap()
        .with_machine_home(&home)
        .with_default_codex_home(home.join("codex"));
    let first = provider_for(&state, &source.id).unwrap();
    assert_eq!(state.live_providers.lock_or_recover().len(), 1);
    keys.record_site(ACCOUNT, "https://other.test").unwrap();
    assert!(
        provider_for(&state, &source.id).is_err(),
        "same token/fingerprint cannot authorize stale site"
    );
    assert!(hosted_provider_registry(&state, &source.id, &source.id).is_err());
    keys.record_site(ACCOUNT, "https://github.com").unwrap();
    assert!(
        Arc::ptr_eq(&first, &provider_for(&state, &source.id).unwrap()),
        "fresh matching observation can reuse existing cache"
    );
    assert!(hosted_provider_registry(&state, &source.id, &source.id).is_ok());

    let injected = hotsheet_extsync::GitHubProvider::new(
        hotsheet_extsync::GitHubConfig::from_connection(&source, "synthetic").unwrap(),
        Arc::new(NeverTransport),
    );
    let injected_state = state.clone().with_ticket_provider(Arc::new(injected));
    keys.record_site(ACCOUNT, "https://other.test").unwrap();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        use tower::ServiceExt;
        let response = crate::app(injected_state)
            .oneshot(
                axum::http::Request::builder()
                    .uri("/providers/github-source/tickets/7")
                    .header("x-hotsheet-secret", "synthetic-secret")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::CONFLICT);
    });
    let before = std::fs::read(config.path()).unwrap();
    let mut command = hotsheet_ticketing::git::launch(&cli);
    isolate(&mut command, &home);
    command
        .args(["--test-fixture-credentials", "-C"])
        .arg(home.join("store"))
        .args(["provider-get", &source.id, "7"]);
    let (ok, _, stderr) = finish(command, &home, "cli-drift");
    assert!(!ok);
    assert!(String::from_utf8_lossy(&stderr).contains("requires endpoint review"));
    assert_eq!(std::fs::read(config.path()).unwrap(), before);

    // Rebuild-instance positive control: actual CLI transport uses the fixed reviewed local endpoint.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let api_base = hotsheet_ticketing::github_origin::api_base_for_web_origin(&origin)
        .unwrap()
        .unwrap();
    let endpoint = Endpoint::Url(api_base.clone());
    let local = ProviderConnection {
        id: "local-reviewed".into(),
        provider: "github".into(),
        locator: "synthetic/project".into(),
        name: None,
        default: false,
        disabled: false,
        settings: serde_json::json!({"credential":{"secret":ACCOUNT},"api_base":api_base}),
    };
    keys.record_site(ACCOUNT, &origin).unwrap();
    EndpointRegistry::new(&config)
        .apply(
            &Request {
                operation_id: ulid::Ulid::new(),
                expected: None,
                connection: local,
                intent: Intent::InferSite {
                    credential_ref: ACCOUNT.into(),
                    endpoint,
                    origin: origin.clone(),
                },
                link: None,
            },
            &Observation::Available {
                credential_ref: ACCOUNT.into(),
                site: origin,
            },
            &NoLinks,
        )
        .unwrap();
    let server = std::thread::spawn(move || {
        use std::io::{Read, Write};
        let deadline = Instant::now() + Duration::from_secs(8);
        for comments in [false, true] {
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            Instant::now() < deadline,
                            "expected owned provider request absent"
                        );
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("owned listener: {error}"),
                }
            };
            // HS2-DJMPGV: on macOS and the BSDs an accepted socket inherits the listener's
            // O_NONBLOCK, so without this a read before the request bytes arrive fails at once.
            stream.set_nonblocking(false).unwrap();
            // HS2-78NQ8M: a hang guard, not a pacing bound.
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                assert!(request.len() < 16_384);
                let mut byte = [0];
                assert_eq!(stream.read(&mut byte).unwrap(), 1);
                request.push(byte[0]);
            }
            let request = String::from_utf8(request).unwrap();
            assert!(request.starts_with("GET /api/v3/repos/synthetic/project/issues/7"));
            assert_eq!(
                request.lines().next().unwrap().contains("/comments"),
                comments
            );
            let body = if comments {
                "[]"
            } else {
                r#"{"number":7,"title":"Synthetic read ticket","body":"","state":"open","state_reason":null,"html_url":"https://synthetic.test/issues/7","created_at":"2026-10-31T00:00:00Z","updated_at":"2026-10-31T00:00:00Z","closed_at":null,"labels":[],"assignees":[],"pull_request":null}"#
            };
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
        }
    });
    let mut command = hotsheet_ticketing::git::launch(&cli);
    isolate(&mut command, &home);
    command
        .args(["--test-fixture-credentials", "-C"])
        .arg(home.join("store"))
        .args(["provider-get", "local-reviewed", "7"]);
    let result = finish(command, &home, "cli-reviewed");
    server.join().unwrap();
    assert!(result.0, "{}", String::from_utf8_lossy(&result.2));
    assert!(String::from_utf8_lossy(&result.1).contains("Synthetic read ticket"));
    runtime.block_on(transfer_cases(&state, &config, &keys, &home));
    runtime.block_on(read_coverage::run(&state, &config, &home));
    runtime.block_on(
        checkout_links::hs2_6h8zy6_actual_b3a_checkout_http_cli_parents(&state, &config, &home),
    );
}

async fn transfer_cases(
    state: &AppState,
    config: &ProviderConfigRegistry,
    keys: &KeyRegistry<OsKeychain>,
    home: &Path,
) {
    use hotsheet_ticketing::github_endpoint::Expected;
    let destination_store = init_store(&home.join("transfer-destination"), "DS");
    let (status, registered) = request(
        state,
        "POST",
        "/stores",
        serde_json::json!({"path":destination_store.root()}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let destination = registered["id"].as_str().unwrap();
    let local_source = multistore::store_url_id(&state.store);
    const BAD: &str = "synthetic-unrelated-account";
    keys.set(BAD, "synthetic-unused").unwrap();
    keys.record_site(BAD, "https://other.test").unwrap();
    // Keep metadata but no credential material: constructing this unrelated source would fail.
    hotsheet_ticketing::SecretStore::delete(&OsKeychain, BAD).unwrap();
    let unrelated = ProviderConnection {
        id: "unrelated-github".into(),
        provider: "github".into(),
        locator: "synthetic/unrelated".into(),
        name: None,
        default: false,
        disabled: false,
        settings: serde_json::json!({"credential":{"secret":BAD}}),
    };
    let observed = Observation::Available {
        credential_ref: BAD.into(),
        site: "https://github.com".into(),
    };
    let mut pending_source_id = None;
    for (index, bad_state) in ["disabled", "drift", "unavailable", "pending"]
        .into_iter()
        .enumerate()
    {
        // HS2-6H8ZY6: independent typed records; never overwrite a pending saved image.
        let mut unrelated = unrelated.clone();
        unrelated.id = format!("unrelated-github-{index}");
        let saved = EndpointRegistry::new(config)
            .apply(
                &Request {
                    operation_id: ulid::Ulid::new(),
                    expected: None,
                    connection: unrelated,
                    intent: Intent::InferSite {
                        credential_ref: BAD.into(),
                        endpoint: Endpoint::Public,
                        origin: "https://github.com".into(),
                    },
                    link: None,
                },
                &observed,
                &NoLinks,
            )
            .unwrap()
            .connection;
        keys.record_site(
            BAD,
            if bad_state == "unavailable" {
                "invalid"
            } else {
                "https://other.test"
            },
        )
        .unwrap();
        if bad_state == "disabled" {
            config.set_disabled(&saved.id, true).unwrap();
        }
        if bad_state == "pending" {
            EndpointRegistry::new(config)
                .apply(
                    &Request {
                        operation_id: ulid::Ulid::new(),
                        expected: Some(Expected::capture(&saved).unwrap()),
                        connection: saved.clone(),
                        intent: Intent::InferSite {
                            credential_ref: BAD.into(),
                            endpoint: Endpoint::Public,
                            origin: "https://github.com".into(),
                        },
                        link: Some(Link {
                            checkout_id: "synthetic-checkout".into(),
                            make_default: false,
                        }),
                    },
                    &observed,
                    &NoLinks,
                )
                .unwrap();
            pending_source_id = Some(saved.id.clone());
        }
        let (status, created) = request(
            state,
            "POST",
            "/tickets",
            serde_json::json!({"title":format!("Selected transfer {index}")}),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let native = created["id"].as_str().unwrap();
        let transfer = serde_json::json!({"source":{"connection_id":local_source,"native_id":native},"destination_connection":destination,"operation_id":format!("selected-{index}")});
        let before = destination_store.list_tickets().unwrap().len();
        let (status, first) =
            request(state, "POST", "/provider-transfers/copy", transfer.clone()).await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "unrelated {bad_state}: {first}"
        );
        let (status, retry) =
            request(state, "POST", "/provider-transfers/copy", transfer.clone()).await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(retry["destination"], first["destination"]);
        assert_eq!(destination_store.list_tickets().unwrap().len(), before + 1);
        let mut moving = transfer;
        moving["confirm"] = true.into();
        let (status, moved) = request(state, "POST", "/provider-transfers/move", moving).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(moved["moved"], true);
        assert_eq!(moved["destination"], first["destination"]);
        let source = state.store.read_ticket(&native.parse().unwrap()).unwrap();
        assert_eq!(
            source.close_reason,
            Some(hotsheet_model::CloseReason::Obsolete)
        );

        // Inverting selection must still deny before injected provider I/O or a destination write.
        let injected = hotsheet_extsync::GitHubProvider::new(
            hotsheet_extsync::GitHubConfig::from_connection(&saved, "unused").unwrap(),
            Arc::new(NeverTransport),
        );
        let selected_state = state.clone().with_ticket_provider(Arc::new(injected));
        for selected_source in [true, false] {
            let denied = serde_json::json!({"source":{"connection_id":if selected_source { &saved.id } else { &local_source },"native_id":if selected_source { "7" } else { native }},
                "destination_connection":if selected_source { destination } else { &saved.id },"operation_id":format!("denied-{index}-{selected_source}"),"confirm":true});
            let before = destination_store.list_tickets().unwrap().len();
            let provider_bytes = std::fs::read(config.path()).unwrap();
            let source_path = state.store.ticket_path(&native.parse().unwrap());
            let source_bytes = std::fs::read(&source_path).unwrap();
            for route in ["/provider-transfers/copy", "/provider-transfers/move"] {
                let (status, _) = request(&selected_state, "POST", route, denied.clone()).await;
                assert_eq!(status, StatusCode::CONFLICT, "bad selected {bad_state}");
            }
            assert_eq!(destination_store.list_tickets().unwrap().len(), before);
            assert_eq!(std::fs::read(config.path()).unwrap(), provider_bytes);
            assert_eq!(std::fs::read(source_path).unwrap(), source_bytes);
        }
    }

    // Fixed saved decisions do not inherit an unrelated account-wide site ban.
    for kind in ["explicit", "legacy", "injection-only"] {
        let id = format!("selected-{kind}");
        if kind != "injection-only" {
            let endpoint = Endpoint::Url("HTTPS://GHE.TEST:443/Fixed/API/".into());
            let fixed = ProviderConnection {
                id: id.clone(),
                provider: "github".into(),
                locator: "synthetic/fixed".into(),
                name: None,
                default: false,
                disabled: false,
                settings: serde_json::json!({"api_base":"HTTPS://GHE.TEST:443/Fixed/API/","credential":{"secret":ACCOUNT}}),
            };
            EndpointRegistry::new(config)
                .apply(
                    &Request {
                        operation_id: ulid::Ulid::new(),
                        expected: None,
                        connection: fixed,
                        intent: if kind == "legacy" {
                            Intent::Legacy
                        } else {
                            Intent::Explicit {
                                credential_ref: ACCOUNT.into(),
                                endpoint,
                            }
                        },
                        link: None,
                    },
                    &Observation::Unavailable,
                    &NoLinks,
                )
                .unwrap();
        }
        // A real Git-backed injected provider makes replacement observable without external traffic.
        let selected_state = state.clone().with_ticket_provider(Arc::new(
            GitProvider::new(&id, state.store.clone()).with_default(true),
        ));
        let (status, created) = request(
            &selected_state,
            "POST",
            "/tickets",
            serde_json::json!({"title":format!("Injected {kind}")}),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let registry = hosted_provider_registry(&selected_state, &id, destination).unwrap();
        assert_eq!(
            registry.get(&id).unwrap().descriptor().provider,
            "git",
            "injected replacement must win over persisted external object"
        );
        assert!(
            registry.get(&id).unwrap().descriptor().default,
            "do not renormalize injected defaults"
        );
        assert_eq!(registry.descriptors().len(), 2);
        let selected_only = hosted_provider_registry(&selected_state, &id, &id).unwrap();
        assert_eq!(
            selected_only.descriptors().len(),
            1,
            "same selected ID is constructed once"
        );
        let before = destination_store.list_tickets().unwrap().len();
        let (status, copied) = request(&selected_state, "POST", "/provider-transfers/copy", serde_json::json!({
            "source":{"connection_id":id,"native_id":created["id"]},"destination_connection":destination,"operation_id":format!("fixed-{kind}") })).await;
        assert_eq!(status, StatusCode::CREATED, "{copied}");
        assert_eq!(destination_store.list_tickets().unwrap().len(), before + 1);
    }
    let before = destination_store.list_tickets().unwrap().len();
    let (status, missing) = request(state, "POST", "/provider-transfers/copy", serde_json::json!({
        "source":{"connection_id":"missing-selected","native_id":"7"},"destination_connection":destination,"operation_id":"missing-selected" })).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(
        missing["error"]
            .as_str()
            .unwrap()
            .contains("missing-selected")
    );
    assert_eq!(destination_store.list_tickets().unwrap().len(), before);
    // HS2-6H8ZY6: retain the actual pending record, not a loop-local or guessed ID.
    let pending_source_id = pending_source_id.expect("fixture must create a pending source");
    let pending = config
        .load()
        .unwrap()
        .into_iter()
        .find(|row| row.id == pending_source_id)
        .unwrap();
    assert!(
        hotsheet_ticketing::github_endpoint::State::load(&pending)
            .unwrap()
            .unwrap()
            .is_pending()
    );
    let (status, _) = request(state, "POST", "/provider-transfers/move", serde_json::json!({
        "source":{"connection_id":pending_source_id,"native_id":"7"},"destination_connection":destination,"operation_id":"unconfirmed" })).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "confirm gate remains before pending-source construction"
    );
}
