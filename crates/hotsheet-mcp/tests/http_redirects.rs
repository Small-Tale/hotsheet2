//! HS2-0BTSX8: real synthetic HTTP and stdio boundaries, never a live server or credential.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use hotsheet_mcp::{Backend, HttpBackend, handle_message};
use serde_json::{Value, json};

const SYNTHETIC_SECRET: &str = "synthetic-mcp-redirect-fixture";
const DEADLINE: Duration = Duration::from_secs(5);

#[derive(Clone, Debug)]
struct Request {
    method: String,
    path: String,
    has_expected_secret: bool,
    has_secret_header: bool,
    body: Value,
}

struct Response {
    status: u16,
    location: Option<String>,
    body: String,
    gate: Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>,
}

impl Response {
    fn json(status: u16, body: Value) -> Self {
        Self {
            status,
            location: None,
            body: body.to_string(),
            gate: None,
        }
    }

    fn redirect(status: u16, location: Option<String>) -> Self {
        Self {
            location,
            ..Self::json(status, json!({"marker":"untrusted-redirect-body"}))
        }
    }
}

/// A blocking listener stopped by an explicit wake connection, not polling/sleeping. The wake
/// is excluded before parsing. A broken baseline is served too, so red tests cannot hang on B.
struct Server {
    address: SocketAddr,
    requests: mpsc::Receiver<Request>,
    stopping: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Server {
    fn start(responses: Vec<Response>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        Self::from_listener(listener, responses)
    }

    fn from_listener(listener: TcpListener, responses: Vec<Response>) -> Self {
        let address = listener.local_addr().unwrap();
        let (recorded, requests) = mpsc::channel();
        let stopping = Arc::new(AtomicBool::new(false));
        let stopped = stopping.clone();
        let thread = thread::spawn(move || {
            let mut responses = VecDeque::from(responses);
            loop {
                let (mut stream, _) = listener.accept().unwrap();
                if stopped.load(Ordering::SeqCst) {
                    break;
                }
                stream.set_read_timeout(Some(DEADLINE)).unwrap();
                stream.set_write_timeout(Some(DEADLINE)).unwrap();
                let request = read_request(&mut stream);
                let presence = request.path.starts_with("/mcp-presence");
                recorded.send(request).unwrap();
                // Initialize legitimately starts/stops presence. It is a separate owner (081PXN),
                // so stdio tests serve its normal selected-origin requests without redirects.
                let response = if presence {
                    Response::json(200, json!({"ok":true}))
                } else {
                    responses
                        .pop_front()
                        .unwrap_or_else(|| Response::json(200, json!({"foreign":true})))
                };
                let location = response
                    .location
                    .map(|url| format!("Location: {url}\r\n"))
                    .unwrap_or_default();
                write!(stream, "HTTP/1.1 {} Fixture\r\nContent-Type: application/json\r\n{location}Content-Length: {}\r\nConnection: close\r\n\r\n", response.status, response.body.len()).unwrap();
                stream.flush().unwrap();
                if let Some((ready, released)) = response.gate {
                    ready.send(()).unwrap();
                    released.recv_timeout(DEADLINE).unwrap();
                }
                // A redirect-rejecting client may close without reading this optional body.
                let _ = stream.write_all(response.body.as_bytes());
            }
        });
        Self {
            address,
            requests,
            stopping,
            thread: Some(thread),
        }
    }

    fn url(&self) -> String {
        format!("http://{}", self.address)
    }
    fn requests(&self) -> Vec<Request> {
        self.requests.try_iter().collect()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect_timeout(&self.address, DEADLINE);
        let result = self.thread.take().unwrap().join();
        if !thread::panicking() {
            result.unwrap();
        }
    }
}

fn read_request(stream: &mut TcpStream) -> Request {
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        let count = stream.read(&mut buffer).unwrap();
        assert!(
            count > 0,
            "synthetic client closed before completing request"
        );
        bytes.extend_from_slice(&buffer[..count]);
        assert!(
            bytes.len() <= 65_536,
            "synthetic request exceeded fixture bound"
        );
        let Some(end) = bytes.windows(4).position(|b| b == b"\r\n\r\n") else {
            continue;
        };
        let headers = String::from_utf8_lossy(&bytes[..end]);
        let fields: Vec<_> = headers
            .lines()
            .skip(1)
            .filter_map(|line| line.split_once(':'))
            .collect();
        let length = fields
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
            .map(|(_, value)| value.trim().parse::<usize>().unwrap())
            .unwrap_or(0);
        if bytes.len() < end + 4 + length {
            continue;
        }
        let mut line = headers.lines().next().unwrap().split_whitespace();
        return Request {
            method: line.next().unwrap().into(),
            path: line.next().unwrap().into(),
            has_expected_secret: fields.iter().any(|(name, value)| {
                name.eq_ignore_ascii_case("x-hotsheet-secret") && value.trim() == SYNTHETIC_SECRET
            }),
            has_secret_header: fields
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case("x-hotsheet-secret")),
            body: serde_json::from_slice(&bytes[end + 4..end + 4 + length]).unwrap_or(Value::Null),
        };
    }
}

