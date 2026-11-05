//! HS2-4J7WCV: every Codex terminal Hot Sheet launches hosts its own session (`--no-daemon`),
//! with or without a model and effort, so its hooks and MCP server are its own children rather
//! than children of the shared app-server daemon. The real `POST /terminals` route runs the
//! bundled Codex plugin against a fake `codex` on `PATH` that records its arguments. This test
//! is alone in its binary because it changes the process `PATH`. Synthetic data only.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use hotsheet_server::{AppState, app};
use hotsheet_ticketing::{FsStore, StoreMetadata};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const SECRET: &str = "synthetic-codex-launch";

async fn open(router: &Router, body: Value) {
    let request = Request::builder()
        .method("POST")
        .uri("/terminals")
        .header("x-hotsheet-secret", SECRET)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
        status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&bytes)
    );
}

/// The arguments the fake `codex` recorded for terminal `id` (bounded wait: the PTY child
/// starts asynchronously).
fn recorded(out: &Path, id: &str) -> Vec<String> {
    let path = out.join(id);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(text) = std::fs::read_to_string(&path) {
            if text.ends_with("--end--\n") {
                return text
                    .lines()
                    .take_while(|line| *line != "--end--")
                    .map(str::to_owned)
                    .collect();
            }
        }
        assert!(Instant::now() < deadline, "codex never started for {id}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn hs2_4j7wcv_every_hot_sheet_codex_terminal_launches_without_the_daemon() {
    let directory = tempfile::tempdir().unwrap();
    let bin = directory.path().join("bin");
    let out = directory.path().join("args");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&out).unwrap();
    let codex = bin.join("codex");
    std::fs::write(
        &codex,
        format!(
            "#!/bin/sh\n[ -n \"$HOTSHEET_TERMINAL_ID\" ] || exit 0\n\
             {{ for arg in \"$@\"; do printf '%s\\n' \"$arg\"; done; echo --end--; }} > '{}/'\"$HOTSHEET_TERMINAL_ID\"\n\
             exec sleep 30\n",
            out.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&codex, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    // SAFETY: this binary holds only this test, and the change precedes every spawn.
    unsafe { std::env::set_var("PATH", path) };

    let project = directory.path().join("project");
    let store = FsStore::init(&project, &StoreMetadata::new("HS")).unwrap();
    let state = AppState::new(store, SECRET.into())
        .unwrap()
        .with_default_codex_home(directory.path().join("no-codex-home"))
        .with_machine_home(directory.path().join("machine-home"));
    let router = app(state);

    let cases = [
        (
            json!({"id": "plain", "connect": "codex"}),
            vec!["--no-daemon"],
        ),
        (
            json!({"id": "model", "connect": "codex", "model": "gpt-6-astra"}),
            vec!["--no-daemon", "--model", "gpt-6-astra"],
        ),
        (
            json!({"id": "effort", "connect": "codex", "model": "gpt-6-astra", "effort": "high"}),
            vec![
                "--no-daemon",
                "--model",
                "gpt-6-astra",
                "--config",
                "model_reasoning_effort=\"high\"",
            ],
        ),
    ];
    for (body, expected) in cases {
        let id = body["id"].as_str().unwrap().to_owned();
        open(&router, body).await;
        let out = out.clone();
        let args = tokio::task::spawn_blocking(move || recorded(&out, &id))
            .await
            .unwrap();
        assert_eq!(args, expected);
    }
    for id in ["plain", "model", "effort"] {
        let request = Request::builder()
            .method("DELETE")
            .uri(format!("/terminals/{id}"))
            .header("x-hotsheet-secret", SECRET)
            .body(Body::empty())
            .unwrap();
        let _ = router.clone().oneshot(request).await.unwrap();
    }
}
