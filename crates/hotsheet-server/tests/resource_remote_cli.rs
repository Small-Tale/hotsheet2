//! HS2-7H1Y4E: `hotsheet-cli resource renew|release --server` against a real server socket,
//! over loopback http and over real HTTPS with a pinned CA. A task with no shared home and no
//! server secret keeps or ends its own claim through the renewal-token route; the token is the
//! only credential and never appears in output. Synthetic names only.

#[path = "../../test-support/workspace_bin.rs"]
mod workspace_bin;

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};

use axum::extract::Request;
use axum::middleware::{self, Next};
use hotsheet_resource_claims::ClaimService;
use hotsheet_server::{AppState, app};
use hotsheet_ticketing::{FsStore, StoreMetadata};
use hotsheet_tls::{Paths, init_ca};
use serde_json::{Value, json};
use tokio::net::TcpListener;

const SECRET: &str = "test-secret";

/// One request as the server saw it.
#[derive(Debug, Clone)]
struct Seen {
    line: String,
    authorization: Option<String>,
    secret_header: bool,
}

struct Server {
    root: tempfile::TempDir,
    home: PathBuf,
    base: String,
    seen: Arc<Mutex<Vec<Seen>>>,
    tls: Option<Paths>,
}

async fn start(https: bool) -> Server {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let store = FsStore::init_git(root.path().join("demo.hs2"), &StoreMetadata::new("HS")).unwrap();
    let state = AppState::new(store, SECRET.into())
        .unwrap()
        .with_machine_home(&home)
        .with_default_codex_home(root.path().join("no-codex-home"));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let record = seen.clone();
    let router = app(state).layer(middleware::from_fn(move |request: Request, next: Next| {
        record.lock().unwrap().push(Seen {
            line: format!("{} {}", request.method(), request.uri().path()),
            authorization: request
                .headers()
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned),
            secret_header: request.headers().contains_key("x-hotsheet-secret"),
        });
        next.run(request)
    }));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    if https {
        let paths = Paths::at(root.path().join("tls"));
        init_ca(&paths, &[]).unwrap();
        let config = hotsheet_server::tls::build_https_server_config(&paths).unwrap();
        tokio::spawn(async move {
            hotsheet_server::tls::serve_tls(listener, router, config, std::future::pending())
                .await
                .unwrap();
        });
        Server {
            root,
            home,
            base: format!("https://127.0.0.1:{}", addr.port()),
            seen,
            tls: Some(paths),
        }
    } else {
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        Server {
            root,
            home,
            base: format!("http://{addr}"),
            seen,
            tls: None,
        }
    }
}

/// Take a claim directly through the shared service (the holder's side).
fn claim(home: &Path, target: &str, seconds: u64) -> Value {
    ClaimService::new(home.to_path_buf())
        .request(
            "POST",
            &format!("/resource-claims/{target}/acquire"),
            &json!({ "duration_ms": seconds * 1000 }),
        )
        .unwrap()
}

