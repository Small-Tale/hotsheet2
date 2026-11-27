//! The one outbound HTTP baseline (HS2-9ABVDW). Every crate that makes HTTP requests builds
//! its `ureq` agents from [`config`], so the defaults are set in one place and pinned by
//! tests, instead of following whatever the HTTP client library defaults to.
//!
//! The baseline keeps the behaviour the workspace had on ureq 2:
//!
//! - **No proxy.** Proxy environment variables (`HTTP_PROXY`, `ALL_PROXY`, …) are never
//!   read. Most requests go to a loopback Hot Sheet server, which a proxy must not see.
//! - **At most [`MAX_REDIRECTS`] redirects** are followed; the next one is an error. Each
//!   redirect drops the `Authorization` and `Cookie` headers and turns a `POST` answered
//!   with 301/302/303 into a `GET`. An agent that must not follow redirects sets
//!   `max_redirects(0)` and gets the 3xx response itself.
//! - **A [`CONNECT_TIMEOUT`]** bounds establishing a connection; nothing else is bounded
//!   unless the caller sets a timeout.
//! - **TLS with the ring provider** and Mozilla's root certificates (`webpki-roots`),
//!   certificate verification and SNI on, whatever process-wide provider another part of
//!   the program installed. Callers with their own trust (a pinned CA, a client
//!   certificate) replace the TLS configuration; certificate verification is never
//!   turned off.
//!   [`build_with_tls`] takes such a complete rustls configuration and uses it as given.
//! - **4xx and 5xx answers are errors** ([`ureq::Error::StatusCode`]) unless the caller
//!   turns `http_status_as_error` off to read the answer's body.
//!
//! [`failure_kind`] sorts a failed request into the categories callers branch on, and
//! [`is_timeout`] recognises a timeout while reading a response body.

use std::io;
use std::sync::Arc;
use std::time::Duration;

use ureq::config::{Config, ConfigBuilder};
use ureq::http::Uri;
use ureq::tls::TlsConfig;
use ureq::typestate::AgentScope;
use ureq::unversioned::resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver};
use ureq::unversioned::transport::{
    ConnectProxyConnector, Connector, DefaultConnector, NextTimeout, TcpConnector,
};

mod own_tls;
use ureq::{Agent, Error, Timeout};

/// Redirects followed before the next one fails with [`FailureKind::TooManyRedirects`].
pub const MAX_REDIRECTS: u32 = 4;

/// How long establishing a connection may take unless the caller sets its own bound.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// The baseline agent configuration. Callers add their own timeouts, redirect limit or TLS
/// trust, then make the agent with [`build`].
pub fn config() -> ConfigBuilder<AgentScope> {
    Agent::config_builder()
        .proxy(None)
        .max_redirects(MAX_REDIRECTS)
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .tls_config(tls())
}

/// The agent for a configuration. Always build agents here rather than with
/// `.build().new_agent()`: a host name that does not resolve then fails as
/// [`ureq::Error::HostNotFound`] on every platform, instead of an operating-system I/O
/// error that [`failure_kind`] could not tell from a broken connection.
pub fn build(config: ConfigBuilder<AgentScope>) -> Agent {
    Agent::with_parts(config.build(), DefaultConnector::new(), LookupResolver)
}

/// The agent for a configuration whose TLS is the caller's own complete rustls
/// configuration (trust roots, client certificate, provider, ALPN), used exactly as given.
/// Plain `http` URLs connect without TLS as usual.
pub fn build_with_tls(config: ConfigBuilder<AgentScope>, tls: Arc<rustls::ClientConfig>) -> Agent {
    let connector =
        ().chain(ConnectProxyConnector::default())
            .chain(TcpConnector::default())
            .chain(own_tls::OwnTlsConnector(tls));
    Agent::with_parts(config.build(), connector, LookupResolver)
}

/// An agent with the baseline configuration and nothing else.
pub fn agent() -> Agent {
    build(config())
}

/// The standard resolver, with a failed lookup reported as `HostNotFound`.
#[derive(Debug)]
struct LookupResolver;

impl Resolver for LookupResolver {
    fn resolve(
        &self,
        uri: &Uri,
        config: &Config,
        timeout: NextTimeout,
    ) -> Result<ResolvedSocketAddrs, Error> {
        DefaultResolver::default()
            .resolve(uri, config, timeout)
            .map_err(|error| match error {
                Error::Io(_) => Error::HostNotFound,
                other => other,
            })
    }
}

