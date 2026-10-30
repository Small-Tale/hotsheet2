//! HS2-081PXN: authored synthetic startup/presence confinement; no user host or credentials.

#[path = "support/git.rs"]
mod git_fixture;
#[path = "startup_presence_redirects/http.rs"]
mod http;
#[path = "startup_presence_redirects/stdio.rs"]
mod stdio;

use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use hotsheet_mcp::presence::{HttpPresence, Presence, PresenceTarget, PresenceTransport};
use hotsheet_ticketing::StoreMetadata;
use serde_json::{Value, json};

use http::{Response, SECRET, Server, gate};
use stdio::{Completed, Process, Project};

fn tools_list() -> Value {
    json!({"jsonrpc":"2.0", "id":1, "method":"tools/list", "params":{}})
}

fn reject_startup(selected: &Server, status: u16) -> Completed {
    let project = Project::new(&selected.url());
    let mut child = project.spawn();
    // Fast startup refusal may close stdin before this harmless queued request is written.
    let _ = child.send(tools_list());
    let output = child.finish();
    project.assert_no_relocation();
    assert!(!output.status.success());
    assert!(
        output
            .stderr
            .contains(&format!("project-open redirect refused (HTTP {status})"))
    );
    assert!(!output.stderr.contains("private-response-marker"));
    assert!(!output.stderr.contains("untrusted-location-marker"));
    assert!(!output.stderr.contains(SECRET));
    let requests = selected.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/projects/open");
    assert!(requests[0].expected_secret && requests[0].secret_header);
    assert!(requests[0].body["root"].is_string());
    output
}

#[test]
fn hs2_081pxn_actual_project_startup_never_follows_a_foreign_redirect() {
    for status in [301, 302, 303, 307, 308, 300, 304, 399] {
        let foreign = Server::start(vec![]);
        let selected = Server::start(vec![Response::redirect(
            status,
            Some(format!("{}/untrusted-location-marker", foreign.url())),
        )]);
        let output = reject_startup(&selected, status);
        assert!(!output.stderr.contains(&foreign.url()));
        // Child exit is the completion barrier; the foreign server answers a broken baseline.
        assert!(foreign.requests().is_empty());
    }
}

#[test]
fn hs2_081pxn_project_startup_rejects_missing_malformed_relative_and_same_origin_locations() {
    for location in [
        None,
        Some("http://["),
        Some("/untrusted-location-marker"),
        Some("untrusted-location-marker"),
    ] {
        let selected = Server::start(vec![Response::redirect(302, location.map(str::to_owned))]);
        reject_startup(&selected, 302);
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let location = format!(
        "http://{}/untrusted-location-marker",
        listener.local_addr().unwrap()
    );
    let selected = Server::from_listener(listener, vec![Response::redirect(307, Some(location))]);
    reject_startup(&selected, 307);
}

#[test]
fn hs2_081pxn_project_startup_rejects_headers_before_reading_a_gated_redirect_body() {
    let (body_gate, control) = gate();
    let mut response = Response::redirect(302, None);
    response.before_body = Some(body_gate);
    let selected = Server::start(vec![response]);
    let project = Project::new(&selected.url());
    let mut child = project.spawn();
    let _ = child.send(tools_list());
    control.wait();
    let completed_before_body = child.wait_before(Duration::from_secs(2));
    control.release();
    let output = child.finish();
    project.assert_no_relocation();
    assert!(
        completed_before_body,
        "project-open waited for an untrusted redirect body"
    );
    assert!(!output.status.success());
    assert!(
        output
            .stderr
            .contains("project-open redirect refused (HTTP 302)")
    );
    assert!(!output.stderr.contains("private-response-marker"));
    assert!(!output.stderr.contains(SECRET));
    assert_eq!(selected.requests().len(), 1);
}

#[test]
fn hs2_081pxn_nonredirected_project_startup_keeps_success_and_http_failure_semantics() {
    for status in [200, 201, 401, 500] {
        let selected = Server::start(vec![Response::json(
            status,
            json!({"checkout":{"id":"synthetic-checkout"}}),
        )]);
        let project = Project::new(&selected.url());
        let mut child = project.spawn();
        let _ = child.send(tools_list());
        let output = child.finish();
        project.assert_no_relocation();
        assert_eq!(output.status.success(), status < 400);
        if status < 400 {
            assert_eq!(output.replies.len(), 1);
            assert!(output.replies[0]["result"]["tools"].is_array());
        }
        assert_eq!(selected.requests().len(), 1);
    }
}

fn target(origin: &str) -> PresenceTarget {
    PresenceTarget::resolve(
        Some(origin),
        Some(SECRET),
        |_| None,
        || None,
        1,
        None,
        "synthetic-lease".into(),
    )
    .unwrap()
}

#[test]
fn hs2_081pxn_presence_upsert_and_release_never_follow_foreign_redirects() {
    for status in [301, 302, 303, 307, 308, 300, 304, 399] {
        let foreign = Server::start(vec![]);
        let location = format!("{}/untrusted-location-marker", foreign.url());
        let selected = Server::start(vec![
            Response::redirect(status, Some(location.clone())),
            Response::redirect(status, Some(location)),
        ]);
        let transport = HttpPresence::default();
        let target = target(&selected.url());
        assert!(!transport.upsert(&target));
        transport.release(&target); // Best effort stays silent, even when cleanup is refused.
        let requests = selected.requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].method, "POST");
        assert_eq!(requests[0].path, "/mcp-presence");
        assert_eq!(requests[0].body, target.body);
        assert_eq!(requests[1].method, "DELETE");
        assert_eq!(requests[1].path, "/mcp-presence/synthetic-lease");
        assert!(
            requests
                .iter()
                .all(|request| request.expected_secret && request.secret_header)
        );
        assert!(foreign.requests().is_empty());
    }
}

