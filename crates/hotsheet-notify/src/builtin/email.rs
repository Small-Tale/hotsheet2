//! `email`: SMTP delivery to one or more recipients, as multipart plain text plus HTML
//! with attachments. TLS uses rustls; the password is a key-registry reference.

use std::sync::Arc;
use std::time::Duration;

use lettre::message::header::ContentType;
use lettre::message::{Attachment as MimeAttachment, Mailbox, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message as Email, SmtpTransport, Transport};
use serde_json::json;

use crate::capability::{
    AttachmentLimits, BodyFormat, Capabilities, NotifierDescriptor, NotifierSource, SettingField,
    SettingKind,
};
use crate::notifier::{DeliveryError, Notifier};
use crate::render::Rendered;
use crate::settings::Resolved;

const MAX_ATTACHMENT_BYTES: usize = 10 * 1024 * 1024;

/// SMTP connection settings for one delivery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    /// `starttls`, `tls` (implicit), or `none`.
    pub security: String,
    pub username: Option<String>,
    pub password: Option<String>,
}

/// Sends a built message. Injectable so tests never open an SMTP connection.
pub trait EmailSender: Send + Sync {
    fn send(&self, config: &SmtpConfig, email: &Email) -> Result<(), DeliveryError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct LiveSmtp;

impl EmailSender for LiveSmtp {
    fn send(&self, config: &SmtpConfig, email: &Email) -> Result<(), DeliveryError> {
        let builder = match config.security.as_str() {
            "tls" => SmtpTransport::relay(&config.host),
            "none" => Ok(SmtpTransport::builder_dangerous(&config.host)),
            _ => SmtpTransport::starttls_relay(&config.host),
        }
        .map_err(|e| {
            DeliveryError::permanent(format!("SMTP TLS setup for {}: {e}", config.host))
        })?;
        let mut builder = builder
            .port(config.port)
            .timeout(Some(Duration::from_secs(30)));
        if let Some(user) = &config.username {
            builder = builder.credentials(Credentials::new(
                user.clone(),
                config.password.clone().unwrap_or_default(),
            ));
        }
        builder.build().send(email).map(|_| ()).map_err(|e| {
            let message = format!("SMTP delivery via {}: {e}", config.host);
            if e.is_permanent() {
                DeliveryError::permanent(message)
            } else {
                DeliveryError::retryable(message)
            }
        })
    }
}

pub struct EmailNotifier {
    descriptor: NotifierDescriptor,
    sender: Arc<dyn EmailSender>,
}

impl EmailNotifier {
    pub fn live() -> EmailNotifier {
        EmailNotifier::with_sender(Arc::new(LiveSmtp))
    }

    pub fn with_sender(sender: Arc<dyn EmailSender>) -> EmailNotifier {
        EmailNotifier {
            descriptor: descriptor(),
            sender,
        }
    }
}

pub fn descriptor() -> NotifierDescriptor {
    NotifierDescriptor {
        id: "email".into(),
        display_name: "Email".into(),
        description: "Sends notifications by email through an SMTP server you choose.".into(),
        capabilities: Capabilities {
            formats: vec![BodyFormat::Html, BodyFormat::Plain],
            max_title_bytes: 200,
            max_body_bytes: 200_000,
            attachments: AttachmentLimits {
                max_count: 10,
                max_bytes: MAX_ATTACHMENT_BYTES,
                max_total_bytes: MAX_ATTACHMENT_BYTES,
                images_only: false,
            },
            priority: false,
            click_url: false,
            audio: false,
        },
        settings: vec![
            SettingField::new("to", "Recipients", SettingKind::List).required(),
            SettingField::new("from", "From Address", SettingKind::String).required(),
            SettingField::new("smtp_host", "SMTP Host", SettingKind::String).required(),
            SettingField::new("smtp_port", "SMTP Port", SettingKind::Number)
                .default_value(json!(587)),
            SettingField::new(
                "security",
                "Connection Security",
                SettingKind::Choice {
                    choices: ["starttls", "tls", "none"].map(String::from).to_vec(),
                },
            )
            .default_value(json!("starttls"))
            .help("'none' sends credentials and mail unencrypted; use it only for a local relay."),
            SettingField::new("username", "SMTP Username", SettingKind::String),
            SettingField::new("password", "SMTP Password", SettingKind::Secret),
        ],
        source: NotifierSource::BuiltIn,
    }
}

fn mailbox(address: &str, field: &str) -> Result<Mailbox, DeliveryError> {
    address
        .parse()
        .map_err(|e| DeliveryError::permanent(format!("invalid {field} address '{address}': {e}")))
}

/// Build the MIME message: plain text (and HTML when rendered) plus attachments.
pub fn build_email(rendered: &Rendered, settings: &Resolved) -> Result<Email, DeliveryError> {
    let recipients = settings.list("to");
    if recipients.is_empty() {
        return Err(DeliveryError::permanent(
            "email needs at least one recipient",
        ));
    }
    let mut builder = Email::builder()
        .from(mailbox(settings.str("from").unwrap_or_default(), "from")?)
        .subject(rendered.title.clone().unwrap_or_else(|| "Hot Sheet".into()));
    for to in &recipients {
        builder = builder.to(mailbox(to, "recipient")?);
    }
    let text = match rendered.format {
        BodyFormat::Html => rendered.plain_alternative.clone().unwrap_or_default(),
        _ => rendered.body.clone(),
    };
    let body = if rendered.format == BodyFormat::Html {
        MultiPart::alternative_plain_html(text, rendered.body.clone())
    } else {
        MultiPart::mixed().singlepart(SinglePart::plain(text))
    };
    let message = if rendered.attachments.is_empty() {
        builder.multipart(body)
    } else {
        let mut mixed = MultiPart::mixed().multipart(body);
        for attachment in &rendered.attachments {
            let content_type = ContentType::parse(&attachment.content_type).unwrap_or(
                ContentType::parse("application/octet-stream").expect("valid MIME type"),
            );
            mixed = mixed.singlepart(
                MimeAttachment::new(attachment.name.clone())
                    .body(attachment.bytes.clone(), content_type),
            );
        }
        builder.multipart(mixed)
    };
    message.map_err(|e| DeliveryError::permanent(format!("building email: {e}")))
}

impl Notifier for EmailNotifier {
    fn descriptor(&self) -> &NotifierDescriptor {
        &self.descriptor
    }

