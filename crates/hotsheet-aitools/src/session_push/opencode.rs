//! The `opencode-http` session-push adapter (HS2-PEJ962). OpenCode's TUI is a client of its
//! own HTTP server; launched with `--port <p>` and `OPENCODE_SERVER_PASSWORD`, that server
//! listens on `127.0.0.1:<p>` behind HTTP Basic auth (user `opencode`).
//!
//! Protocol pinned against opencode 1.17.18 (`tests/fixtures/opencode-1.17.18-session-push.json`):
//!
//! - The TUI creates no session until the first prompt. A session the user starts appears in
//!   `GET /session?directory=<cwd>&roots=true` (newest first); `POST /session?directory=…`
//!   creates one and `POST /tui/select-session` makes the TUI display it.
//! - `POST /session/:id/prompt_async` → `204`; the prompt renders in the attached TUI and a
//!   turn starts. Without `model` it runs on the *config default* model, not the TUI's
//!   `--model`, so the adapter passes the launch model when one was chosen.
//! - `POST /tui/show-toast` shows a banner; `POST /tui/execute-command` runs a TUI command and
//!   answers `true` even for an unknown command, so commands are never "confirmed".
//! - An unknown session id answers `404` (`NotFoundError`); a wrong password answers `401`.

use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use super::http::{HttpPort, HttpRequest, HttpResponse, basic_auth_header, query_escape};
use super::{PushError, PushLaunch, PushMessage, PushReceipt, SessionPush};

/// The manifest mechanism name.
pub const MECHANISM: &str = "opencode-http";

/// OpenCode's fixed Basic-auth user name for `OPENCODE_SERVER_PASSWORD`.
const USER: &str = "opencode";

/// A session created up to this long before the launch time is still this launch's (the
/// launch time is taken before the TUI starts, so this only absorbs clock granularity).
const CREATED_SLACK_MS: u64 = 2_000;

/// The title given to a session Hot Sheet has to create because the user has not started one.
const CREATED_SESSION_TITLE: &str = "Hot Sheet";

/// Push adapter for one launched OpenCode TUI.
pub struct OpenCodeHttpPush {
    base_url: String,
    authorization: String,
    directory: String,
    model: Option<(String, String)>,
    launched_at_ms: u64,
    session: Mutex<Option<String>>,
    http: Arc<dyn HttpPort>,
}

impl OpenCodeHttpPush {
    pub fn new(launch: PushLaunch, http: Arc<dyn HttpPort>) -> Self {
        let model = launch.model.as_deref().and_then(|model| {
            model
                .split_once('/')
                .filter(|(provider, id)| !provider.is_empty() && !id.is_empty())
                .map(|(provider, id)| (provider.to_string(), id.to_string()))
        });
        Self {
            base_url: format!("http://127.0.0.1:{}", launch.port),
            authorization: basic_auth_header(USER, &launch.password),
            directory: launch.directory,
            model,
            launched_at_ms: launch.launched_at_ms,
            session: Mutex::new(None),
            http,
        }
    }

    /// Restore the session chosen before a server restart.
    pub fn restored(
        launch: PushLaunch,
        session_id: Option<String>,
        http: Arc<dyn HttpPort>,
    ) -> Self {
        let adapter = Self::new(launch, http);
        adapter.remember_session(session_id);
        adapter
    }

    fn url(&self, path: &str) -> String {
        format!(
            "{}{path}{}directory={}",
            self.base_url,
            if path.contains('?') { '&' } else { '?' },
            query_escape(&self.directory)
        )
    }

    fn send(
        &self,
        method: &'static str,
        path: &str,
        body: Option<Value>,
    ) -> Result<HttpResponse, PushError> {
        self.http
            .send(&HttpRequest {
                method,
                url: self.url(path),
                authorization: Some(self.authorization.clone()),
                body,
            })
            .map_err(PushError::Unreachable)
    }

    fn expect_success(response: HttpResponse) -> Result<HttpResponse, PushError> {
        if response.is_success() {
            Ok(response)
        } else {
            Err(PushError::Rejected {
                status: response.status,
                body: response.body,
            })
        }
    }

