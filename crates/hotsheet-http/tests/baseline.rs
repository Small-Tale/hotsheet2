//! HS2-9ABVDW: the outbound HTTP baseline against real loopback sockets. Each test pins one
//! behaviour the workspace had on ureq 2, so a later client upgrade cannot change it
//! silently.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use hotsheet_http::{FailureKind, MAX_REDIRECTS, agent, build, config, failure_kind, is_timeout};

/// One request as the fake server saw it. No derived `Debug`: its headers may carry
/// credentials (HS2-CQNE4E).
#[derive(Clone)]
struct Seen {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Seen {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

fn read_request(stream: &TcpStream) -> Option<Seen> {
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let mut parts = line.split_whitespace();
    let method = parts.next()?.to_owned();
    let path = parts.next()?.to_owned();
    let mut headers = Vec::new();
    loop {
        let mut header = String::new();
        reader.read_line(&mut header).ok()?;
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        let (name, value) = header.split_once(':')?;
        headers.push((name.trim().to_owned(), value.trim().to_owned()));
    }
    let length = headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = vec![0; length];
    reader.read_exact(&mut body).ok()?;
    Some(Seen {
        method,
        path,
        headers,
        body,
    })
}

/// A server that answers every request with `answer(request)`, closing each connection
/// after its answer, and reports each request it saw.
fn serve(answer: impl Fn(&Seen) -> Vec<u8> + Send + 'static) -> (String, mpsc::Receiver<Seen>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let Some(seen) = read_request(&stream) else {
                continue;
            };
            let reply = answer(&seen);
            if tx.send(seen).is_err() {
                return;
            }
            let _ = stream.write_all(&reply);
        }
    });
    (origin, rx)
}

fn reply(status: &str, headers: &[(&str, &str)], body: &str) -> Vec<u8> {
    let mut text = format!(
        "HTTP/1.1 {status}\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    );
    for (name, value) in headers {
        text.push_str(&format!("{name}: {value}\r\n"));
    }
    text.push_str("\r\n");
    text.push_str(body);
    text.into_bytes()
}

/// A redirect chain: `/hop/<n>` answers 302 to `/hop/<n - 1>`, `/hop/0` answers 200.
fn hops(seen: &Seen) -> Vec<u8> {
    let n: u32 = seen
        .path
        .strip_prefix("/hop/")
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    if n == 0 {
        reply("200 OK", &[], "arrived")
    } else {
        reply("302 Found", &[("Location", &format!("/hop/{}", n - 1))], "")
    }
}

#[test]
fn hs2_9abvdw_follows_four_redirects_and_fails_on_the_fifth() {
    let (origin, seen) = serve(hops);
    let mut response = agent()
        .get(&format!("{origin}/hop/{MAX_REDIRECTS}"))
        .call()
        .unwrap();
    assert_eq!(response.body_mut().read_to_string().unwrap(), "arrived");
    assert_eq!(seen.try_iter().count(), MAX_REDIRECTS as usize + 1);

    let error = agent()
        .get(&format!("{origin}/hop/{}", MAX_REDIRECTS + 1))
        .call()
        .unwrap_err();
    assert_eq!(failure_kind(&error), FailureKind::TooManyRedirects);
    // ureq 2 sent the fifth request and refused to follow its 3xx.
    assert_eq!(seen.try_iter().count(), MAX_REDIRECTS as usize + 1);
}

#[test]
fn hs2_9abvdw_a_redirect_drops_credentials_and_turns_a_post_into_a_get() {
    let (origin, seen) = serve(hops);
    let mut response = agent()
        .post(&format!("{origin}/hop/1"))
        .header("Authorization", "Bearer synthetic-token")
        .header("Cookie", "synthetic=cookie")
        .header("X-Kept", "kept")
        .send("payload")
        .unwrap();
    assert_eq!(response.body_mut().read_to_string().unwrap(), "arrived");
    let first = seen.recv().unwrap();
    assert_eq!(first.method, "POST");
    assert_eq!(
        first.header("authorization"),
        Some("Bearer synthetic-token")
    );
    assert_eq!(first.body, b"payload");
    let second = seen.recv().unwrap();
    assert_eq!(
        (second.method.as_str(), second.path.as_str()),
        ("GET", "/hop/0")
    );
    assert_eq!(second.header("authorization"), None);
    assert_eq!(second.header("cookie"), None);
    assert_eq!(second.header("x-kept"), Some("kept"));
    assert!(second.body.is_empty());
}

#[test]
fn hs2_9abvdw_no_follow_returns_the_redirect_itself() {
    let (origin, seen) = serve(hops);
    let agent = build(config().max_redirects(0));
    let response = agent.get(&format!("{origin}/hop/3")).call().unwrap();
    assert_eq!(response.status().as_u16(), 302);
    assert_eq!(hotsheet_http::header(&response, "location"), Some("/hop/2"));
    assert_eq!(seen.try_iter().count(), 1);
}

#[test]
fn hs2_9abvdw_error_statuses_fail_unless_the_caller_reads_them() {
    let (origin, _seen) = serve(|seen| match seen.path.as_str() {
        "/missing" => reply("404 Not Found", &[], "{\"code\":\"synthetic\"}"),
        _ => reply("503 Service Unavailable", &[("Retry-After", "7")], "busy"),
    });
    let error = agent()
        .get(&format!("{origin}/missing"))
        .call()
        .unwrap_err();
    assert_eq!(failure_kind(&error), FailureKind::Status(404));
    let error = agent().get(&format!("{origin}/busy")).call().unwrap_err();
    assert_eq!(failure_kind(&error), FailureKind::Status(503));

    let reading = build(config().http_status_as_error(false));
    let mut response = reading.get(&format!("{origin}/missing")).call().unwrap();
    assert_eq!(response.status().as_u16(), 404);
    assert_eq!(
        response.body_mut().read_to_string().unwrap(),
        "{\"code\":\"synthetic\"}"
    );
    let response = reading.get(&format!("{origin}/busy")).call().unwrap();
    assert_eq!(hotsheet_http::header(&response, "retry-after"), Some("7"));
}

#[test]
fn hs2_9abvdw_a_refused_connection_is_a_connection_failure() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let error = agent().get(&format!("{origin}/")).call().unwrap_err();
    assert_eq!(
        failure_kind(&error),
        FailureKind::ConnectionFailed,
        "{error}"
    );
}

