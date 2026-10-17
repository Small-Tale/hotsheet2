//! The injected HTTP port session-push adapters talk through, plus its `ureq` production
//! implementation. Tests substitute a recording fake (unit) or a loopback fake server
//! replaying a captured cassette (integration).

use std::time::Duration;

/// One HTTP request an adapter makes.
#[derive(Debug, Clone, PartialEq)]
pub struct HttpRequest {
    /// `GET` or `POST`.
    pub method: &'static str,
    pub url: String,
    /// The full `Authorization` header value, when the endpoint needs one.
    pub authorization: Option<String>,
    /// A JSON body (`POST` only).
    pub body: Option<serde_json::Value>,
}

/// The status and body text of a response. Non-2xx statuses are responses, not errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

impl HttpResponse {
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// Sends one request. `Err` means the request never got a response (refused, timed out).
pub trait HttpPort: Send + Sync {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, String>;
}

/// The production port: a short-timeout `ureq` agent that never consults proxy settings
/// (every session API is on loopback).
pub struct UreqHttp {
    agent: ureq::Agent,
}

impl UreqHttp {
    pub fn new(timeout: Duration) -> Self {
        Self {
            agent: ureq::AgentBuilder::new().timeout(timeout).build(),
        }
    }
}

impl Default for UreqHttp {
    fn default() -> Self {
        Self::new(Duration::from_secs(10))
    }
}

impl HttpPort for UreqHttp {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, String> {
        let mut call = self.agent.request(request.method, &request.url);
        if let Some(authorization) = &request.authorization {
            call = call.set("Authorization", authorization);
        }
        let result = match &request.body {
            Some(body) => call
                .set("Content-Type", "application/json")
                .send_string(&body.to_string()),
            None => call.call(),
        };
        let response = match result {
            Ok(response) => response,
            Err(ureq::Error::Status(_, response)) => response,
            Err(ureq::Error::Transport(error)) => return Err(error.to_string()),
        };
        let status = response.status();
        let body = response.into_string().map_err(|error| error.to_string())?;
        Ok(HttpResponse { status, body })
    }
}

/// An HTTP Basic `Authorization` header value.
pub fn basic_auth_header(user: &str, password: &str) -> String {
    use base64::Engine as _;
    format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("{user}:{password}"))
    )
}

/// Percent-encode a query-string value (RFC 3986 unreserved characters pass through).
pub fn query_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~' | b'/') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}
