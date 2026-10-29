//! `webhook`: POST (or PUT) the message as JSON to any URL, optionally signed with
//! HMAC-SHA256 so the receiver can verify it came from Hot Sheet (HS2-97ZHMM).

use std::sync::Arc;

use serde_json::{Value, json};

use crate::capability::{
    AttachmentLimits, BodyFormat, Capabilities, NotifierDescriptor, NotifierSource, SettingField,
    SettingKind,
};
use crate::http::{HttpRequest, HttpTransport, LiveHttp, classify, require_http_url};
use crate::notifier::{DeliveryError, Notifier};
use crate::render::Rendered;
use crate::settings::Resolved;

pub struct Webhook {
    descriptor: NotifierDescriptor,
    http: Arc<dyn HttpTransport>,
    clock: fn() -> i64,
}

fn unix_now() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

impl Webhook {
    pub fn live() -> Webhook {
        Webhook::with_transport(Arc::new(LiveHttp))
    }

    pub fn with_transport(http: Arc<dyn HttpTransport>) -> Webhook {
        Webhook {
            descriptor: descriptor(),
            http,
            clock: unix_now,
        }
    }

    #[cfg(test)]
    fn with_clock(mut self, clock: fn() -> i64) -> Webhook {
        self.clock = clock;
        self
    }
}

pub fn descriptor() -> NotifierDescriptor {
    NotifierDescriptor {
        id: "webhook".into(),
        display_name: "Webhook".into(),
        description: "Sends each notification as JSON to a URL you control.".into(),
        capabilities: Capabilities {
            formats: vec![BodyFormat::Markdown, BodyFormat::Plain],
            max_title_bytes: 500,
            max_body_bytes: 64 * 1024,
            attachments: AttachmentLimits::NONE,
            priority: true,
            click_url: true,
            audio: false,
        },
        settings: vec![
            SettingField::new("url", "URL", SettingKind::Secret)
                .required()
                .help("Stored as a credential because webhook URLs usually embed a token."),
            SettingField::new(
                "method",
                "Method",
                SettingKind::Choice {
                    choices: vec!["POST".into(), "PUT".into()],
                },
            )
            .default_value(json!("POST")),
            SettingField::new("signing_secret", "Signing secret", SettingKind::Secret).help(
                "When set, X-Hotsheet-Signature carries sha256=<hex HMAC-SHA256 of \"<timestamp>.<body>\">.",
            ),
        ],
        source: NotifierSource::BuiltIn,
    }
}

/// The JSON body: the rendered message, with attachments described but not uploaded.
pub fn payload(rendered: &Rendered) -> Value {
    json!({
        "event": rendered.event,
        "severity": rendered.severity,
        "title": rendered.title,
        "body": rendered.body,
        "format": rendered.format,
        "url": rendered.url,
        "project": rendered.project,
        "project_name": rendered.project_name,
        "ticket": rendered.ticket,
        "truncated": rendered.truncated,
        "omitted_attachments": rendered.omitted_attachments,
    })
}

/// `sha256=<hex>` of HMAC-SHA256(secret, "<timestamp>.<body>").
pub fn signature(secret: &str, timestamp: i64, body: &[u8]) -> String {
    let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, secret.as_bytes());
    let mut context = ring::hmac::Context::with_key(&key);
    context.update(timestamp.to_string().as_bytes());
    context.update(b".");
    context.update(body);
    let tag = context.sign();
    let hex: String = tag.as_ref().iter().map(|b| format!("{b:02x}")).collect();
    format!("sha256={hex}")
}

impl Notifier for Webhook {
    fn descriptor(&self) -> &NotifierDescriptor {
        &self.descriptor
    }

