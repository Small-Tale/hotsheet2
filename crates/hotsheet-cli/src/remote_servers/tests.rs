use super::*;
use serde_json::{Value, json};
use std::io::Write;
use std::net::TcpListener;

fn credentials() -> RemoteCredentials {
    RemoteCredentials {
        bearer_secret: "synthetic-transport-bearer".into(),
        session_origin: None,
        session_csrf: None,
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
    paired.bearer_secret = format!("session:{}", "A".repeat(43)).into();
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
    paired.bearer_secret = format!("session:{}", "A".repeat(43)).into();
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
fn hs2_h4rebs_https_without_a_reviewed_ca_trusts_only_public_roots() {
    let mut paired = credentials();
    paired.bearer_secret = format!("session:{}", "A".repeat(43)).into();
    // A Tailscale Serve name carries a publicly trusted certificate and no reviewed CA.
    assert_eq!(
        paired
            .trust_fingerprint("https://peer.example.ts.net")
            .unwrap(),
        None
    );
    let client = client_config(&paired).unwrap();
    assert_eq!(client.alpn_protocols, vec![b"http/1.1".to_vec()]);
    let public = remote_roots(None).unwrap();
    assert_eq!(public.len(), webpki_roots::TLS_SERVER_ROOTS.len());
    // A reviewed CA pins exactly that authority; public roots are never mixed in.
    let dir = tempfile::tempdir().unwrap();
    let paths = hotsheet_tls::Paths::at(dir.path().join("tls"));
    hotsheet_tls::init_ca(&paths, &[]).unwrap();
    let ca = std::fs::read_to_string(paths.ca_cert()).unwrap();
    let pinned = remote_roots(Some(&ca)).unwrap();
    assert_eq!(pinned.len(), 1);
    // The private Hot Sheet CA is not a public root, so an unpinned client refuses it.
    let private = pinned.roots[0].subject.as_ref().to_vec();
    assert!(
        !public
            .roots
            .iter()
            .any(|root| root.subject.as_ref() == private.as_slice())
    );
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
        // HS2-4B0KSD: the peer's code tells an expired or revoked approval from any other
        // refusal; an unknown code stays plainly unauthorized.
        (
            401,
            "",
            r#"{"error":"synthetic","code":"session_expired"}"#.into(),
            RemoteFailure::SessionExpired,
        ),
        (
            401,
            "",
            r#"{"error":"synthetic","code":"session_revoked"}"#.into(),
            RemoteFailure::SessionRevoked,
        ),
        (
            401,
            "",
            r#"{"error":"synthetic","code":"session_invalid"}"#.into(),
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

#[test]
fn hs2_fzxxtg_a_pairing_bundle_supplies_identity_and_origin_for_the_same_reader() {
    const SERVER: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
    let dir = tempfile::tempdir().unwrap();
    let paths = hotsheet_tls::Paths::at(dir.path().join("tls"));
    hotsheet_tls::init_ca(&paths, &[]).unwrap();
    let device = hotsheet_tls::issue_device(&paths, "synthetic-orchestrator").unwrap();
    let bundle = pairing_bundle(
        &device,
        SERVER.into(),
        Some("https://site.example.test:8787"),
    )
    .unwrap();
    assert!(bundle.credentials.bearer_secret.is_empty());
    let text = serde_json::to_string(&bundle).unwrap();
    let input = read_remote_credential_input(text.as_bytes()).unwrap();
    assert_eq!(input.server_id.as_deref(), Some(SERVER));
    assert_eq!(input.expected_server_id(None).unwrap(), SERVER);
    assert_eq!(
        input.expected_server_id(Some(SERVER.into())).unwrap(),
        SERVER
    );
    assert!(
        input
            .expected_server_id(Some("01BX5ZZKBKACTAV9WEVGEMMVRZ".into()))
            .is_err(),
        "a typed id must match the bundle's"
    );
    assert_eq!(
        input.origin(None).unwrap(),
        "https://site.example.test:8787"
    );
    assert_eq!(
        input
            .origin(Some("https://site.tailnet.test:8787"))
            .unwrap(),
        "https://site.tailnet.test:8787",
        "the importer may reach the server by another name"
    );
    // The bundle's credentials pass the registry's own validation without a bearer and
    // pin the project CA.
    assert!(
        input
            .credentials
            .trust_fingerprint("https://site.example.test:8787")
            .unwrap()
            .is_some()
    );
    // The legacy plain object still reads and names nothing.
    let plain =
        read_remote_credential_input(serde_json::to_string(&credentials()).unwrap().as_bytes())
            .unwrap();
    assert!(plain.credentials == credentials());
    assert!(plain.expected_server_id(None).is_err());
    assert!(plain.origin(None).is_err());
    assert_eq!(
        read_remote_credentials(text.as_bytes()).unwrap(),
        bundle.credentials
    );
    // A foreign format, a malformed server id and unknown fields are refused.
    for (key, value) in [
        ("format", json!("hotsheet-pairing-bundle/v99")),
        ("server_id", json!("not-a-ulid")),
        ("unexpected", json!(true)),
    ] {
        let mut changed: Value = serde_json::from_str(&text).unwrap();
        changed[key] = value;
        assert!(
            read_remote_credential_input(changed.to_string().as_bytes()).is_err(),
            "{key}"
        );
    }
    assert!(pairing_bundle(&device, SERVER.into(), Some("ftp://site.example.test")).is_err());
}

fn paired_session() -> RemoteCredentials {
    RemoteCredentials {
        bearer_secret: format!("session:{}", "s".repeat(43)).into(),
        session_origin: Some("http://localhost:4175".into()),
        session_csrf: Some("c".repeat(43).into()),
        ..credentials()
    }
}

#[test]
fn hs2_4b0ksd_peer_session_reads_the_lifetime_and_names_each_failure() {
    let device = r#"{"id":"D1","renewed_at":10,"expires_at":20,"reminder_day":45}"#;
    let (origin, worker) = peer_expected(
        vec![response(200, "", device)],
        &format!("hs2_remote_session={}", "s".repeat(43)),
    );
    let read = peer_session_request(&origin, &paired_session(), false).unwrap();
    assert_eq!(read["reminder_day"], json!(45));
    worker.join().unwrap();
    // Renewal carries the paired Origin and CSRF proof.
    let (origin, worker) = peer_expected(vec![response(202, "", device)], "X-Hotsheet-Csrf: ccc");
    assert!(peer_session_request(&origin, &paired_session(), true).is_ok());
    worker.join().unwrap();
    for (body, expected) in [
        (
            r#"{"code":"session_expired"}"#,
            RemoteFailure::SessionExpired,
        ),
        (
            r#"{"code":"session_revoked"}"#,
            RemoteFailure::SessionRevoked,
        ),
        (r#"{"code":"session_invalid"}"#, RemoteFailure::Unauthorized),
    ] {
        let (origin, worker) = peer_expected(vec![response(401, "", body)], "hs2_remote_session=");
        assert_eq!(
            peer_session_request(&origin, &paired_session(), false).unwrap_err(),
            expected
        );
        worker.join().unwrap();
    }
    let (origin, worker) = peer_expected(vec![response(500, "", "{}")], "hs2_remote_session=");
    assert_eq!(
        peer_session_request(&origin, &paired_session(), false).unwrap_err(),
        RemoteFailure::InvalidResponse
    );
    worker.join().unwrap();
    // A certificate binding has no paired session; nothing is sent.
    assert_eq!(
        peer_session_request("http://127.0.0.1:9", &credentials(), true).unwrap_err(),
        RemoteFailure::Unsupported
    );
    // A closed port is unreachable, not expired.
    let closed = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", closed.local_addr().unwrap());
    drop(closed);
    assert_eq!(
        peer_session_request(&origin, &paired_session(), false).unwrap_err(),
        RemoteFailure::Unreachable
    );
}