    fn deliver(
        &self,
        _channel: &str,
        rendered: &Rendered,
        settings: &Resolved,
    ) -> Result<Option<String>, DeliveryError> {
        let email = build_email(rendered, settings)?;
        let port = settings.number("smtp_port").unwrap_or(587.0);
        if !(1.0..=65535.0).contains(&port) {
            return Err(DeliveryError::permanent("SMTP port must be 1-65535"));
        }
        let config = SmtpConfig {
            host: settings.str("smtp_host").unwrap_or_default().to_string(),
            port: port as u16,
            security: settings.str("security").unwrap_or("starttls").to_string(),
            username: settings.str("username").map(str::to_owned),
            password: settings.str("password").map(str::to_owned),
        };
        self.sender.send(&config, &email)?;
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{Attachment, EventKind, Message};
    use crate::render::render;
    use crate::testing::RecordingSmtp;
    use serde_json::{Value, json};

    fn resolved(value: Value) -> Resolved {
        Resolved(serde_json::from_value(value).unwrap())
    }

    fn settings() -> Resolved {
        resolved(json!({
            "to":["lead@example.test","ops@example.test"],
            "from":"Hot Sheet <hs@example.test>",
            "smtp_host":"smtp.example.test",
            "smtp_port":465,
            "security":"tls",
            "username":"hs",
            "password":"pw"
        }))
    }

    fn message() -> Message {
        let mut m = Message::new(EventKind::Digest, "Daily digest");
        m.body = "## Risks\n\n- **HS2-1** regressed".into();
        m.url = Some("https://hs.test/digest".into());
        m
    }

    #[test]
    fn sends_multipart_html_and_text_to_every_recipient() {
        let smtp = Arc::new(RecordingSmtp::default());
        EmailNotifier::with_sender(smtp.clone())
            .deliver(
                "c",
                &render(&message(), &descriptor().capabilities),
                &settings(),
            )
            .unwrap();
        let (config, raw) = smtp.sent().remove(0);
        assert_eq!(
            config,
            SmtpConfig {
                host: "smtp.example.test".into(),
                port: 465,
                security: "tls".into(),
                username: Some("hs".into()),
                password: Some("pw".into()),
            }
        );
        assert!(
            raw.contains("To: lead@example.test, ops@example.test"),
            "{raw}"
        );
        assert!(raw.contains("Subject: Daily digest"));
        assert!(raw.contains("multipart/alternative"));
        assert!(raw.contains("<strong>HS2-1</strong>"));
        assert!(raw.contains("- HS2-1 regressed"));
        assert!(raw.contains("https://hs.test/digest"));
    }

    #[test]
    fn attachments_become_mime_parts() {
        let mut m = message();
        m.attachments = vec![Attachment {
            name: "chart.png".into(),
            content_type: "image/png".into(),
            bytes: vec![1, 2, 3],
        }];
        let email = build_email(&render(&m, &descriptor().capabilities), &settings()).unwrap();
        let raw = String::from_utf8(email.formatted()).unwrap();
        assert!(raw.contains("multipart/mixed"));
        assert!(raw.contains("filename=\"chart.png\""), "{raw}");
    }

    #[test]
    fn invalid_addresses_and_ports_are_permanent() {
        let r = render(&message(), &descriptor().capabilities);
        let mut bad = settings();
        bad.0.insert("to".into(), json!(["not an address"]));
        assert!(!build_email(&r, &bad).unwrap_err().retryable);
        bad.0.insert("to".into(), json!([]));
        assert!(
            build_email(&r, &bad)
                .unwrap_err()
                .message
                .contains("recipient")
        );
        let mut bad = settings();
        bad.0.insert("smtp_port".into(), json!(70000));
        let notifier = EmailNotifier::with_sender(Arc::new(RecordingSmtp::default()));
        assert!(
            notifier
                .deliver("c", &r, &bad)
                .unwrap_err()
                .message
                .contains("port")
        );
    }

    #[test]
    fn sender_failures_propagate() {
        let smtp = Arc::new(RecordingSmtp::failing(DeliveryError::retryable("421 busy")));
        let err = EmailNotifier::with_sender(smtp)
            .deliver(
                "c",
                &render(&message(), &descriptor().capabilities),
                &settings(),
            )
            .unwrap_err();
        assert!(err.retryable);
    }
}