/// The baseline TLS configuration: Mozilla's roots, verification on, ring as provider.
pub fn tls() -> TlsConfig {
    TlsConfig::builder()
        .unversioned_rustls_crypto_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .build()
}

/// Send a request whose method is chosen at run time, with `body` when there is one.
/// ureq's typed builders (`agent.get`, `agent.post`, …) fix the method and whether it
/// carries a body when the program is compiled; this sends the body for any method.
pub fn run(
    agent: &Agent,
    method: &str,
    url: &str,
    headers: &[(&str, &str)],
    body: Option<&[u8]>,
) -> Result<ureq::http::Response<ureq::Body>, Error> {
    let mut request = ureq::http::Request::builder().method(method).uri(url);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    match body {
        Some(body) => agent.run(request.body(body)?),
        // A method that carries a body is sent with an empty one (`Content-Length: 0`), as
        // `send_empty` does. Without one, ureq 3 frames it as a chunked body of unknown
        // size, which ureq 2's `call()` never sent and a server that answers without reading
        // the body turns into a connection reset.
        None if matches!(method, "POST" | "PUT" | "PATCH") => agent.run(request.body(&[][..])?),
        None => agent.run(request.body(())?),
    }
}

/// What kind of failure a request ended in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    /// No connection was made: refused, unreachable, a connect timeout, or a TLS handshake
    /// that failed (including a certificate the trust did not accept).
    ConnectionFailed,
    /// The host name did not resolve.
    Dns,
    /// The URL could not be used: unparsable, without a host, or with an unknown scheme.
    InvalidUrl,
    /// More redirects than the agent follows.
    TooManyRedirects,
    /// The server answered with this 4xx or 5xx status.
    Status(u16),
    /// The answer was not valid HTTP.
    Protocol,
    /// The connection failed after it was made: reset, closed early, or a read or overall
    /// timeout. What the server did with the request is not known.
    Io,
}

/// Sort a request's error into a [`FailureKind`].
pub fn failure_kind(error: &Error) -> FailureKind {
    match error {
        Error::StatusCode(status) => FailureKind::Status(*status),
        Error::HostNotFound | Error::Timeout(Timeout::Resolve) => FailureKind::Dns,
        Error::BadUri(_) | Error::RequireHttpsOnly(_) => FailureKind::InvalidUrl,
        Error::Http(error) if error.is::<ureq::http::uri::InvalidUri>() => FailureKind::InvalidUrl,
        Error::Http(error) if error.is::<ureq::http::uri::InvalidUriParts>() => {
            FailureKind::InvalidUrl
        }
        Error::ConnectionFailed
        | Error::Timeout(Timeout::Connect)
        | Error::Tls(_)
        | Error::TlsRequired
        | Error::Rustls(_)
        | Error::Pem(_)
        | Error::ConnectProxyFailed(_) => FailureKind::ConnectionFailed,
        Error::TooManyRedirects => FailureKind::TooManyRedirects,
        Error::Protocol(_) | Error::LargeResponseHeader(..) => FailureKind::Protocol,
        Error::Io(error) if is_connect_error(error) => FailureKind::ConnectionFailed,
        _ => FailureKind::Io,
    }
}

/// An I/O error that only establishing a connection produces: the TCP connect was refused or
/// could not route, or the TLS handshake failed.
fn is_connect_error(error: &io::Error) -> bool {
    if error
        .get_ref()
        .is_some_and(|inner| inner.is::<rustls::Error>())
    {
        return true;
    }
    matches!(
        error.kind(),
        io::ErrorKind::ConnectionRefused
            | io::ErrorKind::HostUnreachable
            | io::ErrorKind::NetworkUnreachable
            | io::ErrorKind::NetworkDown
            | io::ErrorKind::AddrNotAvailable
    )
}

/// Whether an error from reading a response body is a timeout: a per-read timeout, or the
/// request's overall timeout passing mid-body.
pub fn is_timeout(error: &io::Error) -> bool {
    if matches!(
        error.kind(),
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
    ) {
        return true;
    }
    error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<Error>())
        .is_some_and(|inner| matches!(inner, Error::Timeout(_)))
}

/// A response header as text, or `None` when it is missing or not visible ASCII.
pub fn header<'a, B>(response: &'a ureq::http::Response<B>, name: &str) -> Option<&'a str> {
    response.headers().get(name)?.to_str().ok()
}
