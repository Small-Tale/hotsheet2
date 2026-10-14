//! End-to-end (HS2-E4J72D): the real `hotsheet-mcp` binary, started serverless with
//! `--path` and no Hot Sheet terminal environment, finds its store's server in the
//! machine-local instance registry and registers its presence lease there; it releases the
//! lease when stdin closes. A raw loopback listener stands in for the server's two presence
//! routes so this crate does not depend on the server.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use hotsheet_ticketing::{FsStore, StoreMetadata};

/// Accept connections and report each request's `(method path, secret header, body)`.
fn fake_server(listener: TcpListener, seen: mpsc::Sender<(String, String, String)>) {
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request_line = String::new();
            if reader.read_line(&mut request_line).is_err() {
                continue;
            }
            let (mut secret, mut length) = (String::new(), 0usize);
            loop {
                let mut header = String::new();
                if reader.read_line(&mut header).unwrap_or(0) == 0 || header.trim().is_empty() {
                    break;
                }
                let (name, value) = header.split_once(':').unwrap_or((&header, ""));
                match name.trim().to_ascii_lowercase().as_str() {
                    "x-hotsheet-secret" => secret = value.trim().to_owned(),
                    "content-length" => length = value.trim().parse().unwrap_or(0),
                    _ => {}
                }
            }
            let mut body = vec![0; length];
            let _ = reader.read_exact(&mut body);
            let mut stream = stream;
            let _ = stream.write_all(
                b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{}",
            );
            let parts: Vec<&str> = request_line.split_whitespace().collect();
            let _ = seen.send((
                format!("{} {}", parts[0], parts[1]),
                secret,
                String::from_utf8_lossy(&body).into_owned(),
            ));
        }
    });
}

#[test]
fn serverless_shim_registers_presence_with_the_registry_server_and_releases_on_eof() {
    let home = tempfile::tempdir().unwrap();
    let store_dir = tempfile::tempdir().unwrap();
    FsStore::init(store_dir.path(), &StoreMetadata::new("HS")).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (tx, rx) = mpsc::channel();
    fake_server(listener, tx);

    let instances = home.path().join("instances");
    std::fs::create_dir(&instances).unwrap();
    std::fs::write(
        instances.join("deadbeef.json"),
        serde_json::json!({
            "pid": std::process::id(),
            "url": url,
            "secret": "registry-secret",
            "store_path": store_dir.path().display().to_string(),
            "index_path": "",
            "started_at": "2026-10-13T00:00:00Z",
        })
        .to_string(),
    )
    .unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_hotsheet-mcp"))
        .arg("--path")
        .arg(store_dir.path())
        .env("HOTSHEET_HOME", home.path())
        .env_remove("HOTSHEET_SERVER")
        .env_remove("HOTSHEET_SECRET")
        .env_remove("HOTSHEET_TERMINAL_ID")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    writeln!(
        stdin,
        r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{}}}}"#
    )
    .unwrap();
    stdin.flush().unwrap();

    let (route, secret, body) = rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(route, "POST /mcp-presence");
    assert_eq!(secret, "registry-secret");
    let body: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(body["pid"], child.id());
    assert!(body["terminal_id"].is_null());
    let lease = body["lease_id"].as_str().unwrap().to_owned();

    drop(stdin);
    assert!(child.wait().unwrap().success());
    let (route, secret, _) = rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(route, format!("DELETE /mcp-presence/{lease}"));
    assert_eq!(secret, "registry-secret");
}

#[test]
fn serverless_shim_without_a_registered_server_stays_silent() {
    let home = tempfile::tempdir().unwrap();
    let store_dir = tempfile::tempdir().unwrap();
    FsStore::init(store_dir.path(), &StoreMetadata::new("HS")).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_hotsheet-mcp"))
        .arg("--path")
        .arg(store_dir.path())
        .env("HOTSHEET_HOME", home.path())
        .env_remove("HOTSHEET_SERVER")
        .env_remove("HOTSHEET_SECRET")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    writeln!(
        stdin,
        r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{}}}}"#
    )
    .unwrap();
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("\"id\":1"));
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// HS2-HNXE38 reopen: a Hot Sheet terminal survives a server restart, so its route-back
/// `HOTSHEET_SERVER` can name a dead port. The shim's first heartbeat fails there and the
/// lease moves to the live server the registry records, keeping the terminal id.
#[test]
fn a_stale_terminal_route_back_falls_back_to_the_registry_server() {
    let home = tempfile::tempdir().unwrap();
    let store_dir = tempfile::tempdir().unwrap();
    FsStore::init(store_dir.path(), &StoreMetadata::new("HS")).unwrap();
    let dead = {
        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        format!("http://{}", probe.local_addr().unwrap())
    }; // dropped: nothing listens there any more
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (tx, rx) = mpsc::channel();
    fake_server(listener, tx);
    let instances = home.path().join("instances");
    std::fs::create_dir(&instances).unwrap();
    std::fs::write(
        instances.join("live.json"),
        serde_json::json!({
            "pid": std::process::id(),
            "url": url,
            "secret": "live-secret",
            "store_path": store_dir.path().display().to_string(),
            "index_path": "",
            "started_at": "2026-10-14T00:00:00Z",
        })
        .to_string(),
    )
    .unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_hotsheet-mcp"))
        .arg("--path")
        .arg(store_dir.path())
        .env("HOTSHEET_HOME", home.path())
        .env("HOTSHEET_SERVER", &dead)
        .env("HOTSHEET_SECRET", "dead-secret")
        .env("HOTSHEET_TERMINAL_ID", "term-1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    writeln!(
        stdin,
        r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{}}}}"#
    )
    .unwrap();
    stdin.flush().unwrap();

    let (route, secret, body) = rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(route, "POST /mcp-presence");
    assert_eq!(secret, "live-secret");
    let body: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(body["terminal_id"], "term-1");
    let lease = body["lease_id"].as_str().unwrap().to_owned();

    drop(stdin);
    assert!(child.wait().unwrap().success());
    let (route, secret, _) = rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(route, format!("DELETE /mcp-presence/{lease}"));
    assert_eq!(secret, "live-secret");
}
