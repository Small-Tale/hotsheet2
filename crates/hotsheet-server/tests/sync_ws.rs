//! Real-socket change transport coverage: WebSocket push is primary and the cursor-bearing
//! long-poll endpoint replays anything produced while the socket is unavailable.

use futures_util::{SinkExt, StreamExt};
use hotsheet_server::{AppState, app};
use hotsheet_ticketing::{FsStore, StoreMetadata};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

const SECRET: &str = "test-secret";

async fn request_json(method: &'static str, url: String, body: Option<&str>) -> serde_json::Value {
    let body = body.map(str::to_owned);
    tokio::task::spawn_blocking(move || {
        let request = ureq::request(method, &url)
            .set("x-hotsheet-secret", SECRET)
            .set("content-type", "application/json");
        let response = match body {
            Some(value) => request.send_string(&value).unwrap(),
            None => request.call().unwrap(),
        };
        serde_json::from_str(&response.into_string().unwrap()).unwrap()
    })
    .await
    .unwrap()
}

async fn request_status(method: &'static str, url: String) -> u16 {
    tokio::task::spawn_blocking(move || {
        ureq::request(method, &url)
            .set("x-hotsheet-secret", SECRET)
            .call()
            .unwrap()
            .status()
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

#[tokio::test]
async fn one_sync_socket_keeps_two_checkouts_present_and_closes_each_lease_separately() {
    let directory = tempfile::tempdir().unwrap();
    let primary =
        FsStore::init(&directory.path().join("primary"), &StoreMetadata::new("HS")).unwrap();
    let registry = directory.path().join("checkouts.json");
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(
            listener,
            app(AppState::new(primary, SECRET.into())
                .unwrap()
                .with_checkout_registry(registry)),
        )
        .await
        .unwrap()
    });
    let base = format!("http://{address}");
    let mut ids = Vec::new();
    for (name, prefix) in [("alpha", "AA"), ("beta", "BB")] {
        let checkout = directory.path().join(name);
        let store = directory.path().join(format!("{name}-store"));
        std::fs::create_dir(&checkout).unwrap();
        FsStore::init(&store, &StoreMetadata::new(prefix)).unwrap();
        let body = serde_json::json!({ "root": checkout, "stores": [store] }).to_string();
        let opened = request_json("POST", format!("{base}/projects/open"), Some(&body)).await;
        ids.push(opened["checkout"]["id"].as_str().unwrap().to_owned());
    }
    let stores = || request_json("GET", format!("{base}/stores"), None);
    let hosted = |prefix: &str, stores: &serde_json::Value| {
        stores
            .as_array()
            .unwrap()
            .iter()
            .any(|store| store["prefix"] == prefix)
    };
    let (mut socket, _) = tokio_tungstenite::connect_async(format!(
        "ws://{address}/ws/sync?secret={SECRET}&client=tab&checkout={}&checkout={}",
        ids[0], ids[1]
    ))
    .await
    .expect("multi-checkout sync socket connects");
    for (id, prefix) in ids.iter().zip(["AA", "BB"]) {
        assert_eq!(
            request_status("POST", format!("{base}/checkouts/{id}/close?client=other")).await,
            204
        );
        assert!(
            hosted(prefix, &stores().await),
            "socket still holds the checkout"
        );
    }
    assert_eq!(
        request_status(
            "POST",
            format!("{base}/checkouts/{}/close?client=tab", ids[0])
        )
        .await,
        204
    );
    for _ in 0..50 {
        if !hosted("AA", &stores().await) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let current = stores().await;
    assert!(
        !hosted("AA", &current),
        "closed checkout unhosts independently"
    );
    assert!(
        hosted("BB", &current),
        "other checkout remains present on the socket"
    );
    socket.send(Message::Close(None)).await.unwrap();
    assert_eq!(
        request_status(
            "POST",
            format!("{base}/checkouts/{}/close?client=tab", ids[1])
        )
        .await,
        204
    );
    for _ in 0..50 {
        if !hosted("BB", &stores().await) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(!hosted("BB", &stores().await));
}
