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
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
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
            assert!(
                String::from_utf8(bytes)
                    .unwrap()
                    .contains("synthetic-transport-bearer")
            );
            // Bounded readers may close before receiving the oversized tail.
            let _ = stream.write_all(response.as_bytes());
        }
    });
    (origin, worker)
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
