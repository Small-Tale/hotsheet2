use super::*;
use serde_json::{Value, json};
use std::io::Write;
use std::net::TcpListener;

fn credentials() -> RemoteCredentials {
    RemoteCredentials {
        bearer_secret: "synthetic-transport-bearer".into(),
        ca_pem: None,
        client_certificate_pem: None,
        client_key_pem: None,
    }
}

fn response(status: u16, headers: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status} Synthetic\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn peer(responses: Vec<String>) -> (String, std::thread::JoinHandle<()>) {
    peer_expected(responses, "synthetic-transport-bearer")
}

fn peer_expected(responses: Vec<String>, expected: &str) -> (String, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let expected = expected.to_owned();
    let worker = std::thread::spawn(move || {
        for response in responses {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            std::time::Instant::now() < deadline,
                            "expected synthetic request"
                        );
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("synthetic accept: {error}"),
                }
            };
            // HS2-013E3P: accepted fixture sockets can inherit nonblocking mode.
            stream.set_nonblocking(false).unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut bytes = Vec::new();
            let mut byte = [0; 1];
            while !bytes.ends_with(b"\r\n\r\n") {
                assert_eq!(stream.read(&mut byte).unwrap(), 1);
                bytes.push(byte[0]);
                assert!(bytes.len() < 16_384);
            }
            let request = String::from_utf8(bytes).unwrap();
            assert!(
                request.contains(&expected),
                "missing expected credential header"
            );
            if expected.contains("hs2_remote_session=") {
                assert!(!request.to_ascii_lowercase().contains("x-hotsheet-secret:"));
            }
            // Bounded readers may close before receiving the oversized tail.
            let _ = stream.write_all(response.as_bytes());
        }
    });
    (origin, worker)
}

#[test]
fn hs2_w4jjje_remote_session_uses_cookie_without_forwarding_machine_bearer() {
    let (origin, worker) = peer_expected(
        vec![response(200, "", "{}")],
        "hs2_remote_session=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
    );
    let mut paired = credentials();
    paired.bearer_secret = format!("session:{}", "A".repeat(43));
    let agent = ureq::AgentBuilder::new().redirects(0).build();
    assert_eq!(
        get::<Value>(&agent, &origin, "/synthetic", &paired).unwrap(),
        json!({})
    );
    worker.join().unwrap();
}

#[test]
fn hs2_w4jjje_machine_bearer_is_only_attached_to_loopback_http() {
    let agent = ureq::AgentBuilder::new().build();
    let credentials = credentials();
    for origin in [
        "http://remote.example.test:1234",
        "https://remote.example.test:1234",
    ] {
        let request = remote_auth_request(
            agent.get(&format!("{origin}/synthetic")),
            origin,
            &credentials,
        );
        assert_eq!(request.header("X-Hotsheet-Secret"), None);
        assert_eq!(request.header("Cookie"), None);
    }
    let origin = "http://127.0.0.1:1234";
    let request = remote_auth_request(
        agent.get(&format!("{origin}/synthetic")),
        origin,
        &credentials,
    );
    assert_eq!(
        request.header("X-Hotsheet-Secret"),
        Some("synthetic-transport-bearer")
    );
}

#[test]
fn hs2_w4jjje_https_client_verifies_server_without_device_certificate() {
    let dir = tempfile::tempdir().unwrap();
    let paths = hotsheet_tls::Paths::at(dir.path().join("tls"));
    hotsheet_tls::init_ca(&paths, &[]).unwrap();
    let mut paired = credentials();
    paired.bearer_secret = format!("session:{}", "A".repeat(43));
    paired.ca_pem = Some(std::fs::read_to_string(paths.ca_cert()).unwrap());
    assert!(
        paired
            .trust_fingerprint("https://remote.example.test")
            .unwrap()
            .is_some()
    );
    let client = client_config(&paired).unwrap();
    assert_eq!(client.alpn_protocols, vec![b"http/1.1".to_vec()]);
}

