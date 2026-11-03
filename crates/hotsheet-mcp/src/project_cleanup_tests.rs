//! HS2-4J6V46: real wrapper HTTP calls cannot reopen an orphan owner.
use super::*;
use std::io::{BufRead, BufReader, Read, Write};

#[test]
fn hs2_4j6v46_project_wrapper_cleanup_uses_cached_loopback_without_reopen_or_startup() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    std::fs::create_dir(&home).unwrap();
    // Even an incorrect call to current_route cannot start a daemon from this fixture.
    std::fs::write(home.join("server-bootstrap.hs2"), b"not a directory").unwrap();
    std::fs::write(home.join("checkouts.json"), b"corrupt changed registry").unwrap();
    std::fs::write(home.join("server-identity.json"), b"corrupt identity").unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let path =
        "/checkouts/owner-0123456789ab/orchestration/targets/01ARZ3NDEKTSV4RRFFQ69G5FAV/binding";
    let task = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut calls = Vec::new();
        while calls.len() < 2 {
            match listener.accept() {
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "cleanup wrapper request missing"
                    );
                    std::thread::sleep(std::time::Duration::from_millis(5));
                    continue;
                }
                Err(error) => panic!("owned listener failed: {error}"),
                Ok((mut stream, _)) => {
                    // HS2-013E3P: accepted sockets may inherit the listener's mode.
                    stream.set_nonblocking(false).unwrap();
                    stream
                        .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                        .unwrap();
                    stream
                        .set_write_timeout(Some(std::time::Duration::from_secs(2)))
                        .unwrap();
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut first = String::new();
                    reader.read_line(&mut first).unwrap();
                    assert!(first.contains(path));
                    assert!(!first.contains("/projects/open"));
                    calls.push(first);
                    let mut length = 0;
                    let mut consumed = 0;
                    loop {
                        let mut line = String::new();
                        let count = reader.read_line(&mut line).unwrap();
                        consumed += count;
                        assert!(count > 0 && consumed < 8192);
                        if line == "\r\n" {
                            break;
                        }
                        if let Some(value) =
                            line.to_ascii_lowercase().strip_prefix("content-length:")
                        {
                            length = value.trim().parse::<usize>().unwrap();
                        }
                    }
                    assert!(length <= 16384);
                    let mut body = vec![0; length];
                    reader.read_exact(&mut body).unwrap();
                    let response = r#"{"revision":2,"destination":null}"#;
                    write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",response.len(),response).unwrap();
                }
            }
        }
        calls
    });
    let root = home.join("missing-owner");
    let backend = ProjectBackend {
        root: root.clone(),
        home: home.clone(),
        route: Mutex::new((origin, "synthetic-secret".into(), b"old registry".to_vec())),
    };
    assert_eq!(backend.get(path, &[]).unwrap()["revision"], 2);
    assert_eq!(
        backend
            .send(
                "DELETE",
                path,
                &serde_json::json!({"expected_binding_revision":1})
            )
            .unwrap()["revision"],
        2
    );
    let calls = task.join().unwrap();
    assert!(calls[0].starts_with("GET "));
    assert!(calls[1].starts_with("DELETE "));
    assert!(!root.exists());
    assert_eq!(
        std::fs::read(home.join("checkouts.json")).unwrap(),
        b"corrupt changed registry"
    );
    assert_eq!(
        std::fs::read(home.join("server-identity.json")).unwrap(),
        b"corrupt identity"
    );
    // Exact subset only; bind, resolve, inspect and ordinary metadata keep normal recovery.
    assert!(!hotsheet_cli::orchestration::is_cleanup_request(
        "PUT", path
    ));
    assert!(!hotsheet_cli::orchestration::is_cleanup_request(
        "GET",
        "/checkouts/owner/orchestration/targets/id/resolve"
    ));
    let source = include_str!("main.rs");
    let body = source
        .split("fn route_for_request(")
        .nth(1)
        .unwrap()
        .split("fn current_route(")
        .next()
        .unwrap();
    assert!(body.contains("self.current_route()"));
}

#[test]
fn hs2_4j6v46_project_wrapper_remote_cleanup_origin_is_denied_before_recovery_or_network() {
    let temp = tempfile::tempdir().unwrap();
    let backend = ProjectBackend {
        root: temp.path().join("absent-owner"),
        home: temp.path().join("absent-home"),
        route: Mutex::new((
            "https://offbox.example.test".into(),
            "synthetic".into(),
            Vec::new(),
        )),
    };
    let path = "/checkouts/owner/orchestration/targets/id/binding";
    assert_eq!(backend.get(path, &[]).unwrap_err().status, Some(403));
    assert_eq!(
        backend
            .send("DELETE", path, &serde_json::Value::Null)
            .unwrap_err()
            .status,
        Some(403)
    );
    assert!(!backend.home.exists());
    assert!(!backend.root.exists());
}