#[test]
fn hs2_081pxn_presence_rejects_nonforeign_locations_without_a_second_attempt() {
    for location in [
        None,
        Some("http://["),
        Some("/untrusted-location-marker"),
        Some("untrusted-location-marker"),
    ] {
        let selected = Server::start(vec![
            Response::redirect(302, location.map(str::to_owned)),
            Response::redirect(307, location.map(str::to_owned)),
        ]);
        let transport = HttpPresence::default();
        let target = target(&selected.url());
        assert!(!transport.upsert(&target));
        transport.release(&target);
        assert_eq!(selected.requests().len(), 2);
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let location = format!(
        "http://{}/untrusted-location-marker",
        listener.local_addr().unwrap()
    );
    let selected = Server::from_listener(
        listener,
        vec![
            Response::redirect(302, Some(location.clone())),
            Response::redirect(307, Some(location)),
        ],
    );
    let transport = HttpPresence::default();
    let target = target(&selected.url());
    assert!(!transport.upsert(&target));
    transport.release(&target);
    assert_eq!(selected.requests().len(), 2);
}

#[test]
fn hs2_081pxn_presence_keeps_2xx_errors_and_the_existing_header_timeout() {
    for status in [200, 201, 204, 401, 503] {
        let selected = Server::start(vec![
            Response::json(status, json!({})),
            Response::json(status, json!({})),
        ]);
        let transport = HttpPresence::default();
        let target = target(&selected.url());
        assert_eq!(transport.upsert(&target), status < 400);
        transport.release(&target);
        assert_eq!(selected.requests().len(), 2);
    }
    let unavailable = target("http://127.0.0.1:0");
    let transport = HttpPresence::default();
    assert!(!transport.upsert(&unavailable));
    transport.release(&unavailable);
    // A body gate pins header-only refusal; a header gate pins the existing five-second timeout.
    for gate_headers in [false, true] {
        let (response_gate, control) = gate();
        let mut response = Response::json(if gate_headers { 200 } else { 302 }, json!({}));
        if gate_headers {
            response.before_headers = Some(response_gate);
        } else {
            response.before_body = Some(response_gate);
        }
        let selected = Server::start(vec![response]);
        let target = target(&selected.url());
        let (completed, result) = mpsc::channel();
        let worker = thread::spawn(move || {
            completed
                .send(HttpPresence::default().upsert(&target))
                .unwrap()
        });
        control.wait();
        let before_release =
            result.recv_timeout(Duration::from_secs(if gate_headers { 12 } else { 2 }));
        control.release();
        worker.join().unwrap();
        assert!(!before_release.expect("presence waited for gated data beyond its request bound"));
        assert_eq!(selected.requests().len(), 1);
    }
}

#[test]
fn hs2_081pxn_actual_stdio_initialize_and_eof_keep_presence_best_effort() {
    let foreign = Server::start(vec![]);
    let location = format!("{}/untrusted-location-marker", foreign.url());
    let selected = Server::start(vec![
        Response::redirect(303, Some(location.clone())),
        Response::redirect(307, Some(location)),
    ]);
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let store = root.path().join("store");
    git_fixture::init(&store, &StoreMetadata::new("SYN"));
    let mut child = Process::start(
        &home,
        &["--path", store.to_str().unwrap()],
        Some(&selected.url()),
    );
    child
        .send(json!({"jsonrpc":"2.0", "id":1, "method":"initialize", "params":{}}))
        .unwrap();
    assert_eq!(child.message()["id"], 1);
    let upsert = selected.next();
    assert_eq!(upsert.method, "POST");
    assert_eq!(upsert.path, "/mcp-presence");
    assert!(upsert.expected_secret);
    let lease = upsert.body["lease_id"].as_str().unwrap();
    let output = child.finish();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let requests = selected.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "DELETE");
    assert_eq!(requests[0].path, format!("/mcp-presence/{lease}"));
    assert!(requests[0].expected_secret);
    assert!(foreign.requests().is_empty());
}

#[test]
fn hs2_081pxn_failed_redirect_upsert_can_relocate_only_from_the_trusted_registry() {
    let foreign = Server::start(vec![]);
    let selected = Server::start(vec![Response::redirect(302, Some(foreign.url()))]);
    let trusted = Server::with_secret(
        vec![
            Response::json(200, json!({})),
            Response::json(200, json!({})),
        ],
        "synthetic-registry-bearer",
    );
    let trusted_origin = trusted.url();
    let target = target(&selected.url());
    let body = target.body.clone();
    Presence::start_relocating(
        target,
        HttpPresence::default(),
        Duration::from_secs(60),
        move || Some((trusted_origin.clone(), "synthetic-registry-bearer".into())),
    )
    .stop();
    assert_eq!(selected.requests().len(), 1);
    assert!(foreign.requests().is_empty());
    let requests = trusted.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].body, body);
    assert_eq!(requests[1].method, "DELETE");
    assert_eq!(requests[1].path, "/mcp-presence/synthetic-lease");
    assert!(requests.iter().all(|request| request.expected_secret));
}
