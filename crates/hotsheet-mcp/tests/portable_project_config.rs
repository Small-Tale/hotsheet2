//! A committed MCP entry resolves the code project, including from a nested directory.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};

use hotsheet_ticketing::{FsStore, StoreMetadata};

#[test]
fn portable_project_entry_opens_its_linked_store_and_lists_tools() {
    let root = tempfile::tempdir().unwrap();
    let store = root.path().join("ticket-store");
    FsStore::init(&store, &StoreMetadata::new("HS")).unwrap();
    let project = root.path().join("project");
    let nested = project.join("src");
    std::fs::create_dir_all(project.join(".hotsheet2")).unwrap();
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(
        project.join(".hotsheet2/store"),
        format!("{}\n", store.display()),
    )
    .unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_hotsheet-mcp"))
        .arg("--path")
        .arg(".")
        .current_dir(&nested)
        .env("HOTSHEET_HOME", root.path().join("home"))
        .env_remove("HOTSHEET_STORE")
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
        r#"{{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{{}}}}"#
    )
    .unwrap();
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let reply: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(reply["id"], 1);
    assert!(reply["result"]["tools"].as_array().is_some());
}

#[test]
fn project_entry_opens_source_free_checkout_and_reopens_after_relink_and_restart() {
    fn serve_project(listener: std::net::TcpListener, bodies: &[&str]) -> Vec<String> {
        let mut paths = Vec::new();
        for body in bodies {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 4096];
            loop {
                let count = stream.read(&mut buffer).unwrap();
                assert!(count > 0, "client closed before sending the request");
                request.extend_from_slice(&buffer[..count]);
                let Some(header_end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n")
                else {
                    continue;
                };
                let headers = String::from_utf8_lossy(&request[..header_end]);
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .and_then(|value| value.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                if request.len() >= header_end + 4 + length {
                    break;
                }
            }
            paths.push(
                String::from_utf8_lossy(&request)
                    .lines()
                    .next()
                    .unwrap()
                    .to_owned(),
            );
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
        }
        paths
    }
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let bootstrap = home.join("server-bootstrap.hs2");
    FsStore::init(&bootstrap, &StoreMetadata::new("HS")).unwrap();
    let project = root.path().join("project");
    let nested = project.join("src");
    std::fs::create_dir_all(&nested).unwrap();
    let registry =
        hotsheet_ticketing::checkouts::CheckoutRegistry::new(home.join("checkouts.json"));
    registry
        .register_sources(&project, None, None, vec![], None)
        .unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let instance = hotsheet_cli::external_launch::instance_path(&home, &bootstrap);
    std::fs::create_dir_all(instance.parent().unwrap()).unwrap();
    std::fs::write(
        &instance,
        serde_json::json!({
            "pid":std::process::id(),
            "url":format!("http://{address}"),
            "secret":"synthetic-secret",
            "store_path":bootstrap.canonicalize().unwrap(),
        })
        .to_string(),
    )
    .unwrap();
    let server = std::thread::spawn(move || {
        serve_project(
            listener,
            &[
                r#"{"checkout":{"id":"code-project"}}"#,
                "[]",
                r#"{"checkout":{"id":"code-project"}}"#,
                "[]",
            ],
        )
    });
    let mut child = Command::new(env!("CARGO_BIN_EXE_hotsheet-mcp"))
        .args(["--project", "."])
        .current_dir(&nested)
        .env("HOTSHEET_HOME", &home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let query = |id| {
        serde_json::json!({
            "jsonrpc":"2.0", "id":id, "method":"tools/call",
            "params":{"name":"hotsheet_query","arguments":{}}
        })
    };
    writeln!(stdin, "{}", query(1)).unwrap();
    let mut first = String::new();
    stdout.read_line(&mut first).unwrap();
    if first.is_empty() {
        drop(stdin);
        let output = child.wait_with_output().unwrap();
        panic!(
            "project MCP exited before replying: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let first: serde_json::Value = serde_json::from_str(&first).unwrap();
    assert_eq!(first["id"], 1);
    assert_ne!(first["result"]["isError"], true);

    let linked_store = root.path().join("linked.hs2");
    FsStore::init(&linked_store, &StoreMetadata::new("HS")).unwrap();
    registry
        .add_source(
            project.to_str().unwrap(),
            hotsheet_ticketing::checkouts::TicketSource::git(&linked_store),
            true,
        )
        .unwrap();
    writeln!(stdin, "{}", query(2)).unwrap();
    let mut relinked = String::new();
    stdout.read_line(&mut relinked).unwrap();
    let relinked: serde_json::Value = serde_json::from_str(&relinked).unwrap();
    assert_eq!(relinked["id"], 2);
    assert_ne!(relinked["result"]["isError"], true, "{relinked}");

    let replacement = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let replacement_address = replacement.local_addr().unwrap();
    let replacement_server = std::thread::spawn(move || {
        serve_project(
            replacement,
            &[r#"{"checkout":{"id":"code-project"}}"#, "[]"],
        )
    });
    std::fs::write(
        &instance,
        serde_json::json!({
            "pid":std::process::id(),
            "url":format!("http://{replacement_address}"),
            "secret":"replacement-secret",
            "store_path":bootstrap.canonicalize().unwrap(),
        })
        .to_string(),
    )
    .unwrap();
    writeln!(stdin, "{}", query(3)).unwrap();
    let mut second = String::new();
    stdout.read_line(&mut second).unwrap();
    let second: serde_json::Value = serde_json::from_str(&second).unwrap();
    assert_eq!(second["id"], 3);
    assert_ne!(second["result"]["isError"], true, "{second}");
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let paths = server.join().unwrap();
    assert_eq!(paths[0], "POST /projects/open HTTP/1.1");
    assert!(
        paths[1].starts_with("GET /checkouts/code-project/tickets"),
        "{:?}",
        paths
    );
    assert_eq!(paths[2], "POST /projects/open HTTP/1.1");
    assert!(paths[3].starts_with("GET /checkouts/code-project/tickets"));
    let replacement_paths = replacement_server.join().unwrap();
    assert_eq!(replacement_paths[0], "POST /projects/open HTTP/1.1");
    assert!(replacement_paths[1].starts_with("GET /checkouts/code-project/tickets"));
}
