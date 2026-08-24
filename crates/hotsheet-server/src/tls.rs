//! Tier-1 **mTLS** server config (`docs/04` §4.6, HS2-VT3JMF): a rustls `ServerConfig` that
//! **requires** a client certificate, verifies it chains to the project CA, and rejects any
//! cert whose fingerprint is on the revocation list. The CA + cert material comes from
//! [`hotsheet_tls`]; this module is the rustls half (kept out of that crate so the CLI can
//! issue certs without linking rustls).
//!
//! Chain verification is delegated to rustls's own [`WebPkiClientVerifier`] — we do **not**
//! hand-roll certificate-path validation. We only add a revocation gate on top: reject early
//! if the end-entity cert's SHA-256 fingerprint is revoked, else defer to WebPKI.

use std::fs::File;
use std::io::BufReader;
use std::sync::Arc;

use rustls::DistinguishedName;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, UnixTime};
use rustls::server::WebPkiClientVerifier;
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};

use hotsheet_tls::Paths;

/// Build a mutual-TLS `ServerConfig`: the server presents its leaf, and every client must
/// present a cert that chains to the project CA and isn't revoked. Errors if the store's TLS
/// material isn't initialized (`cert init`).
pub fn build_server_config(paths: &Paths) -> anyhow::Result<rustls::ServerConfig> {
    if !paths.is_initialized() {
        anyhow::bail!("TLS not initialized for this store — run `hotsheet-cli cert init` first");
    }
    // Install a process-default crypto provider once (aws-lc-rs, rustls's default); ignore the
    // error if another part of the process already installed one.
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    // The CA is the sole trust root for client certs.
    let mut roots = rustls::RootCertStore::empty();
    for cert in load_certs(&paths.ca_cert())? {
        roots.add(cert)?;
    }
    let webpki = WebPkiClientVerifier::builder(Arc::new(roots))
        .build()
        .map_err(|e| anyhow::anyhow!("client verifier: {e}"))?;
    let verifier = Arc::new(RevocationCheckingVerifier {
        inner: webpki,
        // Hold the file path (not a snapshot) and re-read it per handshake, so `cert revoke`
        // takes effect on the next connection without a server restart (HS2-MPC0QF).
        revoked_file: paths.revoked(),
    });

    let server_certs = load_certs(&paths.server_cert())?;
    let server_key = load_key(&paths.server_key())?;
    let mut config = rustls::ServerConfig::builder()
        .with_client_cert_verifier(verifier)
        .with_single_cert(server_certs, server_key)?;
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(config)
}

/// Serve `app` over mutual TLS on `listener` until `shutdown` resolves (axum 0.8 has no
/// built-in TLS, so this is the manual `tokio-rustls` acceptor + hyper connection loop). On
/// the shutdown signal we stop accepting; in-flight connections finish in their own tasks.
pub async fn serve_tls(
    listener: tokio::net::TcpListener,
    app: axum::Router,
    config: rustls::ServerConfig,
    shutdown: impl std::future::Future<Output = ()> + Send,
) -> anyhow::Result<()> {
    use hyper_util::rt::{TokioExecutor, TokioIo};
    use hyper_util::server::conn::auto::Builder;
    use hyper_util::service::TowerToHyperService;
    use tokio_rustls::TlsAcceptor;

    let acceptor = TlsAcceptor::from(Arc::new(config));
    let mut shutdown = std::pin::pin!(shutdown);
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let Ok((stream, _peer)) = accepted else { continue };
                let acceptor = acceptor.clone();
                let app = app.clone();
                tokio::spawn(async move {
                    // A failed TLS handshake (no/for bad client cert) just drops the connection.
                    let Ok(tls) = acceptor.accept(stream).await else { return };
                    let io = TokioIo::new(tls);
                    let svc = TowerToHyperService::new(app);
                    let _ = Builder::new(TokioExecutor::new())
                        .serve_connection_with_upgrades(io, svc)
                        .await;
                });
            }
            _ = &mut shutdown => break,
        }
    }
    Ok(())
}

/// Wraps rustls's WebPKI client verifier with a revocation check. Chain validation is the
/// inner verifier's job; we reject a revoked end-entity fingerprint before delegating. The
/// revocation list is re-read from disk on each handshake so a revoke applies live.
#[derive(Debug)]
struct RevocationCheckingVerifier {
    inner: Arc<dyn ClientCertVerifier>,
    revoked_file: std::path::PathBuf,
}

