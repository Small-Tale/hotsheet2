use assert_cmd::Command;
use std::io::{Read, Write};
use std::net::TcpListener;

fn read_request(stream: &mut std::net::TcpStream) -> String {
    let mut bytes = Vec::new();
    let mut buf = [0; 1024];
    while !bytes.ends_with(b"\r\n\r\n") {
        let count = stream.read(&mut buf).unwrap();
        assert!(count > 0);
        bytes.extend_from_slice(&buf[..count]);
    }
    String::from_utf8(bytes).unwrap()
}

fn reply(stream: &mut std::net::TcpStream, body: &str) {
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
    .unwrap();
}

#[test]
fn headless_probe_distinguishes_bridge_from_terminal_hook_and_mcp() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        for (route, body) in [
            ("/permissions/bridge-probe", r#"{"bridge_reachable":true}"#),
            (
                "/terminals",
                r#"[{"id":"codex-tab","alive":true,"last_hook_report":{"at":"2026-10-08T01:00:00Z","source":"permission_request","agent":"codex"}}]"#,
            ),
        ] {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let request = read_request(&mut stream);
            assert!(request.starts_with(&format!("GET {route} HTTP/1.1")));
            assert!(
                request
                    .to_ascii_lowercase()
                    .contains("x-hotsheet-secret: route-secret")
            );
            reply(&mut stream, body);
        }
    });
    let output = Command::cargo_bin("hotsheet-cli")
        .unwrap()
        .env_remove("HOTSHEET_PROJECT")
        .env("HOTSHEET_SERVER", url)
        .env("HOTSHEET_SECRET", "route-secret")
        .env("HOTSHEET_TERMINAL_ID", "codex-tab")
        .args(["hook-diagnose", "--json"])
        .output()
        .unwrap();
    server.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["mcp"], "not_tested");
    assert_eq!(report["permission_bridge"], "reachable");
    assert_eq!(report["terminal_hook"], "not_active");
    assert_eq!(report["last_hook_report"]["source"], "permission_request");
}

#[test]
fn no_hook_route_reports_unconfigured_without_claiming_connection() {
    let output = Command::cargo_bin("hotsheet-cli")
        .unwrap()
        .env_remove("HOTSHEET_PROJECT")
        .env_remove("HOTSHEET_SERVER")
        .env_remove("HOTSHEET_SECRET")
        .env_remove("HOTSHEET_TERMINAL_ID")
        .args(["hook-diagnose", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["permission_bridge"], "unconfigured");
    assert_eq!(report["terminal_hook"], "unknown");
}
