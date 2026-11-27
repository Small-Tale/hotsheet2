#![cfg(unix)] // A `cat` PTY fixture and the detached Unix socket broker.

//! HS2-HA0Z3A: a shared terminal clear through a real server, in both terminal modes (PTYs owned
//! by the server and PTYs in a detached broker). `POST /terminals/{id}/clear` keeps only the
//! cursor's line in the replay, starts a new output stream, and sends every attached viewer a
//! replacement. Walks clear then new output, a second viewer attached during the clear, a
//! reattach (reload) after the clear, a stale `resume_from` (an evicted warm viewport), and a
//! second clear.

use futures_util::{SinkExt, StreamExt};
use hotsheet_server::{AppState, app};
use hotsheet_ticketing::{FsStore, StoreMetadata};
use serde_json::Value;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message as WsMessage;

const SECRET: &str = "test-secret";
const PROMPT: &str = "PROMPT$ ";

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

struct Fixture {
    addr: SocketAddr,
    _dirs: Vec<tempfile::TempDir>,
}

async fn serve(state: AppState, dirs: Vec<tempfile::TempDir>) -> Fixture {
    let tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = tcp.local_addr().unwrap();
    let app = app(state);
    tokio::spawn(async move {
        axum::serve(tcp, app).await.unwrap();
    });
    Fixture { addr, _dirs: dirs }
}

fn store() -> (tempfile::TempDir, AppState) {
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    let state = AppState::new(store, SECRET.into()).unwrap();
    (dir, state)
}

async fn fixture(broker: bool) -> Fixture {
    let (dir, mut state) = store();
    let mut dirs = vec![dir];
    if broker {
        let bdir = tempfile::tempdir().unwrap();
        let sock = bdir.path().join("b.sock");
        let listener = tokio::net::UnixListener::bind(&sock).unwrap();
        tokio::spawn(hotsheet_terminals::serve_broker(
            listener,
            "proj".into(),
            std::sync::Arc::new(hotsheet_terminals::TerminalManager::new()),
        ));
        state = state.with_terminal_broker_at(
            hotsheet_server::terminal_broker::TerminalBroker::at(&sock, "proj"),
        );
        dirs.push(bdir);
    }
    serve(state, dirs).await
}

async fn request(
    method: &'static str,
    addr: SocketAddr,
    path: String,
    body: Option<Value>,
) -> (u16, String) {
    tokio::task::spawn_blocking(move || {
        let body = body.map(|body| body.to_string());
        let mut headers = vec![("x-hotsheet-secret", SECRET)];
        if body.is_some() {
            headers.push(("content-type", "application/json"));
        }
        let mut response = hotsheet_http::run(
            &hotsheet_http::build(hotsheet_http::config().http_status_as_error(false)),
            method,
            &format!("http://{addr}{path}"),
            &headers,
            body.as_deref().map(str::as_bytes),
        )
        .unwrap_or_else(|other| panic!("request failed: {other}"));
        (
            response.status().as_u16(),
            response.body_mut().read_to_string().unwrap(),
        )
    })
    .await
    .unwrap()
}

async fn clear(addr: SocketAddr, id: &str) -> u16 {
    request("POST", addr, format!("/terminals/{id}/clear"), None)
        .await
        .0
}

async fn scrollback(addr: SocketAddr, id: &str) -> String {
    let (status, body) = request("GET", addr, format!("/terminals/{id}"), None).await;
    assert_eq!(status, 200);
    serde_json::from_str::<Value>(&body).unwrap()["scrollback"]
        .as_str()
        .unwrap()
        .to_string()
}

