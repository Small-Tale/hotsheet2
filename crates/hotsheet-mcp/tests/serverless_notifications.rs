//! HS2-FYZ5S9: with no live server for its store, the shipped serverless `hotsheet-mcp --path`
//! process delivers ticket transitions for its own writes, the same messages a
//! `hotsheet-cli` write sends. Reads send nothing, and a live server's registration hands
//! routing back to the server so nothing is sent twice.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc::Receiver;
use std::time::Duration;

#[path = "support/git.rs"]
mod git_fixture;

use hotsheet_ticketing::StoreMetadata;
use serde_json::{Value, json};

/// A request the loopback ntfy server received: its request line and `Title` header.
struct Received {
    line: String,
    title: String,
}

/// A loopback HTTP server that answers every request like ntfy and reports what it got, so
/// the shim's live HTTP transport runs without the internet.
fn loopback() -> (String, Receiver<Received>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let mut title = String::new();
            let mut length = 0;
            loop {
                let mut header = String::new();
                reader.read_line(&mut header).unwrap();
                let header = header.trim_end();
                if header.is_empty() {
                    break;
                }
                let (name, value) = header.split_once(':').unwrap();
                match name.trim().to_ascii_lowercase().as_str() {
                    "title" => title = value.trim().to_string(),
                    "content-length" => length = value.trim().parse().unwrap(),
                    _ => {}
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let reply = r#"{"id":"n1"}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                reply.len()
            )
            .unwrap();
            let _ = tx.send(Received {
                line: line.trim_end().to_string(),
                title,
            });
        }
    });
    (base, rx)
}

fn assert_quiet(received: &Receiver<Received>, why: &str) {
    if let Ok(extra) = received.recv_timeout(Duration::from_millis(800)) {
        panic!("{why}: unexpected {} ({})", extra.line, extra.title);
    }
}

#[test]
fn hs2_fyz5s9_serverless_mcp_writes_notify_and_defer_to_a_live_server() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let store = root.path().join("demo.hs2");
    git_fixture::init(&store, &StoreMetadata::new("HS"));
    let (base, received) = loopback();
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(
        home.join("notifications.json"),
        json!({
            "version": 1,
            "channels": [{"id": "push", "notifier": "ntfy",
                          "settings": {"server": base, "topic": "hs-test"}}],
            "rules": [{"id": "tickets", "channels": ["push"],
                       "events": ["ticket.status_changed", "ticket.blocked"]}],
        })
        .to_string(),
    )
    .unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_hotsheet-mcp"))
        .arg("--path")
        .arg(&store)
        .current_dir(root.path())
        .env("HOTSHEET_HOME", &home)
        .env_remove("HOTSHEET_STORE")
        .env_remove("HOTSHEET_SERVER")
        .env_remove("HOTSHEET_SECRET")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut next_id = 0;
    // The closure owns the pipes, so dropping it closes stdin and ends the session.
    let mut tool = move |name: &str, mut arguments: Value| -> Value {
        next_id += 1;
        if name != "hotsheet_query" && name != "hotsheet_get" {
            arguments["actor_role"] = json!("human");
            arguments["actor_id"] = json!("mcp-test");
        }
        let request = json!({"jsonrpc":"2.0","id":next_id,"method":"tools/call",
                             "params":{"name":name,"arguments":arguments}});
        writeln!(stdin, "{request}").unwrap();
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        let reply: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(reply["id"], next_id, "{reply}");
        let text = reply["result"]["content"][0]["text"].as_str().unwrap();
        serde_json::from_str(text).unwrap_or_else(|_| json!({"error": text}))
    };

    let created = tool("hotsheet_create", json!({"title": "Ship it"}));
    let slug = created["slug"].as_str().unwrap().to_owned();
    assert_quiet(&received, "creating a Not Started ticket");

    tool("hotsheet_update", json!({"id": slug, "status": "started"}));
    let sent = received.recv_timeout(Duration::from_secs(20)).unwrap();
    assert_eq!(sent.line, "POST /hs-test HTTP/1.1");
    assert_eq!(sent.title, format!("{slug} is now Started"));

    // Reads send nothing; a blocked edit sends exactly one message.
    tool("hotsheet_query", json!({}));
    tool("hotsheet_get", json!({"id": slug}));
    tool(
        "hotsheet_update",
        json!({"id": slug, "blocked_reason": "waiting on review"}),
    );
    let sent = received.recv_timeout(Duration::from_secs(20)).unwrap();
    assert_eq!(sent.title, format!("{slug} is blocked"));
    assert_quiet(&received, "one message per transition");

    // A live server hosting the store owns routing: the MCP shim stays quiet.
    let instances = home.join("instances");
    std::fs::create_dir_all(&instances).unwrap();
    std::fs::write(
        instances.join("live.json"),
        json!({"pid": std::process::id(), "url": "http://127.0.0.1:1", "store_path": store, "started_at": "now"}).to_string(),
    )
    .unwrap();
    let done = tool(
        "hotsheet_update",
        json!({"id": slug, "status": "completed"}),
    );
    assert_eq!(done["status"], "completed", "{done}");
    assert_quiet(&received, "the server would send this");

    drop(tool);
    child.wait().unwrap();
}
