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

/// Transport failure text without the request URL. The URL is, for Slack, Teams and
/// webhooks, the credential (HS2-CQNE4E); an error that quotes it (an unusable URL) prints
/// only its origin.
fn transport_error(error: &ureq::Error, url: &str) -> String {
    let text = error.to_string();
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
        // HS2-9ABVDW: an error status is the notifier's to classify, so it is read as a
        // response; the body goes out as given, whatever the method.
        let agent = hotsheet_http::build(
            hotsheet_http::config()
                .http_status_as_error(false)
                .timeout_global(Some(Duration::from_secs(30))),
        );
        let mut call = ureq::http::Request::builder()
            .method(request.method.as_str())
            .uri(&request.url);
        for (name, value) in &request.headers {
            call = call.header(name, value);
        }
        let call = call
            .body(request.body.clone())
            .map_err(|error| transport_error(&ureq::Error::Http(error), &request.url))?;
        let mut response = agent
            .run(call)
            .map_err(|error| transport_error(&error, &request.url))?;
        let status = response.status().as_u16();
        let body = response.body_mut().read_to_string().unwrap_or_default();
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
        // HS2-9ABVDW: an error that quotes the URL itself prints only its origin.
        for url in [
            "ftp://hooks.test/services/synthetic-secret",
            "http://hooks.test/services/synthetic-secret\u{7f}",
        ] {
            let error = LiveHttp
                .send(&HttpRequest {
                    url: url.into(),
                    ..request.clone()
                })
                .unwrap_err();
            assert!(!error.contains("synthetic-secret"), "{error}");
            assert!(!error.is_empty());
        }
    }

    /// HS2-9ABVDW: a loopback server that answers one request with `reply` and hands back
    /// the raw request it read.
    fn answer_once(reply: &'static str) -> (String, std::thread::JoinHandle<String>) {
        use std::io::{BufRead, BufReader, Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut head = String::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                head.push_str(&line);
                if line == "\r\n" {
                    break;
                }
            }
            let length = head
                .lines()
                .find_map(|l| {
                    l.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(|v| v.trim().parse::<usize>().unwrap())
                })
                .unwrap_or(0);
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            stream.write_all(reply.as_bytes()).unwrap();
            head + &String::from_utf8(body).unwrap()
        });
        (url, server)
    }

    /// HS2-9ABVDW: an error status is a response with its body, not a transport failure,
    /// and the request goes out with its method, headers and body unchanged.
    #[test]
    fn hs2_9abvdw_live_http_returns_error_statuses_with_their_body() {
        let (url, server) = answer_once(
            "HTTP/1.1 503 Service Unavailable\r\nConnection: close\r\nContent-Length: 12\r\n\r\nsynthetic-ko",
        );
        let response = LiveHttp
            .send(&HttpRequest {
                method: "PUT".into(),
                url: format!("{url}/hook"),
                headers: vec![("X-Synthetic".into(), "header-value".into())],
                body: b"synthetic-body".to_vec(),
            })
            .unwrap();
        assert_eq!(
            response,
            HttpResponse {
                status: 503,
                body: "synthetic-ko".into()
            }
        );
        let seen = server.join().unwrap().to_ascii_lowercase();
        assert!(seen.starts_with("put /hook http/1.1\r\n"), "{seen}");
        assert!(seen.contains("x-synthetic: header-value\r\n"), "{seen}");
        assert!(seen.ends_with("\r\n\r\nsynthetic-body"), "{seen}");
    }

    #[test]
    fn encodes_non_ascii_and_multiline_headers() {
        assert_eq!(header_text("plain title"), "plain title");
        assert_eq!(header_text("a\nb"), "=?UTF-8?B?YQpi?=");
        assert!(header_text("café").starts_with("=?UTF-8?B?"));
    }
}
