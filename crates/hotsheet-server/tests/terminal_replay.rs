//! Opt-in attach replay protocol through a real server (HS2-K93QHA), in both terminal modes:
//! PTYs owned by the server and PTYs in a detached broker. Legacy viewers keep the unframed
//! replay; opted-in viewers get a capped, compressed, announced replay followed by the same live
//! stream, in order.

use futures_util::{SinkExt, StreamExt};
use hotsheet_server::{AppState, app};
use hotsheet_ticketing::{FsStore, StoreMetadata};
use std::io::Read as _;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message as WsMessage;

const SECRET: &str = "test-secret";
const LINES: usize = 3_000;

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

struct Fixture {
    addr: SocketAddr,
    _dirs: Vec<tempfile::TempDir>,
}

async fn fixture(broker: bool) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    let mut dirs = vec![dir];
    let mut state = AppState::new(store, SECRET.into()).unwrap();
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
    let tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = tcp.local_addr().unwrap();
    let app = app(state);
    tokio::spawn(async move {
        axum::serve(tcp, app).await.unwrap();
    });
    Fixture { addr, _dirs: dirs }
}

/// Open a terminal that prints colored lines, then echoes its input through `cat`.
async fn colored_terminal(addr: SocketAddr, id: &'static str) -> String {
    let base = format!("http://{addr}");
    let script = format!(
        "stty -echo; i=0; while [ $i -lt {LINES} ]; do printf '\\033[32mline %d \\303\\251\\033[0m\\n' $i; i=$((i+1)); done; printf 'READY\\n'; exec cat"
    );
    let body =
        serde_json::json!({"command": "/bin/sh", "args": ["-c", script], "id": id}).to_string();
    let created = base.clone();
    let id = tokio::task::spawn_blocking(move || {
        let response = ureq::post(&format!("{created}/terminals"))
            .set("x-hotsheet-secret", SECRET)
            .set("content-type", "application/json")
            .send_string(&body)
            .unwrap();
        let value: serde_json::Value =
            serde_json::from_str(&response.into_string().unwrap()).unwrap();
        value["id"].as_str().unwrap().to_string()
    })
    .await
    .unwrap();
    let wait_id = id.clone();
    tokio::task::spawn_blocking(move || {
        for _ in 0..500 {
            let response = ureq::get(&format!("{base}/terminals/{wait_id}"))
                .set("x-hotsheet-secret", SECRET)
                .call()
                .unwrap();
            let value: serde_json::Value =
                serde_json::from_str(&response.into_string().unwrap()).unwrap();
            if value["scrollback"]
                .as_str()
                .unwrap_or_default()
                .contains("READY")
            {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("terminal never printed READY");
    })
    .await
    .unwrap();
    id
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

fn json(message: &WsMessage) -> serde_json::Value {
    match message {
        WsMessage::Text(text) => serde_json::from_str(text.as_str()).unwrap(),
        other => panic!("expected a text frame, got {other:?}"),
    }
}

fn binary(message: WsMessage) -> Vec<u8> {
    match message {
        WsMessage::Binary(bytes) => bytes.to_vec(),
        other => panic!("expected a binary frame, got {other:?}"),
    }
}

fn inflate(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    flate2::read::DeflateDecoder::new(bytes)
        .read_to_end(&mut out)
        .unwrap();
    out
}

/// Read frames until live output containing `needle` arrives; every binary must be unframed.
async fn live_output_until(ws: &mut Ws, needle: &str) {
    let mut seen = String::new();
    while !seen.contains(needle) {
        match next(ws).await {
            WsMessage::Binary(bytes) => seen.push_str(&String::from_utf8_lossy(&bytes)),
            WsMessage::Text(text) => assert!(
                !text.as_str().contains("terminal_frame"),
                "live output was framed: {text}"
            ),
            _ => {}
        }
    }
}

async fn capped_compressed_replay_then_live_output(broker: bool) {
    let fixture = fixture(broker).await;
    let id = colored_terminal(fixture.addr, "replay").await;

    let mut legacy = connect(fixture.addr, &id, "").await;
    let full = binary(next(&mut legacy).await);
    assert!(String::from_utf8_lossy(&full).contains("READY"));
    let _ = legacy.close(None).await;

    let mut framed = connect(
        fixture.addr,
        &id,
        "&replay_protocol=2&replay_max_bytes=8192&replay_encoding=deflate-raw",
    )
    .await;
    let header = json(&next(&mut framed).await);
    let expected = hotsheet_terminals::replay_suffix(&full, 8192);
    assert_eq!(
        header,
        serde_json::json!({"terminal_frame": {"kind": "replay", "encoding": "deflate-raw", "bytes": expected.len(), "stream": header["terminal_frame"]["stream"], "end_offset": full.len()}}),
        "broker={broker}"
    );
    let compressed = binary(next(&mut framed).await);
    assert!(compressed.len() < expected.len() / 2, "broker={broker}");
    assert_eq!(inflate(&compressed), expected, "broker={broker}");
    assert!(
        expected.starts_with(b"\x1b[32mline ") || expected.starts_with(b"line "),
        "the capped replay starts at a line, not inside a sequence"
    );

    framed
        .send(WsMessage::Text("after-replay\n".into()))
        .await
        .unwrap();
    live_output_until(&mut framed, "after-replay").await;
}

#[tokio::test]
async fn hs2_k93qha_opted_in_viewer_gets_a_capped_compressed_replay_then_live_output() {
    capped_compressed_replay_then_live_output(false).await;
}

#[tokio::test]
async fn hs2_k93qha_broker_viewer_gets_a_capped_compressed_replay_then_live_output() {
    capped_compressed_replay_then_live_output(true).await;
}

#[tokio::test]
async fn hs2_k93qha_unknown_protocol_values_keep_the_legacy_replay() {
    let fixture = fixture(false).await;
    let id = colored_terminal(fixture.addr, "legacy").await;
    let mut ws = connect(
        fixture.addr,
        &id,
        "&replay_protocol=x&replay_max_bytes=8192&replay_encoding=deflate-raw",
    )
    .await;
    let replay = binary(next(&mut ws).await);
    assert!(replay.len() > 8192);
    assert!(String::from_utf8_lossy(&replay).contains("READY"));
}

async fn send_input(addr: SocketAddr, id: &str, text: &'static str) {
    let url = format!("http://{addr}/terminals/{id}/input");
    tokio::task::spawn_blocking(move || {
        ureq::post(&url)
            .set("x-hotsheet-secret", SECRET)
            .set("content-type", "application/json")
            .send_string(&serde_json::json!({ "data": text }).to_string())
            .unwrap();
    })
    .await
    .unwrap();
}

/// The legacy full replay right now (no output is trimmed in these small terminals, so its
/// length is the stream's end offset).
async fn full_replay(addr: SocketAddr, id: &str) -> Vec<u8> {
    let mut ws = connect(addr, id, "").await;
    let replay = binary(next(&mut ws).await);
    let _ = ws.close(None).await;
    replay
}

async fn resume_appends_only_missed_output(broker: bool) {
    let fixture = fixture(broker).await;
    let addr = fixture.addr;
    let id = colored_terminal(addr, "resume").await;

    // First attach: learn the end offset, then count live bytes as a viewer does.
    let mut ws = connect(addr, &id, "&replay_protocol=2&replay_encoding=deflate-raw").await;
    let header = json(&next(&mut ws).await);
    let stream = header["terminal_frame"]["stream"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(stream.len(), 16);
    let mut offset = header["terminal_frame"]["end_offset"].as_u64().unwrap();
    let _ = binary(next(&mut ws).await);
    ws.send(WsMessage::Text("first\n".into())).await.unwrap();
    let mut seen = String::new();
    while !seen.contains("first") {
        if let WsMessage::Binary(bytes) = next(&mut ws).await {
            offset += bytes.len() as u64;
            seen.push_str(&String::from_utf8_lossy(&bytes));
        }
    }
    ws.close(None).await.unwrap();

    // Output produced while no viewer is attached.
    send_input(addr, &id, "while-away\n").await;
    let mut full = full_replay(addr, &id).await;
    while !String::from_utf8_lossy(&full).contains("while-away\r\n") {
        tokio::time::sleep(Duration::from_millis(20)).await;
        full = full_replay(addr, &id).await;
    }

    // Reconnect from the counted offset: exactly the missed bytes, appended.
    let mut ws = connect(
        addr,
        &id,
        &format!("&replay_protocol=2&replay_max_bytes=8192&resume_from={stream}:{offset}"),
    )
    .await;
    let header = json(&next(&mut ws).await);
    let missed = &full[usize::try_from(offset).unwrap()..];
    assert_eq!(
        header,
        serde_json::json!({"terminal_frame": {"kind": "resume", "encoding": "identity", "bytes": missed.len(), "stream": stream, "end_offset": full.len()}}),
        "broker={broker}"
    );
    assert_eq!(binary(next(&mut ws).await), missed, "broker={broker}");
    ws.send(WsMessage::Text("after-resume\n".into()))
        .await
        .unwrap();
    live_output_until(&mut ws, "after-resume").await;
    let _ = ws.close(None).await;

    // An offset outside the window (or larger than the cap allows) gets a full capped replay.
    let mut ws = connect(
        addr,
        &id,
        &format!("&replay_protocol=2&replay_max_bytes=4096&resume_from={stream}:0"),
    )
    .await;
    let header = json(&next(&mut ws).await);
    assert_eq!(
        header["terminal_frame"]["kind"], "replay",
        "broker={broker}"
    );
    assert!(header["terminal_frame"]["bytes"].as_u64().unwrap() <= 4096);
}

#[tokio::test]
async fn hs2_k93qha_a_reconnect_resumes_with_only_missed_output() {
    resume_appends_only_missed_output(false).await;
}

#[tokio::test]
async fn hs2_k93qha_a_broker_reconnect_resumes_with_only_missed_output() {
    resume_appends_only_missed_output(true).await;
}
