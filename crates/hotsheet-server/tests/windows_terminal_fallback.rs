#![cfg(windows)]

//! HS2-2TD7M7: native Windows proof that the server's in-process PTY path works.

use std::time::{Duration, Instant};

use hotsheet_server::{AppState, app};
use hotsheet_ticketing::{FsStore, StoreMetadata};
use serde_json::{Value, json};
use tokio::net::TcpListener;

const SECRET: &str = "hs2-2td7m7-synthetic";

async fn request(base: &str, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
    let base = base.to_owned();
    let method = method.to_owned();
    let path = path.to_owned();
    tokio::task::spawn_blocking(move || {
        let request = ureq::request(&method, &format!("{base}{path}"))
            .set("x-hotsheet-secret", SECRET)
            .set("content-type", "application/json");
        let response = match body {
            Some(body) => request.send_string(&body.to_string()),
            None => request.call(),
        };
        let response = match response {
            Ok(response) | Err(ureq::Error::Status(_, response)) => response,
            Err(error) => panic!("fixture request failed: {error}"),
        };
        let status = response.status();
        let text = response.into_string().unwrap();
        let body = if text.is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&text).unwrap()
        };
        (status, body)
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn hs2_2td7m7_windows_server_hosts_terminal_in_process() {
    let fixture = tempfile::tempdir().unwrap();
    let store = FsStore::init(fixture.path(), &StoreMetadata::new("TST")).unwrap();
    let state = AppState::new(store, SECRET.into()).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app(state)).await.unwrap() });

    let (status, opened) = request(
        &base,
        "POST",
        "/terminals",
        Some(json!({"id":"windows-fixture","command":"cmd.exe","args":["/Q","/K"]})),
    )
    .await;
    assert_eq!(status, 200, "{opened}");
    assert_eq!(opened["kind"], "shell");

    let (status, input) = request(
        &base,
        "POST",
        "/terminals/windows-fixture/input",
        Some(json!({"data":"set /a 604*71\r"})),
    )
    .await;
    assert_eq!(status, 204, "{input}");

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let (status, read) = request(&base, "GET", "/terminals/windows-fixture", None).await;
        assert_eq!(status, 200, "{read}");
        if read["scrollback"]
            .as_str()
            .is_some_and(|scrollback| scrollback.contains("42884"))
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Windows PTY produced no output: {read}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let (status, deleted) = request(&base, "DELETE", "/terminals/windows-fixture", None).await;
    assert_eq!(status, 204, "{deleted}");
    server.abort();
}