/// A terminal with old output above a prompt that has no newline yet, then `cat` without echo.
async fn prompt_terminal(addr: SocketAddr) -> String {
    let script = format!(
        "stty -echo; i=0; while [ $i -lt 50 ]; do printf 'OLD-OUTPUT %d\\n' $i; i=$((i+1)); done; printf '{PROMPT}'; exec cat"
    );
    let body = serde_json::json!({"command": "/bin/sh", "args": ["-c", script], "id": "clear-me"});
    let (status, created) = request("POST", addr, "/terminals".into(), Some(body)).await;
    assert_eq!(status, 200, "{created}");
    let id = serde_json::from_str::<Value>(&created).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    for _ in 0..500 {
        if scrollback(addr, &id).await.ends_with(PROMPT) {
            return id;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("terminal never printed its prompt");
}

async fn connect(addr: SocketAddr, id: &str, query: &str) -> Ws {
    let url = format!("ws://{addr}/terminals/{id}/attach?secret={SECRET}{query}");
    tokio_tungstenite::connect_async(url).await.unwrap().0
}

async fn next(ws: &mut Ws) -> WsMessage {
    tokio::time::timeout(Duration::from_secs(10), ws.next())
        .await
        .expect("frame within 10 s")
        .unwrap()
        .unwrap()
}

/// One replay a viewer received: its kind (`legacy-replace` for the unframed control) and
/// stream position, and its payload.
#[derive(Debug)]
struct Received {
    kind: String,
    stream: Option<String>,
    payload: Vec<u8>,
}

/// Read frames until a replay announcement or the legacy replace control and its payload,
/// skipping size and liveness frames. Live output before it is returned in `live`.
async fn next_replay(ws: &mut Ws, live: &mut Vec<u8>) -> Received {
    loop {
        match next(ws).await {
            WsMessage::Text(text) => {
                let frame: Value = serde_json::from_str(text.as_str()).unwrap();
                let (kind, stream) = if let Some(header) = frame.get("terminal_frame") {
                    assert_eq!(header["encoding"], "identity");
                    (
                        header["kind"].as_str().unwrap().to_string(),
                        header["stream"].as_str().map(str::to_string),
                    )
                } else if frame.get("terminal_replay").is_some() {
                    ("legacy-replace".to_string(), None)
                } else {
                    continue;
                };
                let WsMessage::Binary(payload) = next(ws).await else {
                    panic!("a replay announcement is followed by its payload");
                };
                return Received {
                    kind,
                    stream,
                    payload: payload.to_vec(),
                };
            }
            WsMessage::Binary(bytes) => live.extend_from_slice(&bytes),
            _ => {}
        }
    }
}

/// Read live output until `needle` arrives; a replacement on the way fails the test.
async fn live_until(ws: &mut Ws, needle: &str) -> String {
    let mut seen = String::new();
    while !seen.contains(needle) {
        match next(ws).await {
            WsMessage::Binary(bytes) => seen.push_str(&String::from_utf8_lossy(&bytes)),
            WsMessage::Text(text) => assert!(
                !text.as_str().contains("terminal_frame") && !text.as_str().contains("replay"),
                "unexpected replacement before {needle:?}: {text}"
            ),
            _ => {}
        }
    }
    seen
}

const FRAMED: &str = "&replay_protocol=2";

async fn shared_clear_transitions(broker: bool) {
    let fixture = fixture(broker).await;
    let addr = fixture.addr;
    let id = prompt_terminal(addr).await;

    // Two viewers stay attached through the clear: a framed one and a legacy one.
    let mut framed = connect(addr, &id, FRAMED).await;
    let first = next_replay(&mut framed, &mut Vec::new()).await;
    assert_eq!(first.kind, "replay", "broker={broker}");
    assert!(String::from_utf8_lossy(&first.payload).contains("OLD-OUTPUT 0"));
    let old_stream = first.stream.clone().expect("a stream id");
    let old_offset = first.payload.len();
    let mut legacy = connect(addr, &id, "").await;
    let WsMessage::Binary(legacy_first) = next(&mut legacy).await else {
        panic!("a legacy attach starts with its replay");
    };
    assert!(String::from_utf8_lossy(&legacy_first).contains("OLD-OUTPUT 49"));

    // Clear: both viewers get the cursor's line as a replacement, on a new stream.
    assert_eq!(clear(addr, &id).await, 204, "broker={broker}");
    let replaced = next_replay(&mut framed, &mut Vec::new()).await;
    assert_eq!(replaced.kind, "replace", "broker={broker}");
    assert_eq!(replaced.payload, PROMPT.as_bytes(), "broker={broker}");
    let cleared_stream = replaced.stream.clone().expect("a stream id");
    assert_ne!(cleared_stream, old_stream, "a clear starts a new stream");
    let legacy_replaced = next_replay(&mut legacy, &mut Vec::new()).await;
    assert_eq!(legacy_replaced.kind, "legacy-replace", "broker={broker}");
    assert_eq!(legacy_replaced.payload, PROMPT.as_bytes());
    assert_eq!(scrollback(addr, &id).await, PROMPT, "broker={broker}");

    // Clear then new output: both viewers get it live, after the replacement.
    framed
        .send(WsMessage::Text("after-clear\n".into()))
        .await
        .unwrap();
    live_until(&mut framed, "after-clear").await;
    live_until(&mut legacy, "after-clear").await;

    // A reload after the clear replays only what followed the cursor's line.
    let mut reload = connect(addr, &id, FRAMED).await;
    let reloaded = next_replay(&mut reload, &mut Vec::new()).await;
    assert_eq!(reloaded.kind, "replay");
    assert_eq!(reloaded.stream.as_deref(), Some(cleared_stream.as_str()));
    assert_eq!(
        String::from_utf8_lossy(&reloaded.payload),
        format!("{PROMPT}after-clear\r\n"),
        "broker={broker}"
    );
    let _ = reload.close(None).await;

    // An evicted warm viewport restored with its pre-clear position cannot resume across the
    // clear: it gets the whole cleared replay instead of appending to its old screen.
    let mut restored = connect(
        addr,
        &id,
        &format!("{FRAMED}&resume_from={old_stream}:{old_offset}"),
    )
    .await;
    let restored_replay = next_replay(&mut restored, &mut Vec::new()).await;
    assert_eq!(restored_replay.kind, "replay", "broker={broker}");
    assert!(!String::from_utf8_lossy(&restored_replay.payload).contains("OLD-OUTPUT"));
    let _ = restored.close(None).await;

    // Clear twice: the cursor sits at the start of an empty line, so nothing is kept.
    assert_eq!(clear(addr, &id).await, 204);
    let twice = next_replay(&mut framed, &mut Vec::new()).await;
    assert_eq!(twice.kind, "replace");
    assert!(twice.payload.is_empty(), "broker={broker}: {twice:?}");
    assert_ne!(twice.stream.as_deref(), Some(cleared_stream.as_str()));
    let legacy_twice = next_replay(&mut legacy, &mut Vec::new()).await;
    assert!(legacy_twice.payload.is_empty());
    assert_eq!(clear(addr, &id).await, 204, "clearing an empty replay");
    let thrice = next_replay(&mut framed, &mut Vec::new()).await;
    assert!(thrice.payload.is_empty());

    // Output after repeated clears still streams and replays.
    legacy
        .send(WsMessage::Text("after-twice\n".into()))
        .await
        .unwrap();
    live_until(&mut framed, "after-twice").await;
    assert_eq!(scrollback(addr, &id).await, "after-twice\r\n");

    assert_eq!(
        clear(addr, "no-such-terminal").await,
        404,
        "broker={broker}"
    );
    let (status, _) = request("DELETE", addr, format!("/terminals/{id}"), None).await;
    assert!((200..300).contains(&status));
}

#[tokio::test]
async fn hs2_ha0z3a_a_shared_clear_reaches_replays_and_attached_viewers() {
    shared_clear_transitions(false).await;
}

#[tokio::test]
async fn hs2_ha0z3a_a_broker_shared_clear_reaches_replays_and_attached_viewers() {
    shared_clear_transitions(true).await;
}

/// A detached broker that predates `clear_replay` answers it as a bad request. The server reports
/// that as unsupported instead of a generic failure, so a client keeps its local clear.
#[tokio::test]
async fn hs2_ha0z3a_an_older_broker_reports_the_shared_clear_as_unsupported() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let bdir = tempfile::tempdir().unwrap();
    let sock = bdir.path().join("old.sock");
    let listener = tokio::net::UnixListener::bind(&sock).unwrap();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let (read, mut write) = stream.into_split();
                let mut lines = BufReader::new(read).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let reply = if line.contains("\"clear_replay\"") {
                        r#"{"result":"err","message":"bad request: unknown variant `clear_replay`"}"#
                    } else {
                        r#"{"result":"not_found"}"#
                    };
                    let _ = write.write_all(format!("{reply}\n").as_bytes()).await;
                }
            });
        }
    });
    let (dir, state) = store();
    let state = state.with_terminal_broker_at(
        hotsheet_server::terminal_broker::TerminalBroker::at(&sock, "proj"),
    );
    let fixture = serve(state, vec![dir, bdir]).await;
    let (status, body) = request("POST", fixture.addr, "/terminals/old/clear".into(), None).await;
    assert_eq!(status, 501, "{body}");
    assert!(body.contains("predates the shared clear"), "{body}");
}
