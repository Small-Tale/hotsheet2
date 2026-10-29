//! `ntfy`: push notifications through an ntfy server (ntfy.sh or self-hosted). Markdown
//! bodies, priority from severity, tags, a click URL, and one attachment uploaded with
//! the message.

use std::sync::Arc;

use serde_json::{Value, json};

use crate::capability::{
    AttachmentLimits, BodyFormat, Capabilities, NotifierDescriptor, NotifierSource, SettingField,
    SettingKind,
};
use crate::http::{HttpRequest, HttpTransport, LiveHttp, classify, header_text};
use crate::message::Severity;
use crate::notifier::{DeliveryError, Notifier};
use crate::render::Rendered;
use crate::settings::Resolved;

pub const DEFAULT_SERVER: &str = "https://ntfy.sh";
/// ntfy.sh's message cap; longer bodies would be turned into an attachment by the server.
const MAX_MESSAGE_BYTES: usize = 4096;
/// ntfy.sh's default per-file attachment cap.
const DEFAULT_ATTACHMENT_BYTES: usize = 15 * 1024 * 1024;

pub struct Ntfy {
    descriptor: NotifierDescriptor,
    http: Arc<dyn HttpTransport>,
}

impl Ntfy {
    pub fn live() -> Ntfy {
        Ntfy::with_transport(Arc::new(LiveHttp))
    }

    pub fn with_transport(http: Arc<dyn HttpTransport>) -> Ntfy {
        Ntfy {
            descriptor: descriptor(),
            http,
        }
    }
}

pub fn descriptor() -> NotifierDescriptor {
    NotifierDescriptor {
        id: "ntfy".into(),
        display_name: "ntfy Push".into(),
        description: "Sends push notifications to phones and desktops subscribed to an ntfy topic.".into(),
        capabilities: Capabilities {
            formats: vec![BodyFormat::Markdown, BodyFormat::Plain],
            max_title_bytes: 250,
            max_body_bytes: MAX_MESSAGE_BYTES,
            attachments: AttachmentLimits {
                max_count: 1,
                max_bytes: DEFAULT_ATTACHMENT_BYTES,
                max_total_bytes: DEFAULT_ATTACHMENT_BYTES,
                images_only: false,
            },
            priority: true,
            click_url: true,
            audio: false,
        },
        settings: vec![
            SettingField::new("server", "Server", SettingKind::String)
                .default_value(json!(DEFAULT_SERVER))
                .help("The ntfy server's base URL."),
            SettingField::new("topic", "Topic", SettingKind::String)
                .required()
                .help("Letters, digits, '-' and '_'. Anyone who knows a public topic can read it; choose a hard-to-guess name or use an access token."),
            SettingField::new("token", "Access Token", SettingKind::Secret)
                .help("Optional bearer token for a protected topic."),
            SettingField::new(
                "priority",
                "Priority",
                SettingKind::Choice {
                    choices: ["auto", "min", "low", "default", "high", "urgent"]
                        .map(String::from)
                        .to_vec(),
                },
            )
            .default_value(json!("auto"))
            .help("'auto' maps severity: info and success to default, warning to high, error to urgent."),
            SettingField::new("tags", "Tags", SettingKind::List),
        ],
        source: NotifierSource::BuiltIn,
    }
}

fn priority(setting: Option<&str>, severity: Severity) -> &'static str {
    match setting.unwrap_or("auto") {
        "min" => "1",
        "low" => "2",
        "default" => "3",
        "high" => "4",
        "urgent" => "5",
        _ => match severity {
            Severity::Info | Severity::Success => "3",
            Severity::Warning => "4",
            Severity::Error => "5",
        },
    }
}

