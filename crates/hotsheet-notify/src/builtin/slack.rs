//! `slack`: post to a Slack incoming webhook as Block Kit, converting the CommonMark body to
//! Slack mrkdwn (HS2-97ZHMM).

use std::sync::Arc;

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use serde_json::{Value, json};

use crate::capability::{
    AttachmentLimits, BodyFormat, Capabilities, NotifierDescriptor, NotifierSource, SettingField,
    SettingKind,
};
use crate::http::{HttpRequest, HttpTransport, LiveHttp, classify, require_http_url};
use crate::notifier::{DeliveryError, Notifier};
use crate::render::{Rendered, markdown_to_plain, truncate_utf8};
use crate::settings::Resolved;

/// Slack caps a section's text at 3,000 characters; mrkdwn conversion can lengthen the
/// rendered body slightly, so the final text is capped again.
const SECTION_LIMIT: usize = 3000;

pub struct Slack {
    descriptor: NotifierDescriptor,
    http: Arc<dyn HttpTransport>,
}

impl Slack {
    pub fn live() -> Slack {
        Slack::with_transport(Arc::new(LiveHttp))
    }

    pub fn with_transport(http: Arc<dyn HttpTransport>) -> Slack {
        Slack {
            descriptor: descriptor(),
            http,
        }
    }
}

pub fn descriptor() -> NotifierDescriptor {
    NotifierDescriptor {
        id: "slack".into(),
        display_name: "Slack".into(),
        description: "Posts to a Slack channel through an incoming webhook.".into(),
        capabilities: Capabilities {
            formats: vec![BodyFormat::Markdown],
            max_title_bytes: 150,
            max_body_bytes: 2800,
            attachments: AttachmentLimits::NONE,
            priority: false,
            click_url: true,
            audio: false,
        },
        settings: vec![
            SettingField::new("webhook_url", "Incoming webhook URL", SettingKind::Secret)
                .required()
                .help("From a Slack app's Incoming Webhooks page; it is the credential."),
        ],
        source: NotifierSource::BuiltIn,
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// CommonMark to Slack mrkdwn: `*bold*`, `_italic_`, `~strike~`, `` `code` ``,
/// `<url|text>` links, headings as bold lines, and `•` bullets. Raw HTML stays text.
pub fn markdown_to_mrkdwn(markdown: &str) -> String {
    let mut out = String::new();
    let mut link: Option<(String, String)> = None;
    let mut depth = 0usize;
    let push = |out: &mut String, link: &mut Option<(String, String)>, text: &str| match link {
        Some((_, label)) => label.push_str(text),
        None => out.push_str(text),
    };
    for event in Parser::new_ext(markdown, Options::ENABLE_STRIKETHROUGH) {
        match event {
            Event::Text(t) | Event::Html(t) | Event::InlineHtml(t) => {
                push(&mut out, &mut link, &escape(&t))
            }
            Event::Code(t) => push(&mut out, &mut link, &format!("`{}`", escape(&t))),
            Event::SoftBreak => push(&mut out, &mut link, " "),
            Event::HardBreak => push(&mut out, &mut link, "\n"),
            Event::Start(Tag::Strong) | Event::End(TagEnd::Strong) => {
                push(&mut out, &mut link, "*")
            }
            Event::Start(Tag::Emphasis) | Event::End(TagEnd::Emphasis) => {
                push(&mut out, &mut link, "_")
            }
            Event::Start(Tag::Strikethrough) | Event::End(TagEnd::Strikethrough) => {
                push(&mut out, &mut link, "~")
            }
            Event::Start(Tag::Heading { .. }) => out.push('*'),
            Event::End(TagEnd::Heading(_)) => out.push_str("*\n\n"),
            Event::Start(Tag::Link { dest_url, .. }) => {
                link = Some((dest_url.to_string(), String::new()))
            }
            Event::End(TagEnd::Link) => {
                if let Some((url, label)) = link.take() {
                    let url = escape(&url).replace('|', "%7C");
                    if label.is_empty() {
                        out.push_str(&format!("<{url}>"));
                    } else {
                        out.push_str(&format!("<{url}|{}>", label.replace('|', "/")));
                    }
                }
            }
            Event::Start(Tag::List(_)) => depth += 1,
            Event::End(TagEnd::List(_)) => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    out.push_str("\n\n");
                }
            }
            Event::Start(Tag::Item) => {
                if !out.is_empty() && !out.ends_with('\n') {
                    out.push('\n');
                }
                out.push_str(&"    ".repeat(depth.saturating_sub(1)));
                out.push_str("• ");
            }
            Event::Start(Tag::CodeBlock(_)) => out.push_str("```\n"),
            Event::End(TagEnd::CodeBlock) => out.push_str("```\n\n"),
            Event::End(TagEnd::Paragraph) if depth == 0 => out.push_str("\n\n"),
            Event::Rule => out.push_str("----\n\n"),
            _ => {}
        }
    }
    out.trim().to_string()
}

