//! HS2-4BG2QN: the shipped serverless `hotsheet-mcp --path` process asks the running host
//! to sync a store after each tool call that committed to it with a headless push, the same
//! `POST /sync/kick` a `hotsheet-cli` write sends (HS2-D9ZGAE). With no host, or a host
//! that refuses, the tool result carries a local-only notice naming `hotsheet-cli sync`
//! and stays a success. The host is a loopback fake registered under a temporary
//! `HOTSHEET_HOME`, so the test never reaches a real host.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::time::Duration;

#[path = "support/git.rs"]
mod git_fixture;

use hotsheet_ticketing::StoreMetadata;
use serde_json::{Value, json};

const SECRET: &str = "synthetic-host-secret";

/// One request the fake host received.
struct Received {
    line: String,
    secret: String,
    body: Value,
}

/// A loopback host that answers `POST /sync/kick` with 202, or with 409 and a JSON error
/// while `refuse` is set, and reports every request.
fn fake_host(refuse: Arc<AtomicBool>) -> (String, Receiver<Received>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let mut secret = String::new();
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
                    "x-hotsheet-secret" => secret = value.trim().to_string(),
                    "content-length" => length = value.trim().parse().unwrap(),
                    _ => {}
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let (status, reply) = if refuse.load(Ordering::SeqCst) {
                (
                    "409 Conflict",
                    r#"{"error":"store is not registered with this host"}"#,
                )
            } else {
                ("202 Accepted", "{}")
            };
            write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                reply.len()
            )
            .unwrap();
            let _ = tx.send(Received {
                line: line.trim_end().to_string(),
                secret,
                body: serde_json::from_slice(&body).unwrap_or(Value::Null),
            });
        }
    });
    (base, rx)
}

fn git(dir: &Path, args: &[&str]) {
    let output = hotsheet_ticketing::git::command_in(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "synthetic Git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_no_kick(received: &Receiver<Received>, why: &str) {
    if let Ok(extra) = received.recv_timeout(Duration::from_millis(500)) {
        panic!("{why}: unexpected {}", extra.line);
    }
}

#[test]
fn hs2_4bg2qn_serverless_mcp_writes_kick_the_host_or_report_local_only() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let store = root.path().join("demo.hs2");
    let remote = root.path().join("remote.git");
    std::fs::create_dir_all(&home).unwrap();
    git_fixture::init(&store, &StoreMetadata::new("HS"));
    std::fs::create_dir_all(&remote).unwrap();
    git(&remote, &["init", "-q", "--bare"]);
    git(
        &store,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&store, &["push", "-q", "-u", "origin", "HEAD"]);

    let mut child = Command::new(env!("CARGO_BIN_EXE_hotsheet-mcp"))
        .arg("--path")
        .arg(&store)
        .current_dir(root.path())
        .env("HOTSHEET_HOME", &home)
        .env("HOTSHEET_NATIVE_SECRETS", "deny")
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
        assert_eq!(reply["result"]["isError"], false, "{reply}");
        reply["result"].clone()
    };
    let notices = |result: &Value| -> Vec<String> {
        result["content"].as_array().unwrap()[1..]
            .iter()
            .map(|item| item["text"].as_str().unwrap().to_owned())
            .collect()
    };
    let store_name = store.file_name().unwrap().to_str().unwrap().to_owned();

    // No host: the write succeeds and says its commits may be local only.
    let created = tool("hotsheet_create", json!({"title": "Synthetic MCP write"}));
    let ticket: Value =
        serde_json::from_str(created["content"][0]["text"].as_str().unwrap()).unwrap();
    let slug = ticket["slug"].as_str().unwrap().to_owned();
    let local_only = notices(&created);
    assert_eq!(local_only.len(), 1, "{created}");
    assert!(
        local_only[0].contains("may be local only"),
        "{local_only:?}"
    );
    assert!(local_only[0].contains("no Hot Sheet host is running"));
    assert!(local_only[0].contains("hotsheet-cli -C "), "{local_only:?}");
    assert!(local_only[0].contains(&format!("{store_name} sync`")));

    // A read commits nothing and carries no notice.
    let read = tool("hotsheet_get", json!({"id": slug}));
    assert!(notices(&read).is_empty(), "{read}");

    // A live machine host under this HOTSHEET_HOME accepts an authenticated kick.
    let refuse = Arc::new(AtomicBool::new(false));
    let (url, received) = fake_host(Arc::clone(&refuse));
    let anchor = hotsheet_cli::external_launch::machine_host_store(&home);
    std::fs::create_dir_all(&anchor).unwrap();
    let record = hotsheet_cli::external_launch::instance_path(&home, &anchor);
    std::fs::create_dir_all(record.parent().unwrap()).unwrap();
    std::fs::write(
        &record,
        json!({"pid": std::process::id(), "url": url, "secret": SECRET, "store_path": anchor})
            .to_string(),
    )
    .unwrap();

    let updated = tool("hotsheet_update", json!({"id": slug, "status": "started"}));
    assert!(notices(&updated).is_empty(), "{updated}");
    let kick = received.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(kick.line, "POST /sync/kick HTTP/1.1");
    assert_eq!(kick.secret, SECRET);
    assert_eq!(
        Path::new(kick.body["store"].as_str().unwrap()),
        store.canonicalize().unwrap()
    );
    assert_no_kick(&received, "one kick per committed store");

    tool("hotsheet_query", json!({}));
    assert_no_kick(&received, "a read never kicks");

    // A host that refuses the store is a local-only notice, never a failure.
    refuse.store(true, Ordering::SeqCst);
    let refused = tool(
        "hotsheet_update",
        json!({"id": slug, "title": "Synthetic MCP write, renamed"}),
    );
    received.recv_timeout(Duration::from_secs(10)).unwrap();
    let refused_notices = notices(&refused);
    assert_eq!(refused_notices.len(), 1, "{refused}");
    assert!(
        refused_notices[0]
            .contains("did not accept a sync request: store is not registered with this host"),
        "{refused_notices:?}"
    );
    assert!(refused_notices[0].contains(&format!("{store_name} sync`")));

    drop(tool);
    child.wait().unwrap();
}