fn backend(server: &Server) -> HttpBackend {
    HttpBackend::new(server.url(), SYNTHETIC_SECRET)
}

#[test]
fn hs2_0btsx8_get_and_mutations_never_follow_foreign_redirects() {
    for (method, statuses) in [
        ("GET", &[301, 302, 303, 307, 308][..]),
        ("POST", &[301, 302, 303, 307, 308][..]),
        ("DELETE", &[302, 307, 308][..]),
    ] {
        for &status in statuses {
            let foreign = Server::start(vec![]);
            let selected = Server::start(vec![Response::redirect(
                status,
                Some(format!("{}/foreign", foreign.url())),
            )]);
            let result = if method == "GET" {
                backend(&selected).get("/tickets", &[])
            } else {
                backend(&selected).send(method, "/tickets/synthetic", &json!({"title":"synthetic"}))
            };
            // Check routing independently of whether the old decoder reported a success or error.
            assert!(
                foreign.requests().is_empty(),
                "foreign request for {method}/{status}"
            );
            let error = result.unwrap_err();
            assert_eq!(error.status, Some(status));
            assert!(error.message.contains("redirect"));
            assert!(!error.message.contains("untrusted-redirect-body"));
            let sent = selected.requests();
            assert_eq!(sent.len(), 1);
            assert_eq!(sent[0].method, method);
            assert!(sent[0].has_expected_secret);
            if method != "GET" {
                assert_eq!(sent[0].body, json!({"title":"synthetic"}));
            }
        }
    }
}

#[test]
fn hs2_0btsx8_rejects_relative_same_origin_missing_and_malformed_locations() {
    for location in [
        None,
        Some("/same-origin"),
        Some("http://["),
        Some("same-origin"),
    ] {
        let selected = Server::start(vec![Response::redirect(302, location.map(str::to_owned))]);
        let error = backend(&selected).get("/tickets", &[]).unwrap_err();
        assert_eq!(error.status, Some(302));
        assert!(error.message.contains("redirect"));
        assert_eq!(selected.requests().len(), 1);
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let location = format!("http://{}/same-origin", listener.local_addr().unwrap());
    let selected = Server::from_listener(listener, vec![Response::redirect(302, Some(location))]);
    assert_eq!(
        backend(&selected).get("/tickets", &[]).unwrap_err().status,
        Some(302)
    );
    assert_eq!(selected.requests().len(), 1);
    for status in [300, 304, 399] {
        let selected = Server::start(vec![Response::redirect(status, None)]);
        assert_eq!(
            backend(&selected).get("/tickets", &[]).unwrap_err().status,
            Some(status)
        );
        assert_eq!(selected.requests().len(), 1);
    }
}

#[test]
fn hs2_0btsx8_redirect_error_does_not_wait_for_the_response_body() {
    let (ready, headers_sent) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let mut response = Response::redirect(302, None);
    response.gate = Some((ready, released));
    let selected = Server::start(vec![response]);
    let client = backend(&selected);
    let (completed, result) = mpsc::channel();
    let worker = thread::spawn(move || completed.send(client.get("/tickets", &[])).unwrap());
    headers_sent.recv_timeout(DEADLINE).unwrap();
    let before_release = result.recv_timeout(Duration::from_secs(2));
    // Always release/join before asserting, including when a broken baseline waits for the body.
    release.send(()).unwrap();
    worker.join().unwrap();
    let error = before_release
        .expect("redirect handling waited for a gated body")
        .unwrap_err();
    assert_eq!(error.status, Some(302));
}

fn workers_call() -> Value {
    json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
        "name":"hotsheet_workers","arguments":{"checkout":"synthetic/project","after":"cursor+1","wait_ms":123}
    }})
}

