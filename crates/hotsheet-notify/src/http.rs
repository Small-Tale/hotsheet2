//! A minimal injectable HTTP boundary for HTTP-based notifiers.

use std::fmt;
use std::time::Duration;

use hotsheet_sync::secret::RedactedHeaders;

use crate::notifier::DeliveryError;

/// One outgoing request. `Debug` is hand-written (HS2-CQNE4E): credential header values
/// (`Authorization`, signature, token and key headers) print `<redacted>`, the URL keeps
/// only its origin because Slack, Teams and webhook URLs are themselves secrets, and the
/// body prints as a byte count. The fields, and so the bytes sent, are untouched.
#[derive(Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl fmt::Debug for HttpRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpRequest")
            .field("method", &self.method)
            .field("url", &redact_url(&self.url))
            .field(
                "headers",
                &RedactedHeaders::new(
                    self.headers
                        .iter()
                        .map(|(name, value)| (name.as_str(), value.as_str())),
                ),
            )
            .field("body", &format_args!("<{} bytes>", self.body.len()))
            .finish()
    }
}

/// A URL reduced to `scheme://host[:port]`, with `/<redacted>` standing in for any path or
/// query. Userinfo (`user:pass@`) is always dropped. Text without a scheme is redacted whole.
pub fn redact_url(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return "<redacted>".into();
    };
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..end];
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let tail = rest[end..].trim_start_matches('/');
    if tail.is_empty() {
        format!("{scheme}://{host}")
    } else {
        format!("{scheme}://{host}/<redacted>")
    }
}

