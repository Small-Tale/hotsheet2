//! Real-socket change transport coverage: WebSocket push is primary and the cursor-bearing
//! long-poll endpoint replays anything produced while the socket is unavailable.

use futures_util::{SinkExt, StreamExt};
use hotsheet_server::{AppState, app};
use hotsheet_ticketing::{FsStore, StoreMetadata};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

const SECRET: &str = "test-secret";

async fn request_json(
    method: &'static str,
    url: String,
    body: Option<&'static str>,
) -> serde_json::Value {
    tokio::task::spawn_blocking(move || {
        let request = ureq::request(method, &url)
            .set("x-hotsheet-secret", SECRET)
            .set("content-type", "application/json");
        let response = match body {
            Some(value) => request.send_string(value).unwrap(),
            None => request.call().unwrap(),
        };
        serde_json::from_str(&response.into_string().unwrap()).unwrap()
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn websocket_push_and_long_poll_fallback_share_one_replay_cursor() {
    let directory = tempfile::tempdir().unwrap();
    let store = FsStore::init(directory.path(), &StoreMetadata::new("HS")).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app(AppState::new(store, SECRET.into()).unwrap()))
            .await
            .unwrap()
    });
    let base = format!("http://{address}");

    let handshake = request_json("GET", format!("{base}/ws/poll?timeout_ms=0"), None).await;
    assert_eq!(handshake["cursor"], 0);

    let (mut socket, _) =
        tokio_tungstenite::connect_async(format!("ws://{address}/ws/sync?secret={SECRET}"))
            .await
            .expect("sync socket connects");
    request_json(
        "POST",
        format!("{base}/tickets"),
        Some(r#"{"title":"live push"}"#),
    )
    .await;
    let pushed = tokio::time::timeout(std::time::Duration::from_secs(2), socket.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let pushed: serde_json::Value = serde_json::from_str(pushed.to_text().unwrap()).unwrap();
    assert_eq!(pushed["kind"], "created");
    assert_eq!(pushed["cursor"], 1);
    socket.send(Message::Close(None)).await.unwrap();

    request_json(
        "POST",
        format!("{base}/tickets"),
        Some(r#"{"title":"poll replay"}"#),
    )
    .await;
    let replay = request_json("GET", format!("{base}/ws/poll?since=1&timeout_ms=0"), None).await;
    assert_eq!(replay["cursor"], 2);
    assert_eq!(replay["events"].as_array().unwrap().len(), 1);
    assert_eq!(replay["events"][0]["cursor"], 2);
    assert_eq!(replay["events"][0]["kind"], "created");
}
