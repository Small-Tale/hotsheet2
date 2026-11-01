//! HS2-DJWTB0: real authenticated transfer parents in an explicitly isolated child.
use super::*;
use hotsheet_ticketing::{KeyRegistry, OsKeychain, StoreMetadata};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

mod scenarios;

const CHILD: &str = "HOTSHEET_DJWTB0_TRANSFER_CHILD";
const ACCOUNT: &str = "synthetic-disabled-transfer-account";
const MISSING_ACCOUNT: &str = "synthetic-disabled-transfer-missing";

// HS2-081PXN/HS2-DJWTB0: a project fixture owns a real Git metadata baseline.
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
        vec!["commit", "-q", "-m", "Initialize synthetic transfer store"],
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

async fn request(
    state: &AppState,
    route: &str,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    use tower::ServiceExt;
    let response = crate::app(state.clone())
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri(route)
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
    (status, serde_json::from_slice(&bytes).unwrap())
}

fn isolate(command: &mut Command, home: &Path) {
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
        .env_remove(hotsheet_ticketing::secrets::env_name(ACCOUNT))
        .env_remove(hotsheet_ticketing::secrets::env_name(MISSING_ACCOUNT));
}

struct OwnedChild(Option<Child>);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

// Polling bounds the running-child wait; native spawn/kill/wait themselves are OS I/O.
fn finish(mut command: Command, home: &Path, name: &str, bound: u64) -> (bool, Vec<u8>, Vec<u8>) {
    let stdout = home.join(format!("{name}.stdout"));
    let stderr = home.join(format!("{name}.stderr"));
    command
        .stdin(Stdio::null())
        .stdout(std::fs::File::create(&stdout).unwrap())
        .stderr(std::fs::File::create(&stderr).unwrap());
    let mut owned = OwnedChild(Some(command.spawn().unwrap()));
    let started = Instant::now();
    let status = loop {
        if let Some(status) = owned.0.as_mut().unwrap().try_wait().unwrap() {
            break status;
        }
        assert!(
            started.elapsed() < Duration::from_secs(bound),
            "owned fixture exceeded polling bound"
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

#[test]
fn hs2_djwtb0_owned_authenticated_transfer_admission() {
    let home = tempfile::tempdir().unwrap();
    let mut command = hotsheet_ticketing::git::launch(std::env::current_exe().unwrap());
    isolate(&mut command, home.path());
    command.env(CHILD, "1").args([
        "--exact",
        "routes::providers::disabled_transfer_tests::hs2_djwtb0_owned_transfer_child",
        "--ignored",
        "--nocapture",
    ]);
    let (ok, stdout, stderr) = finish(command, home.path(), "child", 90);
    assert!(
        ok,
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&stdout),
        String::from_utf8_lossy(&stderr)
    );
}

#[test]
#[ignore = "HS2-DJWTB0 owned fixture entry point; use the isolated parent"]
fn hs2_djwtb0_owned_transfer_child() {
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
    let mut capability = hotsheet_ticketing::git::launch(cli);
    isolate(&mut capability, &home);
    capability.arg("--test-fixture-capabilities");
    let (ok, stdout, _) = finish(capability, &home, "capability", 10);
    assert!(ok);
    let checked: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(checked["isolated_native_credentials"], true);
    hotsheet_aitools::enable_test_permission_vault();
    hotsheet_ticketing::secrets::enable_test_credentials().unwrap();
    let store = init_store(&home.join("store"), "HS");
    let config = ProviderConfigRegistry::new(store.root().join("providers.json"));
    let keys = KeyRegistry::new(&home, OsKeychain);
    keys.set(ACCOUNT, "synthetic-transfer-token").unwrap();
    let state = AppState::new(store, "synthetic-secret".into())
        .unwrap()
        .with_machine_home(&home)
        .with_default_codex_home(home.join("codex"));
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(scenarios::run(&state, &config, &home));
}