#[test]
fn hs2_1y45mz_transport_refuses_redirect_without_forwarding_the_bearer() {
    let destination = TcpListener::bind("127.0.0.1:0").unwrap();
    destination.set_nonblocking(true).unwrap();
    let headers = format!(
        "Location: http://{}/untrusted\r\n",
        destination.local_addr().unwrap()
    );
    let (origin, worker) = peer(vec![response(302, &headers, "")]);
    assert_eq!(
        MtlsRemoteTransport
            .probe(&origin, &credentials())
            .unwrap_err(),
        RemoteFailure::Redirect
    );
    worker.join().unwrap();
    assert_eq!(
        destination.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[test]
fn hs2_1y45mz_transport_bounds_responses_and_discards_private_peer_errors() {
    for (status, headers, body, expected) in [
        (
            401,
            "",
            "synthetic-private-detail".into(),
            RemoteFailure::Unauthorized,
        ),
        (
            403,
            "",
            "synthetic-private-detail".into(),
            RemoteFailure::Unauthorized,
        ),
        (
            404,
            "",
            "synthetic-private-detail".into(),
            RemoteFailure::Unsupported,
        ),
        (
            500,
            "",
            "synthetic-private-detail".into(),
            RemoteFailure::InvalidResponse,
        ),
        (200, "", "not json".into(), RemoteFailure::InvalidResponse),
        (
            200,
            "x-hotsheet-partial: true\r\n",
            "{}".into(),
            RemoteFailure::ResponseLimit,
        ),
        (
            200,
            "x-hotsheet-truncated: true\r\n",
            "{}".into(),
            RemoteFailure::ResponseLimit,
        ),
        (
            200,
            "",
            "x".repeat(MAX_REMOTE_RESPONSE_BYTES + 1),
            RemoteFailure::ResponseLimit,
        ),
    ] {
        let (origin, worker) = peer(vec![response(status, headers, &body)]);
        let agent = ureq::AgentBuilder::new().redirects(0).build();
        assert_eq!(
            get::<Value>(&agent, &origin, "/synthetic", &credentials()).unwrap_err(),
            expected
        );
        worker.join().unwrap();
        assert!(!expected.to_string().contains("synthetic-private-detail"));
    }
}

#[test]
fn hs2_1y45mz_transport_confirms_the_authenticated_identity_after_checkout_discovery() {
    let identity =
        json!({"generation":"hs2", "protocol":1, "server_id":"01ARZ3NDEKTSV4RRFFQ69G5FAV"});
    let changed =
        json!({"generation":"hs2", "protocol":1, "server_id":"01ARZ3NDEKTSV4RRFFQ69G5FAW"});
    let (origin, worker) = peer(vec![
        response(200, "", &identity.to_string()),
        response(200, "", "[]"),
        response(200, "", &changed.to_string()),
    ]);
    assert_eq!(
        MtlsRemoteTransport
            .probe(&origin, &credentials())
            .unwrap_err(),
        RemoteFailure::IdentityChanged
    );
    worker.join().unwrap();
}

fn hs2_4j6v46_projection() -> hotsheet_ticketing::orchestration::CheckoutProjection {
    use hotsheet_ticketing::orchestration::{
        CheckoutProjection, SourceState, TargetSource, checkout_authority,
    };
    let sources = vec![TargetSource {
        connection_id: "synthetic".into(),
        provider: "synthetic".into(),
        identity: Some("a".repeat(64)),
        state: SourceState::Available,
        capabilities: Some(hotsheet_ticketing::ProviderCapabilities::git()),
    }];
    CheckoutProjection {
        schema_version: 1,
        server_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
        checkout_id: "synthetic-checkout".into(),
        checkout_authority: checkout_authority(&sources).unwrap(),
        sources,
    }
}
#[test]
fn hs2_4j6v46_metadata_transport_confirms_server_and_checkout_after_bounded_response() {
    use hotsheet_ticketing::orchestration::TargetFailure;
    use hotsheet_ticketing::remote_servers::RemoteCheckoutTransport;
    for changed_server in [false, true] {
        let projection = hs2_4j6v46_projection();
        let confirmed = json!({"generation":"hs2","protocol":1,"server_id":if changed_server {"01ARZ3NDEKTSV4RRFFQ69G5FAW"}else{projection.server_id.as_str()}});
        let (origin, worker) = peer(vec![
            response(200, "", &serde_json::to_string(&projection).unwrap()),
            response(200, "", &confirmed.to_string()),
        ]);
        let result = MtlsRemoteTransport.checkout_projection(
            &origin,
            &credentials(),
            &projection.checkout_id,
        );
        if changed_server {
            assert_eq!(result.unwrap_err(), TargetFailure::IdentityChanged);
        } else {
            assert_eq!(result.unwrap(), projection);
        }
        worker.join().unwrap();
    }
    let mut projection = hs2_4j6v46_projection();
    projection.checkout_id = "other-checkout".into();
    let confirmed = json!({"generation":"hs2","protocol":1,"server_id":projection.server_id});
    let (origin, worker) = peer(vec![
        response(200, "", &serde_json::to_string(&projection).unwrap()),
        response(200, "", &confirmed.to_string()),
    ]);
    assert_eq!(
        MtlsRemoteTransport
            .checkout_projection(&origin, &credentials(), "synthetic-checkout")
            .unwrap_err(),
        TargetFailure::IdentityChanged
    );
    worker.join().unwrap();
}
#[test]
fn hs2_4j6v46_metadata_transport_limits_old_peers_private_errors_and_partial_data() {
    use hotsheet_ticketing::orchestration::{MAX_PROJECTION_BYTES, TargetFailure};
    use hotsheet_ticketing::remote_servers::RemoteCheckoutTransport;
    for (status, headers, body, expected) in [
        (
            404,
            "",
            "synthetic-private-detail".into(),
            TargetFailure::UnsupportedMetadata,
        ),
        (
            401,
            "",
            "synthetic-private-detail".into(),
            TargetFailure::Unauthorized,
        ),
        (
            500,
            "",
            "synthetic-private-detail".into(),
            TargetFailure::InvalidResponse,
        ),
        (200, "", "not JSON".into(), TargetFailure::InvalidResponse),
        (
            200,
            "x-hotsheet-partial: true\r\n",
            "{}".into(),
            TargetFailure::ResponseLimit,
        ),
        (
            200,
            "x-hotsheet-truncated: true\r\n",
            "{}".into(),
            TargetFailure::ResponseLimit,
        ),
        (
            200,
            "",
            "x".repeat(MAX_PROJECTION_BYTES + 1),
            TargetFailure::ResponseLimit,
        ),
    ] {
        let (origin, worker) = peer(vec![response(status, headers, &body)]);
        let error = MtlsRemoteTransport
            .checkout_projection(&origin, &credentials(), "synthetic-checkout")
            .unwrap_err();
        assert_eq!(error, expected);
        assert!(!error.to_string().contains("synthetic-private-detail"));
        worker.join().unwrap();
    }
}
#[test]
fn hs2_4j6v46_metadata_transport_refuses_redirect_and_route_relocation_before_request() {
    use hotsheet_ticketing::orchestration::TargetFailure;
    use hotsheet_ticketing::remote_servers::RemoteCheckoutTransport;
    let destination = TcpListener::bind("127.0.0.1:0").unwrap();
    destination.set_nonblocking(true).unwrap();
    let headers = format!(
        "Location: http://{}/stolen\r\n",
        destination.local_addr().unwrap()
    );
    let (origin, worker) = peer(vec![response(302, &headers, "")]);
    assert_eq!(
        MtlsRemoteTransport
            .checkout_projection(&origin, &credentials(), "synthetic-checkout")
            .unwrap_err(),
        TargetFailure::Redirect
    );
    worker.join().unwrap();
    assert_eq!(
        destination.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    for id in [
        "",
        ".",
        "..",
        "../other",
        "x?token=synthetic",
        "x#fragment",
        "x%2Fother",
    ] {
        assert_eq!(
            MtlsRemoteTransport
                .checkout_projection("https://unrequested.example.test", &credentials(), id)
                .unwrap_err(),
            TargetFailure::InvalidResponse
        );
    }
}
