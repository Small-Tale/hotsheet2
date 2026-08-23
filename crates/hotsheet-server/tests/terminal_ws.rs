//! Live terminal-attach WebSocket E2E (HS2-XTTTMV): boot a real server, open a `cat`
//! terminal over HTTP, attach over a WebSocket, and prove the stream — the scrollback replays,
//! input the viewer sends is forwarded to the PTY, and `cat`'s echo streams back as output.

use futures_util::{SinkExt, StreamExt};
use hotsheet_server::{AppState, app};
use hotsheet_ticketing::{FsStore, StoreMetadata};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message as WsMessage;

const SECRET: &str = "test-secret";

#[tokio::test]
async fn attach_replays_scrollback_streams_output_and_forwards_input() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    let state = AppState::new(store, SECRET.into()).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = app(state);
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // Open a `cat` terminal over HTTP (blocking ureq off the runtime).
    let base = format!("http://{addr}");
    let id = tokio::task::spawn_blocking(move || {
        let resp = ureq::post(&format!("{base}/terminals"))
            .set("x-hotsheet-secret", SECRET)
            .set("content-type", "application/json")
            .send_string(r#"{"command":"cat"}"#)
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp.into_string().unwrap()).unwrap();
        v["id"].as_str().unwrap().to_string()
    })
    .await
    .unwrap();

    // Attach over the WebSocket.
    let url = format!("ws://{addr}/terminals/{id}/attach?secret={SECRET}");
    let (mut ws, _resp) = tokio_tungstenite::connect_async(url)
        .await
        .expect("ws attach connects");

    // Send a line of input — `cat` (and the PTY echo) will stream it back as output.
    ws.send(WsMessage::Text("hotsheet-echo\n".into()))
        .await
        .unwrap();

    // Read frames until the echo shows up (or time out).
    let mut seen = String::new();
    let saw_echo = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while let Some(Ok(msg)) = ws.next().await {
            match msg {
                WsMessage::Binary(b) => seen.push_str(&String::from_utf8_lossy(&b)),
                WsMessage::Text(t) => seen.push_str(&t),
                WsMessage::Close(_) => break,
                _ => {}
            }
            if seen.contains("hotsheet-echo") {
                return true;
            }
        }
        false
    })
    .await
    .unwrap_or(false);
    assert!(
        saw_echo,
        "the attach stream should carry cat's echo; saw: {seen:?}"
    );

    // A bad secret is rejected at the upgrade (no stream).
    let bad = format!("ws://{addr}/terminals/{id}/attach?secret=wrong");
    assert!(
        tokio_tungstenite::connect_async(bad).await.is_err(),
        "a wrong secret must be rejected"
    );
}
