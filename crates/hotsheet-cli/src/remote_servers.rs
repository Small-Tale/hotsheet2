//! Shared live adapter for the CLI, HTTP service and serverless MCP remote registry.
//! HS2-1Y45MZ keeps authenticated transport outside the network-free ticketing core.

use std::io::{BufReader, Read};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use hotsheet_ticketing::remote_servers::{
    MAX_REMOTE_RESPONSE_BYTES, RegisterRemote, RemoteCredentials, RemoteDiscovery, RemoteError,
    RemoteFailure, RemoteIdentity, RemoteRegistry, RemoteSnapshot, RemoteTransport, RemoteView,
    ReviewRemote, RevokeRemote, normalize_remote_origin, validate_remote_checkouts,
};
use hotsheet_ticketing::{OsKeychain, SecretStore};
use serde::de::DeserializeOwned;

pub struct RemoteRuntime {
    registry: RemoteRegistry,
    secrets: Arc<dyn SecretStore + Send + Sync>,
    transport: Arc<dyn RemoteTransport + Send + Sync>,
}

impl RemoteRuntime {
    pub fn live(home: &Path) -> Self {
        Self::new(
            RemoteRegistry::new(home.join("remotes.json")),
            Arc::new(OsKeychain::at(home)),
            Arc::new(MtlsRemoteTransport),
        )
    }

    pub fn new(
        registry: RemoteRegistry,
        secrets: Arc<dyn SecretStore + Send + Sync>,
        transport: Arc<dyn RemoteTransport + Send + Sync>,
    ) -> Self {
        Self {
            registry,
            secrets,
            transport,
        }
    }

    pub fn list(&self) -> Result<Vec<RemoteView>, RemoteError> {
        self.registry.list()
    }

    pub fn register(&self, mut request: RegisterRemote) -> Result<RemoteDiscovery, RemoteError> {
        request.origin = normalize_remote_origin(&request.origin)?;
        self.registry.register(
            request,
            self.secrets.as_ref(),
            self.transport.as_ref(),
            now_ms(),
        )
    }

    pub fn review(&self, id: &str, request: ReviewRemote) -> Result<RemoteDiscovery, RemoteError> {
        self.registry.review(
            id,
            request,
            self.secrets.as_ref(),
            self.transport.as_ref(),
            now_ms(),
        )
    }

    pub fn revoke(&self, id: &str, expected_revision: u64) -> Result<RemoteView, RemoteError> {
        self.registry
            .revoke(id, expected_revision, self.secrets.as_ref())
    }

    pub fn discover(&self, id: &str) -> Result<RemoteDiscovery, RemoteError> {
        self.registry
            .discover(id, self.secrets.as_ref(), self.transport.as_ref(), now_ms())
    }