fn text(output: &Output) -> (String, String) {
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn json_of(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or(Value::Null)
}

fn token_of(grant: &Value) -> String {
    grant["renewal_token"].as_str().unwrap().to_owned()
}

#[tokio::test(flavor = "multi_thread")]
async fn hs2_7h1y4e_a_task_with_only_its_token_renews_and_releases_through_the_server() {
    let server = start(false).await;
    let held = claim(&server.home, "vms/one", 60);
    let token = token_of(&held);
    let base = server.base.clone();
    let output = tokio::task::spawn_blocking({
        let token = token.clone();
        let args = [
            "renew",
            "--server",
            &base,
            "--token",
            &token,
            "--duration",
            "600s",
        ]
        .map(str::to_owned);
        let root = server.root.path().to_path_buf();
        move || {
            Command::new(workspace_bin::path("hotsheet-cli"))
                .arg("resource")
                .args(&args)
                .env("HOTSHEET_HOME", root.join("guest-home"))
                .env_remove("HOTSHEET_RESOURCE_CLAIM_TOKEN")
                .current_dir(&root)
                .stdin(Stdio::null())
                .output()
                .unwrap()
        }
    })
    .await
    .unwrap();
    let (out, err) = text(&output);
    assert!(output.status.success(), "{out}{err}");
    let renewed = json_of(&output);
    assert_eq!(renewed["claim"]["claim_id"], held["claim_id"], "{out}");
    assert!(
        renewed["claim"]["expires_at_ms"].as_u64().unwrap()
            > held["expires_at_ms"].as_u64().unwrap(),
        "the lease was extended: {out}"
    );
    for stream in [&out, &err] {
        assert!(!stream.contains(&token), "the token is never printed");
    }

    // What crossed the wire: the token as a bearer, no server secret, on the token route.
    let seen = server.seen.lock().unwrap().clone();
    let route: Vec<&Seen> = seen
        .iter()
        .filter(|s| s.line == "POST /resource-claim-token/renew")
        .collect();
    assert_eq!(route.len(), 1, "{seen:?}");
    assert_eq!(
        route[0].authorization.as_deref(),
        Some(format!("Bearer {token}").as_str())
    );
    assert!(
        seen.iter().all(|s| !s.secret_header),
        "the server secret is never sent: {seen:?}"
    );

    let released = tokio::task::spawn_blocking({
        let (base, token) = (server.base.clone(), token.clone());
        let root = server.root.path().to_path_buf();
        move || {
            Command::new(workspace_bin::path("hotsheet-cli"))
                .args(["resource", "release", "--server", &base, "--token", &token])
                .env("HOTSHEET_HOME", root.join("guest-home"))
                .current_dir(&root)
                .stdin(Stdio::null())
                .output()
                .unwrap()
        }
    })
    .await
    .unwrap();
    assert!(released.status.success(), "{}", text(&released).1);
    assert!(
        !ClaimService::new(server.home.clone())
            .request("GET", "/resource-claims/vms/one", &Value::Null)
            .unwrap()["held"]
            .as_bool()
            .unwrap()
    );

    // The token is revoked: the same renewal now fails, names the code, and still prints no token.
    let (base, root) = (server.base.clone(), server.root.path().to_path_buf());
    let again = tokio::task::spawn_blocking(move || {
        Command::new(workspace_bin::path("hotsheet-cli"))
            .args([
                "resource",
                "renew",
                "--server",
                &base,
                "--token",
                &token,
                "--duration",
                "60s",
            ])
            .env("HOTSHEET_HOME", root.join("guest-home"))
            .current_dir(&root)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    assert!(!again.status.success());
    let (out, err) = text(&again);
    assert!(
        out.contains("token_invalid") || err.contains("token_invalid"),
        "{out}{err}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn hs2_7h1y4e_a_wrong_target_a_bad_url_and_plain_http_off_loopback_send_nothing() {
    let server = start(false).await;
    let held = claim(&server.home, "vms/one", 60);
    let token = token_of(&held);
    let (base, root) = (server.base.clone(), server.root.path().to_path_buf());
    let (wrong_target, off_loopback, bad_token, userinfo) = tokio::task::spawn_blocking({
        let token = token.clone();
        move || {
            let run = |args: Vec<String>| {
                Command::new(workspace_bin::path("hotsheet-cli"))
                    .arg("resource")
                    .args(&args)
                    .env("HOTSHEET_HOME", root.join("guest-home"))
                    .env_remove("HOTSHEET_RESOURCE_CLAIM_TOKEN")
                    .current_dir(&root)
                    .stdin(Stdio::null())
                    .output()
                    .unwrap()
            };
            (
                run(vec![
                    "renew".into(),
                    "vms/other".into(),
                    "--server".into(),
                    base.clone(),
                    "--token".into(),
                    token.clone(),
                    "--duration".into(),
                    "60s".into(),
                ]),
                run(vec![
                    "renew".into(),
                    "--server".into(),
                    "http://10.255.255.1:9".into(),
                    "--token".into(),
                    token.clone(),
                    "--duration".into(),
                    "60s".into(),
                ]),
                run(vec![
                    "renew".into(),
                    "--server".into(),
                    base.clone(),
                    "--token".into(),
                    "not-a-token".into(),
                    "--duration".into(),
                    "60s".into(),
                ]),
                run(vec![
                    "renew".into(),
                    "--server".into(),
                    "https://user:pw@host".into(),
                    "--token".into(),
                    token.clone(),
                    "--duration".into(),
                    "60s".into(),
                ]),
            )
        }
    })
    .await
    .unwrap();
    for (output, needle) in [
        (&wrong_target, "is for vms/one, not vms/other"),
        (&off_loopback, "clear text"),
        (&bad_token, "token_invalid"),
        (&userinfo, "without credentials"),
    ] {
        let (out, err) = text(output);
        assert!(!output.status.success(), "{needle}");
        assert!(
            format!("{out}{err}").contains(needle),
            "{needle}: {out}{err}"
        );
        assert!(
            !format!("{out}{err}").contains(&token),
            "{needle}: the token is never printed"
        );
    }
    assert!(
        server
            .seen
            .lock()
            .unwrap()
            .iter()
            .all(|s| !s.line.contains("resource-claim-token")),
        "none of these reached the server"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn hs2_7h1y4e_https_with_a_pinned_authority_works_and_an_untrusted_one_is_refused_with_the_fix_named()
 {
    let server = start(true).await;
    let held = claim(&server.home, "vms/one", 60);
    let token = token_of(&held);
    let ca = server.tls.as_ref().unwrap().ca_cert();
    let (base, root, token_a) = (
        server.base.clone(),
        server.root.path().to_path_buf(),
        token.clone(),
    );
    let (pinned, unpinned, wrong_ca) = tokio::task::spawn_blocking(move || {
        let run = |extra: Vec<String>| {
            let mut args = vec![
                "resource".to_owned(),
                "renew".into(),
                "--server".into(),
                base.clone(),
                "--token".into(),
                token_a.clone(),
                "--duration".into(),
                "300s".into(),
            ];
            args.extend(extra);
            Command::new(workspace_bin::path("hotsheet-cli"))
                .args(&args)
                .env("HOTSHEET_HOME", root.join("guest-home"))
                .env_remove("HOTSHEET_RESOURCE_CLAIM_TOKEN")
                .current_dir(&root)
                .stdin(Stdio::null())
                .output()
                .unwrap()
        };
        // A CA that did not issue the server certificate.
        let other = Paths::at(root.join("other-tls"));
        init_ca(&other, &[]).unwrap();
        (
            run(vec!["--ca-cert".into(), ca.display().to_string()]),
            run(vec![]),
            run(vec![
                "--ca-cert".into(),
                other.ca_cert().display().to_string(),
            ]),
        )
    })
    .await
    .unwrap();
    let (out, err) = text(&pinned);
    assert!(pinned.status.success(), "{out}{err}");
    assert_eq!(json_of(&pinned)["claim"]["claim_id"], held["claim_id"]);
    for (output, label) in [(&unpinned, "no --ca-cert"), (&wrong_ca, "another CA")] {
        let (out, err) = text(output);
        assert!(!output.status.success(), "{label}");
        assert!(
            format!("{out}{err}").contains("--ca-cert"),
            "{label} names the fix: {out}{err}"
        );
        assert!(!format!("{out}{err}").contains(&token), "{label}: no token");
    }
    let seen = server.seen.lock().unwrap().clone();
    assert_eq!(
        seen.iter()
            .filter(|s| s.line == "POST /resource-claim-token/renew")
            .count(),
        1,
        "only the pinned handshake reached the route: {seen:?}"
    );
    assert!(seen.iter().all(|s| !s.secret_header));
}