    fn cached_session(&self) -> Option<String> {
        self.session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn remember_session(&self, session: Option<String>) {
        *self
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = session;
    }

    /// The session this TUI is working in: the cached one, else the newest root session in
    /// the launch directory created since the launch, else a new one the TUI is switched to.
    fn resolve_session(&self) -> Result<String, PushError> {
        if let Some(session) = self.cached_session() {
            return Ok(session);
        }
        let listed = Self::expect_success(self.send("GET", "/session?roots=true", None)?)?;
        let sessions: Value =
            serde_json::from_str(&listed.body).map_err(|error| PushError::Rejected {
                status: listed.status,
                body: format!("unreadable session list: {error}"),
            })?;
        let newest = sessions
            .as_array()
            .into_iter()
            .flatten()
            .filter(|session| session.get("parentID").is_none_or(Value::is_null))
            .filter(|session| {
                session["directory"].as_str() == Some(self.directory.as_str())
                    && session["time"]["created"]
                        .as_u64()
                        .is_some_and(|created| created + CREATED_SLACK_MS >= self.launched_at_ms)
            })
            .max_by_key(|session| session["time"]["updated"].as_u64().unwrap_or(0))
            .and_then(|session| session["id"].as_str().map(str::to_string));
        let session = match newest {
            Some(session) => session,
            None => {
                let created = Self::expect_success(self.send(
                    "POST",
                    "/session",
                    Some(json!({ "title": CREATED_SESSION_TITLE })),
                )?)?;
                let id = serde_json::from_str::<Value>(&created.body)
                    .ok()
                    .and_then(|session| session["id"].as_str().map(str::to_string))
                    .ok_or_else(|| PushError::Rejected {
                        status: created.status,
                        body: format!("created session has no id: {}", created.body),
                    })?;
                // Show the new session so the prompt is visible; delivery does not depend on it.
                let _ = self.send(
                    "POST",
                    "/tui/select-session",
                    Some(json!({ "sessionID": id })),
                );
                id
            }
        };
        self.remember_session(Some(session.clone()));
        Ok(session)
    }

    fn prompt_body(&self, text: &str) -> Value {
        let mut body = json!({ "parts": [{ "type": "text", "text": text }] });
        if let Some((provider, model)) = &self.model {
            body["model"] = json!({ "providerID": provider, "modelID": model });
        }
        body
    }

    fn prompt(&self, session: &str, text: &str) -> Result<HttpResponse, PushError> {
        self.send(
            "POST",
            &format!("/session/{}/prompt_async", query_escape(session)),
            Some(self.prompt_body(text)),
        )
    }
}

impl SessionPush for OpenCodeHttpPush {
    fn mechanism(&self) -> &'static str {
        MECHANISM
    }

    fn session_id(&self) -> Option<String> {
        self.cached_session()
    }

    fn message(&self, message: &PushMessage) -> Result<PushReceipt, PushError> {
        match message {
            PushMessage::Command { command } => {
                let command = command.trim();
                if command.is_empty() {
                    return Err(PushError::Invalid("command must not be empty".into()));
                }
                Self::expect_success(self.send(
                    "POST",
                    "/tui/execute-command",
                    Some(json!({ "command": command })),
                )?)?;
                Ok(PushReceipt {
                    mechanism: MECHANISM,
                    session_id: self.cached_session(),
                    confirmed: false,
                })
            }
            PushMessage::Prompt { text, banner } => {
                if text.trim().is_empty() {
                    return Err(PushError::Invalid("message text must not be empty".into()));
                }
                let mut session = self.resolve_session()?;
                if let Some(banner) = banner.as_deref().filter(|banner| !banner.trim().is_empty()) {
                    // The banner is a courtesy; a failed toast must not block the prompt.
                    let _ = self.send(
                        "POST",
                        "/tui/show-toast",
                        Some(json!({ "title": "Hot Sheet", "message": banner, "variant": "info" })),
                    );
                }
                let mut response = self.prompt(&session, text)?;
                if response.status == 404 {
                    // The remembered session was deleted: find (or create) the current one once.
                    self.remember_session(None);
                    session = self.resolve_session()?;
                    response = self.prompt(&session, text)?;
                }
                Self::expect_success(response)?;
                Ok(PushReceipt {
                    mechanism: MECHANISM,
                    session_id: Some(session),
                    confirmed: true,
                })
            }
        }
    }
}