/// The Block Kit payload: a header, the mrkdwn section, an optional link button, and a
/// plain-text fallback for notifications and clients without blocks.
pub fn payload(rendered: &Rendered) -> Value {
    let title = rendered.title.clone().unwrap_or_default();
    let (section, _) = truncate_utf8(&markdown_to_mrkdwn(&rendered.body), SECTION_LIMIT);
    let fallback = if title.is_empty() {
        markdown_to_plain(&rendered.body, false)
    } else {
        title.clone()
    };
    let mut blocks = Vec::new();
    if !title.is_empty() {
        blocks
            .push(json!({"type":"header","text":{"type":"plain_text","text":title,"emoji":false}}));
    }
    if !section.is_empty() {
        blocks.push(json!({"type":"section","text":{"type":"mrkdwn","text":section}}));
    }
    if let Some(url) = &rendered.url {
        blocks.push(json!({"type":"actions","elements":[
            {"type":"button","text":{"type":"plain_text","text":"Open in Hot Sheet"},"url":url}
        ]}));
    }
    json!({"text": fallback, "blocks": blocks})
}

impl Notifier for Slack {
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
        require_http_url(url, "Slack webhook URL")?;
        let response = self
            .http
            .send(&HttpRequest {
                method: "POST".into(),
                url: url.into(),
                headers: vec![("Content-Type".into(), "application/json".into())],
                body: payload(rendered).to_string().into_bytes(),
            })
            .map_err(|e| DeliveryError::retryable(format!("could not reach Slack: {e}")))?;
        classify(&response, "Slack")?;
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
    fn converts_markdown_to_mrkdwn() {
        let text = markdown_to_mrkdwn(
            "## Risks\n\n- **HS2-1** _regressed_ in `ci`\n- see [the run](https://ci.test/1?a=1&b=2)\n\nDone <now> & ~~later~~.",
        );
        assert_eq!(
            text,
            "*Risks*\n\n• *HS2-1* _regressed_ in `ci`\n• see <https://ci.test/1?a=1&amp;b=2|the run>\n\nDone &lt;now&gt; &amp; ~later~."
        );
    }

    #[test]
    fn posts_block_kit_with_header_section_button_and_fallback() {
        let mut m = Message::new(EventKind::TicketFeedbackNeeded, "HS2-7 needs your feedback");
        m.body = "Pick **A** or **B**".into();
        m.url = Some("https://hs.test/HS2-7".into());
        let rendered = render(&m, &descriptor().capabilities);
        let http = Arc::new(FakeHttp::replying(200, "ok"));
        Slack::with_transport(http.clone())
            .deliver(
                "c",
                &rendered,
                &Resolved(
                    serde_json::from_value(json!({"webhook_url":"https://hooks.slack.test/T/B/x"}))
                        .unwrap(),
                ),
            )
            .unwrap();
        let request = http.requests().remove(0);
        assert_eq!(request.url, "https://hooks.slack.test/T/B/x");
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        assert_eq!(body["text"], "HS2-7 needs your feedback");
        assert_eq!(
            body["blocks"][0]["text"]["text"],
            "HS2-7 needs your feedback"
        );
        assert_eq!(body["blocks"][1]["text"]["type"], "mrkdwn");
        assert_eq!(body["blocks"][1]["text"]["text"], "Pick *A* or *B*");
        assert_eq!(
            body["blocks"][2]["elements"][0]["url"],
            "https://hs.test/HS2-7"
        );
    }

    #[test]
    fn slack_errors_and_bad_urls() {
        let rendered = render(
            &Message::new(EventKind::Test, "t"),
            &descriptor().capabilities,
        );
        let settings =
            Resolved(serde_json::from_value(json!({"webhook_url":"https://h.test"})).unwrap());
        let slack = Slack::with_transport(Arc::new(FakeHttp::replying(404, "no_service")));
        let err = slack.deliver("c", &rendered, &settings).unwrap_err();
        assert!(!err.retryable && err.message.contains("no_service"));
        let slack = Slack::with_transport(Arc::new(FakeHttp::replying(429, "rate_limited")));
        assert!(
            slack
                .deliver("c", &rendered, &settings)
                .unwrap_err()
                .retryable
        );
        let bad = Resolved(serde_json::from_value(json!({"webhook_url":"slack.com"})).unwrap());
        assert!(!slack.deliver("c", &rendered, &bad).unwrap_err().retryable);
    }
}
