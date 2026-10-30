//! HS2-1Y45MZ: real authenticated TLS, CLI and service/MCP compositions on synthetic homes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use hotsheet_cli::remote_servers::{MtlsRemoteTransport, RemoteRuntime};
use hotsheet_server::{AppState, app};
use hotsheet_sync::LockExt;
use hotsheet_ticketing::remote_servers::{RemoteCredentials, RemoteIdentity, RemoteRegistry};
use hotsheet_ticketing::{FsStore, SecretError, SecretStore, StoreMetadata};
use hotsheet_tls::Paths;
use serde_json::Value;

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
        FsStore::init(root, &StoreMetadata::new("TST")).unwrap()
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

pub(super) fn cli(home: &Path, arguments: &[&str]) -> Value {
    let binary = PathBuf::from(env!("CARGO_BIN_EXE_hotsheet-server"))
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
        .args(["--test-fixture-credentials", "remote"])
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
        .env_remove("HOTSHEET_STORE")
        .env_remove("HOTSHEET_SERVER")
        .env_remove("HOTSHEET_SECRET")
        .env_remove("HOTSHEET_TERMINAL_ID")
        .current_dir(home);
    command
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
        ca_pem: Some(ca_pem),
        client_certificate_pem: Some(certificate.pem()),
        client_key_pem: Some(key.serialize_pem()),
    }
}

pub(super) fn mcp(home: &Path, backend: &[&str], arguments: Value) -> Value {
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
    let sibling = PathBuf::from(env!("CARGO_BIN_EXE_hotsheet-server"));
    let cli = sibling.with_file_name(format!("hotsheet-cli{}", std::env::consts::EXE_SUFFIX));
    let checked = isolated_command(&cli, home)
        .arg("--test-fixture-capabilities")
        .output()
        .unwrap();
    assert!(checked.status.success());
    let checked: Value = serde_json::from_slice(&checked.stdout).unwrap();
    assert_eq!(checked["isolated_native_credentials"], true);
    let binary = sibling.with_file_name(format!("hotsheet-mcp{}", std::env::consts::EXE_SUFFIX));
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
            "params":{"name":"hotsheet_remotes", "arguments":arguments}})
        )
        .unwrap();
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while child.0.as_mut().unwrap().try_wait().unwrap().is_none() {
        assert!(
            std::time::Instant::now() < deadline,
            "owned MCP fixture timed out"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let output = child.0.take().unwrap().wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains(SECRET));
    assert!(!String::from_utf8_lossy(&output.stderr).contains(SECRET));
    let reply: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(reply["result"]["isError"], false, "{reply}");
    serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
}
