//! OpenCode v2 session push (HS2-W42KWS). The v2 TUI attaches to a private `serve` process;
//! Hot Sheet creates its session with a host-assigned id before the TUI starts. V2's public
//! API is `/api/session/{id}/prompt`, not v1's `/session/{id}/prompt_async` and `/tui` routes.

use std::sync::Arc;

use hotsheet_sync::secret::SecretString;
use serde_json::json;

use super::http::{HttpPort, HttpRequest, basic_auth_header, query_escape};
use super::{PushError, PushMessage, PushReceipt, SessionPush};

pub const MECHANISM: &str = "opencode-v2-http";

pub struct OpenCodeV2Push {
    base_url: String,
    authorization: SecretString,
    session_id: String,
    http: Arc<dyn HttpPort>,
}

impl OpenCodeV2Push {
    pub fn new(port: u16, password: &str, session_id: String, http: Arc<dyn HttpPort>) -> Self {
        Self {
            base_url: format!("http://127.0.0.1:{port}"),
            authorization: basic_auth_header("opencode", password).into(),
            session_id,
            http,
        }
    }
}

impl SessionPush for OpenCodeV2Push {
    fn mechanism(&self) -> &'static str {
        MECHANISM
    }

    fn session_id(&self) -> Option<String> {
        Some(self.session_id.clone())
    }

    fn message(&self, message: &PushMessage) -> Result<PushReceipt, PushError> {
        let PushMessage::Prompt { text, .. } = message else {
            // V2's public `session.command` runs agent commands, not TUI commands such as
            // `agent_cycle`. There is no public TUI-command route in the v2 OpenAPI contract.
            return Err(PushError::Unsupported(
                "OpenCode v2 does not expose TUI commands through its public API".into(),
            ));
        };
        if text.trim().is_empty() {
            return Err(PushError::Invalid("message text must not be empty".into()));
        }
        let response = self
            .http
            .send(&HttpRequest {
                method: "POST",
                url: format!(
                    "{}/api/session/{}/prompt",
                    self.base_url,
                    query_escape(&self.session_id)
                ),
                authorization: Some(self.authorization.clone()),
                body: Some(json!({ "text": text })),
            })
            .map_err(PushError::Unreachable)?;
        if !response.is_success() {
            return Err(PushError::Rejected {
                status: response.status,
                body: response.body,
            });
        }
        Ok(PushReceipt {
            mechanism: MECHANISM,
            session_id: Some(self.session_id.clone()),
            confirmed: true,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_push::http::HttpResponse;
    use std::sync::Mutex;

    #[derive(Default)]
    struct RecordingHttp(Mutex<Vec<HttpRequest>>);

    impl HttpPort for RecordingHttp {
        fn send(&self, request: &HttpRequest) -> Result<HttpResponse, String> {
            self.0.lock().unwrap().push(request.clone());
            Ok(HttpResponse {
                status: 200,
                body: r#"{"data":{"id":"msg_synthetic"}}"#.into(),
            })
        }
    }

    #[test]
    fn hs2_w42kws_pushes_to_the_host_assigned_v2_session() {
        let http = Arc::new(RecordingHttp::default());
        let push = OpenCodeV2Push::new(4096, "synthetic", "ses_synthetic".into(), http.clone());
        let receipt = push
            .message(&PushMessage::Prompt {
                text: "synthetic prompt".into(),
                banner: Some("notice".into()),
            })
            .unwrap();
        assert_eq!(receipt.mechanism, MECHANISM);
        assert_eq!(receipt.session_id.as_deref(), Some("ses_synthetic"));
        assert!(receipt.confirmed);
        let sent = http.0.lock().unwrap();
        assert_eq!(sent.len(), 1);
        assert_eq!(
            sent[0].url,
            "http://127.0.0.1:4096/api/session/ses_synthetic/prompt"
        );
        assert_eq!(sent[0].body, Some(json!({ "text": "synthetic prompt" })));
        assert!(
            sent[0]
                .authorization
                .as_ref()
                .unwrap()
                .expose()
                .starts_with("Basic ")
        );
    }

    #[test]
    fn hs2_w42kws_does_not_send_v1_tui_commands_to_v2() {
        let http = Arc::new(RecordingHttp::default());
        let push = OpenCodeV2Push::new(4096, "synthetic", "ses_synthetic".into(), http.clone());
        assert!(matches!(
            push.message(&PushMessage::Command {
                command: "agent_cycle".into(),
            }),
            Err(PushError::Unsupported(_))
        ));
        assert!(http.0.lock().unwrap().is_empty());
    }
}
