//! `teams`: post an Adaptive Card to a Microsoft Teams workflow webhook (HS2-97ZHMM).
//! Adaptive Card text blocks render the CommonMark subset (bold, italic, lists, links), so
//! the body is passed as Markdown.

use std::sync::Arc;

use serde_json::{Value, json};

use crate::capability::{
    AttachmentLimits, BodyFormat, Capabilities, NotifierDescriptor, NotifierSource, SettingField,
    SettingKind,
};
use crate::http::{HttpRequest, HttpTransport, LiveHttp, classify, require_http_url};
use crate::message::Severity;
use crate::notifier::{DeliveryError, Notifier};
use crate::render::Rendered;
use crate::settings::Resolved;

pub struct Teams {
    descriptor: NotifierDescriptor,
    http: Arc<dyn HttpTransport>,
}

impl Teams {
    pub fn live() -> Teams {
        Teams::with_transport(Arc::new(LiveHttp))
    }

    pub fn with_transport(http: Arc<dyn HttpTransport>) -> Teams {
        Teams {
            descriptor: descriptor(),
            http,
        }
    }
}

pub fn descriptor() -> NotifierDescriptor {
    NotifierDescriptor {
        id: "teams".into(),
        display_name: "Microsoft Teams".into(),
        description: "Posts an Adaptive Card to a Teams channel through a workflow webhook.".into(),
        capabilities: Capabilities {
            formats: vec![BodyFormat::Markdown],
            max_title_bytes: 250,
            // Teams rejects payloads over about 28 KB; leave room for the card envelope.
            max_body_bytes: 20_000,
            attachments: AttachmentLimits::NONE,
            priority: true,
            click_url: true,
            audio: false,
        },
        settings: vec![
            SettingField::new("webhook_url", "Workflow Webhook URL", SettingKind::Secret)
                .required()
                .help("From the Teams workflow \"Post to a channel when a webhook request is received\"."),
        ],
        source: NotifierSource::BuiltIn,
    }
}

/// The message envelope Teams workflows accept: one Adaptive Card 1.4 attachment.
pub fn payload(rendered: &Rendered) -> Value {
    let color = match rendered.severity {
        Severity::Error => "Attention",
        Severity::Warning => "Warning",
        Severity::Success => "Good",
        Severity::Info => "Default",
    };
    let mut body = Vec::new();
    if let Some(title) = rendered.title.as_deref().filter(|t| !t.is_empty()) {
        body.push(json!({"type":"TextBlock","text":title,"weight":"Bolder","size":"Medium","wrap":true,"color":color}));
    }
    if !rendered.body.is_empty() {
        body.push(json!({"type":"TextBlock","text":rendered.body,"wrap":true}));
    }
    let mut card = json!({
        "$schema": "http://adaptivecards.io/schemas/adaptive-card.json",
        "type": "AdaptiveCard",
        "version": "1.4",
        "body": body,
    });
    if let Some(url) = &rendered.url {
        card["actions"] = json!([{"type":"Action.OpenUrl","title":"Open in Hot Sheet","url":url}]);
    }
    json!({
        "type": "message",
        "attachments": [{"contentType":"application/vnd.microsoft.card.adaptive","content":card}],
    })
}

impl Notifier for Teams {
    fn descriptor(&self) -> &NotifierDescriptor {
        &self.descriptor
    }

    fn deliver(
        &self,
        _channel: &str,
        rendered: &Rendered,
        settings: &Resolved,
    ) -> Result<Option<String>, DeliveryError> {
        let url = settings.str("webhook_url").unwrap_or_default();
        require_http_url(url, "Teams webhook URL")?;
        let response = self
            .http
            .send(&HttpRequest {
                method: "POST".into(),
                url: url.into(),
                headers: vec![("Content-Type".into(), "application/json".into())],
                body: payload(rendered).to_string().into_bytes(),
            })
            .map_err(|e| DeliveryError::retryable(format!("could not reach Teams: {e}")))?;
        classify(&response, "Teams")?;
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{EventKind, Message};
    use crate::render::render;
    use crate::testing::FakeHttp;

    #[test]
    fn posts_an_adaptive_card_with_severity_color_and_link() {
        let mut m = Message::new(
            EventKind::ProjectStopped,
            "Hot Sheet project demo has stalled",
        );
        m.body = "The claim on **HS2-1** expired.".into();
        m.severity = Severity::Error;
        m.url = Some("https://hs.test/demo".into());
        let http = Arc::new(FakeHttp::replying(202, ""));
        Teams::with_transport(http.clone())
            .deliver(
                "c",
                &render(&m, &descriptor().capabilities),
                &Resolved(
                    serde_json::from_value(json!({"webhook_url":"https://prod.workflows.test/x"}))
                        .unwrap(),
                ),
            )
            .unwrap();
        let body: Value = serde_json::from_slice(&http.requests()[0].body).unwrap();
        assert_eq!(body["type"], "message");
        let card = &body["attachments"][0]["content"];
        assert_eq!(
            body["attachments"][0]["contentType"],
            "application/vnd.microsoft.card.adaptive"
        );
        assert_eq!(card["version"], "1.4");
        assert_eq!(
            card["body"][0]["text"],
            "Hot Sheet project demo has stalled"
        );
        assert_eq!(card["body"][0]["color"], "Attention");
        assert_eq!(card["body"][1]["text"], "The claim on **HS2-1** expired.");
        assert_eq!(card["actions"][0]["url"], "https://hs.test/demo");
    }

    #[test]
    fn teams_failures_are_classified() {
        let rendered = render(
            &Message::new(EventKind::Test, "t"),
            &descriptor().capabilities,
        );
        let settings =
            Resolved(serde_json::from_value(json!({"webhook_url":"https://h.test"})).unwrap());
        assert!(
            !Teams::with_transport(Arc::new(FakeHttp::replying(400, "bad card")))
                .deliver("c", &rendered, &settings)
                .unwrap_err()
                .retryable
        );
        assert!(
            Teams::with_transport(Arc::new(FakeHttp::unreachable()))
                .deliver("c", &rendered, &settings)
                .unwrap_err()
                .retryable
        );
    }
}