#[test]
fn hs2_0btsx8_workers_poll_reports_redirect_as_tool_error_and_preserves_success() {
    for status in [200, 302] {
        let foreign = Server::start(vec![]);
        let response = if status == 200 {
            Response::json(200, json!({"cursor":"next","workers":[]}))
        } else {
            Response::redirect(status, Some(foreign.url()))
        };
        let selected = Server::start(vec![response]);
        let reply = handle_message(&workers_call(), &backend(&selected)).unwrap();
        assert_eq!(reply["result"]["isError"], status != 200);
        let text = reply["result"]["content"][0]["text"].as_str().unwrap();
        if status == 200 {
            assert_eq!(
                serde_json::from_str::<Value>(text).unwrap()["cursor"],
                "next"
            );
        } else {
            assert!(text.contains("302") && text.contains("redirect"));
        }
        let sent = selected.requests();
        assert_eq!(sent.len(), 1);
        assert_eq!(
            sent[0].path,
            "/checkouts/synthetic%2Fproject/workers?after=cursor%2B1&wait_ms=123"
        );
        assert!(sent[0].has_expected_secret);
        assert!(foreign.requests().is_empty());
    }
}

#[test]
fn hs2_0btsx8_redirected_health_cannot_replace_auth_diagnostics() {
    for status in [401, 403] {
        for relative in [false, true] {
            let foreign = Server::start(vec![Response::json(
                200,
                json!({"generation":"hs2","ticket_prefix":"FOREIGN"}),
            )]);
            let location = if relative {
                "/other-health".into()
            } else {
                foreign.url()
            };
            let mut health = Response::redirect(302, Some(location));
            health.body = json!({"generation":"hs2","ticket_prefix":"REDIRECT"}).to_string();
            let selected = Server::start(vec![
                Response::json(status, json!({"error":"synthetic auth failure"})),
                health,
            ]);
            let error = backend(&selected).get("/tickets", &[]).unwrap_err();
            assert_eq!(error.status, Some(status));
            assert_eq!(error.message, "synthetic auth failure");
            let sent = selected.requests();
            assert_eq!(sent.len(), 2);
            assert!(sent[0].has_expected_secret);
            assert_eq!(sent[1].path, "/health");
            assert!(!sent[1].has_secret_header);
            assert!(foreign.requests().is_empty());
        }
    }
}

#[test]
fn hs2_0btsx8_nonredirected_mutation_and_server_error_keep_their_contract() {
    let selected = Server::start(vec![
        Response::json(201, json!({"id":"synthetic"})),
        Response::json(422, json!({"error":"synthetic invalid input"})),
    ]);
    let client = backend(&selected);
    assert_eq!(
        client
            .send("POST", "/tickets", &json!({"title":"synthetic"}))
            .unwrap(),
        json!({"id":"synthetic"})
    );
    let error = client
        .send("PUT", "/tickets/synthetic", &json!({"title":"synthetic"}))
        .unwrap_err();
    assert_eq!(error.status, Some(422));
    assert_eq!(error.message, "synthetic invalid input");
    let sent = selected.requests();
    assert_eq!(sent.len(), 2);
    assert!(sent.iter().all(|request| request.has_expected_secret));
}

#[test]
fn hs2_0btsx8_stdio_proxy_rejects_a_workers_poll_redirect() {
    let foreign = Server::start(vec![]);
    let selected = Server::start(vec![Response::redirect(303, Some(foreign.url()))]);
    let home = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_hotsheet-mcp"))
        .args(["--server", &selected.url(), "--secret", SYNTHETIC_SECRET])
        .current_dir(home.path())
        .env("HOTSHEET_HOME", home.path())
        .env_remove("HOTSHEET_STORE")
        .env_remove("HOTSHEET_SERVER")
        .env_remove("HOTSHEET_SECRET")
        .env_remove("HOTSHEET_TERMINAL_ID")
        .env_remove("HOTSHEET_AGENT")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    writeln!(
        stdin,
        "{}",
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}})
    )
    .unwrap();
    writeln!(stdin, "{}", workers_call()).unwrap();
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let replies: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(replies.len(), 2);
    assert_eq!(replies[0]["id"], 1);
    assert_eq!(replies[1]["id"], 2);
    assert_eq!(replies[1]["result"]["isError"], true);
    assert!(
        replies[1]["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("303")
    );
    let requests = selected.requests();
    let tools: Vec<_> = requests
        .iter()
        .filter(|request| !request.path.starts_with("/mcp-presence"))
        .collect();
    assert_eq!(tools.len(), 1);
    assert!(
        tools[0]
            .path
            .starts_with("/checkouts/synthetic%2Fproject/workers?")
    );
    assert!(requests.iter().all(|request| request.has_expected_secret));
    assert!(foreign.requests().is_empty());
}
