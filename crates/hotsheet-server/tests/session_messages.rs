//! E2E for the **message session** operation (HS2-PEJ962): a Connect-launched AI terminal
//! whose plugin declares `[session_push] mechanism = "opencode-http"` is launched with a
//! loopback port and a per-session password, advertises the `message` connection action, and
//! `POST /connections/{id}/message` delivers through the real adapter and `ureq` port to the
//! session's API (a loopback fake of OpenCode's server here). Tools without a mechanism, an
//! unreachable session, and ended terminals fail explicitly.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use base64::Engine as _;
use hotsheet_server::{AppState, app};
use hotsheet_ticketing::{FsStore, StoreMetadata};
use http_body_util::BodyExt;
use tower::ServiceExt;

const SECRET: &str = "test-secret";

fn authed(method: &str, uri: &str, body: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("x-hotsheet-secret", SECRET);
    if body.is_some() {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
    }
    builder
        .body(body.map_or(Body::empty(), |text| Body::from(text.to_string())))
        .unwrap()
}

async fn send(
    router: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<&str>,
) -> (StatusCode, serde_json::Value) {
    let response = router
        .clone()
        .oneshot(authed(method, uri, body))
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

/// A store, a plugin directory with a push-capable and a push-less fake tool, and the app.
fn fixture() -> (tempfile::TempDir, tempfile::TempDir, axum::Router) {
    let store_dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(store_dir.path(), &StoreMetadata::new("HS")).unwrap();
    let plugins = tempfile::tempdir().unwrap();
    // The fake "TUI" prints the wiring it was launched with: `sh -c <script> fake --port <p>`.
    write_plugin(
        plugins.path(),
        "fake-push",
        r#"
[session_push]
mechanism = "opencode-http"
port_args = ["--port", "{port}"]
password_env = "OPENCODE_SERVER_PASSWORD"
"#,
    );
    write_plugin(plugins.path(), "fake-nopush", "");
    let state = AppState::new(store, SECRET.into())
        .unwrap()
        .with_plugin_dirs(vec![plugins.path().to_path_buf()]);
    (store_dir, plugins, app(state))
}

fn write_plugin(root: &std::path::Path, id: &str, extra: &str) {
    let dir = root.join(id);
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("instructions.md"), "Fake setup marker\n").unwrap();
    std::fs::write(
        dir.join("manifest.toml"),
        format!(
            r#"
id = "{id}"
display_name = "Fake {id}"
product_name = "Fake {id}"
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
program = "/bin/sh"
args = ["-c", "printf 'wired:%s:%s:%s:end' \"$1\" \"$2\" \"$OPENCODE_SERVER_PASSWORD\"; sleep 30", "fake"]
{extra}"#
        ),
    )
    .unwrap();
}