// HS2-C7HPDB: not on Windows. There a plain-HTTP reply to a TLS client is aborted by the OS
// (os error 10053) before rustls reports anything, and an abort cannot be told from one after
// the request was sent, which must stay `Io`. The certificate case this stands for is covered
// on every platform by own_tls.rs (a CA the roots do not hold fails as `ConnectionFailed`).
#[cfg(not(windows))]
#[test]
fn hs2_9abvdw_a_failed_tls_handshake_is_a_connection_failure() {
    // A plain-HTTP server answering a TLS client: the handshake fails, as it would for a
    // certificate the trust refuses. ureq 2 reported both as a connection failure.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let _ = stream.write_all(&reply("200 OK", &[], "plain"));
        }
    });
    let error = agent()
        .get(&format!("https://127.0.0.1:{port}/"))
        .call()
        .unwrap_err();
    assert_eq!(
        failure_kind(&error),
        FailureKind::ConnectionFailed,
        "{error}"
    );
}

#[test]
fn hs2_9abvdw_unusable_urls_and_hosts_are_classified() {
    let error = agent().get("not a url").call().unwrap_err();
    assert_eq!(failure_kind(&error), FailureKind::InvalidUrl, "{error}");
    let error = agent().get("ftp://127.0.0.1/").call().unwrap_err();
    assert_eq!(failure_kind(&error), FailureKind::InvalidUrl, "{error}");
    let error = agent()
        .get("http://hs2-9abvdw.invalid/")
        .call()
        .unwrap_err();
    assert_eq!(failure_kind(&error), FailureKind::Dns, "{error}");
}

#[test]
fn hs2_9abvdw_read_to_string_is_limited_to_ten_megabytes() {
    const LIMIT: usize = 10 * 1024 * 1024;
    let (origin, _seen) = serve(|seen| {
        let size = if seen.path == "/at" { LIMIT } else { LIMIT + 1 };
        let mut bytes =
            format!("HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: {size}\r\n\r\n")
                .into_bytes();
        bytes.resize(bytes.len() + size, b'a');
        bytes
    });
    let mut response = agent().get(&format!("{origin}/at")).call().unwrap();
    assert_eq!(response.body_mut().read_to_string().unwrap().len(), LIMIT);
    let mut response = agent().get(&format!("{origin}/over")).call().unwrap();
    assert!(response.body_mut().read_to_string().is_err());
}

