//! Secret / primary-store authentication middleware.

use crate::AppState;
use crate::api_error::ApiError;
use axum::extract::ConnectInfo;
use axum::extract::{FromRequestParts, Request, State};
use axum::http::request::Parts;
use axum::http::{Extensions, Method, StatusCode};
use axum::middleware::Next;
use axum::response::Response;

// ---- auth ------------------------------------------------------------------------

/// Routes that read or write the primary store without a checkout or hosted-store id.
/// Keep project/store-scoped routes available when only the primary path was replaced.
fn primary_store_route(path: &str) -> bool {
    const PREFIXES: &[&str] = &[
        "/tickets",
        "/claim-next",
        "/batch",
        "/groups",
        "/setup",
        "/analytics",
        "/confidence-report",
        "/phase-report",
        "/commands",
        "/command-groups",
        "/command-runs",
        "/views",
        "/activity",
        "/ai-settings",
        "/terminal-settings",
        "/provider-connections",
    ];
    path == "/providers"
        || PREFIXES.iter().any(|prefix| {
            path == *prefix
                || path
                    .strip_prefix(prefix)
                    .is_some_and(|rest| rest.starts_with('/'))
        })
}

// ---- host access (HS2-S8R3PD) ----------------------------------------------------

/// How [`require_secret`] authenticated a request. It is inserted on every request the
/// middleware admits, so a handler can tell the trusted host from a remote browser.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Caller {
    /// The machine secret from an allowed (loopback or local-listener) peer.
    LocalBearer,
    /// A client certificate accepted by the TLS listener.
    Certificate,
    /// A paired browser session; `local` is its host-trust grant (HS2-85W6BE).
    BrowserSession { local: bool },
}

/// HS2-CRBR8X: who sent a request, as the hosting machine's session journal names it:
/// `cert-<fingerprint>` for a device certificate, `browser-<device id>` for a paired
/// browser, `local` for the machine secret. Inserted beside [`Caller`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Requester(pub(crate) String);

impl Caller {
    /// Whether this caller may act on the host machine itself: launch a host application
    /// (difftool, open, reveal) or name an arbitrary host path. A request that never passed
    /// [`require_secret`] carries no caller and is refused.
    pub(crate) fn host_trusted(caller: Option<&Self>) -> bool {
        matches!(
            caller,
            Some(Self::LocalBearer | Self::Certificate | Self::BrowserSession { local: true })
        )
    }
}

/// Stable error code for [`host_access_required`].
pub(crate) const HOST_ACCESS_REQUIRED_CODE: &str = "host_access_required";

/// The refusal a remote browser session without the local grant gets from a host-only route.
pub(crate) fn host_access_required() -> ApiError {
    ApiError::new(
        StatusCode::FORBIDDEN,
        "this action runs on the computer hosting Hot Sheet and needs host access for this device",
    )
    .with_code(HOST_ACCESS_REQUIRED_CODE)
}

/// The one server-side check for host-only routes (HS2-S8R3PD). The browser bridge hides its
/// own host-only routes, but every project API route is reachable through its proxy, so the
/// server refuses host-launching and arbitrary-host-path routes itself.
pub(crate) fn require_host_access(extensions: &Extensions) -> Result<(), ApiError> {
    if Caller::host_trusted(extensions.get::<Caller>()) {
        Ok(())
    } else {
        Err(host_access_required())
    }
}

/// Extractor for a route that always acts on the host. List it before the body so a refused
/// request never reaches checkout resolution or body parsing.
pub(crate) struct HostAccess;

impl<S: Send + Sync> FromRequestParts<S> for HostAccess {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        require_host_access(&parts.extensions).map(|()| Self)
    }
}

/// Extractor for a route that acts on the host only for some bodies (a Git ticket source's
/// locator is a host path; another provider's locator is not). Call [`HostTrust::require`]
/// on that branch.
pub(crate) struct HostTrust(Result<(), ApiError>);

impl HostTrust {
    pub(crate) fn require(self) -> Result<(), ApiError> {
        self.0
    }
}

impl<S: Send + Sync> FromRequestParts<S> for HostTrust {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(Self(require_host_access(&parts.extensions)))
    }
}