fn valid_topic(topic: &str) -> bool {
    !topic.is_empty()
        && topic.len() <= 64
        && topic
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

impl Notifier for Ntfy {
    fn descriptor(&self) -> &NotifierDescriptor {
        &self.descriptor
    }

    fn deliver(
        &self,
        _channel: &str,
        rendered: &Rendered,
        settings: &Resolved,
    ) -> Result<Option<String>, DeliveryError> {
        let server = settings
            .str("server")
            .unwrap_or(DEFAULT_SERVER)
            .trim_end_matches('/');
        if !(server.starts_with("https://") || server.starts_with("http://")) {
            return Err(DeliveryError::permanent(
                "ntfy server must be an http(s) URL",
            ));
        }
        let topic = settings.str("topic").unwrap_or_default();
        if !valid_topic(topic) {
            return Err(DeliveryError::permanent(
                "ntfy topic must be 1-64 letters, digits, '-' or '_'",
            ));
        }
        let mut headers = vec![(
            "Priority".to_string(),
            priority(settings.str("priority"), rendered.severity).to_string(),
        )];
        if let Some(title) = rendered.title.as_deref().filter(|t| !t.is_empty()) {
            headers.push(("Title".into(), header_text(title)));
        }
        let mut tags = settings.list("tags");
        match rendered.severity {
            Severity::Warning => tags.push("warning".into()),
            Severity::Error => tags.push("rotating_light".into()),
            Severity::Success => tags.push("white_check_mark".into()),
            Severity::Info => {}
        }
        if !tags.is_empty() {
            headers.push(("Tags".into(), header_text(&tags.join(","))));
        }
        if let Some(url) = &rendered.url {
            headers.push(("Click".into(), url.clone()));
        }
        if rendered.format == BodyFormat::Markdown {
            headers.push(("Markdown".into(), "yes".into()));
        }
        if let Some(token) = settings.str("token") {
            headers.push(("Authorization".into(), format!("Bearer {token}")));
        }
        let url = format!("{server}/{topic}");
        let request = match rendered.attachments.first() {
            // An attachment is the request body, so the message moves to a header.
            Some(attachment) => {
                headers.push(("Filename".into(), header_text(&attachment.name)));
                headers.push(("Message".into(), header_text(&rendered.body)));
                HttpRequest {
                    method: "PUT".into(),
                    url,
                    headers,
                    body: attachment.bytes.clone(),
                }
            }
            None => HttpRequest {
                method: "POST".into(),
                url,
                headers,
                body: rendered.body.as_bytes().to_vec(),
            },
        };
        let response = self
            .http
            .send(&request)
            .map_err(|e| DeliveryError::retryable(format!("could not reach ntfy: {e}")))?;
        classify(&response, "ntfy")?;
        Ok(serde_json::from_str::<Value>(&response.body)
            .ok()
            .and_then(|v| v.get("id").and_then(Value::as_str).map(str::to_owned)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{Attachment, EventKind, Message};
    use crate::render::render;
    use crate::testing::FakeHttp;

    fn resolved(value: Value) -> Resolved {
        Resolved(serde_json::from_value(value).unwrap())
    }

    fn header<'a>(request: &'a HttpRequest, name: &str) -> Option<&'a str> {
        request
            .headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    fn message() -> Message {
        let mut m = Message::new(EventKind::TicketFeedbackNeeded, "HS2-7 needs feedback");
        m.body = "Pick **A** or **B**".into();
        m.severity = Severity::Warning;
        m.url = Some("https://hs.test/HS2-7".into());
        m
    }

    #[test]
    fn posts_markdown_with_priority_tags_click_and_token() {
        let http = Arc::new(FakeHttp::replying(200, r#"{"id":"abc"}"#));
        let ntfy = Ntfy::with_transport(http.clone());
        let id = ntfy
            .deliver("c", &render(&message(), &descriptor().capabilities),
                &resolved(json!({"server":"https://ntfy.example/","topic":"hs-team","token":"tk","tags":["hotsheet"]})),
            )
            .unwrap();
        assert_eq!(id.as_deref(), Some("abc"));
        let request = http.requests().remove(0);
        assert_eq!(request.method, "POST");
        assert_eq!(request.url, "https://ntfy.example/hs-team");
        assert_eq!(request.body, b"Pick **A** or **B**");
        assert_eq!(header(&request, "Title"), Some("HS2-7 needs feedback"));
        assert_eq!(header(&request, "Priority"), Some("4"));
        assert_eq!(header(&request, "Tags"), Some("hotsheet,warning"));
        assert_eq!(header(&request, "Click"), Some("https://hs.test/HS2-7"));
        assert_eq!(header(&request, "Markdown"), Some("yes"));
        assert_eq!(header(&request, "Authorization"), Some("Bearer tk"));
    }

    #[test]
    fn attachment_is_put_with_message_header() {
        let http = Arc::new(FakeHttp::replying(200, "{}"));
        let mut m = message();
        m.body = "line one\nline two".into();
        m.attachments = vec![Attachment {
            name: "shot.png".into(),
            content_type: "image/png".into(),
            bytes: vec![9; 4],
        }];
        Ntfy::with_transport(http.clone())
            .deliver(
                "c",
                &render(&m, &descriptor().capabilities),
                &resolved(json!({"topic":"t","priority":"min"})),
            )
            .unwrap();
        let request = http.requests().remove(0);
        assert_eq!(request.method, "PUT");
        assert_eq!(request.url, "https://ntfy.sh/t");
        assert_eq!(request.body, vec![9; 4]);
        assert_eq!(header(&request, "Filename"), Some("shot.png"));
        assert!(
            header(&request, "Message")
                .unwrap()
                .starts_with("=?UTF-8?B?")
        );
        assert_eq!(header(&request, "Priority"), Some("1"));
        assert_eq!(header(&request, "Authorization"), None);
    }

    #[test]
    fn rejects_bad_topic_and_server_and_classifies_failures() {
        let r = render(&message(), &descriptor().capabilities);
        let ntfy = Ntfy::with_transport(Arc::new(FakeHttp::replying(200, "{}")));
        assert!(
            !ntfy
                .deliver("c", &r, &resolved(json!({"topic":"a/b"})))
                .unwrap_err()
                .retryable
        );
        assert!(
            !ntfy
                .deliver(
                    "c",
                    &r,
                    &resolved(json!({"topic":"a","server":"file:///x"}))
                )
                .unwrap_err()
                .retryable
        );
        let ntfy = Ntfy::with_transport(Arc::new(FakeHttp::replying(429, "slow down")));
        assert!(
            ntfy.deliver("c", &r, &resolved(json!({"topic":"a"})))
                .unwrap_err()
                .retryable
        );
        let ntfy = Ntfy::with_transport(Arc::new(FakeHttp::unreachable()));
        assert!(
            ntfy.deliver("c", &r, &resolved(json!({"topic":"a"})))
                .unwrap_err()
                .retryable
        );
    }

    #[test]
    fn auto_priority_follows_severity() {
        assert_eq!(priority(None, Severity::Info), "3");
        assert_eq!(priority(Some("auto"), Severity::Error), "5");
        assert_eq!(priority(Some("high"), Severity::Info), "4");
    }
}
