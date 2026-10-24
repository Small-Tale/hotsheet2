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
use hotsheet_server::terminal_broker::TerminalBroker;
use hotsheet_server::{AppState, app, resume_broker_terminal_sessions};
use hotsheet_terminals::TerminalManager;
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
    launch_in(router, tool, id, None).await
}

async fn launch_in(
    router: &axum::Router,
    tool: &str,
    id: &str,
    cwd: Option<&std::path::Path>,
) -> (String, String, String) {
    let body = serde_json::json!({ "connect": tool, "id": id, "cwd": cwd }).to_string();
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
async fn a_broker_terminal_keeps_its_push_session_across_a_server_restart() {
    let store_dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(store_dir.path(), &StoreMetadata::new("HS")).unwrap();
    let plugins = tempfile::tempdir().unwrap();
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
    let socket_dir = tempfile::tempdir().unwrap();
    let socket = socket_dir.path().join("broker.sock");
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    let broker_task = tokio::spawn(hotsheet_terminals::serve_broker(
        listener,
        "session-push-project".into(),
        Arc::new(TerminalManager::new()),
    ));
    let broker = TerminalBroker::at(&socket, "session-push-project");
    let state = AppState::new(store, SECRET.into())
        .unwrap()
        .with_plugin_dirs(vec![plugins.path().to_path_buf()])
        .with_terminal_broker_at(broker.clone());
    let router = app(state);
    let project = tempfile::tempdir().unwrap();
    let (flag, port, password) =
        launch_in(&router, "fake-push", "survives", Some(project.path())).await;
    assert_eq!(flag, "--port");
    assert!(password.len() >= 40);
    let seen = fake_opencode(port.parse().unwrap());
    let (status, delivered) = send(
        &router,
        "POST",
        "/connections/survives/message",
        Some(r#"{"text":"before restart"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{delivered}");
    assert_eq!(delivered["session_id"], "ses_e2e");

    // A second AppState has no adapters until startup reattaches to the detached broker.
    let restarted = AppState::new(FsStore::open(store_dir.path()).unwrap(), SECRET.into())
        .unwrap()
        .with_plugin_dirs(vec![plugins.path().to_path_buf()])
        .with_terminal_broker_at(broker.clone());
    resume_broker_terminal_sessions(&restarted).await;
    let router = app(restarted);
    let (status, connections) = send(&router, "GET", "/connections", None).await;
    assert_eq!(status, StatusCode::OK);
    let listed = connection(&connections, "survives");
    assert_eq!(
        listed["project"],
        project
            .path()
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .as_ref()
    );
    assert_eq!(listed["actions"], serde_json::json!(["message"]));
    assert_eq!(listed["session_id"], "ses_e2e");
    assert!(!connections.to_string().contains(&password));
    let (_, terminals) = send(&router, "GET", "/terminals", None).await;
    assert!(!terminals.to_string().contains(&password));
    let (status, delivered) = send(
        &router,
        "POST",
        "/connections/survives/message",
        Some(r#"{"text":"after restart"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{delivered}");
    assert_eq!(delivered["session_id"], "ses_e2e");

    // Opening the same id from another fresh server reattaches instead of replacing the
    // broker's port, password, and resolved session.
    let reattaching = AppState::new(FsStore::open(store_dir.path()).unwrap(), SECRET.into())
        .unwrap()
        .with_plugin_dirs(vec![plugins.path().to_path_buf()])
        .with_terminal_broker_at(broker);
    let reattach_router = app(reattaching);
    let (_, reattached_port, reattached_password) =
        launch(&reattach_router, "fake-push", "survives").await;
    assert_eq!(reattached_port, port);
    assert_eq!(reattached_password, password);
    let (_, connections) = send(&reattach_router, "GET", "/connections", None).await;
    assert_eq!(
        connection(&connections, "survives")["project"],
        project
            .path()
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .as_ref()
    );
    assert_eq!(
        connection(&connections, "survives")["session_id"],
        "ses_e2e"
    );
    let (status, delivered) = send(
        &reattach_router,
        "POST",
        "/connections/survives/message",
        Some(r#"{"text":"after reattach"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{delivered}");
    assert_eq!(delivered["session_id"], "ses_e2e");
    let routes: Vec<_> = seen
        .lock()
        .unwrap()
        .iter()
        .map(|entry| entry.route.clone())
        .collect();
    assert_eq!(
        routes
            .iter()
            .filter(|route| route.as_str() == "GET /session")
            .count(),
        1
    );
    assert_eq!(
        routes
            .iter()
            .filter(|route| route.as_str() == "POST /session/ses_e2e/prompt_async")
            .count(),
        3
    );
    let (status, _) = send(&reattach_router, "DELETE", "/terminals/survives", None).await;
    assert!(status.is_success());
    broker_task.abort();
}

#[tokio::test]
async fn a_terminal_connection_belongs_to_its_launch_checkout_project() {
    let store_dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(store_dir.path(), &StoreMetadata::new("HS")).unwrap();
    let project_dir = tempfile::tempdir().unwrap();
    let checkout = project_dir.path().join("code");
    std::fs::create_dir(&checkout).unwrap();
    let source = FsStore::init(
        project_dir.path().join("tickets"),
        &StoreMetadata::new("TK"),
    )
    .unwrap();
    let registry_path = project_dir.path().join("checkouts.json");
    hotsheet_ticketing::checkouts::CheckoutRegistry::new(&registry_path)
        .register(&checkout, None, None, vec![source.root().to_path_buf()])
        .unwrap();
    let plugins = tempfile::tempdir().unwrap();
    write_plugin(plugins.path(), "fake-nopush", "");
    let state = AppState::new(store, SECRET.into())
        .unwrap()
        .with_plugin_dirs(vec![plugins.path().to_path_buf()])
        .with_checkout_registry(registry_path);
    let router = app(state);
    let body =
        serde_json::json!({"connect":"fake-nopush","id":"checkout-ai","cwd":checkout}).to_string();
    let (status, terminal) = send(&router, "POST", "/terminals", Some(&body)).await;
    assert_eq!(status, StatusCode::OK, "{terminal}");
    let (_, connections) = send(&router, "GET", "/connections", None).await;
    assert_eq!(
        connection(&connections, "checkout-ai")["project"],
        checkout.canonicalize().unwrap().to_string_lossy().as_ref()
    );
    let (status, _) = send(&router, "DELETE", "/terminals/checkout-ai", None).await;
    assert!(status.is_success());
}

#[tokio::test]
async fn a_push_capable_terminal_is_launched_reachable_and_receives_prompts_and_commands() {
    let (_store, _plugins, router) = fixture();
    let project = tempfile::tempdir().unwrap();
    let (flag, port, password) =
        launch_in(&router, "fake-push", "push-1", Some(project.path())).await;
    assert_eq!(flag, "--port");
    let port: u16 = port.parse().unwrap();
    assert!(
        password.len() >= 40,
        "a random per-session password: {password}"
    );
    let seen = fake_opencode(port);

    let (_, list) = send(&router, "GET", "/connections", None).await;
    let listed = connection(&list, "push-1");
    assert_eq!(
        listed["project"],
        project
            .path()
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .as_ref()
    );
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
async fn a_push_capable_terminal_reopens_after_exit_with_a_fresh_adapter() {
    let (_store, plugins, router) = fixture();
    let manifest = plugins.path().join("fake-push/manifest.toml");
    let original = std::fs::read_to_string(&manifest).unwrap();
    std::fs::write(&manifest, original.replace("sleep 30", "sleep 0.1")).unwrap();
    let (_, first_port, first_password) = launch(&router, "fake-push", "push-reopen").await;
    for _ in 0..100 {
        let (_, terminal) = send(&router, "GET", "/terminals/push-reopen", None).await;
        if terminal["alive"] == false {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let (_, terminal) = send(&router, "GET", "/terminals/push-reopen", None).await;
    assert_eq!(terminal["alive"], false);

    std::fs::write(&manifest, original).unwrap();
    let (_, second_port, second_password) = launch(&router, "fake-push", "push-reopen").await;
    assert!(first_port.parse::<u16>().is_ok());
    assert!(second_port.parse::<u16>().is_ok());
    assert_ne!(first_password, second_password);
    tokio::time::sleep(std::time::Duration::from_millis(700)).await;
    let (_, connections) = send(&router, "GET", "/connections", None).await;
    let reopened = connection(&connections, "push-reopen");
    assert_eq!(reopened["actions"], serde_json::json!(["message"]));
    let (status, _) = send(&router, "DELETE", "/terminals/push-reopen", None).await;
    assert!(status.is_success());
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

#[cfg(unix)]
#[tokio::test]
async fn a_brokered_codex_launch_can_bootstrap_only_once() {
    let store_dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(store_dir.path(), &StoreMetadata::new("HS")).unwrap();
    let plugins = tempfile::tempdir().unwrap();
    write_plugin(
        plugins.path(),
        "fake-codex",
        "[session_push]\nmechanism = \"codex-queue\"",
    );
    let socket_dir = tempfile::tempdir().unwrap();
    let socket = socket_dir.path().join("broker.sock");
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    let broker_task = tokio::spawn(hotsheet_terminals::serve_broker(
        listener,
        "codex-bootstrap-project".into(),
        Arc::new(TerminalManager::new()),
    ));
    let broker = TerminalBroker::at(&socket, "codex-bootstrap-project");
    let router = app(AppState::new(store, SECRET.into())
        .unwrap()
        .with_plugin_dirs(vec![plugins.path().to_path_buf()])
        .with_terminal_broker_at(broker.clone()));
    launch(&router, "fake-codex", "broker-codex").await;
    let (status, first) = send(
        &router,
        "POST",
        "/connections/broker-codex/message",
        Some(r#"{"text":"first"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(first["mechanism"], "codex-pty-bootstrap");
    let (status, repeat) = send(
        &router,
        "POST",
        "/connections/broker-codex/message",
        Some(r#"{"text":"repeat"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{repeat}");
    // A server restart must not mint another first-message opportunity.
    let restarted = AppState::new(FsStore::open(store_dir.path()).unwrap(), SECRET.into())
        .unwrap()
        .with_plugin_dirs(vec![plugins.path().to_path_buf()])
        .with_terminal_broker_at(broker);
    resume_broker_terminal_sessions(&restarted).await;
    let (status, _) = send(
        &app(restarted),
        "POST",
        "/connections/broker-codex/message",
        Some(r#"{"text":"after restart"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let _ = send(&router, "DELETE", "/terminals/broker-codex", None).await;
    broker_task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_new_codex_tui_accepts_one_prompt_before_its_thread_is_reported() {
    use std::os::unix::fs::PermissionsExt;
    const THREAD: &str = "01a14736-2ae1-7e92-8da0-5f8db11b19b0";
    let store_dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(store_dir.path(), &StoreMetadata::new("HS")).unwrap();
    let plugins = tempfile::tempdir().unwrap();
    // Built-in plugin IDs take precedence over fixture directories. Use a distinct ID so
    // CI does not try to launch the real Codex binary from the bundled manifest.
    let dir = plugins.path().join("fake-codex");
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("instructions.md"), "Fake setup marker\n").unwrap();
    std::fs::write(
        dir.join("manifest.toml"),
        r#"
id = "fake-codex"
display_name = "Codex"
product_name = "Codex"
tier = "cli-agent"
[instructions]
target = "AGENTS.md"
section = "instructions.md"
[mcp]
target = ".mcp.json"
format = "claude-json"
server_name = "hotsheet"
command = "hotsheet-mcp"
args = ["--path", "{store}"]
[launch]
program = "/bin/cat"
[session_push]
mechanism = "codex-queue"
"#,
    )
    .unwrap();
    let bin = tempfile::tempdir().unwrap();
    let log = bin.path().join("queued");
    let codex = bin.path().join("codex");
    std::fs::write(&codex, format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\necho 'Queued message m-7 for thread {THREAD}.'\n", log.display())).unwrap();
    std::fs::set_permissions(&codex, std::fs::Permissions::from_mode(0o755)).unwrap();
    let router = app(AppState::new(store, SECRET.into())
        .unwrap()
        .with_plugin_dirs(vec![plugins.path().to_path_buf()])
        .with_session_push_program("codex", codex));
    let (status, _) = send(
        &router,
        "POST",
        "/terminals",
        Some(r#"{"connect":"fake-codex","id":"fresh"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, list) = send(&router, "GET", "/connections", None).await;
    assert!(
        connection(&list, "fresh")["actions"]
            .to_string()
            .contains("message")
    );
    let (status, first) = send(
        &router,
        "POST",
        "/connections/fresh/message",
        Some(r#"{"text":"first line\nsecond line"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(first["mechanism"], "codex-pty-bootstrap");
    assert_eq!(first["confirmed"], false);
    let mut seen_prompt = false;
    for _ in 0..100 {
        let (_, terminal) = send(&router, "GET", "/terminals/fresh", None).await;
        let output = terminal["scrollback"].as_str().unwrap_or_default();
        if output.contains("first line") && output.contains("second line") {
            seen_prompt = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(
        seen_prompt,
        "the first prompt did not reach the launched PTY"
    );
    // Reattaching must not refresh the one-shot adapter.
    assert_eq!(
        send(
            &router,
            "POST",
            "/terminals",
            Some(r#"{"connect":"fake-codex","id":"fresh"}"#)
        )
        .await
        .0,
        StatusCode::OK
    );
    let (status, again) = send(
        &router,
        "POST",
        "/connections/fresh/message",
        Some(r#"{"text":"repeat"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{again}");
    let (status, command) = send(
        &router,
        "POST",
        "/connections/fresh/message",
        Some(r#"{"command":"agent_cycle"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{command}");
    let (status, invalid) = send(
        &router,
        "POST",
        "/connections/fresh/message",
        Some(r#"{"text":"bad\u001b[2J"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{invalid}");
    let report = serde_json::json!({ "agent": "codex", "session_id": THREAD });
    assert_eq!(
        send(
            &router,
            "POST",
            "/terminals/fresh/ai-connection",
            Some(&report.to_string())
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let (status, queued) = send(
        &router,
        "POST",
        "/connections/fresh/message",
        Some(r#"{"text":"after hook"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{queued}");
    assert_eq!(queued["mechanism"], "codex-queue");
    assert_eq!(
        std::fs::read_to_string(log).unwrap(),
        format!("queue\n--thread={THREAD}\n--message=after hook\n")
    );
    let _ = send(&router, "DELETE", "/terminals/fresh", None).await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_hook_reported_codex_thread_receives_prompts_through_codex_queue() {
    // HS2-FKM97G: the Codex SessionStart hook reports the thread id and CODEX_HOME; the server
    // registers the built-in `codex-queue` adapter for that terminal, and the shared operation
    // runs `codex queue` (a fake binary here) for exactly that thread and home.
    use std::os::unix::fs::PermissionsExt;
    const THREAD: &str = "01a14736-2ae1-7e92-8da0-5f8db11b19b0";
    let store_dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(store_dir.path(), &StoreMetadata::new("HS")).unwrap();
    let bin = tempfile::tempdir().unwrap();
    let log = bin.path().join("argv");
    let codex = bin.path().join("codex");
    let script = |body: &str| {
        std::fs::write(&codex, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&codex, std::fs::Permissions::from_mode(0o755)).unwrap();
    };
    script(&format!(
        "printf '%s\\n' \"$CODEX_HOME\" \"$@\" > '{}'\necho 'WARNING: noise' >&2\necho 'Queued message m-7 for thread {THREAD}.'",
        log.display()
    ));
    // No daemon socket under either home, so Codex halt detection stays idle.
    let home = bin.path().join("codex-home");
    let router = app(AppState::new(store, SECRET.into())
        .unwrap()
        .with_session_push_program("codex", codex.clone())
        .with_default_codex_home(bin.path().join("no-default-home")));
    let prompt = r#"{"text":"A human answered your FEEDBACK NEEDED note on HS-1."}"#;
    let message = |router: axum::Router, body: &'static str| async move {
        send(&router, "POST", "/connections/codexing/message", Some(body)).await
    };

    let (status, _) = send(
        &router,
        "POST",
        "/terminals",
        Some(r#"{"command":"cat","id":"codexing"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // Nothing reported yet: no adapter.
    let (status, body) = message(router.clone(), prompt).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    let connect = serde_json::json!({
        "agent": "codex",
        "session_id": THREAD,
        "codex_home": home,
    })
    .to_string();
    let report = |body: String| {
        let router = router.clone();
        async move {
            send(
                &router,
                "POST",
                "/terminals/codexing/ai-connection",
                Some(&body),
            )
            .await
            .0
        }
    };
    assert_eq!(report(connect.clone()).await, StatusCode::NO_CONTENT);
    let (status, body) = message(router.clone(), prompt).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["delivered"], true);
    assert_eq!(body["mechanism"], "codex-queue");
    assert_eq!(body["session_id"], THREAD);
    assert_eq!(body["confirmed"], true);
    assert_eq!(
        std::fs::read_to_string(&log).unwrap(),
        format!(
            "{}\nqueue\n--thread={THREAD}\n--message=A human answered your FEEDBACK NEEDED note on HS-1.\n",
            home.display()
        )
    );

    // A permission ask from the same session keeps the reported home.
    std::fs::remove_file(&log).unwrap();
    let ask = serde_json::json!({ "agent": "codex", "session_id": THREAD, "source": "permission_request" });
    assert_eq!(report(ask.to_string()).await, StatusCode::NO_CONTENT);
    assert_eq!(message(router.clone(), prompt).await.0, StatusCode::OK);
    assert!(
        std::fs::read_to_string(&log)
            .unwrap()
            .starts_with(&format!("{}\n", home.display()))
    );

    // Codex has no UI-command channel.
    let (status, body) = message(router.clone(), r#"{"command":"agent_cycle"}"#).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["code"], "session_push_unsupported");

    // Codex refusing the thread (for example, under a different CODEX_HOME) is unreachable.
    script(
        "echo 'Error: failed to queue session message: no rollout found for thread id x' >&2\nexit 1",
    );
    let (status, body) = message(router.clone(), prompt).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(body["code"], "session_unreachable");
    assert!(
        body["error"].as_str().unwrap().contains("no rollout found"),
        "{body}"
    );

    // A report without a thread id leaves nothing to message; a new report re-registers.
    assert_eq!(
        report(r#"{"agent":"codex"}"#.into()).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        message(router.clone(), prompt).await.0,
        StatusCode::NOT_FOUND
    );
    report(connect).await;
    assert_eq!(
        message(router.clone(), prompt).await.0,
        StatusCode::BAD_GATEWAY
    );

    // SessionEnd drops the adapter.
    send(&router, "DELETE", "/terminals/codexing/ai-connection", None).await;
    assert_eq!(
        message(router.clone(), prompt).await.0,
        StatusCode::NOT_FOUND
    );
    let _ = send(&router, "DELETE", "/terminals/codexing", None).await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_hook_reported_claude_inbox_receives_prompts_and_its_token_is_never_served() {
    // HS2-M15CHN: Claude's SessionStart hook reports its inbox socket, token, and version; the
    // server registers the built-in `claude-inbox` adapter for that terminal, and the shared
    // operation writes the 2.1.302 line shape to exactly that socket. Walk the states: no
    // report, a live inbox, a same-session permission re-report, a command, a vanished
    // socket, an old Claude, a new session without an inbox, and SessionEnd.
    use std::os::unix::net::UnixListener;
    let store_dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(store_dir.path(), &StoreMetadata::new("HS")).unwrap();
    let router = app(AppState::new(store, SECRET.into()).unwrap());
    let prompt = r#"{"text":"Ticket HS-1 changed; re-read it."}"#;
    let message = |router: axum::Router, body: &'static str| async move {
        send(&router, "POST", "/connections/inbox/message", Some(body)).await
    };
    let (status, _) = send(
        &router,
        "POST",
        "/terminals",
        Some(r#"{"command":"cat","id":"inbox"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        message(router.clone(), prompt).await.0,
        StatusCode::NOT_FOUND
    );

    let sockets = tempfile::tempdir().unwrap();
    let socket = sockets.path().join("cc.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let reader = std::thread::spawn(move || {
        let mut frames = Vec::new();
        for _ in 0..2 {
            let (mut conn, _) = listener.accept().unwrap();
            let mut got = String::new();
            conn.read_to_string(&mut got).unwrap();
            frames.push(got);
        }
        frames
    });
    let claude = |session: &str, version: &str| {
        serde_json::json!({
            "agent": "claude",
            "session_id": session,
            "messaging_socket": socket.display().to_string(),
            "messaging_token": "secret-inbox-token",
            "tool_version": version,
        })
        .to_string()
    };
    let report = |body: String| {
        let router = router.clone();
        async move {
            send(
                &router,
                "POST",
                "/terminals/inbox/ai-connection",
                Some(&body),
            )
            .await
            .0
        }
    };
    assert_eq!(
        report(claude("s-1", "2.1.302")).await,
        StatusCode::NO_CONTENT
    );
    let (status, listed) = send(&router, "GET", "/terminals", None).await;
    assert_eq!(status, StatusCode::OK);
    let raw = listed.to_string();
    assert!(
        !raw.contains("secret-inbox-token") && !raw.contains("cc.sock"),
        "inbox details stay in the server: {raw}"
    );
    let (status, body) = message(router.clone(), prompt).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["mechanism"], "claude-inbox");
    assert_eq!(body["session_id"], "s-1");
    assert_eq!(body["confirmed"], false, "the socket never answers");
    // The same session re-reporting without its inbox (a permission ask) keeps it.
    let ask = serde_json::json!({ "agent": "claude", "session_id": "s-1", "source": "permission_request" });
    assert_eq!(report(ask.to_string()).await, StatusCode::NO_CONTENT);
    assert_eq!(
        message(router.clone(), r#"{"text":"second"}"#).await.0,
        StatusCode::OK
    );
    let frames = reader.join().unwrap();
    let lines: Vec<serde_json::Value> = frames[0]
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        lines,
        vec![
            serde_json::json!({ "type": "auth", "token": "secret-inbox-token" }),
            serde_json::json!({
                "type": "user",
                "message": { "role": "user", "content": "Ticket HS-1 changed; re-read it." },
                "from": "hotsheet",
                "session_id": "s-1",
            }),
        ]
    );
    assert!(frames[1].contains(r#""content":"second""#));

    // Claude has no UI-command channel; a vanished socket is unreachable.
    let (status, body) = message(router.clone(), r#"{"command":"agent_cycle"}"#).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["code"], "session_push_unsupported");
    std::fs::remove_file(&socket).unwrap();
    let (status, body) = message(router.clone(), prompt).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    assert_eq!(body["code"], "session_unreachable");

    // A Claude older than the inbox registers nothing; neither does a new session without one.
    report(claude("s-2", "2.1.200")).await;
    assert_eq!(
        message(router.clone(), prompt).await.0,
        StatusCode::NOT_FOUND
    );
    report(claude("s-3", "2.1.302")).await;
    assert_eq!(
        message(router.clone(), prompt).await.0,
        StatusCode::BAD_GATEWAY
    );
    report(r#"{"agent":"claude","session_id":"s-4"}"#.into()).await;
    assert_eq!(
        message(router.clone(), prompt).await.0,
        StatusCode::NOT_FOUND
    );

    // SessionEnd drops the adapter.
    report(claude("s-5", "2.1.302")).await;
    send(
        &router,
        "DELETE",
        "/terminals/inbox/ai-connection?session_id=s-5",
        None,
    )
    .await;
    assert_eq!(
        message(router.clone(), prompt).await.0,
        StatusCode::NOT_FOUND
    );
    let _ = send(&router, "DELETE", "/terminals/inbox", None).await;
}
