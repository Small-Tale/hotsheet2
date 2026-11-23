//! HS2-1Y45MZ: real authenticated TLS, CLI and service/MCP compositions on synthetic homes.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use hotsheet_cli::remote_servers::{MtlsRemoteTransport, RemoteRuntime};
use hotsheet_server::{AppState, app};
use hotsheet_sync::LockExt;
use hotsheet_ticketing::remote_servers::{RemoteCredentials, RemoteIdentity, RemoteRegistry};
use hotsheet_ticketing::{FsStore, SecretError, SecretStore, StoreMetadata};
use hotsheet_tls::Paths;
use serde_json::Value;

#[path = "../../../test-support/first_launch.rs"]
mod first_launch;
#[path = "../../../test-support/workspace_bin.rs"]
mod workspace_bin;

pub(super) const SECRET: &str = "synthetic-remote-bearer";
pub(super) const ID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

#[derive(Default)]
pub(super) struct Memory(pub(super) Mutex<HashMap<String, String>>);
impl SecretStore for Memory {
    fn set(&self, id: &str, value: &str) -> Result<(), SecretError> {
        self.0.lock_or_recover().insert(id.into(), value.into());
        Ok(())
    }
    fn get(&self, id: &str) -> Result<Option<String>, SecretError> {
        Ok(self.0.lock_or_recover().get(id).cloned())
    }
    fn delete(&self, id: &str) -> Result<bool, SecretError> {
        Ok(self.0.lock_or_recover().remove(id).is_some())
    }
}

pub(super) struct Running {
    pub(super) origin: String,
    pub(super) address: std::net::SocketAddr,
    stop: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<anyhow::Result<()>>,
}
impl Running {
    pub(super) async fn stop(self) -> std::net::SocketAddr {
        let _ = self.stop.send(());
        tokio::time::timeout(std::time::Duration::from_secs(10), self.task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        self.address
    }
}

pub(super) async fn serve(
    state: AppState,
    paths: &Paths,
    address: Option<std::net::SocketAddr>,
) -> Running {
    let listener =
        tokio::net::TcpListener::bind(address.unwrap_or_else(|| "127.0.0.1:0".parse().unwrap()))
            .await
            .unwrap();
    let address = listener.local_addr().unwrap();
    let config = hotsheet_server::tls::build_server_config(paths).unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(hotsheet_server::tls::serve_tls_with_acl(
        listener,
        app(state),
        config,
        Some(paths.acl()),
        async move {
            let _ = stopped.await;
        },
    ));
    Running {
        origin: format!("https://{address}"),
        address,
        stop,
        task,
    }
}

pub(super) fn state(root: &Path, home: &Path) -> AppState {
    let store = if root.join(hotsheet_ticketing::STORE_METADATA_FILE).is_file() {
        FsStore::open(root).unwrap()
    } else {
        FsStore::init_git(root, &StoreMetadata::new("TST")).unwrap()
    };
    AppState::new(store, SECRET.into())
        .unwrap()
        .with_machine_home(home)
        .with_cache_dir(home.join("cache"))
        .with_default_codex_home(home.join("no-codex-home"))
}

pub(super) fn credentials(device: &hotsheet_tls::DeviceCert) -> RemoteCredentials {
    RemoteCredentials {
        bearer_secret: SECRET.into(),
        session_origin: None,
        session_csrf: None,
        ca_pem: Some(device.ca_pem.clone()),
        client_certificate_pem: Some(device.cert_pem.clone()),
        client_key_pem: Some(device.key_pem.clone()),
    }
}

pub(super) fn identity(home: &Path) -> RemoteIdentity {
    hotsheet_ticketing::remote_servers::server_identity(home, || ulid::Ulid::new().to_string())
        .unwrap()
}

pub(super) fn runtime(home: &Path, memory: &Arc<Memory>) -> RemoteRuntime {
    RemoteRuntime::new(
        RemoteRegistry::new(home.join("remotes.json")),
        memory.clone(),
        Arc::new(MtlsRemoteTransport),
    )
}

/// HS2-RCR4A6: the owned `hotsheet-cli` snapshot, as a sibling of the server binary.
/// Its only caller, `e2e.rs`, is a Unix-only module.
#[cfg(unix)]
pub(super) fn cli_binary() -> std::path::PathBuf {
    workspace_bin::path("hotsheet-server")
        .with_file_name(format!("hotsheet-cli{}", std::env::consts::EXE_SUFFIX))
}

pub(super) fn cli(home: &Path, arguments: &[&str]) -> Value {
    cli_named(home, &["remote"], arguments)
}

pub(super) fn cli_named(home: &Path, prefix: &[&str], arguments: &[&str]) -> Value {
    let binary = workspace_bin::path("hotsheet-server")
        .with_file_name(format!("hotsheet-cli{}", std::env::consts::EXE_SUFFIX));
    assert!(
        binary.is_file(),
        "build the owned CLI/server/MCP sibling artifacts once before this suite"
    );
    let checked = isolated_command(&binary, home)
        .arg("--test-fixture-capabilities")
        .output()
        .unwrap();
    assert!(checked.status.success(), "fixture capability probe failed");
    let checked: Value = serde_json::from_slice(&checked.stdout).unwrap();
    assert_eq!(
        checked["isolated_native_credentials"], true,
        "rebuild the owned debug CLI with hotsheet-aitools/test-permission-vault before this suite"
    );
    let output = isolated_command(&binary, home)
        .arg("--test-fixture-credentials")
        .args(prefix)
        .args(arguments)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains(SECRET));
    serde_json::from_slice(&output.stdout).unwrap()
}

