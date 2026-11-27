//! Shared live adapter for the CLI, HTTP service and serverless MCP remote registry.
//! HS2-1Y45MZ keeps authenticated transport outside the network-free ticketing core.

use std::io::{BufReader, Read};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use hotsheet_ticketing::remote_servers::{
    MAX_REMOTE_RESPONSE_BYTES, PAIRING_BUNDLE_FORMAT, PairingBundle, RegisterRemote,
    RemoteCredentials, RemoteDiscovery, RemoteError, RemoteFailure, RemoteIdentity, RemoteRegistry,
    RemoteSnapshot, RemoteTransport, RemoteView, ReviewRemote, RevokeRemote,
    normalize_remote_origin, validate_remote_checkouts,
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

    /// For the same-machine desktop bridge only. Never return this through a browser route.
    pub fn bridge_connection(
        &self,
        id: &str,
    ) -> Result<(RemoteView, RemoteCredentials), RemoteError> {
        self.registry.bridge_connection(id, self.secrets.as_ref())
    }

    /// HS2-4B0KSD: the peer's view of this machine's paired session for binding `id`: its
    /// lifetime and reminder day, or why it no longer works (expired, revoked, unreachable).
    /// `renew` asks the peer's host to renew it in place; the binding, its credential and its
    /// selected projects stay as they are.
    pub fn peer_session(&self, id: &str, renew: bool) -> Result<PeerSession, RemoteError> {
        let (binding, credentials) = self.bridge_connection(id)?;
        Ok(
            match peer_session_request(&binding.origin, &credentials, renew) {
                Ok(device) => PeerSession {
                    binding_id: binding.id,
                    device: Some(device),
                    failure: None,
                },
                Err(failure) => PeerSession {
                    binding_id: binding.id,
                    device: None,
                    failure: Some(failure),
                },
            },
        )
    }

    pub fn selected_projects(
        &self,
    ) -> Result<Vec<hotsheet_ticketing::remote_servers::RemoteSelectedProject>, RemoteError> {
        self.registry.selected_projects()
    }

    pub fn select_project(&self, id: &str, checkout: &str) -> Result<(), RemoteError> {
        self.registry.select_project(id, checkout)
    }

    pub fn unselect_project(&self, id: &str, checkout: &str) -> Result<(), RemoteError> {
        self.registry.unselect_project(id, checkout)
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
    Ok(read_remote_credential_input(input)?.credentials)
}

/// HS2-FZXXTG: credential input for `remote register`/`review`, the desktop's private CLI
/// call and the MCP tool alike: a plain credential object, or a pairing bundle written by
/// `cert issue --bundle`, which also names the serving machine's identity and origin.
pub struct RemoteCredentialInput {
    pub credentials: RemoteCredentials,
    /// The server id a pairing bundle names; `None` for a plain credential object.
    pub server_id: Option<String>,
    /// The origin a pairing bundle suggests.
    pub origin: Option<String>,
}

impl RemoteCredentialInput {
    /// The server id to verify: an explicit one must equal the bundle's, so a bundle for
    /// another machine is never registered under a typed id.
    pub fn expected_server_id(&self, explicit: Option<String>) -> Result<String, RemoteError> {
        match (explicit, &self.server_id) {
            (Some(explicit), Some(bundled)) if &explicit != bundled => Err(RemoteError::Invalid(
                "the pairing bundle names another server id than --expected-server-id",
            )),
            (Some(explicit), _) => Ok(explicit),
            (None, Some(bundled)) => Ok(bundled.clone()),
            (None, None) => Err(RemoteError::Invalid(
                "an expected server id is required unless a pairing bundle names one",
            )),
        }
    }

    /// The origin to register: an explicit one wins, since the importing machine may reach
    /// the server by another name; the bundle's suggestion is the default.
    pub fn origin(&self, explicit: Option<&str>) -> Result<String, RemoteError> {
        match (explicit, &self.origin) {
            (Some(explicit), _) => normalize_remote_origin(explicit),
            (None, Some(bundled)) => normalize_remote_origin(bundled),
            (None, None) => Err(RemoteError::Invalid(
                "an origin is required unless the pairing bundle suggests one",
            )),
        }
    }
}

pub fn read_remote_credential_input(
    input: impl Read,
) -> Result<RemoteCredentialInput, RemoteError> {
    let mut bytes = Vec::new();
    input.take(400 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 400 * 1024 {
        return Err(RemoteError::Invalid("remote credential input is too large"));
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|_| RemoteError::Invalid("invalid remote credential input"))?;
    if value.get("format").is_none() {
        return Ok(RemoteCredentialInput {
            credentials: serde_json::from_value(value)
                .map_err(|_| RemoteError::Invalid("invalid remote credential input"))?,
            server_id: None,
            origin: None,
        });
    }
    let bundle: PairingBundle = serde_json::from_value(value)
        .map_err(|_| RemoteError::Invalid("invalid pairing bundle"))?;
    if bundle.format != PAIRING_BUNDLE_FORMAT {
        return Err(RemoteError::Invalid("unsupported pairing bundle format"));
    }
    if bundle.server_id.parse::<ulid::Ulid>().is_err() {
        return Err(RemoteError::Invalid(
            "pairing bundle server id is not a ULID",
        ));
    }
    Ok(RemoteCredentialInput {
        credentials: bundle.credentials,
        server_id: Some(bundle.server_id),
        origin: bundle.origin,
    })
}

/// HS2-FZXXTG: the bundle `cert issue --bundle` writes for one newly issued device. The
/// device certificate is the identity, so the bundle carries no bearer.
pub fn pairing_bundle(
    device: &hotsheet_tls::DeviceCert,
    server_id: String,
    origin: Option<&str>,
) -> Result<PairingBundle, RemoteError> {
    Ok(PairingBundle {
        format: PAIRING_BUNDLE_FORMAT.into(),
        server_id,
        device: device.name.clone(),
        origin: origin.map(normalize_remote_origin).transpose()?,
        credentials: RemoteCredentials {
            bearer_secret: hotsheet_sync::secret::SecretString::default(),
            session_origin: None,
            session_csrf: None,
            ca_pem: Some(device.ca_pem.clone()),
            client_certificate_pem: Some(device.cert_pem.clone()),
            client_key_pem: Some(device.key_pem.clone()),
        },
    })
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
        let agent = remote_agent(origin, credentials)?;
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

impl hotsheet_ticketing::remote_servers::RemoteCheckoutTransport for MtlsRemoteTransport {
    fn checkout_projection(
        &self,
        origin: &str,
        credentials: &RemoteCredentials,
        checkout_id: &str,
    ) -> Result<
        hotsheet_ticketing::orchestration::CheckoutProjection,
        hotsheet_ticketing::orchestration::TargetFailure,
    > {
        use hotsheet_ticketing::orchestration::{
            CheckoutProjection, MAX_PROJECTION_BYTES, TargetFailure,
        };
        // Registered ids use unreserved ASCII. Explicitly reject dot path segments;
        // never let an id relocate this authenticated request to another endpoint.
        if checkout_id.is_empty()
            || checkout_id.len() > 160
            || matches!(checkout_id, "." | "..")
            || !checkout_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        {
            return Err(TargetFailure::InvalidResponse);
        }
        let agent = remote_agent(origin, credentials).map_err(TargetFailure::from)?;
        let projection: CheckoutProjection = get_limited(
            &agent,
            origin,
            &format!("/remote-checkouts/{checkout_id}/resolution"),
            credentials,
            MAX_PROJECTION_BYTES,
        )
        .map_err(TargetFailure::from)?;
        // The registry verified the peer before this call. Confirm again after the
        // bounded metadata response; a restart/replacement cannot lend it identity.
        let confirmed: RemoteIdentity =
            get(&agent, origin, "/remote-identity", credentials).map_err(TargetFailure::from)?;
        confirmed.validate().map_err(TargetFailure::from)?;
        if projection.server_id != confirmed.server_id || projection.checkout_id != checkout_id {
            return Err(TargetFailure::IdentityChanged);
        }
        Ok(projection)
    }

    /// HS2-AZ15GA: the same pinned, no-redirect, time-bounded agent and credentials as
    /// discovery. Every status the peer returns is the caller's to interpret; only
    /// transport, trust, redirect and size failures are reported here.
    fn target_call(
        &self,
        origin: &str,
        credentials: &RemoteCredentials,
        method: &str,
        path: &str,
        body: Option<&[u8]>,
    ) -> Result<
        hotsheet_ticketing::remote_servers::TargetReply,
        hotsheet_ticketing::orchestration::TargetFailure,
    > {
        use hotsheet_ticketing::orchestration::TargetFailure;
        if !matches!(method, "GET" | "POST" | "PATCH" | "PUT" | "DELETE") || !path.starts_with('/')
        {
            return Err(TargetFailure::InvalidResponse);
        }
        let agent = remote_agent(origin, credentials).map_err(TargetFailure::from)?;
        let mut headers = remote_auth_headers(method, origin, credentials);
        headers.push(("Accept", "application/json".into()));
        if body.is_some() {
            headers.push(("Content-Type", "application/json".into()));
        }
        let response = match send(&agent, method, &format!("{origin}{path}"), &headers, body) {
            Ok(response) => response,
            Err(error) => {
                return Err(if connection_failed(&error) {
                    TargetFailure::Unreachable
                } else {
                    TargetFailure::TrustFailed
                });
            }
        };
        let status = response.status().as_u16();
        if (300..400).contains(&status) {
            return Err(TargetFailure::Redirect);
        }
        let mut bytes = Vec::new();
        response
            .into_body()
            .into_reader()
            .take((MAX_TARGET_REPLY_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| TargetFailure::Unreachable)?;
        if bytes.len() > MAX_TARGET_REPLY_BYTES {
            return Err(TargetFailure::ResponseLimit);
        }
        Ok(hotsheet_ticketing::remote_servers::TargetReply {
            status,
            body: bytes,
        })
    }
}

/// The peer's answer about this machine's paired session (HS2-4B0KSD).
#[derive(Debug, serde::Serialize)]
pub struct PeerSession {
    pub binding_id: String,
    /// The peer's device record: `renewed_at`, `expires_at`, `idle_expires_at`,
    /// `reminder_day`, `renewal_requested_at`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<RemoteFailure>,
}

/// Read (`GET /remote-auth/me`) or ask to renew (`POST /remote-auth/renewal`) this machine's
/// paired session on the peer. A certificate binding has no paired session to renew.
pub fn peer_session_request(
    origin: &str,
    credentials: &RemoteCredentials,
    renew: bool,
) -> Result<serde_json::Value, RemoteFailure> {
    if credentials.session_token().is_none() {
        return Err(RemoteFailure::Unsupported);
    }
    let agent = remote_agent(origin, credentials)?;
    let url = format!(
        "{origin}{}",
        if renew {
            "/remote-auth/renewal"
        } else {
            "/remote-auth/me"
        }
    );
    let method = if renew { "POST" } else { "GET" };
    let mut headers = remote_auth_headers(method, origin, credentials);
    headers.push(("Accept", "application/json".into()));
    if renew {
        let (Some(paired_origin), Some(csrf)) = (
            credentials.session_origin.as_deref(),
            credentials
                .session_csrf
                .as_ref()
                .map(hotsheet_sync::secret::SecretString::expose),
        ) else {
            return Err(RemoteFailure::MissingCredentials);
        };
        headers.retain(|(name, _)| !matches!(*name, "Origin" | "X-Hotsheet-Csrf"));
        headers.push(("Origin", paired_origin.into()));
        headers.push(("X-Hotsheet-Csrf", csrf.into()));
    }
    let response = refuse_error_statuses(send(&agent, method, &url, &headers, None))?;
    if !matches!(response.status().as_u16(), 200 | 202) {
        return Err(RemoteFailure::InvalidResponse);
    }
    let mut bytes = Vec::new();
    response
        .into_body()
        .into_reader()
        .take(64 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| RemoteFailure::Unreachable)?;
    if bytes.len() > 64 * 1024 {
        return Err(RemoteFailure::ResponseLimit);
    }
    let device: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| RemoteFailure::InvalidResponse)?;
    if !device.is_object() {
        return Err(RemoteFailure::InvalidResponse);
    }
    Ok(device)
}

/// One ticket with its notes fits comfortably; anything larger is not a ticket reply.
const MAX_TARGET_REPLY_BYTES: usize = 2 * 1024 * 1024;

fn remote_agent(
    origin: &str,
    credentials: &RemoteCredentials,
) -> Result<ureq::Agent, RemoteFailure> {
    let normalized = normalize_remote_origin(origin).map_err(|_| RemoteFailure::TrustFailed)?;
    if normalized != origin {
        return Err(RemoteFailure::TrustFailed);
    }
    credentials
        .trust_fingerprint(origin)
        .map_err(|_| RemoteFailure::TrustFailed)?;
    // HS2-9ABVDW: error statuses are read here, for the peer's own refusal code.
    let settings = hotsheet_http::config()
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_connect(Some(Duration::from_secs(5)))
        .timeout_per_read(Some(Duration::from_secs(10)))
        .timeout_per_write(Some(Duration::from_secs(5)))
        .timeout_global(Some(Duration::from_secs(15)));
    Ok(if origin.starts_with("https://") {
        hotsheet_http::build_with_tls(settings, Arc::new(client_config(credentials)?))
    } else {
        hotsheet_http::build(settings)
    })
}

/// A reviewed CA pins the peer to exactly that authority; without one the peer must present a
/// publicly trusted certificate, such as a Tailscale Serve name (HS2-H4REBS).
fn remote_roots(ca: Option<&str>) -> Result<rustls::RootCertStore, RemoteFailure> {
    let Some(ca) = ca else {
        return Ok(rustls::RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        });
    };
    let mut roots = rustls::RootCertStore::empty();
    for certificate in rustls_pemfile::certs(&mut BufReader::new(ca.as_bytes())) {
        roots
            .add(certificate.map_err(|_| RemoteFailure::TrustFailed)?)
            .map_err(|_| RemoteFailure::TrustFailed)?;
    }
    Ok(roots)
}

fn client_config(credentials: &RemoteCredentials) -> Result<rustls::ClientConfig, RemoteFailure> {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let roots = remote_roots(credentials.ca_pem.as_deref())?;
    if roots.is_empty() {
        return Err(RemoteFailure::TrustFailed);
    }
    let builder = rustls::ClientConfig::builder().with_root_certificates(roots);
    let mut config = if let (Some(certificates), Some(key)) = (
        credentials.client_certificate_pem.as_deref(),
        credentials.client_key_pem.as_deref(),
    ) {
        let certificates = rustls_pemfile::certs(&mut BufReader::new(certificates.as_bytes()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| RemoteFailure::TrustFailed)?;
        let key = rustls_pemfile::private_key(&mut BufReader::new(key.as_bytes()))
            .map_err(|_| RemoteFailure::TrustFailed)?
            .ok_or(RemoteFailure::TrustFailed)?;
        builder
            .with_client_auth_cert(certificates, key)
            .map_err(|_| RemoteFailure::TrustFailed)?
    } else {
        builder.with_no_client_auth()
    };
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(config)
}

fn get<T: DeserializeOwned>(
    agent: &ureq::Agent,
    origin: &str,
    path: &str,
    credentials: &RemoteCredentials,
) -> Result<T, RemoteFailure> {
    get_limited(agent, origin, path, credentials, MAX_REMOTE_RESPONSE_BYTES)
}

fn get_limited<T: DeserializeOwned>(
    agent: &ureq::Agent,
    origin: &str,
    path: &str,
    credentials: &RemoteCredentials,
    limit: usize,
) -> Result<T, RemoteFailure> {
    let mut headers = remote_auth_headers("GET", origin, credentials);
    headers.push(("Accept", "application/json".into()));
    let response = refuse_error_statuses(send(
        agent,
        "GET",
        &format!("{origin}{path}"),
        &headers,
        None,
    ))?;
    if response.status().is_redirection() {
        return Err(RemoteFailure::Redirect);
    }
    if response.status().as_u16() != 200 {
        return Err(RemoteFailure::InvalidResponse);
    }
    if hotsheet_http::header(&response, "x-hotsheet-truncated") == Some("true")
        || hotsheet_http::header(&response, "x-hotsheet-partial") == Some("true")
    {
        return Err(RemoteFailure::ResponseLimit);
    }
    let mut bytes = Vec::new();
    response
        .into_body()
        .into_reader()
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| RemoteFailure::Unreachable)?;
    if bytes.len() > limit {
        return Err(RemoteFailure::ResponseLimit);
    }
    serde_json::from_slice(&bytes).map_err(|_| RemoteFailure::InvalidResponse)
}

/// One request to a peer with these headers. An error status comes back as a response
/// (the remote agent reads them), so callers see the peer's own code (HS2-9ABVDW).
fn send(
    agent: &ureq::Agent,
    method: &str,
    url: &str,
    headers: &[(&'static str, String)],
    body: Option<&[u8]>,
) -> Result<ureq::http::Response<ureq::Body>, ureq::Error> {
    let headers: Vec<(&str, &str)> = headers
        .iter()
        .map(|(name, value)| (*name, value.as_str()))
        .collect();
    hotsheet_http::run(agent, method, url, &headers, body)
}

/// Whether no connection to the peer was made (refused, unreachable, a connect timeout or a
/// failed TLS handshake). Anything else after the request left is a trust failure.
fn connection_failed(error: &ureq::Error) -> bool {
    hotsheet_http::failure_kind(error) == hotsheet_http::FailureKind::ConnectionFailed
}

/// A peer answer with its error statuses mapped: 401/403 by the peer's code, 404
/// unsupported, any other 4xx/5xx an invalid response.
fn refuse_error_statuses(
    result: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
) -> Result<ureq::http::Response<ureq::Body>, RemoteFailure> {
    let response = match result {
        Ok(response) => response,
        // An agent that reports statuses as errors gives no body to read the code from.
        Err(ureq::Error::StatusCode(401 | 403)) => return Err(RemoteFailure::Unauthorized),
        Err(ureq::Error::StatusCode(404)) => return Err(RemoteFailure::Unsupported),
        Err(ureq::Error::StatusCode(_)) => return Err(RemoteFailure::InvalidResponse),
        Err(error) => {
            return Err(if connection_failed(&error) {
                RemoteFailure::Unreachable
            } else {
                RemoteFailure::TrustFailed
            });
        }
    };
    match response.status().as_u16() {
        // HS2-4B0KSD: an expired or revoked paired session is its own failure, so the desktop
        // offers renewal instead of retrying; the peer's code says which.
        401 | 403 => Err(auth_failure(response)),
        404 => Err(RemoteFailure::Unsupported),
        400.. => Err(RemoteFailure::InvalidResponse),
        _ => Ok(response),
    }
}

/// The peer's auth refusal by its stable `code` (HS2-4B0KSD). Only the code is read; the
/// peer's message never reaches the caller.
fn auth_failure(response: ureq::http::Response<ureq::Body>) -> RemoteFailure {
    let mut bytes = Vec::new();
    let _ = response
        .into_body()
        .into_reader()
        .take(4096)
        .read_to_end(&mut bytes);
    let code = serde_json::from_slice::<serde_json::Value>(&bytes)
        .ok()
        .and_then(|body| body.get("code")?.as_str().map(str::to_owned));
    match code.as_deref() {
        Some("session_expired") => RemoteFailure::SessionExpired,
        Some("session_revoked") => RemoteFailure::SessionRevoked,
        _ => RemoteFailure::Unauthorized,
    }
}

/// The credential headers for one request to a peer.
fn remote_auth_headers(
    method: &str,
    origin: &str,
    credentials: &RemoteCredentials,
) -> Vec<(&'static str, String)> {
    let mut headers = Vec::new();
    if let Some(token) = credentials.session_token() {
        // HS2-AW2AKF: a server-to-server call presents the paired credential as a bearer,
        // which the peer accepts for writes without browser Origin and CSRF proof. The
        // cookie stays for a peer that predates bearer sessions; for its writes, send the
        // paired origin and CSRF proof stored at pairing. Reads never send the CSRF proof,
        // so a stale one cannot refuse them.
        headers.push(("Authorization", format!("Bearer {token}")));
        headers.push(("Cookie", format!("hs2_remote_session={token}")));
        if !matches!(method, "GET" | "HEAD")
            && let (Some(paired_origin), Some(csrf)) = (
                credentials.session_origin.as_deref(),
                credentials
                    .session_csrf
                    .as_ref()
                    .map(hotsheet_sync::secret::SecretString::expose),
            )
        {
            headers.push(("Origin", paired_origin.to_owned()));
            headers.push(("X-Hotsheet-Csrf", csrf.to_owned()));
        }
    } else if origin.starts_with("http://127.0.0.1:") || origin.starts_with("http://[::1]:") {
        headers.push((
            "X-Hotsheet-Secret",
            credentials.bearer_secret.expose().to_owned(),
        ));
    }
    headers
}

#[cfg(test)]
#[path = "remote_servers/tests.rs"]
mod tests;