    fn deliver(
        &self,
        _channel: &str,
        rendered: &Rendered,
        settings: &Resolved,
    ) -> Result<Option<String>, DeliveryError> {
        let url = settings.str("url").unwrap_or_default();
        require_http_url(url, "webhook URL")?;
        let body = payload(rendered).to_string().into_bytes();
        let mut headers = vec![("Content-Type".to_string(), "application/json".to_string())];
        if let Some(secret) = settings.str("signing_secret") {
            let timestamp = (self.clock)();
            headers.push(("X-Hotsheet-Timestamp".into(), timestamp.to_string()));
            headers.push((
                "X-Hotsheet-Signature".into(),
                signature(secret, timestamp, &body),
            ));
        }
        let request = HttpRequest {
            method: settings.str("method").unwrap_or("POST").to_string(),
            url: url.to_string(),
            headers,
            body,
        };
        let response = self
            .http
            .send(&request)
            .map_err(|e| DeliveryError::retryable(format!("could not reach the webhook: {e}")))?;
        classify(&response, "the webhook")?;
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{EventKind, Message, Severity, TicketFacts};
    use crate::render::render;
    use crate::testing::FakeHttp;

    fn resolved(value: Value) -> Resolved {
        Resolved(serde_json::from_value(value).unwrap())
    }

    fn rendered() -> Rendered {
        let mut m = Message::new(EventKind::TicketBlocked, "HS2-1 is blocked");
        m.body = "**Waiting** on review".into();
        m.severity = Severity::Warning;
        m.url = Some("https://hs.test/HS2-1".into());
        m.ticket = Some(TicketFacts {
            slug: "HS2-1".into(),
            ..TicketFacts::default()
        });
        render(&m, &descriptor().capabilities)
    }

    #[test]
    fn posts_the_message_as_json_and_signs_it() {
        let http = Arc::new(FakeHttp::replying(204, ""));
        Webhook::with_transport(http.clone())
            .with_clock(|| 1_800_000_000)
            .deliver(
                "c",
                &rendered(),
                &resolved(
                    json!({"url":"https://hooks.test/in","signing_secret":"s3","method":"POST"}),
                ),
            )
            .unwrap();
        let request = http.requests().remove(0);
        assert_eq!(
            (request.method.as_str(), request.url.as_str()),
            ("POST", "https://hooks.test/in")
        );
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        assert_eq!(body["event"], "ticket.blocked");
        assert_eq!(body["severity"], "warning");
        assert_eq!(body["title"], "HS2-1 is blocked");
        assert_eq!(body["body"], "**Waiting** on review");
        assert_eq!(body["format"], "markdown");
        assert_eq!(body["url"], "https://hs.test/HS2-1");
        assert_eq!(body["ticket"]["slug"], "HS2-1");
        let header = |name: &str| {
            request
                .headers
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| v.clone())
        };
        assert_eq!(
            header("X-Hotsheet-Timestamp").as_deref(),
            Some("1800000000")
        );
        assert_eq!(
            header("X-Hotsheet-Signature"),
            Some(signature("s3", 1_800_000_000, &request.body))
        );
    }

    #[test]
    fn signature_matches_a_known_hmac_vector() {
        // RFC 4231 test case 2: key "Jefe", data "what do ya want for nothing?".
        let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, b"Jefe");
        let tag = ring::hmac::sign(&key, b"what do ya want for nothing?");
        let hex: String = tag.as_ref().iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            hex,
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        // Our framing is "<timestamp>.<body>".
        assert_eq!(signature("Jefe", 12, b"x"), {
            let tag = ring::hmac::sign(&key, b"12.x");
            format!(
                "sha256={}",
                tag.as_ref()
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>()
            )
        });
    }

    #[test]
    fn unsigned_put_bad_urls_and_failures() {
        let http = Arc::new(FakeHttp::replying(200, "ok"));
        let hook = Webhook::with_transport(http.clone());
        hook.deliver(
            "c",
            &rendered(),
            &resolved(json!({"url":"http://127.0.0.1:9/x","method":"PUT"})),
        )
        .unwrap();
        let request = http.requests().remove(0);
        assert_eq!(request.method, "PUT");
        assert!(
            !request
                .headers
                .iter()
                .any(|(n, _)| n.starts_with("X-Hotsheet"))
        );
        let err = hook
            .deliver("c", &rendered(), &resolved(json!({"url":"ftp://x"})))
            .unwrap_err();
        assert!(!err.retryable);
        let hook = Webhook::with_transport(Arc::new(FakeHttp::replying(502, "bad gateway")));
        assert!(
            hook.deliver("c", &rendered(), &resolved(json!({"url":"https://h.test"})))
                .unwrap_err()
                .retryable
        );
    }
}
