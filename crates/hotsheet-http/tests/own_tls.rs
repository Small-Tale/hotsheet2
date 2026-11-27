//! HS2-9ABVDW: an agent with the caller's own rustls configuration uses it exactly as
//! given: its roots decide trust, its client certificate is presented, and its ALPN list
//! is offered, as the ureq 2 agents built from the same configuration did.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::{Arc, mpsc};
use std::thread;

use hotsheet_http::{FailureKind, build_with_tls, config, failure_kind};
use rcgen::{BasicConstraints, CertificateParams, IsCa, KeyPair};
use rustls::RootCertStore;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::server::WebPkiClientVerifier;

fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

struct Pki {
    ca: CertificateDer<'static>,
    server: (CertificateDer<'static>, PrivateKeyDer<'static>),
    client: (CertificateDer<'static>, PrivateKeyDer<'static>),
}

fn make_pki() -> Pki {
    let ca_key = KeyPair::generate().unwrap();
    let mut ca_params = CertificateParams::new(Vec::<String>::new()).unwrap();
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let ca = ca_params.self_signed(&ca_key).unwrap();
    let leaf = |names: Vec<String>| {
        let key = KeyPair::generate().unwrap();
        let cert = CertificateParams::new(names)
            .unwrap()
            .signed_by(&key, &ca, &ca_key)
            .unwrap();
        (
            cert.der().clone(),
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der())),
        )
    };
    Pki {
        ca: ca.der().clone(),
        server: leaf(vec!["localhost".into()]),
        client: leaf(vec!["synthetic-client".into()]),
    }
}

/// What the TLS server saw on one connection.
#[derive(Debug)]
struct Handshake {
    alpn: Option<Vec<u8>>,
    client_certificate: bool,
}

/// A TLS server for `localhost` that optionally accepts client certificates from the CA,
/// answers one request per connection and reports each handshake.
fn tls_server(pki: &Pki) -> (u16, mpsc::Receiver<Handshake>) {
    let mut roots = RootCertStore::empty();
    roots.add(pki.ca.clone()).unwrap();
    let verifier = WebPkiClientVerifier::builder_with_provider(Arc::new(roots), provider())
        .allow_unauthenticated()
        .build()
        .unwrap();
    let mut server = rustls::ServerConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_client_cert_verifier(verifier)
        .with_single_cert(vec![pki.server.0.clone()], pki.server.1.clone_key())
        .unwrap();
    server.alpn_protocols = vec![b"http/1.1".to_vec()];
    let server = Arc::new(server);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut socket) = stream else { return };
            let mut connection = rustls::ServerConnection::new(server.clone()).unwrap();
            if connection.complete_io(&mut socket).is_err() {
                continue;
            }
            let handshake = Handshake {
                alpn: connection.alpn_protocol().map(<[u8]>::to_vec),
                client_certificate: connection.peer_certificates().is_some(),
            };
            let mut tls = rustls::StreamOwned::new(connection, socket);
            let mut reader = BufReader::new(&mut tls);
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
            }
            let _ = tls.write_all(
                b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 6\r\n\r\nsecure",
            );
            let _ = tls.flush();
            tls.conn.send_close_notify();
            let _ = tls.conn.complete_io(&mut tls.sock);
            if tx.send(handshake).is_err() {
                return;
            }
        }
    });
    (port, rx)
}

fn client_config(
    trusted: &CertificateDer<'static>,
    pki: &Pki,
    with_cert: bool,
) -> rustls::ClientConfig {
    let mut roots = RootCertStore::empty();
    roots.add(trusted.clone()).unwrap();
    let builder = rustls::ClientConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots);
    let mut config = if with_cert {
        builder
            .with_client_auth_cert(vec![pki.client.0.clone()], pki.client.1.clone_key())
            .unwrap()
    } else {
        builder.with_no_client_auth()
    };
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    config
}

#[test]
fn hs2_9abvdw_own_tls_uses_the_given_roots_client_certificate_and_alpn() {
    let pki = make_pki();
    let (port, seen) = tls_server(&pki);
    let url = format!("https://localhost:{port}/");
    for with_cert in [true, false, true] {
        let agent = build_with_tls(config(), Arc::new(client_config(&pki.ca, &pki, with_cert)));
        let mut response = agent.get(&url).call().unwrap();
        assert_eq!(response.body_mut().read_to_string().unwrap(), "secure");
        let handshake = seen.recv().unwrap();
        assert_eq!(handshake.alpn.as_deref(), Some(&b"http/1.1"[..]));
        assert_eq!(handshake.client_certificate, with_cert);
    }

    // A root the server's chain does not lead to: the handshake fails, as a connection
    // failure, and no request is answered.
    let other = make_pki();
    let agent = build_with_tls(config(), Arc::new(client_config(&other.ca, &pki, false)));
    let error = agent.get(&url).call().unwrap_err();
    assert_eq!(
        failure_kind(&error),
        FailureKind::ConnectionFailed,
        "{error}"
    );
    assert!(error.to_string().contains("certificate"), "{error}");
}

#[test]
fn hs2_9abvdw_own_tls_leaves_plain_http_alone() {
    let pki = make_pki();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                break;
            }
        }
        let _ = stream
            .write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 5\r\n\r\nplain");
    });
    let agent = build_with_tls(config(), Arc::new(client_config(&pki.ca, &pki, false)));
    let mut response = agent.get(&url).call().unwrap();
    assert_eq!(response.body_mut().read_to_string().unwrap(), "plain");
}
