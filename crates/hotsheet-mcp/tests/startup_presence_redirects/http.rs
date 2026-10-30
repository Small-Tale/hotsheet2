//! Synthetic HTTP boundary for HS2-081PXN; no quiet-window network assertions.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde_json::{Value, json};

pub(super) const DEADLINE: Duration = Duration::from_secs(10);
pub(super) const SECRET: &str = "synthetic-startup-presence-bearer";

#[derive(Debug)]
pub(super) struct Request {
    pub method: String,
    pub path: String,
    pub expected_secret: bool,
    pub secret_header: bool,
    pub body: Value,
}

#[derive(Default)]
struct Release {
    released: Mutex<bool>,
    ready: Condvar,
}
impl Release {
    fn release(&self) {
        *self.released.lock().unwrap() = true;
        self.ready.notify_all();
    }
}

pub(super) struct ResponseGate {
    sent: mpsc::Sender<()>,
    release: Arc<Release>,
}
pub(super) struct GateControl {
    sent: mpsc::Receiver<()>,
    release: Arc<Release>,
}
impl GateControl {
    pub fn wait(&self) {
        self.sent.recv_timeout(DEADLINE).unwrap();
    }
    pub fn release(&self) {
        self.release.release();
    }
}
impl Drop for GateControl {
    fn drop(&mut self) {
        self.release();
    }
}
pub(super) fn gate() -> (ResponseGate, GateControl) {
    let (sent, received) = mpsc::channel();
    let release = Arc::new(Release::default());
    (
        ResponseGate {
            sent,
            release: release.clone(),
        },
        GateControl {
            sent: received,
            release,
        },
    )
}
impl ResponseGate {
    fn wait(self) {
        let _ = self.sent.send(());
        let released = self.release.released.lock().unwrap();
        drop(
            self.release
                .ready
                .wait_timeout_while(released, Duration::from_secs(20), |released| !*released)
                .unwrap(),
        );
    }
}

pub(super) struct Response {
    pub status: u16,
    pub location: Option<String>,
    pub body: String,
    pub before_headers: Option<ResponseGate>,
    pub before_body: Option<ResponseGate>,
}
impl Response {
    pub fn json(status: u16, body: Value) -> Self {
        Self {
            status,
            location: None,
            body: body.to_string(),
            before_headers: None,
            before_body: None,
        }
    }
    pub fn redirect(status: u16, location: Option<String>) -> Self {
        Self {
            location,
            ..Self::json(
                status,
                json!({"checkout":{"id":"synthetic-checkout"}, "marker":"private-response-marker"}),
            )
        }
    }
}

/// A wake connection stops blocking accept; it is excluded before request parsing.
/// Method completion or child exit precedes negative assertions. A broken baseline's
/// foreign listener answers valid checkout JSON rather than merely closing its port.
pub(super) struct Server {
    address: SocketAddr,
    requests: mpsc::Receiver<Request>,
    stopped: Arc<AtomicBool>,
    gates: Vec<Arc<Release>>,
    thread: Option<JoinHandle<()>>,
}
impl Server {
    pub fn start(responses: Vec<Response>) -> Self {
        Self::with_secret(responses, SECRET)
    }
    pub fn with_secret(responses: Vec<Response>, expected_secret: &str) -> Self {
        Self::from_parts(
            TcpListener::bind("127.0.0.1:0").unwrap(),
            responses,
            expected_secret,
        )
    }
    pub fn from_listener(listener: TcpListener, responses: Vec<Response>) -> Self {
        Self::from_parts(listener, responses, SECRET)
    }
    fn from_parts(listener: TcpListener, responses: Vec<Response>, expected_secret: &str) -> Self {
        let address = listener.local_addr().unwrap();
        let (recorded, requests) = mpsc::channel();
        let stopped = Arc::new(AtomicBool::new(false));
        let worker_stop = stopped.clone();
        let gates = responses
            .iter()
            .flat_map(|response| [&response.before_headers, &response.before_body])
            .filter_map(|gate| gate.as_ref().map(|gate| gate.release.clone()))
            .collect();
        let expected_secret = expected_secret.to_owned();
        let thread = thread::spawn(move || {
            let mut responses = VecDeque::from(responses);
            loop {
                let (mut stream, _) = listener.accept().unwrap();
                if worker_stop.load(Ordering::SeqCst) {
                    break;
                }
                stream.set_nonblocking(false).unwrap();
                stream.set_read_timeout(Some(DEADLINE)).unwrap();
                stream.set_write_timeout(Some(DEADLINE)).unwrap();
                let request = read_request(&mut stream, &expected_secret);
                let _ = recorded.send(request);
                let response = responses.pop_front().unwrap_or_else(|| {
                    Response::json(200, json!({"checkout":{"id":"synthetic-checkout"}}))
                });
                if let Some(gate) = response.before_headers {
                    gate.wait();
                }
                let location = response
                    .location
                    .map(|value| format!("Location: {value}\r\n"))
                    .unwrap_or_default();
                if write!(stream, "HTTP/1.1 {} Fixture\r\nContent-Type: application/json\r\n{location}Content-Length: {}\r\nConnection: close\r\n\r\n", response.status, response.body.len()).and_then(|_| stream.flush()).is_err() {
                    continue; // A timed-out synthetic client may close before gated headers.
                }
                if let Some(gate) = response.before_body {
                    gate.wait();
                }
                // A redirect-rejecting client need not consume the response body.
                let _ = stream.write_all(response.body.as_bytes());
            }
        });
        Self {
            address,
            requests,
            stopped,
            gates,
            thread: Some(thread),
        }
    }
    pub fn url(&self) -> String {
        format!("http://{}", self.address)
    }
    pub fn next(&self) -> Request {
        self.requests.recv_timeout(DEADLINE).unwrap()
    }
    pub fn requests(&self) -> Vec<Request> {
        self.requests.try_iter().collect()
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
        for gate in &self.gates {
            gate.release();
        }
        let _ = TcpStream::connect_timeout(&self.address, DEADLINE);
        let result = self.thread.take().unwrap().join();
        if !thread::panicking() {
            result.unwrap();
        }
    }
}

fn read_request(stream: &mut TcpStream, expected_secret: &str) -> Request {
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        let count = stream.read(&mut buffer).unwrap();
        assert!(count > 0, "synthetic request ended early");
        bytes.extend_from_slice(&buffer[..count]);
        assert!(bytes.len() <= 65_536, "synthetic request bound exceeded");
        let Some(end) = bytes.windows(4).position(|bytes| bytes == b"\r\n\r\n") else {
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
        assert!(length <= 32_768);
        if bytes.len() < end + 4 + length {
            continue;
        }
        let mut line = headers.lines().next().unwrap().split_whitespace();
        return Request {
            method: line.next().unwrap().into(),
            path: line.next().unwrap().into(),
            expected_secret: fields.iter().any(|(name, value)| {
                name.eq_ignore_ascii_case("x-hotsheet-secret") && value.trim() == expected_secret
            }),
            secret_header: fields
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case("x-hotsheet-secret")),
            body: serde_json::from_slice(&bytes[end + 4..end + 4 + length]).unwrap_or(Value::Null),
        };
    }
}