    /// The same machine-local route workflow used by serverless MCP and HTTP handlers.
    pub fn request(
        &self,
        method: &str,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, RemoteError> {
        fn value(result: impl serde::Serialize) -> Result<serde_json::Value, RemoteError> {
            serde_json::to_value(result).map_err(|_| RemoteError::Corrupt)
        }
        let mut body = body.clone();
        if let Some(object) = body.as_object_mut() {
            object.remove("actor");
        }
        let parts: Vec<_> = path.trim_start_matches('/').split('/').collect();
        match (method, parts.as_slice()) {
            ("GET", ["remotes"]) => value(self.list()?),
            ("GET", ["remote-identity"]) => {
                let home = self
                    .registry
                    .path()
                    .parent()
                    .ok_or(RemoteError::Invalid("registry needs a machine directory"))?;
                value(hotsheet_ticketing::remote_servers::server_identity(
                    home,
                    || ulid::Ulid::new().to_string(),
                )?)
            }
            ("POST", ["remotes"]) => value(
                self.register(
                    serde_json::from_value(body)
                        .map_err(|_| RemoteError::Invalid("invalid registration request"))?,
                )?,
            ),
            ("PUT", ["remotes", id]) => value(
                self.review(
                    id,
                    serde_json::from_value(body)
                        .map_err(|_| RemoteError::Invalid("invalid review request"))?,
                )?,
            ),
            ("DELETE", ["remotes", id]) => {
                let request: RevokeRemote = serde_json::from_value(body)
                    .map_err(|_| RemoteError::Invalid("invalid revoke request"))?;
                value(self.revoke(id, request.expected_revision)?)
            }
            ("GET", ["remotes", id, "checkouts"]) => value(self.discover(id)?),
            _ => Err(RemoteError::Invalid("unsupported remote registry route")),
        }
    }
}

pub fn read_remote_credentials(input: impl Read) -> Result<RemoteCredentials, RemoteError> {
    let mut bytes = Vec::new();
    input.take(400 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 400 * 1024 {
        return Err(RemoteError::Invalid("remote credential input is too large"));
    }
    serde_json::from_slice(&bytes)
        .map_err(|_| RemoteError::Invalid("invalid remote credential input"))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|time| time.as_millis().try_into().unwrap_or(u64::MAX))
        .unwrap_or(0)
}

pub struct MtlsRemoteTransport;

impl RemoteTransport for MtlsRemoteTransport {
    fn probe(
        &self,
        origin: &str,
        credentials: &RemoteCredentials,
    ) -> Result<RemoteSnapshot, RemoteFailure> {
        let normalized = normalize_remote_origin(origin).map_err(|_| RemoteFailure::TrustFailed)?;
        if normalized != origin {
            return Err(RemoteFailure::TrustFailed);
        }
        credentials
            .trust_fingerprint(origin)
            .map_err(|_| RemoteFailure::TrustFailed)?;
        let mut builder = ureq::AgentBuilder::new()
            .redirects(0)
            .timeout_connect(Duration::from_secs(5))
            .timeout_read(Duration::from_secs(10))
            .timeout_write(Duration::from_secs(5))
            .timeout(Duration::from_secs(15));
        if origin.starts_with("https://") {
            builder = builder.tls_config(Arc::new(client_config(credentials)?));
        }
        let agent = builder.build();
        let identity: RemoteIdentity = get(&agent, origin, "/remote-identity", credentials)?;
        identity.validate()?;
        let checkouts: Vec<hotsheet_ticketing::remote_servers::RemoteCheckout> =
            get(&agent, origin, "/remote-checkouts", credentials)?;
        validate_remote_checkouts(&checkouts)?;
        // A restart/replacement between requests must not return another server's projects.
        let confirmed: RemoteIdentity = get(&agent, origin, "/remote-identity", credentials)?;
        confirmed.validate()?;
        if identity != confirmed {
            return Err(RemoteFailure::IdentityChanged);
        }
        Ok(RemoteSnapshot {
            identity,
            checkouts,
        })
    }
}

fn client_config(credentials: &RemoteCredentials) -> Result<rustls::ClientConfig, RemoteFailure> {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let ca = credentials
        .ca_pem
        .as_deref()
        .ok_or(RemoteFailure::TrustFailed)?;
    let mut roots = rustls::RootCertStore::empty();
    for certificate in rustls_pemfile::certs(&mut BufReader::new(ca.as_bytes())) {
        roots
            .add(certificate.map_err(|_| RemoteFailure::TrustFailed)?)
            .map_err(|_| RemoteFailure::TrustFailed)?;
    }
    if roots.is_empty() {
        return Err(RemoteFailure::TrustFailed);
    }
    let certificates = credentials
        .client_certificate_pem
        .as_deref()
        .ok_or(RemoteFailure::TrustFailed)?;
    let certificates = rustls_pemfile::certs(&mut BufReader::new(certificates.as_bytes()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| RemoteFailure::TrustFailed)?;
    let key = credentials
        .client_key_pem
        .as_deref()
        .ok_or(RemoteFailure::TrustFailed)?;
    let key = rustls_pemfile::private_key(&mut BufReader::new(key.as_bytes()))
        .map_err(|_| RemoteFailure::TrustFailed)?
        .ok_or(RemoteFailure::TrustFailed)?;
    let mut config = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_client_auth_cert(certificates, key)
        .map_err(|_| RemoteFailure::TrustFailed)?;
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(config)
}

fn get<T: DeserializeOwned>(
    agent: &ureq::Agent,
    origin: &str,
    path: &str,
    credentials: &RemoteCredentials,
) -> Result<T, RemoteFailure> {
    let result = agent
        .get(&format!("{origin}{path}"))
        .set("X-Hotsheet-Secret", &credentials.bearer_secret)
        .set("Accept", "application/json")
        .call();
    let response = match result {
        Ok(response) => response,
        Err(ureq::Error::Status(401 | 403, _)) => return Err(RemoteFailure::Unauthorized),
        Err(ureq::Error::Status(404, _)) => return Err(RemoteFailure::Unsupported),
        Err(ureq::Error::Status(300..=399, _)) => return Err(RemoteFailure::Redirect),
        Err(ureq::Error::Status(_, _)) => return Err(RemoteFailure::InvalidResponse),
        Err(ureq::Error::Transport(error)) => {
            return Err(if error.kind() == ureq::ErrorKind::ConnectionFailed {
                RemoteFailure::Unreachable
            } else {
                RemoteFailure::TrustFailed
            });
        }
    };
    if (300..400).contains(&response.status()) {
        return Err(RemoteFailure::Redirect);
    }
    if response.status() != 200 {
        return Err(RemoteFailure::InvalidResponse);
    }
    if response.header("x-hotsheet-truncated") == Some("true")
        || response.header("x-hotsheet-partial") == Some("true")
    {
        return Err(RemoteFailure::ResponseLimit);
    }
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take((MAX_REMOTE_RESPONSE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| RemoteFailure::Unreachable)?;
    if bytes.len() > MAX_REMOTE_RESPONSE_BYTES {
        return Err(RemoteFailure::ResponseLimit);
    }
    serde_json::from_slice(&bytes).map_err(|_| RemoteFailure::InvalidResponse)
}

#[cfg(test)]
#[path = "remote_servers/tests.rs"]
mod tests;