pub(crate) async fn require_secret(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let presented = req
        .headers()
        .get("x-hotsheet-secret")
        .and_then(|v| v.to_str().ok());
    let is_mutation = matches!(
        *req.method(),
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    );
    let peer = req
        .extensions()
        .get::<ConnectInfo<std::net::SocketAddr>>()
        .map(|info| info.0);
    let certificate = req
        .extensions()
        .get::<crate::tls::CertificateAuthenticated>()
        .is_some();
    let (caller, requester) = if certificate {
        let device = req
            .extensions()
            .get::<crate::tls::CertificateFingerprint>()
            .map_or_else(|| "cert".to_owned(), |device| format!("cert-{}", device.0));
        (Caller::Certificate, device)
    } else if presented == Some(state.secret.as_str()) && state.local_bearer_allowed(peer) {
        (Caller::LocalBearer, "local".to_owned())
    } else {
        let device =
            crate::routes::remote_auth::session(&state, req.headers(), is_mutation, false)?;
        (
            Caller::BrowserSession {
                local: device.local,
            },
            format!("browser-{}", device.id),
        )
    };
    req.extensions_mut().insert(caller);
    req.extensions_mut().insert(Requester(requester));
    if primary_store_route(req.uri().path()) {
        state.require_primary_store_identity()?;
    }
    let is_restart = req.uri().path() == "/lifecycle/restart";
    // HS2-RFJ1GN: compaction closes admission itself, then checks quiescence. Counting
    // its own request as an active mutation would make that check always fail.
    let is_compaction = req.method() == Method::POST
        && req.uri().path().starts_with("/checkouts/")
        && req.uri().path().ends_with("/repository/compaction");
    let _mutation = if is_mutation && !is_restart && !is_compaction {
        Some(state.begin_mutation().ok_or_else(|| {
            ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "server is quiescing for a safe restart; retry after reconnecting",
            )
        })?)
    } else {
        None
    };
    Ok(next.run(req).await)
}

#[cfg(test)]
mod primary_store_route_tests {
    use super::primary_store_route;

    #[test]
    fn guards_primary_data_without_capturing_scoped_or_machine_routes() {
        for path in [
            "/tickets",
            "/tickets/HS-123/close",
            "/claim-next",
            "/batch",
            "/providers",
            "/provider-connections/github",
            "/analytics/tickets",
            "/commands/one/run",
            "/activity",
            "/terminal-settings",
        ] {
            assert!(primary_store_route(path), "{path}");
        }
        for path in [
            "/checkouts/project/tickets",
            "/stores/store/tickets",
            "/providers/github/tickets",
            "/projects/open",
            "/compatibility",
            "/permissions",
            "/terminals",
            "/tickets-extra",
        ] {
            assert!(!primary_store_route(path), "{path}");
        }
    }
}

#[cfg(test)]
mod host_access_tests {
    use super::{Caller, require_host_access};
    use axum::http::{Extensions, StatusCode};

    #[test]
    fn hs2_s8r3pd_only_host_trusted_callers_pass() {
        for (caller, trusted) in [
            (None, false),
            (Some(Caller::LocalBearer), true),
            (Some(Caller::Certificate), true),
            (Some(Caller::BrowserSession { local: true }), true),
            (Some(Caller::BrowserSession { local: false }), false),
        ] {
            let mut extensions = Extensions::new();
            if let Some(caller) = caller {
                extensions.insert(caller);
            }
            match require_host_access(&extensions) {
                Ok(()) => assert!(trusted, "{caller:?}"),
                Err(error) => {
                    assert!(!trusted, "{caller:?}");
                    assert_eq!(error.status, StatusCode::FORBIDDEN);
                }
            }
        }
    }

    /// HS2-S8R3PD policy: a route handler that starts a host program must take the
    /// `HostAccess` extractor, so a new launcher cannot skip the shared check.
    #[test]
    fn hs2_s8r3pd_every_host_launching_handler_takes_host_access() {
        const LAUNCHERS: &[&str] = &[
            "repository_browser::act_on_host_path(",
            "repository_browser::act_on_file(",
            "code_review::launch(",
            "code_review::launch_repository(",
        ];
        let routes = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/routes");
        let mut pending = vec![routes];
        let mut launching = 0;
        while let Some(dir) = pending.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    pending.push(path);
                    continue;
                }
                if path.extension().is_none_or(|ext| ext != "rs") {
                    continue;
                }
                let source = std::fs::read_to_string(&path).unwrap();
                // Production code only: test modules (this one included) name the launchers.
                let production = source.split("#[cfg(test)]").next().unwrap_or_default();
                for handler in production.split("pub(crate) async fn ").skip(1) {
                    if !LAUNCHERS.iter().any(|launcher| handler.contains(launcher)) {
                        continue;
                    }
                    launching += 1;
                    let signature = handler.split(") ->").next().unwrap_or_default();
                    let name = handler.split('(').next().unwrap_or_default();
                    assert!(
                        signature.contains("HostAccess"),
                        "{}: `{name}` launches a host program without HostAccess",
                        path.display()
                    );
                }
            }
        }
        assert_eq!(
            launching, 5,
            "host-launching handlers changed; review the inventory"
        );
    }
}
