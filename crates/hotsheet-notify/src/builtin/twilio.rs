//! `twilio`: SMS through the Twilio Messages API, one message per recipient (HS2-97ZHMM).
//! MMS needs publicly hosted media, which Hot Sheet does not provide (HS2-K57DK0).

use std::sync::Arc;

use base64::Engine;
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde_json::{Value, json};

use crate::capability::{
    AttachmentLimits, BodyFormat, Capabilities, NotifierDescriptor, NotifierSource, SettingField,
    SettingKind,
};
use crate::http::{HttpRequest, HttpTransport, LiveHttp, classify, require_http_url};
use crate::notifier::{DeliveryError, Notifier};
use crate::render::Rendered;
use crate::settings::Resolved;

pub const DEFAULT_API_BASE: &str = "https://api.twilio.com";

pub struct Twilio {
    descriptor: NotifierDescriptor,
    http: Arc<dyn HttpTransport>,
}

impl Twilio {
    pub fn live() -> Twilio {
        Twilio::with_transport(Arc::new(LiveHttp))
    }

    pub fn with_transport(http: Arc<dyn HttpTransport>) -> Twilio {
        Twilio {
            descriptor: descriptor(),
            http,
        }
    }
}

pub fn descriptor() -> NotifierDescriptor {
    NotifierDescriptor {
        id: "twilio".into(),
        display_name: "SMS (Twilio)".into(),
        description: "Texts one or more phone numbers through your Twilio account.".into(),
        capabilities: Capabilities {
            formats: vec![BodyFormat::Plain],
            max_title_bytes: 0,
            // Twilio's limit is 1,600 characters; bytes keep multibyte text inside it.
            max_body_bytes: 1600,
            attachments: AttachmentLimits::NONE,
            priority: false,
            click_url: false,
            audio: false,
        },
        settings: vec![
            SettingField::new("account_sid", "Account SID", SettingKind::String).required(),
            SettingField::new("auth_token", "Auth Token", SettingKind::Secret).required(),
            SettingField::new("from", "From Number", SettingKind::String)
                .required()
                .help("A Twilio number or messaging service SID (MG…)."),
            SettingField::new("to", "Recipients", SettingKind::List)
                .required()
                .help("E.164 numbers, for example +15551234567."),
            SettingField::new("api_base", "API Base URL", SettingKind::String)
                .default_value(json!(DEFAULT_API_BASE)),
        ],
        source: NotifierSource::BuiltIn,
    }
}

fn form(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{k}={}", utf8_percent_encode(v, NON_ALPHANUMERIC)))
        .collect::<Vec<_>>()
        .join("&")
}

impl Notifier for Twilio {
    fn descriptor(&self) -> &NotifierDescriptor {
        &self.descriptor
    }

