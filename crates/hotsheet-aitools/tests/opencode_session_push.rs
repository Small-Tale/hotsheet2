//! Replays the opencode 1.17.18 session-push cassette (HS2-PEJ962) through the production
//! adapter **and** the production `ureq` port against a loopback fake server, so the wire
//! format (paths, query, Basic auth, JSON bodies, status handling) is checked without a
//! live TUI. Each exchange is asserted in order: method, path, `directory` query, and body.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::thread::JoinHandle;

use hotsheet_aitools::session_push::{
    HttpPort, OpenCodeHttpPush, PushError, PushLaunch, PushMessage, SessionPush, UreqHttp,
    basic_auth_header,
};
use serde_json::Value;

const PASSWORD: &str = "fixture-password";

fn cassette() -> Value {
    serde_json::from_str(include_str!("fixtures/opencode-1.17.18-session-push.json")).unwrap()
}

struct Seen {
    method: String,
    path: String,
    query: String,
    authorization: Option<String>,
    body: Option<Value>,
}

/// Serve `exchanges` in order, one connection each, answering with a wrong-password 401 when
/// the Basic credentials do not match.
fn serve(exchanges: Vec<Value>) -> (u16, JoinHandle<Vec<Seen>>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = std::thread::spawn(move || {
        let expected_auth = basic_auth_header("opencode", PASSWORD);
        let mut seen = Vec::new();
        for exchange in exchanges {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let mut parts = line.split_whitespace();
            let method = parts.next().unwrap().to_string();
            let target = parts.next().unwrap().to_string();
            let (path, query) = target.split_once('?').unwrap_or((&target, ""));
            let mut length = 0usize;
            let mut authorization = None;
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
            let body = (!raw.is_empty()).then(|| serde_json::from_slice(&raw).unwrap());
            let (status, payload) = if authorization.as_deref() == Some(expected_auth.as_str()) {
                let status = exchange["status"].as_u64().unwrap();
                let payload = match &exchange["response"] {
                    Value::Null => String::new(),
                    value => value.to_string(),
                };
                (status, payload)
            } else {
                (401, String::new())
            };
            write!(
                stream,
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                payload.len()
            )
            .unwrap();
            seen.push(Seen {
                method,
                path: path.to_string(),
                query: query.to_string(),
                authorization,
                body,
            });
        }
        seen
    });
    (port, handle)
}

fn scenario(name: &str) -> Vec<Value> {
    cassette()["scenarios"][name].as_array().unwrap().clone()
}

fn adapter(port: u16, password: &str, model: Option<&str>) -> OpenCodeHttpPush {
    let fixture = cassette();
    let http: Arc<dyn HttpPort> = Arc::new(UreqHttp::default());
    OpenCodeHttpPush::new(
        PushLaunch {
            port,
            password: password.to_string(),
            directory: fixture["directory"].as_str().unwrap().to_string(),
            model: model.map(str::to_string),
            launched_at_ms: fixture["launched_at_ms"].as_u64().unwrap(),
        },
        http,
    )
}

fn assert_matches(exchanges: &[Value], seen: &[Seen]) {
    assert_eq!(exchanges.len(), seen.len());
    for (exchange, seen) in exchanges.iter().zip(seen) {
        assert_eq!(seen.method, exchange["method"].as_str().unwrap());
        assert_eq!(seen.path, exchange["path"].as_str().unwrap());
        assert!(
            seen.query.ends_with("directory=/hotsheet-fixture/proj"),
            "{}",
            seen.query
        );
        if let Some(query) = exchange["query"].as_object() {
            for (key, value) in query {
                assert!(
                    seen.query
                        .contains(&format!("{key}={}", value.as_str().unwrap())),
                    "{key} missing from {}",
                    seen.query
                );
            }
        }
        assert_eq!(seen.body.as_ref(), exchange.get("request"), "{}", seen.path);
        assert!(seen.authorization.is_some());
    }
}

#[test]
fn a_fresh_tui_gets_a_created_selected_session_a_toast_and_the_prompt() {
    let exchanges = scenario("fresh_tui_create_select_toast_prompt");
    let (port, server) = serve(exchanges.clone());
    let receipt = adapter(port, PASSWORD, None)
        .message(&PushMessage::Prompt {
            text: "reply with only the word pong".into(),
            banner: Some("New note on HS2-TEST".into()),
        })
        .unwrap();
    assert_eq!(
        receipt.session_id.as_deref(),
        Some("ses_eb8ca5a4fffevXILESlkgFxalb")
    );
    assert!(receipt.confirmed);
    assert_matches(&exchanges, &server.join().unwrap());
}

#[test]
fn the_users_newest_session_from_this_launch_receives_the_prompt_with_the_launch_model() {
    let exchanges = scenario("user_started_session_is_listed_newest_first");
    let (port, server) = serve(exchanges.clone());
    let push = adapter(port, PASSWORD, Some("opencode/deepseek-v4.1-flash-free"));
    let receipt = push
        .message(&PushMessage::Prompt {
            text: "reply with only the word zebrafish".into(),
            banner: None,
        })
        .unwrap();
    assert_eq!(
        receipt.session_id.as_deref(),
        Some("ses_eb8c91f4affedx0zSRB1L1Nht3")
    );
    assert_eq!(push.session_id(), receipt.session_id);
    assert_matches(&exchanges, &server.join().unwrap());
}

#[test]
fn a_tui_command_is_delivered_but_unconfirmed() {
    let exchanges = scenario("tui_command_is_unconfirmed");
    let (port, server) = serve(exchanges.clone());
    let receipt = adapter(port, PASSWORD, None)
        .message(&PushMessage::Command {
            command: "agent_cycle".into(),
        })
        .unwrap();
    assert!(!receipt.confirmed);
    assert_matches(&exchanges, &server.join().unwrap());
}

#[test]
fn a_wrong_password_is_rejected_with_401() {
    let exchanges = scenario("tui_command_is_unconfirmed");
    let (port, server) = serve(exchanges);
    let error = adapter(port, "not-the-password", None)
        .message(&PushMessage::Command {
            command: "agent_cycle".into(),
        })
        .unwrap_err();
    assert_eq!(
        error,
        PushError::Rejected {
            status: 401,
            body: String::new()
        }
    );
    let seen = server.join().unwrap();
    assert_ne!(
        seen[0].authorization.as_deref(),
        Some(basic_auth_header("opencode", PASSWORD).as_str())
    );
}

#[test]
fn a_session_api_that_is_not_listening_is_unreachable() {
    let port = TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let error = adapter(port, PASSWORD, None)
        .message(&PushMessage::Command {
            command: "agent_cycle".into(),
        })
        .unwrap_err();
    assert!(matches!(error, PushError::Unreachable(_)), "{error:?}");
}
