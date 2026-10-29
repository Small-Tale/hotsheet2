//! A minimal injectable HTTP boundary for HTTP-based notifiers.

use std::time::Duration;

use crate::notifier::DeliveryError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
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
            Err(error) => return Err(error.to_string()),
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
    fn encodes_non_ascii_and_multiline_headers() {
        assert_eq!(header_text("plain title"), "plain title");
        assert_eq!(header_text("a\nb"), "=?UTF-8?B?YQpi?=");
        assert!(header_text("café").starts_with("=?UTF-8?B?"));
    }
}
