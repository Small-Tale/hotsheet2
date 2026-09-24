//! Live terminal-attach WebSocket E2E (HS2-XTTTMV): boot a real server, open a `cat`
//! terminal over HTTP, attach over a WebSocket, and prove the stream — the scrollback replays,
//! input the viewer sends is forwarded to the PTY, and `cat`'s echo streams back as output.

use futures_util::{SinkExt, StreamExt};
use hotsheet_server::{AppState, app};
use hotsheet_ticketing::{FsStore, StoreMetadata};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message as WsMessage;

const SECRET: &str = "test-secret";

async fn wait_for_terminal_text(base: String, id: String, needle: &'static str) {
    tokio::task::spawn_blocking(move || {
        for _ in 0..100 {
            let response = ureq::get(&format!("{base}/terminals/{id}"))
                .set("x-hotsheet-secret", SECRET)
                .call()
                .unwrap();
            let value: serde_json::Value =
                serde_json::from_str(&response.into_string().unwrap()).unwrap();
            if value["scrollback"]
                .as_str()
                .unwrap_or_default()
                .contains(needle)
            {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("terminal never produced readiness marker {needle}");
    })
    .await
    .unwrap();
}

fn workspace_binary(name: &str) -> std::path::PathBuf {
    if let Some(path) = std::env::var_os(format!("CARGO_BIN_EXE_{name}")) {
        return path.into();
    }
    let exe = std::env::current_exe().expect("current test executable");
    let profile = exe
        .parent()
        .and_then(std::path::Path::parent)
        .expect("target profile directory");
    profile.join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
}

#[tokio::test]
async fn fake_agent_permission_round_trip_in_a_terminal_persists_always_rule() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    let rules = dir.path().join("permission-rules.json");
    let plugin_root = tempfile::tempdir().unwrap();
    let plugin = plugin_root.path().join("fake-permission");
    std::fs::create_dir(&plugin).unwrap();
    std::fs::write(plugin.join("instructions.md"), "Fake permission agent\n").unwrap();
    let fake_agent = workspace_binary("hs-fake-agent");
    assert!(
        fake_agent.is_file(),
        "build the workspace hs-fake-agent binary at {}",
        fake_agent.display()
    );
    std::fs::write(
        plugin.join("manifest.toml"),
        format!(
            r#"
id = "fake-permission"
display_name = "Fake Permission"
product_name = "Fake Permission Agent"
tier = "cli-agent"
[instructions]
target = "AGENTS.md"
section = "instructions.md"
[mcp]
target = ".mcp.json"
format = "claude-json"
server_name = "hotsheet"
command = "hotsheet-mcp"
args = ["--path", "{{store}}"]
[launch]
program = {fake_agent:?}
args = ["--permission", "Bash", "rm important"]
"#
        ),
    )
    .unwrap();

    let state = AppState::new(store, SECRET.into())
        .unwrap()
        .with_permission_rules(&rules)
        .with_plugin_dirs(vec![plugin_root.path().to_path_buf()]);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{addr}");
    state.set_terminal_server_url(base.clone());
    tokio::spawn(async move { axum::serve(listener, app(state)).await.unwrap() });

    let output = tokio::task::spawn_blocking(move || {
        let opened = ureq::post(&format!("{base}/terminals"))
            .set("x-hotsheet-secret", SECRET)
            .set("content-type", "application/json")
            .send_string(r#"{"connect":"fake-permission","id":"perm-agent"}"#)
            .unwrap();
        assert_eq!(opened.status(), 200);

        let pending_id = loop {
            let value: serde_json::Value = serde_json::from_str(
                &ureq::get(&format!("{base}/permissions"))
                    .set("x-hotsheet-secret", SECRET)
                    .call()
                    .unwrap()
                    .into_string()
                    .unwrap(),
            )
            .unwrap();
            if let Some(id) = value
                .as_array()
                .and_then(|a| a.first())
                .and_then(|p| p["id"].as_u64())
            {
                break id;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        let answer = ureq::post(&format!("{base}/permissions/{pending_id}"))
            .set("x-hotsheet-secret", SECRET)
            .set("content-type", "application/json")
            .send_string(r#"{"decision":"allow","scope":"always"}"#)
            .unwrap();
        assert_eq!(answer.status(), 200);

        for _ in 0..100 {
            let value: serde_json::Value = serde_json::from_str(
                &ureq::get(&format!("{base}/terminals/perm-agent"))
                    .set("x-hotsheet-secret", SECRET)
                    .call()
                    .unwrap()
                    .into_string()
                    .unwrap(),
            )
            .unwrap();
            let output = value["scrollback"].as_str().unwrap_or_default().to_string();
            if output.contains("permission:allow") {
                return output;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        String::new()
    })
    .await
    .unwrap();
    assert!(output.contains("permission:allow"), "{output}");
    let saved = std::fs::read_to_string(rules).unwrap();
    assert!(saved.contains("rm important"), "{saved}");
}

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
            .send_string(
                r#"{"command":"/bin/sh","args":["-c","stty -echo; printf 'DIRECT-READY\\n'; exec cat"]}"#,
            )
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp.into_string().unwrap()).unwrap();
        v["id"].as_str().unwrap().to_string()
    })
    .await
    .unwrap();
    wait_for_terminal_text(format!("http://{addr}"), id.clone(), "DIRECT-READY").await;

    // Write immediately before attach. The PTY drain can publish this concurrently with the
    // snapshot/subscription handoff; it must land in exactly one side of that boundary.
    let handoff_base = format!("http://{addr}");
    let handoff_id = id.clone();
    tokio::task::spawn_blocking(move || {
        ureq::post(&format!("{handoff_base}/terminals/{handoff_id}/input"))
            .set("x-hotsheet-secret", SECRET)
            .set("content-type", "application/json")
            .send_string(r#"{"data":"direct-handoff-marker\n"}"#)
            .unwrap();
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
    assert_eq!(
        seen.matches("direct-handoff-marker").count(),
        1,
        "concurrent direct-server handoff must deliver its marker exactly once: {seen:?}"
    );

    // A bad secret is rejected at the upgrade (no stream).
    let bad = format!("ws://{addr}/terminals/{id}/attach?secret=wrong");
    assert!(
        tokio_tungstenite::connect_async(bad).await.is_err(),
        "a wrong secret must be rejected"
    );
}

#[tokio::test]
async fn bounded_attach_replay_starts_after_a_truncated_ansi_sequence() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    let state = AppState::new(store, SECRET.into()).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app(state)).await.unwrap();
    });

    let base = format!("http://{addr}");
    let open_base = base.clone();
    let id = tokio::task::spawn_blocking(move || {
        let fill = hotsheet_terminals::SCROLLBACK_BYTES - 2;
        let script = format!(
            "printf 'prefix\\033[38;5;61m'; head -c {fill} /dev/zero | LC_ALL=C tr '\\000' A; sleep 5"
        );
        let body = serde_json::json!({
            "command": "/bin/sh",
            "args": ["-c", script],
        })
        .to_string();
        let response = ureq::post(&format!("{open_base}/terminals"))
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

    let wait_base = base.clone();
    let wait_id = id.clone();
    tokio::task::spawn_blocking(move || {
        for _ in 0..200 {
            let response = ureq::get(&format!("{wait_base}/terminals/{wait_id}"))
                .set("x-hotsheet-secret", SECRET)
                .call()
                .unwrap();
            let value: serde_json::Value =
                serde_json::from_str(&response.into_string().unwrap()).unwrap();
            if value["scrollback"].as_str().unwrap_or_default().len()
                >= hotsheet_terminals::SCROLLBACK_BYTES - 2
            {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("terminal never filled the bounded replay");
    })
    .await
    .unwrap();

    let url = format!("ws://{addr}/terminals/{id}/attach?secret={SECRET}");
    let (mut ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    let first = tokio::time::timeout(std::time::Duration::from_secs(5), ws.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let WsMessage::Binary(replay) = first else {
        panic!("first attach frame was not the binary replay: {first:?}");
    };
    assert_eq!(replay.len(), hotsheet_terminals::SCROLLBACK_BYTES - 2);
    assert!(
        replay.iter().all(|byte| *byte == b'A'),
        "replay exposed a truncated ANSI parameter prefix: {:?}",
        &replay[..replay.len().min(16)]
    );
}

#[tokio::test]
async fn empty_attach_replay_is_an_explicit_binary_boundary() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    let state = AppState::new(store, SECRET.into()).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app(state)).await.unwrap();
    });

    let base = format!("http://{addr}");
    let id = tokio::task::spawn_blocking(move || {
        let response = ureq::post(&format!("{base}/terminals"))
            .set("x-hotsheet-secret", SECRET)
            .set("content-type", "application/json")
            .send_string(r#"{"command":"/bin/sh","args":["-c","sleep 2"]}"#)
            .unwrap();
        let value: serde_json::Value =
            serde_json::from_str(&response.into_string().unwrap()).unwrap();
        value["id"].as_str().unwrap().to_string()
    })
    .await
    .unwrap();

    let url = format!("ws://{addr}/terminals/{id}/attach?secret={SECRET}");
    let (mut ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    let first = tokio::time::timeout(std::time::Duration::from_secs(5), ws.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(matches!(first, WsMessage::Binary(replay) if replay.is_empty()));
}

/// Boot a server + open a `cat` terminal; returns (tempdir, addr, terminal id).
async fn boot_with_cat() -> (tempfile::TempDir, std::net::SocketAddr, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    let state = AppState::new(store, SECRET.into()).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = app(state);
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
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
    (dir, addr, id)
}

#[tokio::test]
async fn a_size_claim_resizes_the_pty_and_broadcasts_the_decision() {
    let (_d, addr, id) = boot_with_cat().await;
    let url = format!("ws://{addr}/terminals/{id}/attach?secret={SECRET}");
    let (mut ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();

    // A focused viewport claims 100x40 (the terminal spawned at 80x24). The server feeds the
    // arbiter, resizes the PTY, and broadcasts the decision back as a Text control frame.
    ws.send(WsMessage::Text(
        r#"{"resize":{"viewer_id":"v1","cols":100,"rows":40,"focus":true}}"#.into(),
    ))
    .await
    .unwrap();

    let got = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while let Some(Ok(msg)) = ws.next().await {
            if let WsMessage::Text(t) = msg {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&t) {
                    if let Some(size) = v.get("pty_size") {
                        return Some((
                            size["cols"].as_u64().unwrap(),
                            size["rows"].as_u64().unwrap(),
                            v["driven_by"].as_str().unwrap_or("").to_string(),
                        ));
                    }
                }
            }
        }
        None
    })
    .await
    .unwrap_or(None);

    assert_eq!(
        got,
        Some((100, 40, "v1".to_string())),
        "the size claim should resize the PTY and broadcast {{pty_size, driven_by}}"
    );
}

/// The live WS attach must also work when terminals live in the **detached broker**
/// (HS2-ERT00F item 4): the server bridges the WebSocket to a streaming broker connection, so
/// the scrollback replays, input the viewer sends reaches the broker-hosted PTY, `cat`'s echo
/// streams back, and a size claim routes through to the broker's arbiter.
#[tokio::test]
async fn broker_mode_attach_streams_through_the_broker() {
    use hotsheet_server::terminal_broker::TerminalBroker;
    use std::sync::Arc;

    // A broker (stand-in for the separate process) on a temp socket.
    let bdir = tempfile::tempdir().unwrap();
    let sock = bdir.path().join("b.sock");
    let listener = tokio::net::UnixListener::bind(&sock).unwrap();
    tokio::spawn(hotsheet_terminals::serve_broker(
        listener,
        "proj".into(),
        Arc::new(hotsheet_terminals::TerminalManager::new()),
    ));

    // A server whose terminals route through that broker.
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    let state = AppState::new(store, SECRET.into())
        .unwrap()
        .with_terminal_broker_at(TerminalBroker::at(&sock, "proj"));
    let tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = tcp.local_addr().unwrap();
    let app = app(state);
    tokio::spawn(async move {
        axum::serve(tcp, app).await.unwrap();
    });

    // Open a `cat` terminal over HTTP — it lives in the broker.
    let base = format!("http://{addr}");
    let id = tokio::task::spawn_blocking(move || {
        let resp = ureq::post(&format!("{base}/terminals"))
            .set("x-hotsheet-secret", SECRET)
            .set("content-type", "application/json")
            .send_string(
                r#"{"command":"/bin/sh","args":["-c","stty -echo; printf 'BROKER-READY\\n'; exec cat"],"id":"bws"}"#,
            )
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp.into_string().unwrap()).unwrap();
        v["id"].as_str().unwrap().to_string()
    })
    .await
    .unwrap();
    assert_eq!(id, "bws");
    wait_for_terminal_text(format!("http://{addr}"), id.clone(), "BROKER-READY").await;

    let handoff_base = format!("http://{addr}");
    let handoff_id = id.clone();
    tokio::task::spawn_blocking(move || {
        ureq::post(&format!("{handoff_base}/terminals/{handoff_id}/input"))
            .set("x-hotsheet-secret", SECRET)
            .set("content-type", "application/json")
            .send_string(r#"{"data":"broker-handoff-marker\n"}"#)
            .unwrap();
    })
    .await
    .unwrap();

    // Attach over the WebSocket — bridged to the broker stream.
    let url = format!("ws://{addr}/terminals/{id}/attach?secret={SECRET}");
    let (mut ws, _resp) = tokio_tungstenite::connect_async(url)
        .await
        .expect("broker-mode ws attach connects");

    // A size claim routes through to the broker's arbiter, which broadcasts the decision back.
    ws.send(WsMessage::Text(
        r#"{"resize":{"viewer_id":"v1","cols":90,"rows":30,"focus":true}}"#.into(),
    ))
    .await
    .unwrap();
    // Input streams to the broker-hosted PTY; `cat` echoes it back as live output.
    ws.send(WsMessage::Text("broker-echo\n".into()))
        .await
        .unwrap();

    let (mut saw_echo, mut saw_size, mut seen_output) = (false, false, String::new());
    let done = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while let Some(Ok(msg)) = ws.next().await {
            match msg {
                WsMessage::Binary(b) => seen_output.push_str(&String::from_utf8_lossy(&b)),
                WsMessage::Text(t) => {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&t) {
                        if v.get("pty_size").is_some() {
                            saw_size = v["pty_size"]["cols"].as_u64() == Some(90)
                                && v["driven_by"].as_str() == Some("v1");
                        }
                    }
                }
                WsMessage::Close(_) => break,
                _ => {}
            }
            saw_echo = seen_output.contains("broker-echo");
            if saw_echo && saw_size {
                return true;
            }
        }
        false
    })
    .await
    .unwrap_or(false);

    assert!(
        done && saw_echo && saw_size,
        "broker attach should stream cat's echo AND route the size claim through \
         (echo={saw_echo}, size={saw_size})"
    );
    assert_eq!(
        seen_output.matches("broker-handoff-marker").count(),
        1,
        "concurrent broker handoff must deliver its marker exactly once: {seen_output:?}"
    );
}

/// A custom shell-command button must run the command AND keep the terminal open so its output stays
/// visible instead of exiting the instant the command finishes (HS2-2BKPGK).
#[tokio::test]
async fn shell_command_terminal_runs_the_command_and_stays_open() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    let state = AppState::new(store, SECRET.into()).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{addr}");
    tokio::spawn(async move { axum::serve(listener, app(state)).await.unwrap() });
    let (scrollback, alive) = tokio::task::spawn_blocking(move || {
        let opened = ureq::post(&format!("{base}/terminals"))
            .set("x-hotsheet-secret", SECRET)
            .set("content-type", "application/json")
            .send_string(r#"{"shell_command":"echo hs2-shell-marker","id":"sc1"}"#)
            .unwrap();
        assert_eq!(opened.status(), 200);
        let read = || -> (String, bool) {
            let value: serde_json::Value = serde_json::from_str(
                &ureq::get(&format!("{base}/terminals/sc1"))
                    .set("x-hotsheet-secret", SECRET)
                    .call()
                    .unwrap()
                    .into_string()
                    .unwrap(),
            )
            .unwrap();
            (
                value["scrollback"].as_str().unwrap_or_default().to_string(),
                value["alive"].as_bool().unwrap_or(false),
            )
        };
        for _ in 0..300 {
            if read().0.contains("hs2-shell-marker") {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        // Give the re-exec'd interactive shell a moment, then confirm the terminal is still open.
        std::thread::sleep(std::time::Duration::from_millis(200));
        read()
    })
    .await
    .unwrap();
    assert!(
        scrollback.contains("hs2-shell-marker"),
        "command output missing: {scrollback:?}"
    );
    assert!(
        alive,
        "the shell-command terminal exited instead of staying open: {scrollback:?}"
    );
}
