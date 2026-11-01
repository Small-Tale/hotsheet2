//! HS2-HWC0FS: actual authenticated account handler in an owned isolated child.
#![cfg(any(unix, windows))]
use std::{
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

#[test]
fn hs2_hwc0fs_actual_account_parent_backfills_private_metadata_without_public_generation() {
    let root = tempfile::tempdir().unwrap();
    for dir in [
        "user",
        "config",
        "data",
        "state",
        "cache",
        "tmp",
        "empty-git-hooks",
        "empty-git-template",
    ] {
        std::fs::create_dir(root.path().join(dir)).unwrap();
    }
    std::fs::write(root.path().join("empty-git-config"), b"").unwrap();
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--ignored",
            "--exact",
            "hs2_hwc0fs_account_parent_owned_child",
            "--nocapture",
        ])
        .env_clear()
        .current_dir(root.path())
        .env("HS2_HWC_ACCOUNT_ROOT", root.path())
        .env("HOTSHEET_HOME", root.path().join("private-key-home"))
        .env("HOME", root.path().join("user"))
        .env("USERPROFILE", root.path().join("user"))
        .env("CODEX_HOME", root.path().join("no-codex"))
        .env("XDG_CONFIG_HOME", root.path().join("config"))
        .env("XDG_DATA_HOME", root.path().join("data"))
        .env("XDG_STATE_HOME", root.path().join("state"))
        .env("XDG_CACHE_HOME", root.path().join("cache"))
        .env("HOTSHEET_CACHE_DIR", root.path().join("cache"))
        .env("TMPDIR", root.path().join("tmp"))
        .env("TMP", root.path().join("tmp"))
        .env("TEMP", root.path().join("tmp"))
        .env("HOTSHEET_AI_TOOL_PROBES", "off")
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    command
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", root.path().join("empty-git-config"))
        .env("GIT_TEMPLATE_DIR", root.path().join("empty-git-template"));
    // The owned Git fixture needs the installed Git executable, not inherited Git
    // locators/config. The shared launcher also scrubs locator environment.
    if let Some(path) = std::env::var_os("PATH") {
        command.env("PATH", path);
    }
    if let Some(system_root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", system_root);
    }
    let mut child = OwnedChild(command.spawn().unwrap());
    let deadline = Instant::now() + Duration::from_secs(40);
    loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            assert!(status.success(), "owned account child failed: {status}");
            break;
        }
        assert!(
            Instant::now() < deadline,
            "owned account child exceeded completion budget"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    // Cooperative waiting is bounded; native spawn/kill/wait and filesystem I/O
    // are not an end-to-end real-time containment guarantee.
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "HS2-HWC0FS: launched only by the owned private account parent"]
async fn hs2_hwc0fs_account_parent_owned_child() {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use hotsheet_ticketing::{FsStore, KeyRegistry, OsKeychain, StoreMetadata};
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    let root = PathBuf::from(
        std::env::var_os("HS2_HWC_ACCOUNT_ROOT").expect("explicit owned root required"),
    );
    let home = root.join("private-key-home");
    assert_eq!(
        PathBuf::from(std::env::var_os("HOTSHEET_HOME").unwrap()),
        home
    );
    assert!(
        hotsheet_ticketing::secrets::test_credentials_available(),
        "build this debug test with hotsheet-aitools/test-permission-vault"
    );
    hotsheet_ticketing::secrets::enable_test_credentials().unwrap();
    assert!(
        !home.exists(),
        "explicit capability assertion precedes every fixture credential operation"
    );
    let account = "github-app-hwc-account-synthetic";
    let keys = KeyRegistry::new(&home, OsKeychain);
    let bundle = serde_json::json!({"kind":"github_app", "client_id":"synthetic-client",
        "web_base":"https://synthetic.example.test/", "token":{"access_token":"synthetic-token"}})
    .to_string();
    keys.set(account, &bundle).unwrap();
    assert!(keys.list().unwrap()[0].site.is_none());
    let before: serde_json::Value =
        serde_json::from_slice(&std::fs::read(home.join("keys.json")).unwrap()).unwrap();
    assert!(before[account]["credential_generation"].is_string());

    let store_root = root.join("owned-store");
    std::fs::create_dir(&store_root).unwrap();
    let git = |args: &[&str]| {
        let output = hotsheet_ticketing::git::command_in(&store_root)
            .arg("-c")
            .arg(format!(
                "core.hooksPath={}",
                root.join("empty-git-hooks").display()
            ))
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "owned synthetic Git setup failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init", "--quiet"]);
    let store = FsStore::init(&store_root, &StoreMetadata::new("HS")).unwrap();
    git(&["add", "--", "."]);
    git(&[
        "-c",
        "user.name=Synthetic",
        "-c",
        "user.email=synthetic@example.test",
        "-c",
        "commit.gpgsign=false",
        "commit",
        "--quiet",
        "-m",
        "Synthetic fixture metadata",
    ]);
    let state = hotsheet_server::AppState::new(store, "synthetic-hwc-auth".into())
        .unwrap()
        .with_machine_home(&home)
        .with_default_codex_home(root.join("no-codex"))
        .with_checkout_registry(home.join("checkouts.json"));
    let router = hotsheet_server::app(state);
    tokio::time::timeout(Duration::from_secs(20), async {
        for pass in 0..2 {
            let response = router
                .clone()
                .oneshot(
                    Request::builder()
                        .uri("/accounts")
                        .header("x-hotsheet-secret", "synthetic-hwc-auth")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let body = response.into_body().collect().await.unwrap().to_bytes();
            let accounts: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(accounts.as_array().unwrap().len(), 1);
            assert_eq!(accounts[0]["id"], account);
            assert_eq!(accounts[0]["host"], "synthetic.example.test");
            assert!(accounts[0]["sources"].as_array().unwrap().is_empty());
            assert!(accounts[0].get("credential_generation").is_none());
            assert!(!String::from_utf8_lossy(&body).contains("synthetic-token"));
            let published = std::fs::read(home.join("keys.json")).unwrap();
            let published: serde_json::Value = serde_json::from_slice(&published).unwrap();
            assert_eq!(published[account]["site"], "https://synthetic.example.test");
            assert_eq!(
                published[account]["credential_generation"],
                before[account]["credential_generation"]
            );
            if pass == 0 {
                keys.record_identity(account, "synthetic-login").unwrap();
            } else {
                assert_eq!(accounts[0]["identity"], "synthetic-login");
            }
        }
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/accounts")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    })
    .await
    .expect("owned account composition exceeded cooperative budget");
    keys.delete(account).unwrap();
    assert!(keys.list().unwrap().is_empty());
}