/// Transport failure text without the request URL. ureq's `Display` leads with the full
/// URL, which for Slack, Teams and webhooks is the credential (HS2-CQNE4E).
fn transport_error(error: &ureq::Transport, url: &str) -> String {
    let mut text = error.kind().to_string();
    if let Some(message) = error.message() {
        text.push_str(": ");
        text.push_str(message);
    }
    if let Some(source) = std::error::Error::source(error) {
        text.push_str(&format!(": {source}"));
    }
    if url.is_empty() {
        text
    } else {
        text.replace(url, &redact_url(url))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

pub trait HttpTransport: Send + Sync {
    /// Send one request. `Err` is a transport failure (DNS, TLS, connection), not an
    /// HTTP error status, which is returned as a response.
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, String>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct LiveHttp;

impl HttpTransport for LiveHttp {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, String> {
        let mut call =
            ureq::request(&request.method, &request.url).timeout(Duration::from_secs(30));
        for (name, value) in &request.headers {
            call = call.set(name, value);
        }
        let response = match call.send_bytes(&request.body) {
            Ok(response) => response,
            Err(ureq::Error::Status(_, response)) => response,
            Err(ureq::Error::Transport(error)) => {
                return Err(transport_error(&error, &request.url));
            }
        };
        let status = response.status();
        let body = response.into_string().unwrap_or_default();
        Ok(HttpResponse { status, body })
    }
}

/// Refuse anything but an http(s) URL before sending a credential-bearing request.
pub fn require_http_url(url: &str, what: &str) -> Result<(), DeliveryError> {
    if url.starts_with("https://") || url.starts_with("http://") {
        Ok(())
    } else {
        Err(DeliveryError::permanent(format!(
            "{what} must be an http(s) URL"
        )))
    }
}

/// Map an HTTP status to the delivery outcome every HTTP notifier shares.
pub fn classify(response: &HttpResponse, provider: &str) -> Result<(), DeliveryError> {
    let detail = || crate::notifier::tail(response.body.as_bytes(), 300);
    match response.status {
        200..=299 => Ok(()),
        401 | 403 => Err(DeliveryError::permanent(format!(
            "{provider} rejected the credentials ({}): {}",
            response.status,
            detail()
        ))),
        408 | 425 | 429 | 500..=599 => Err(DeliveryError::retryable(format!(
            "{provider} is unavailable ({}): {}",
            response.status,
            detail()
        ))),
        status => Err(DeliveryError::permanent(format!(
            "{provider} refused the message ({status}): {}",
            detail()
        ))),
    }
}

/// Header-safe text: printable ASCII passes through, anything else is RFC 2047 encoded
/// (`=?UTF-8?B?…?=`), which ntfy and most HTTP notification services accept.
pub fn header_text(text: &str) -> String {
    use base64::Engine;
    if text.bytes().all(|b| (0x20..0x7f).contains(&b)) {
        text.to_string()
    } else {
        format!(
            "=?UTF-8?B?{}?=",
            base64::engine::general_purpose::STANDARD.encode(text.as_bytes())
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(status: u16) -> HttpResponse {
        HttpResponse {
            status,
            body: "detail".into(),
        }
    }

    #[test]
    fn classifies_statuses() {
        assert!(classify(&response(200), "ntfy").is_ok());
        assert!(!classify(&response(401), "ntfy").unwrap_err().retryable);
        assert!(classify(&response(429), "ntfy").unwrap_err().retryable);
        assert!(classify(&response(503), "ntfy").unwrap_err().retryable);
        let err = classify(&response(400), "ntfy").unwrap_err();
        assert!(!err.retryable && err.message.contains("detail"));
    }

    #[test]
    fn hs2_cqne4e_redacts_url_paths_queries_and_userinfo() {
        assert_eq!(
            redact_url("https://hooks.slack.test/services/T0/B0/synthetic"),
            "https://hooks.slack.test/<redacted>"
        );
        assert_eq!(
            redact_url("https://user:synthetic@ntfy.test:8443"),
            "https://ntfy.test:8443"
        );
        assert_eq!(
            redact_url("http://hook.test/?token=synthetic"),
            "http://hook.test/<redacted>"
        );
        assert_eq!(redact_url("https://ntfy.test/"), "https://ntfy.test");
        assert_eq!(redact_url("not a url synthetic"), "<redacted>");
    }

    #[test]
    fn hs2_cqne4e_request_debug_redacts_credentials_and_keeps_names() {
        let request = HttpRequest {
            method: "POST".into(),
            url: "https://user:synthetic-pass@hooks.test/in/synthetic-path?key=synthetic-q".into(),
            headers: vec![
                ("Authorization".into(), "Bearer synthetic-token".into()),
                ("Content-Type".into(), "application/json".into()),
                ("X-Hotsheet-Signature".into(), "sha256=synthetic-sig".into()),
            ],
            body: b"{}".to_vec(),
        };
        for printed in [format!("{request:?}"), format!("{request:#?}")] {
            assert!(!printed.contains("synthetic"), "{printed}");
            assert!(printed.contains("Authorization"), "{printed}");
            assert!(printed.contains("X-Hotsheet-Signature"), "{printed}");
            assert!(printed.contains("application/json"), "{printed}");
            assert!(
                printed.contains("https://hooks.test/<redacted>"),
                "{printed}"
            );
            assert!(printed.contains("<2 bytes>"), "{printed}");
        }
        // Debug is only a view: the request still carries the exact credential.
        assert_eq!(request.headers[0].1, "Bearer synthetic-token");
    }

    #[test]
    fn hs2_cqne4e_transport_errors_never_print_the_url() {
        let url = "http://127.0.0.1:9/services/synthetic-secret";
        let request = HttpRequest {
            method: "POST".into(),
            url: url.into(),
            headers: Vec::new(),
            body: Vec::new(),
        };
        let error = LiveHttp.send(&request).unwrap_err();
        assert!(!error.contains("synthetic-secret"), "{error}");
        assert!(!error.is_empty());
    }

    #[test]
    fn encodes_non_ascii_and_multiline_headers() {
        assert_eq!(header_text("plain title"), "plain title");
        assert_eq!(header_text("a\nb"), "=?UTF-8?B?YQpi?=");
        assert!(header_text("café").starts_with("=?UTF-8?B?"));
    }
}