fn isolated_command(binary: &Path, home: &Path) -> std::process::Command {
    for directory in [
        "user-home",
        "codex-home",
        "cache",
        "config",
        "data",
        "state",
        "tmp",
    ] {
        std::fs::create_dir_all(home.join(directory)).unwrap();
    }
    let mut command = std::process::Command::new(binary);
    command
        .env("HOME", home.join("user-home"))
        .env("CODEX_HOME", home.join("codex-home"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_STATE_HOME", home.join("state"))
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("HOTSHEET_HOME", home)
        .env("HOTSHEET_CACHE_DIR", home.join("cache"))
        .env("HOTSHEET_AI_TOOL_PROBES", "off")
        .env("TMPDIR", home.join("tmp"))
        .env("TEMP", home.join("tmp"))
        .env("TMP", home.join("tmp"))
        .env_remove("HOTSHEET_STORE")
        .env_remove("HOTSHEET_SERVER")
        .env_remove("HOTSHEET_SECRET")
        .env_remove("HOTSHEET_TERMINAL_ID")
        // HS2-SBQ3T8: a suite launched from an AI session must not inherit its actor.
        .env_remove("HOTSHEET_ACTOR_ROLE")
        .env_remove("HOTSHEET_ACTOR_ID")
        .current_dir(home);
    command
}

/// HS2-SBQ3T8: the stderr of an owned CLI invocation that must fail.
pub(super) fn cli_failure(home: &Path, prefix: &[&str], arguments: &[&str]) -> String {
    let binary = workspace_bin::path("hotsheet-server")
        .with_file_name(format!("hotsheet-cli{}", std::env::consts::EXE_SUFFIX));
    let output = isolated_command(&binary, home)
        .arg("--test-fixture-credentials")
        .args(prefix)
        .args(arguments)
        .output()
        .unwrap();
    assert!(!output.status.success(), "command unexpectedly succeeded");
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// HS2-CRBR8X: what an owned CLI invocation printed, stdout then stderr, whether or not
/// it succeeded.
pub(super) fn cli_outcome(home: &Path, prefix: &[&str], arguments: &[&str]) -> String {
    let binary = workspace_bin::path("hotsheet-server")
        .with_file_name(format!("hotsheet-cli{}", std::env::consts::EXE_SUFFIX));
    let output = isolated_command(&binary, home)
        .arg("--test-fixture-credentials")
        .args(prefix)
        .args(arguments)
        .output()
        .unwrap();
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

pub(super) fn write_credentials(path: &Path, credentials: &RemoteCredentials) {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .unwrap()
        .write_all(&serde_json::to_vec(credentials).unwrap())
        .unwrap();
}

pub(super) fn renew_server_leaf(paths: &Paths, hosts: Vec<String>) {
    let ca_pem = std::fs::read_to_string(paths.ca_cert()).unwrap();
    let ca_key =
        rcgen::KeyPair::from_pem(&std::fs::read_to_string(paths.ca_key()).unwrap()).unwrap();
    let issuer = rcgen::CertificateParams::from_ca_cert_pem(&ca_pem)
        .unwrap()
        .self_signed(&ca_key)
        .unwrap();
    let key = rcgen::KeyPair::generate().unwrap();
    let mut params = rcgen::CertificateParams::new(hosts).unwrap();
    params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];
    let cert = params.signed_by(&key, &issuer, &ca_key).unwrap();
    std::fs::write(paths.server_cert(), cert.pem()).unwrap();
    std::fs::write(paths.server_key(), key.serialize_pem()).unwrap();
}

pub(super) fn expired_credentials(paths: &Paths) -> RemoteCredentials {
    let ca_pem = std::fs::read_to_string(paths.ca_cert()).unwrap();
    let ca_key =
        rcgen::KeyPair::from_pem(&std::fs::read_to_string(paths.ca_key()).unwrap()).unwrap();
    let issuer = rcgen::CertificateParams::from_ca_cert_pem(&ca_pem)
        .unwrap()
        .self_signed(&ca_key)
        .unwrap();
    let key = rcgen::KeyPair::generate().unwrap();
    let mut params = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
    params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ClientAuth];
    params.not_before = time::OffsetDateTime::now_utc() - time::Duration::days(3);
    params.not_after = time::OffsetDateTime::now_utc() - time::Duration::days(1);
    let certificate = params.signed_by(&key, &issuer, &ca_key).unwrap();
    RemoteCredentials {
        bearer_secret: SECRET.into(),
        session_origin: None,
        session_csrf: None,
        ca_pem: Some(ca_pem),
        client_certificate_pem: Some(certificate.pem()),
        client_key_pem: Some(key.serialize_pem()),
    }
}

pub(super) fn mcp(home: &Path, backend: &[&str], arguments: Value) -> Value {
    mcp_named(home, backend, "hotsheet_remotes", arguments)
}

pub(super) fn mcp_named(home: &Path, backend: &[&str], tool: &str, arguments: Value) -> Value {
    let reply = mcp_reply(home, backend, tool, arguments);
    assert_eq!(reply["result"]["isError"], false, "{reply}");
    serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
}
pub(super) fn mcp_rejected(home: &Path, backend: &[&str], tool: &str, arguments: Value) {
    let reply = mcp_reply(home, backend, tool, arguments);
    assert_eq!(reply["result"]["isError"], true, "{reply}");
}
fn mcp_reply(home: &Path, backend: &[&str], tool: &str, arguments: Value) -> Value {
    use std::io::Write;
    use std::process::{Child, Stdio};
    struct OwnedChild(Option<Child>);
    impl Drop for OwnedChild {
        fn drop(&mut self) {
            if let Some(child) = &mut self.0 {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
    // HS2-4J6V46: every project child must select this already-live owned test
    // process, never bootstrap/launch a real daemon during fixture startup.
    let project_record = if backend.contains(&"--project") {
        let bootstrap = home.join("server-bootstrap.hs2");
        let path = hotsheet_cli::external_launch::instance_path(home, &bootstrap);
        let bytes = std::fs::read(&path).expect("owned project fixture instance required");
        let record: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(record["pid"].as_u64(), Some(u64::from(std::process::id())));
        assert_eq!(
            record["store_path"].as_str(),
            hotsheet_sync::paths::canonicalize(&bootstrap)
                .unwrap()
                .to_str()
        );
        let live = hotsheet_cli::external_launch::discover_running_server(&bootstrap, home)
            .expect("owned fixture process must be live");
        assert!(hotsheet_ticketing::remote_servers::remote_registry_origin_is_loopback(&live.url));
        Some((path, bytes))
    } else {
        None
    };
    let sibling = workspace_bin::path("hotsheet-server");
    let cli = sibling.with_file_name(format!("hotsheet-cli{}", std::env::consts::EXE_SUFFIX));
    let checked = isolated_command(&cli, home)
        .arg("--test-fixture-capabilities")
        .output()
        .unwrap();
    assert!(checked.status.success());
    let checked: Value = serde_json::from_slice(&checked.stdout).unwrap();
    assert_eq!(checked["isolated_native_credentials"], true);
    // Launched once before the 15 s fixture window below starts (HS2-04D6DW).
    let binary = first_launch::launched(
        sibling.with_file_name(format!("hotsheet-mcp{}", std::env::consts::EXE_SUFFIX)),
        &["--version"],
    );
    let mut child = OwnedChild(Some(
        isolated_command(&binary, home)
            .args(backend)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    ));
    {
        let mut input = child.0.as_mut().unwrap().stdin.take().unwrap();
        writeln!(
            input,
            "{}",
            serde_json::json!({"jsonrpc":"2.0", "id":1, "method":"tools/call",
            "params":{"name":tool, "arguments":arguments}})
        )
        .unwrap();
    }
    // HS2-E76Z9V: the reply is the condition. A first run dispatch makes about 40 Git
    // process spawns (each claim is committed), so under a loaded gate one call can take
    // longer than any fixed bound that fits an idle machine; a fixed 15 s bound failed
    // there. Waiting for the exit also drains both pipes, so a chatty child cannot block
    // on a full pipe. A genuine hang is still ended by nextest's slow-timeout
    // (HS2-TT8E7S, `.config/nextest.toml`); `OwnedChild` kills the child if the request
    // cannot be written.
    let output = child.0.take().unwrap().wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains(SECRET));
    assert!(!String::from_utf8_lossy(&output.stderr).contains(SECRET));
    let reply: Value = serde_json::from_slice(&output.stdout).unwrap();
    if let Some((path, bytes)) = project_record {
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
    reply
}

/// HS2-AW2AKF: how the controller machine is paired with a peer.
#[derive(Clone, Copy, Debug)]
pub(super) enum Pairing {
    /// A device certificate over mTLS (`cert issue --bundle`).
    Certificate,
    /// A paired-device session over HTTP, the desktop's remote pairing
    /// (`desktopRemoteBeginPair` / `FinishPair`, then `remotes register`).
    Session,
}

/// The browser origin the desktop pairs from; the peer binds the session to it.
const PAIRED_ORIGIN: &str = "http://localhost:4175";

enum PeerControl {
    Certificate(Paths),
    Session {
        router: axum::Router,
        device: String,
    },
}

/// A served peer and the credentials this machine registers it with (HS2-AW2AKF).
pub(super) struct PairedPeer {
    pub(super) origin: String,
    pub(super) credentials: RemoteCredentials,
    running: Running,
    control: PeerControl,
}

impl PairedPeer {
    /// Serve `state` and pair with it as `pairing` does, read-write.
    pub(super) async fn serve(pairing: Pairing, state: AppState, root: &Path) -> Self {
        match pairing {
            Pairing::Certificate => {
                let paths = Paths::at(root.join("tls"));
                hotsheet_tls::init_ca(&paths, &[]).unwrap();
                let device = hotsheet_tls::issue_device(&paths, "synthetic-orchestrator").unwrap();
                hotsheet_tls::set_device_role(
                    &paths,
                    "synthetic-orchestrator",
                    hotsheet_tls::DeviceRole::ReadWrite,
                )
                .unwrap();
                let running = serve(state, &paths, None).await;
                Self {
                    origin: running.origin.clone(),
                    credentials: credentials(&device),
                    running,
                    control: PeerControl::Certificate(paths),
                }
            }
            Pairing::Session => Self::serve_session(state).await,
        }
    }

    async fn serve_session(state: AppState) -> Self {
        let state = state.with_machine_remote_auth().unwrap();
        let router = app(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let served = router.clone();
        let task = tokio::spawn(async move {
            axum::serve(
                listener,
                served.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .with_graceful_shutdown(async move {
                let _ = stopped.await;
            })
            .await
            .map_err(anyhow::Error::from)
        });
        let origin = format!("http://{address}");
        // The desktop's pairing: request from the browser origin, the host approves
        // read-write, the requester redeems and keeps the session and CSRF cookies.
        let (status, pairing, _) = owner_call(
            &router,
            "POST",
            "/remote-auth/pair",
            Some(serde_json::json!({"label": "Synthetic orchestrator"})),
            false,
        )
        .await;
        assert_eq!(status, 201, "{pairing}");
        let (status, body, _) = owner_call(
            &router,
            "POST",
            "/remote-auth/owner/approve",
            Some(serde_json::json!({"id": pairing["id"], "role": "read_write"})),
            true,
        )
        .await;
        assert_eq!(status, 204, "{body}");
        let (status, device, cookies) = owner_call(
            &router,
            "POST",
            "/remote-auth/redeem",
            Some(serde_json::json!({"id": pairing["id"], "challenge": pairing["challenge"]})),
            false,
        )
        .await;
        assert_eq!(status, 200, "{device}");
        let cookie = |name: &str| {
            cookies
                .iter()
                .find_map(|value| {
                    value
                        .strip_prefix(&format!("{name}="))
                        .and_then(|rest| rest.split(';').next())
                })
                .unwrap()
                .to_owned()
        };
        let credentials = RemoteCredentials {
            bearer_secret: format!("session:{}", cookie("hs2_remote_session")),
            session_origin: Some(PAIRED_ORIGIN.into()),
            session_csrf: Some(cookie("hs2_remote_csrf")),
            ca_pem: None,
            client_certificate_pem: None,
            client_key_pem: None,
        };
        Self {
            origin: origin.clone(),
            credentials,
            running: Running {
                origin,
                address,
                stop,
                task,
            },
            control: PeerControl::Session {
                router,
                device: device["id"].as_str().unwrap().to_owned(),
            },
        }
    }

    /// The peer narrows this machine's pairing to read-only.
    pub(super) async fn read_only(&self) {
        match &self.control {
            PeerControl::Certificate(paths) => {
                hotsheet_tls::set_device_role(
                    paths,
                    "synthetic-orchestrator",
                    hotsheet_tls::DeviceRole::ReadOnly,
                )
                .unwrap();
            }
            PeerControl::Session { router, device } => {
                let (status, body, _) = owner_call(
                    router,
                    "PATCH",
                    &format!("/remote-auth/owner/devices/{device}"),
                    Some(serde_json::json!({"role": "read_only"})),
                    true,
                )
                .await;
                assert_eq!(status, 200, "{body}");
            }
        }
    }

    pub(super) async fn stop(self) -> std::net::SocketAddr {
        self.running.stop().await
    }
}

/// One in-process request to the peer's router, from the paired browser origin, or from
/// the hosting machine itself with its secret.
async fn owner_call(
    router: &axum::Router,
    method: &str,
    path: &str,
    body: Option<Value>,
    owner: bool,
) -> (u16, Value, Vec<String>) {
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    let mut request = axum::http::Request::builder()
        .method(method)
        .uri(path)
        .header("origin", PAIRED_ORIGIN)
        .header("content-type", "application/json");
    if owner {
        request = request.header("x-hotsheet-secret", SECRET);
    }
    let request = request
        .body(axum::body::Body::from(
            body.map(|value| value.to_string()).unwrap_or_default(),
        ))
        .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status().as_u16();
    let cookies = response
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|value| value.to_str().ok().map(str::to_owned))
        .collect();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        cookies,
    )
}