impl ClientCertVerifier for RevocationCheckingVerifier {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        self.inner.root_hint_subjects()
    }

    fn verify_client_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        now: UnixTime,
    ) -> Result<ClientCertVerified, rustls::Error> {
        // Cheap reject first: a revoked device never gets in, even with a valid chain. The
        // list is re-read per handshake (handshakes are infrequent), so `cert revoke` is live.
        let fpr = hotsheet_tls::fingerprint_of_der(end_entity.as_ref());
        if hotsheet_tls::load_revoked_file(&self.revoked_file).contains(&fpr) {
            return Err(rustls::Error::General(
                "client certificate has been revoked".into(),
            ));
        }
        self.inner
            .verify_client_cert(end_entity, intermediates, now)
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

fn load_certs(path: &std::path::Path) -> anyhow::Result<Vec<CertificateDer<'static>>> {
    let mut reader = BufReader::new(File::open(path)?);
    let certs = rustls_pemfile::certs(&mut reader).collect::<Result<Vec<_>, _>>()?;
    if certs.is_empty() {
        anyhow::bail!("no certificate in {}", path.display());
    }
    Ok(certs)
}

fn load_key(path: &std::path::Path) -> anyhow::Result<PrivateKeyDer<'static>> {
    let mut reader = BufReader::new(File::open(path)?);
    rustls_pemfile::private_key(&mut reader)?
        .ok_or_else(|| anyhow::anyhow!("no private key in {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use hotsheet_tls::{init_ca, issue_device, revoke_device};

    /// Decode the first certificate in a PEM to DER (test/verify helper).
    fn pem_first_der(pem: &str) -> CertificateDer<'static> {
        let mut reader = std::io::BufReader::new(pem.as_bytes());
        rustls_pemfile::certs(&mut reader).next().unwrap().unwrap()
    }

    /// Build a verifier over a fresh CA and exercise accept / reject-foreign / reject-revoked
    /// at the verifier layer (the security-critical decision), without a live socket.
    #[test]
    fn verifier_accepts_a_ca_signed_cert_rejects_foreign_and_revoked() {
        let home = tempfile::tempdir().unwrap();
        let paths = Paths::at(home.path().join("tls"));
        init_ca(&paths, &[]).unwrap();
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

        // Rebuild just the verifier (not the whole ServerConfig, which needs a live handshake).
        let mut roots = rustls::RootCertStore::empty();
        for c in load_certs(&paths.ca_cert()).unwrap() {
            roots.add(c).unwrap();
        }
        let webpki = WebPkiClientVerifier::builder(Arc::new(roots))
            .build()
            .unwrap();
        let now = UnixTime::now();

        // A device cert this CA issued verifies.
        let good = issue_device(&paths, "laptop").unwrap();
        let good_der = pem_first_der(&good.cert_pem);
        // One verifier over the live revoked-file path — reused across the revoke to prove
        // hot-reload (HS2-MPC0QF): no rebuild between the accept and the reject.
        let verifier = RevocationCheckingVerifier {
            inner: webpki,
            revoked_file: paths.revoked(),
        };
        assert!(
            verifier.verify_client_cert(&good_der, &[], now).is_ok(),
            "a CA-signed device cert should verify"
        );

        // A cert from a DIFFERENT CA is rejected (chain failure via the inner verifier).
        let other_home = tempfile::tempdir().unwrap();
        let other = Paths::at(other_home.path().join("tls"));
        init_ca(&other, &[]).unwrap();
        let foreign = issue_device(&other, "laptop").unwrap();
        let foreign_der = pem_first_der(&foreign.cert_pem);
        assert!(
            verifier.verify_client_cert(&foreign_der, &[], now).is_err(),
            "a cert from an unrelated CA must be rejected"
        );

        // Revoking the good device makes its (still CA-valid) cert fail on the SAME verifier —
        // it re-reads the revocation file per check, no restart/rebuild.
        revoke_device(&paths, "laptop").unwrap();
        assert!(
            verifier.verify_client_cert(&good_der, &[], now).is_err(),
            "a revoked cert must be rejected live, without rebuilding the verifier"
        );
    }
}
