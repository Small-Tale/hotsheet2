//! End-to-end mutual-TLS handshake test (HS2-VT3JMF): boot the real `serve_tls` loop on an
//! ephemeral port and prove the security contract over a live socket —
//! a client with a CA-issued device cert gets a 200; a client with **no** client cert is
//! rejected at the handshake; a **revoked** device is rejected too.

use std::io::BufReader;
use std::sync::Arc;

use hotsheet_server::{AppState, app};
use hotsheet_ticketing::{FsStore, StoreMetadata};
use hotsheet_tls::{Paths, init_ca, issue_device, revoke_device};
use rustls::RootCertStore;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::TlsConnector;

const SECRET: &str = "test-secret";

/// Boot a real mTLS server on an ephemeral port; returns its addr + the tls Paths.
async fn boot() -> (tempfile::TempDir, std::net::SocketAddr, Paths) {
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    let state = AppState::new(store, SECRET.into()).unwrap();
    let paths = Paths::at(dir.path().join("tls"));
    init_ca(&paths, &[]).unwrap(); // server cert covers localhost + 127.0.0.1

    let config = hotsheet_server::tls::build_server_config(&paths).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        // Never-resolving shutdown: the task is dropped when the test ends.
        hotsheet_server::tls::serve_tls(listener, app(state), config, std::future::pending())
            .await
            .unwrap();
    });
    (dir, addr, paths)
}

fn root_store(paths: &Paths) -> RootCertStore {
    let mut roots = RootCertStore::empty();
    let mut r = BufReader::new(std::fs::File::open(paths.ca_cert()).unwrap());
    for c in rustls_pemfile::certs(&mut r) {
        roots.add(c.unwrap()).unwrap();
    }
    roots
}

fn client_identity(
    cert_pem: &str,
    key_pem: &str,
) -> (Vec<CertificateDer<'static>>, PrivateKeyDer<'static>) {
    let certs = rustls_pemfile::certs(&mut BufReader::new(cert_pem.as_bytes()))
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let key = rustls_pemfile::private_key(&mut BufReader::new(key_pem.as_bytes()))
        .unwrap()
        .unwrap();
    (certs, key)
}

/// Connect with the given rustls client config and return the raw HTTP response to
/// `GET /health`, or an error if the TLS handshake failed.
async fn get_health(
    addr: std::net::SocketAddr,
    config: rustls::ClientConfig,
) -> anyhow::Result<String> {
    let connector = TlsConnector::from(Arc::new(config));
    let tcp = tokio::net::TcpStream::connect(addr).await?;
    let server_name = ServerName::try_from("localhost").unwrap();
    let mut tls = connector.connect(server_name, tcp).await?; // handshake (client cert sent here)
    tls.write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await?;
    let mut buf = Vec::new();
    tls.read_to_end(&mut buf).await?;
    Ok(String::from_utf8_lossy(&buf).to_string())
}

#[tokio::test]
async fn a_ca_issued_client_cert_gets_in_and_no_cert_or_revoked_is_rejected() {
    let (_d, addr, paths) = boot().await;
    let roots = root_store(&paths);

    // (1) A client presenting a CA-issued device cert completes the handshake and gets 200.
    let dev = issue_device(&paths, "laptop").unwrap();
    let (chain, key) = client_identity(&dev.cert_pem, &dev.key_pem);
    let good = rustls::ClientConfig::builder()
        .with_root_certificates(roots.clone())
        .with_client_auth_cert(chain, key)
        .unwrap();
    let resp = get_health(addr, good)
        .await
        .expect("valid client cert should connect");
    assert!(resp.contains("200 OK"), "expected 200, got:\n{resp}");
    assert!(resp.contains("\"status\":\"ok\""));

    // (2) A client with NO client cert is rejected — the server requires mutual auth.
    let anon = rustls::ClientConfig::builder()
        .with_root_certificates(roots.clone())
        .with_no_client_auth();
    assert!(
        get_health(addr, anon).await.is_err(),
        "a client with no certificate must be rejected"
    );

    // (3) Revocation is LIVE (HS2-MPC0QF): revoke the device, then reconnect to the SAME
    // already-running server — no restart, no config rebuild — and it's rejected because the
    // verifier re-reads the revocation file per handshake.
    revoke_device(&paths, "laptop").unwrap();
    let (chain, key) = client_identity(&dev.cert_pem, &dev.key_pem);
    let revoked_client = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_client_auth_cert(chain, key)
        .unwrap();
    assert!(
        get_health(addr, revoked_client).await.is_err(),
        "a revoked device must be rejected live, without restarting the server"
    );
}