    fn deliver(
        &self,
        _channel: &str,
        rendered: &Rendered,
        settings: &Resolved,
    ) -> Result<Option<String>, DeliveryError> {
        let sid = settings.str("account_sid").unwrap_or_default();
        if !sid.chars().all(|c| c.is_ascii_alphanumeric()) || sid.is_empty() {
            return Err(DeliveryError::permanent(
                "Twilio account SID must be letters and digits",
            ));
        }
        let base = settings
            .str("api_base")
            .unwrap_or(DEFAULT_API_BASE)
            .trim_end_matches('/');
        require_http_url(base, "Twilio API base URL")?;
        let token = settings.str("auth_token").unwrap_or_default();
        let auth = base64::engine::general_purpose::STANDARD.encode(format!("{sid}:{token}"));
        let from = settings.str("from").unwrap_or_default();
        let sender = if from.starts_with("MG") {
            "MessagingServiceSid"
        } else {
            "From"
        };
        let url = format!("{base}/2010-04-01/Accounts/{sid}/Messages.json");
        let mut sent = Vec::new();
        let mut failures = Vec::new();
        let mut retryable = true;
        for to in settings.list("to") {
            let request = HttpRequest {
                method: "POST".into(),
                url: url.clone(),
                headers: vec![
                    ("Authorization".into(), format!("Basic {auth}")),
                    (
                        "Content-Type".into(),
                        "application/x-www-form-urlencoded".into(),
                    ),
                ],
                body: form(&[("To", &to), (sender, from), ("Body", &rendered.body)]).into_bytes(),
            };
            let outcome = self
                .http
                .send(&request)
                .map_err(|e| DeliveryError::retryable(format!("could not reach Twilio: {e}")))
                .and_then(|response| {
                    classify(&response, "Twilio")?;
                    Ok(serde_json::from_str::<Value>(&response.body)
                        .ok()
                        .and_then(|v| v.get("sid").and_then(Value::as_str).map(str::to_owned)))
                });
            match outcome {
                Ok(id) => sent.push(id.unwrap_or_default()),
                Err(error) => {
                    retryable &= error.retryable;
                    failures.push(format!("{to}: {}", error.message));
                }
            }
        }
        if failures.is_empty() {
            return Ok(Some(sent.join(",")).filter(|ids| !ids.is_empty()));
        }
        let message = format!("Twilio did not deliver to {}", failures.join("; "));
        // A retry resends to every recipient, so retry only when nobody received it.
        Err(if retryable && sent.is_empty() {
            DeliveryError::retryable(message)
        } else {
            DeliveryError::permanent(message)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::HttpResponse;
    use crate::message::{EventKind, Message};
    use crate::render::render;
    use crate::testing::FakeHttp;

    fn settings(to: Value) -> Resolved {
        Resolved(
            serde_json::from_value(json!({
                "account_sid":"AC123","auth_token":"tok","from":"+15550000000","to":to,
                "api_base":"https://api.twilio.test/"
            }))
            .unwrap(),
        )
    }

    fn rendered() -> Rendered {
        let mut m = Message::new(
            EventKind::ProjectStopped,
            "Hot Sheet project demo has stopped",
        );
        m.body = "Completed: **HS2-1**.".into();
        render(&m, &descriptor().capabilities)
    }

    fn reply(status: u16, body: &str) -> Result<HttpResponse, String> {
        Ok(HttpResponse {
            status,
            body: body.into(),
        })
    }

    #[test]
    fn sends_one_form_encoded_message_per_recipient_with_basic_auth() {
        let http = Arc::new(FakeHttp::sequence(vec![
            reply(201, r#"{"sid":"SM1"}"#),
            reply(201, r#"{"sid":"SM2"}"#),
        ]));
        let id = Twilio::with_transport(http.clone())
            .deliver(
                "c",
                &rendered(),
                &settings(json!(["+15551112222", "+15553334444"])),
            )
            .unwrap();
        assert_eq!(id.as_deref(), Some("SM1,SM2"));
        let requests = http.requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests[0].url,
            "https://api.twilio.test/2010-04-01/Accounts/AC123/Messages.json"
        );
        assert!(
            requests[0]
                .headers
                .contains(&("Authorization".into(), "Basic QUMxMjM6dG9r".into()))
        );
        let body = String::from_utf8(requests[1].body.clone()).unwrap();
        assert!(
            body.starts_with("To=%2B15553334444&From=%2B15550000000&Body=Hot%20Sheet%20project"),
            "{body}"
        );
        assert!(
            body.ends_with("stopped%0A%0ACompleted%3A%20HS2%2D1%2E"),
            "title folded, Markdown stripped: {body}"
        );
    }

    #[test]
    fn partial_delivery_is_permanent_and_total_outage_is_retryable() {
        let http = Arc::new(FakeHttp::sequence(vec![
            reply(201, "{}"),
            reply(400, r#"{"code":21211}"#),
        ]));
        let err = Twilio::with_transport(http)
            .deliver("c", &rendered(), &settings(json!(["+1", "+2"])))
            .unwrap_err();
        assert!(
            !err.retryable && err.message.contains("+2: Twilio refused"),
            "{err}"
        );

        let http = Arc::new(FakeHttp::sequence(vec![
            reply(201, "{}"),
            reply(503, "down"),
        ]));
        let err = Twilio::with_transport(http)
            .deliver("c", &rendered(), &settings(json!(["+1", "+2"])))
            .unwrap_err();
        assert!(!err.retryable, "someone already got it, so no resend");

        let http = Arc::new(FakeHttp::replying(503, "down"));
        assert!(
            Twilio::with_transport(http)
                .deliver("c", &rendered(), &settings(json!(["+1", "+2"])))
                .unwrap_err()
                .retryable
        );
    }

    #[test]
    fn messaging_service_sender_and_bad_sid() {
        let http = Arc::new(FakeHttp::replying(201, "{}"));
        let mut s = settings(json!(["+1"]));
        s.0.insert("from".into(), json!("MG999"));
        Twilio::with_transport(http.clone())
            .deliver("c", &rendered(), &s)
            .unwrap();
        assert!(
            String::from_utf8(http.requests()[0].body.clone())
                .unwrap()
                .contains("MessagingServiceSid=MG999")
        );
        s.0.insert("account_sid".into(), json!("AC/../x"));
        assert!(
            !Twilio::with_transport(http)
                .deliver("c", &rendered(), &s)
                .unwrap_err()
                .retryable
        );
    }
}