/// Launch `tool` in terminal `id` and wait for the fake TUI to print its wiring.
async fn launch(router: &axum::Router, tool: &str, id: &str) -> (String, String, String) {
    let body = format!(r#"{{"connect":"{tool}","id":"{id}"}}"#);
    let (status, json) = send(router, "POST", "/terminals", Some(&body)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    for _ in 0..100 {
        let (_, terminal) = send(router, "GET", &format!("/terminals/{id}"), None).await;
        let scrollback = terminal["scrollback"].as_str().unwrap_or_default();
        if let Some(start) = scrollback.find("wired:") {
            if let Some(end) = scrollback[start..].find(":end") {
                let fields: Vec<&str> = scrollback[start + 6..start + end].split(':').collect();
                return (
                    fields[0].to_string(),
                    fields[1].to_string(),
                    fields[2].to_string(),
                );
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("the fake TUI never printed its wiring");
}

#[derive(Debug, Clone)]
struct Seen {
    route: String,
    query: String,
    authorization: Option<String>,
    body: Option<serde_json::Value>,
}

/// A loopback fake of OpenCode's session API on `port`, answering until the test ends.
fn fake_opencode(port: u16) -> Arc<Mutex<Vec<Seen>>> {
    let listener = TcpListener::bind(("127.0.0.1", port)).unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let record = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            if reader.read_line(&mut line).is_err() {
                continue;
            }
            let mut parts = line.split_whitespace();
            let method = parts.next().unwrap_or_default().to_string();
            let target = parts.next().unwrap_or_default().to_string();
            let (path, query) = target.split_once('?').unwrap_or((&target, ""));
            let (mut length, mut authorization) = (0usize, None);
            loop {
                let mut header = String::new();
                reader.read_line(&mut header).unwrap();
                let header = header.trim_end();
                if header.is_empty() {
                    break;
                }
                let (name, value) = header.split_once(':').unwrap();
                match name.to_ascii_lowercase().as_str() {
                    "content-length" => length = value.trim().parse().unwrap(),
                    "authorization" => authorization = Some(value.trim().to_string()),
                    _ => {}
                }
            }
            let mut raw = vec![0; length];
            reader.read_exact(&mut raw).unwrap();
            let route = format!("{method} {path}");
            let (status, payload) = match route.as_str() {
                "GET /session" => (200, "[]".to_string()),
                "POST /session" => (200, r#"{"id":"ses_e2e","directory":"/x"}"#.to_string()),
                "POST /session/ses_e2e/prompt_async" => (204, String::new()),
                _ => (200, "true".to_string()),
            };
            record.lock().unwrap().push(Seen {
                route,
                query: query.to_string(),
                authorization,
                body: (!raw.is_empty()).then(|| serde_json::from_slice(&raw).unwrap()),
            });
            let _ = write!(
                stream,
                "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                payload.len()
            );
        }
    });
    seen
}

fn connection(list: &serde_json::Value, id: &str) -> serde_json::Value {
    list.as_array()
        .unwrap()
        .iter()
        .find(|connection| connection["id"] == id)
        .cloned()
        .unwrap_or(serde_json::Value::Null)
}

#[tokio::test]
async fn a_push_capable_terminal_is_launched_reachable_and_receives_prompts_and_commands() {
    let (_store, _plugins, router) = fixture();
    let (flag, port, password) = launch(&router, "fake-push", "push-1").await;
    assert_eq!(flag, "--port");
    let port: u16 = port.parse().unwrap();
    assert!(
        password.len() >= 40,
        "a random per-session password: {password}"
    );
    let seen = fake_opencode(port);

    let (_, list) = send(&router, "GET", "/connections", None).await;
    let listed = connection(&list, "push-1");
    assert_eq!(listed["actions"], serde_json::json!(["message"]));
    assert!(listed.get("session_id").is_none(), "{listed}");

    let (status, delivered) = send(
        &router,
        "POST",
        "/connections/push-1/message",
        Some(r#"{"text":"A human added a note to HS-1","banner":"New note on HS-1"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{delivered}");
    assert_eq!(
        delivered,
        serde_json::json!({
            "delivered": true, "mechanism": "opencode-http", "session_id": "ses_e2e", "confirmed": true
        })
    );
    let requests = seen.lock().unwrap().clone();
    let routes: Vec<&str> = requests.iter().map(|seen| seen.route.as_str()).collect();
    assert_eq!(
        routes,
        [
            "GET /session",
            "POST /session",
            "POST /tui/select-session",
            "POST /tui/show-toast",
            "POST /session/ses_e2e/prompt_async"
        ]
    );
    let expected_auth = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("opencode:{password}"))
    );
    assert!(
        requests
            .iter()
            .all(|seen| seen.authorization.as_deref() == Some(expected_auth.as_str()))
    );
    assert!(
        requests
            .iter()
            .all(|seen| seen.query.contains("directory="))
    );
    assert_eq!(
        requests[4].body,
        Some(
            serde_json::json!({ "parts": [{ "type": "text", "text": "A human added a note to HS-1" }] })
        )
    );

    // The connection now reports the tool-native session it is messaging.
    let (_, list) = send(&router, "GET", "/connections", None).await;
    assert_eq!(connection(&list, "push-1")["session_id"], "ses_e2e");

    // A second prompt reuses the session; a command runs in the TUI unconfirmed.
    let (status, _) = send(
        &router,
        "POST",
        "/connections/push-1/message",
        Some(r#"{"text":"again"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, command) = send(
        &router,
        "POST",
        "/connections/push-1/message",
        Some(r#"{"command":"agent_cycle"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{command}");
    assert_eq!(command["confirmed"], false);
    let routes: Vec<String> = seen.lock().unwrap()[5..]
        .iter()
        .map(|seen| seen.route.clone())
        .collect();
    assert_eq!(
        routes,
        [
            "POST /session/ses_e2e/prompt_async",
            "POST /tui/execute-command"
        ]
    );

    // Malformed requests are rejected before any delivery.
    for body in [
        r#"{"text":"a","command":"b"}"#,
        r#"{}"#,
        r#"{"command":"b","banner":"c"}"#,
        r#"{"text":"   "}"#,
    ] {
        let (status, _) = send(&router, "POST", "/connections/push-1/message", Some(body)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    }
    assert_eq!(seen.lock().unwrap().len(), 7);

    // Once the terminal ends, its connection and adapter are gone.
    let (status, _) = send(&router, "DELETE", "/terminals/push-1", None).await;
    assert!(status.is_success(), "{status}");
    let mut gone = None;
    for _ in 0..100 {
        let (status, json) = send(
            &router,
            "POST",
            "/connections/push-1/message",
            Some(r#"{"text":"late"}"#),
        )
        .await;
        if status == StatusCode::NOT_FOUND {
            gone = Some(json);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(
        gone.expect("the ended terminal stops accepting messages")["code"],
        "connection_not_found"
    );
}

#[tokio::test]
async fn a_tool_without_a_push_mechanism_reports_the_capability_unsupported() {
    let (_store, _plugins, router) = fixture();
    let (flag, port, password) = launch(&router, "fake-nopush", "plain-1").await;
    assert_eq!(
        (flag.as_str(), port.as_str(), password.as_str()),
        ("", "", ""),
        "no push wiring is added to a tool without [session_push]"
    );
    let (_, list) = send(&router, "GET", "/connections", None).await;
    assert!(connection(&list, "plain-1").get("actions").is_none());

    let (status, json) = send(
        &router,
        "POST",
        "/connections/plain-1/message",
        Some(r#"{"text":"hello"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{json}");
    assert_eq!(json["code"], "session_push_unsupported");
    assert_eq!(
        json["error"],
        "Fake fake-nopush sessions cannot be messaged"
    );

    let (status, json) = send(
        &router,
        "POST",
        "/connections/no-such/message",
        Some(r#"{"text":"hello"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(json["code"], "connection_not_found");
    let _ = send(&router, "DELETE", "/terminals/plain-1", None).await;
}

#[tokio::test]
async fn an_unreachable_session_api_is_a_502() {
    let (_store, _plugins, router) = fixture();
    // Nothing listens on the allocated port: the fake TUI never starts a server.
    launch(&router, "fake-push", "push-down").await;
    let (status, json) = send(
        &router,
        "POST",
        "/connections/push-down/message",
        Some(r#"{"text":"hello"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{json}");
    assert_eq!(json["code"], "session_unreachable");
    let _ = send(&router, "DELETE", "/terminals/push-down", None).await;
}