/// A server that sends headers and part of a body, then holds the connection silently
/// until the test ends.
fn stall() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    thread::spawn(move || {
        let mut held = Vec::new();
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            if read_request(&stream).is_none() {
                continue;
            }
            let _ = stream.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\ndata: first\n",
            );
            held.push(stream);
        }
    });
    origin
}

#[test]
fn hs2_9abvdw_a_stalled_body_times_out_on_the_per_read_and_the_overall_bound() {
    let origin = stall();
    for agent in [
        build(config().timeout_per_read(Some(Duration::from_millis(200)))),
        build(config().timeout_global(Some(Duration::from_millis(400)))),
    ] {
        let started = Instant::now();
        let response = agent.get(&format!("{origin}/stream")).call().unwrap();
        let mut reader = BufReader::new(response.into_body().into_reader());
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert_eq!(line, "data: first\n");
        let error = reader.read_line(&mut line).unwrap_err();
        assert!(is_timeout(&error), "{error:?}");
        assert!(started.elapsed() < Duration::from_secs(10));
    }
    assert!(!is_timeout(&std::io::Error::other("synthetic")));
}

const PROXY_CHILD: &str = "HS2_9ABVDW_PROXY_CHILD";

/// Proxy environment variables are never read: with every proxy variable pointing at a
/// closed port, a request still reaches the server directly. Runs in a child process so
/// the variables never touch the test process's environment.
#[test]
fn hs2_9abvdw_ignores_proxy_environment_variables() {
    if let Ok(origin) = std::env::var(PROXY_CHILD) {
        let mut response = agent().get(&format!("{origin}/direct")).call().unwrap();
        assert_eq!(response.body_mut().read_to_string().unwrap(), "direct");
        return;
    }
    let (origin, seen) = serve(|_| reply("200 OK", &[], "direct"));
    let closed = TcpListener::bind("127.0.0.1:0").unwrap();
    let proxy = format!("http://{}", closed.local_addr().unwrap());
    drop(closed);
    let mut child = std::process::Command::new(std::env::current_exe().unwrap());
    child
        .args([
            "--exact",
            "hs2_9abvdw_ignores_proxy_environment_variables",
            "--quiet",
        ])
        .env(PROXY_CHILD, &origin)
        .env_remove("NO_PROXY")
        .env_remove("no_proxy");
    for name in [
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
    ] {
        child.env(name, &proxy);
    }
    let output = child.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(seen.recv().unwrap().path, "/direct");
    assert!(agent().config().proxy().is_none());
}

#[test]
fn hs2_9abvdw_run_sends_any_method_with_or_without_a_body() {
    let (origin, seen) = serve(|_| reply("200 OK", &[], "done"));
    let agent = agent();
    let headers = [("Content-Type", "application/json"), ("X-Synthetic", "1")];
    for (method, body) in [
        ("PATCH", Some(&b"{\"a\":1}"[..])),
        ("DELETE", Some(&b"{}"[..])),
        ("DELETE", None),
        ("GET", None),
    ] {
        let mut response =
            hotsheet_http::run(&agent, method, &format!("{origin}/run"), &headers, body).unwrap();
        assert_eq!(response.body_mut().read_to_string().unwrap(), "done");
        let request = seen.recv().unwrap();
        assert_eq!(request.method, method);
        assert_eq!(request.header("x-synthetic"), Some("1"));
        assert_eq!(request.body, body.unwrap_or_default());
    }
    // A body-less POST, PUT or PATCH sends an empty body with `Content-Length: 0`, never a
    // chunked one, as ureq 2's `call()` did not send one either.
    for method in ["POST", "PUT", "PATCH"] {
        hotsheet_http::run(&agent, method, &format!("{origin}/run"), &[], None).unwrap();
        let request = seen.recv().unwrap();
        assert_eq!(request.method, method);
        assert_eq!(request.header("content-length"), Some("0"), "{method}");
        assert_eq!(request.header("transfer-encoding"), None, "{method}");
        assert!(request.body.is_empty());
    }
    let error = hotsheet_http::run(&agent, "NOT A METHOD", &origin, &[], None).unwrap_err();
    assert!(matches!(error, ureq::Error::Http(_)), "{error}");
}
