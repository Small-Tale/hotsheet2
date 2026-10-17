//! The `claude-inbox` session-push adapter (HS2-M15CHN): Claude Code 2.1.224+ binds a
//! per-session Unix socket for cross-session messaging and exports its path and a per-session
//! token to hooks as `CLAUDE_CODE_MESSAGING_SOCKET` and `CLAUDE_CODE_MESSAGING_TOKEN`
//! (code.claude.com/docs/en/cross-session-messaging). An idle session starts a turn with a
//! posted message; a busy one reads it between tool calls.
//!
//! The address is **reported, not launch-wired**: Hot Sheet's `SessionStart` hook reports the
//! socket, the token, and the Claude version, and the host registers this adapter for the
//! terminal then ([`super::adapter_for_reported`]).
//!
//! The newline-delimited JSON shape is undocumented. It was pinned live against `claude
//! 2.1.302`: a real interactive session in a Hot Sheet terminal started a turn with a message
//! posted by `hotsheet-server` and answered it (cassette
//! `tests/fixtures/claude-2.1.302-inbox-message.json`). It matches the binary's own
//! injection hint:
//!
//! ```text
//! {"type":"auth","token":"<CLAUDE_CODE_MESSAGING_TOKEN>"}
//! {"type":"user","message":{"role":"user","content":"<text>"},"from":"hotsheet","session_id":"<id>"}
//! ```
//!
//! `session_id` makes Claude drop a line meant for an earlier session. The socket writes
//! nothing back, so a post proves only that the socket accepted the line, not that the
//! session's inbound controls delivered it ([`PushReceipt::confirmed`] is `false`).

use serde_json::json;
use std::path::Path;
use std::time::Duration;

use super::{PushError, PushMessage, PushReceipt, ReportedSession, SessionPush};

/// The manifest mechanism name.
pub const MECHANISM: &str = "claude-inbox";
/// The reported address field naming the inbox socket (`CLAUDE_CODE_MESSAGING_SOCKET`).
pub const SOCKET_FIELD: &str = "messaging_socket";
/// The reported field carrying the per-session token (`CLAUDE_CODE_MESSAGING_TOKEN`).
pub const TOKEN_FIELD: &str = "messaging_token";
/// The reported field carrying the Claude Code version, for the version check.
pub const VERSION_FIELD: &str = "tool_version";
/// First Claude Code release with the inbox socket on macOS and Linux.
pub const MIN_INBOX_VERSION: (u32, u32, u32) = (2, 1, 224);
/// The release whose line shape this adapter was verified against live.
pub const VERIFIED_INBOX_VERSION: &str = "2.1.302";
/// The sender label Claude records for a Hot Sheet message.
pub const SENDER: &str = "hotsheet";
/// Keep a message far below Claude's ~1M-character same-machine cap.
pub const MAX_MESSAGE_CHARS: usize = 100_000;
/// How long one post may block on the socket.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

/// Pushes into one Claude Code session through its inbox socket.
#[derive(Clone)]
pub struct ClaudeInboxPush {
    socket: String,
    token: Option<String>,
    session_id: Option<String>,
    timeout: Duration,
}

impl std::fmt::Debug for ClaudeInboxPush {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClaudeInboxPush")
            .field("socket", &self.socket)
            .field("token", &self.token.as_ref().map(|_| "<redacted>"))
            .field("session_id", &self.session_id)
            .finish()
    }
}

impl ClaudeInboxPush {
    /// The adapter for a hook-reported Claude session. `None` without a socket, or when the
    /// reported Claude predates the inbox (a known version below [`MIN_INBOX_VERSION`]; an
    /// unknown version is accepted, because exporting a socket at all implies support).
    pub fn from_reported(reported: &ReportedSession) -> Option<Self> {
        let field = |name: &str| {
            reported
                .fields
                .get(name)
                .map(|value| value.trim())
                .filter(|value| !value.is_empty())
        };
        let socket = socket_path(field(SOCKET_FIELD)?).to_owned();
        if !version_supported(field(VERSION_FIELD)) {
            return None;
        }
        Some(Self {
            socket,
            token: field(TOKEN_FIELD).map(str::to_owned),
            session_id: reported.session_id.clone().filter(|id| !id.is_empty()),
            timeout: DEFAULT_TIMEOUT,
        })
    }

    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

impl SessionPush for ClaudeInboxPush {
    fn mechanism(&self) -> &'static str {
        MECHANISM
    }

    fn session_id(&self) -> Option<String> {
        self.session_id.clone()
    }

    fn message(&self, message: &PushMessage) -> Result<PushReceipt, PushError> {
        // Claude has no separate banner; the message itself is shown in the transcript.
        let PushMessage::Prompt { text, .. } = message else {
            return Err(PushError::Unsupported(
                "Claude sessions accept prompts, not UI commands".into(),
            ));
        };
        let frame = message_frame(self.token.as_deref(), self.session_id.as_deref(), text)?;
        post(&self.socket, &frame, self.timeout)?;
        Ok(PushReceipt {
            mechanism: MECHANISM,
            session_id: self.session_id.clone(),
            // The socket never answers; delivery or holding is the session's decision.
            confirmed: false,
        })
    }
}

/// The Claude Code version a hook runs under, from the environment Claude gives it:
/// `AI_AGENT` (`claude-code_2-1-302_agent` or `claude-code/2.1.302`) or, failing that, the
/// versioned install path in `CLAUDE_CODE_EXECPATH` (`…/versions/2.1.302`).
pub fn claude_version_from_env(ai_agent: Option<&str>, execpath: Option<&str>) -> Option<String> {
    let from_agent = ai_agent.and_then(|agent| {
        let rest = agent
            .strip_prefix("claude-code_")
            .or_else(|| agent.strip_prefix("claude-code/"))?;
        let version = rest.split('_').next()?.replace('-', ".");
        parse_version(&version).map(|_| version)
    });
    from_agent.or_else(|| {
        let name = Path::new(execpath?).file_name()?.to_str()?;
        parse_version(name).map(|_| name.to_owned())
    })
}

fn parse_version(version: &str) -> Option<(u32, u32, u32)> {
    let mut parts = version.trim().split('.');
    let triple = (
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    );
    parts.next().is_none().then_some(triple)
}

/// Whether a session running `version` has an inbox: only a known version below
/// [`MIN_INBOX_VERSION`] is refused.
pub fn version_supported(version: Option<&str>) -> bool {
    !version
        .and_then(parse_version)
        .is_some_and(|triple| triple < MIN_INBOX_VERSION)
}

/// The socket path from `CLAUDE_CODE_MESSAGING_SOCKET`; `/status` prints the same address
/// with a `uds:` prefix, so accept either.
pub fn socket_path(env_value: &str) -> &str {
    env_value.strip_prefix("uds:").unwrap_or(env_value).trim()
}

/// The bytes posted for one message: the auth line (when the session exported a token)
/// then the `user` line.
pub fn message_frame(
    token: Option<&str>,
    session_id: Option<&str>,
    text: &str,
) -> Result<String, PushError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(PushError::Invalid("the message is empty".into()));
    }
    let chars = text.chars().count();
    if chars > MAX_MESSAGE_CHARS {
        return Err(PushError::Invalid(format!(
            "the message is {chars} characters; the limit is {MAX_MESSAGE_CHARS}"
        )));
    }
    let mut frame = String::new();
    if let Some(token) = token.filter(|token| !token.is_empty()) {
        frame.push_str(&json!({ "type": "auth", "token": token }).to_string());
        frame.push('\n');
    }
    let mut user = json!({
        "type": "user",
        "message": { "role": "user", "content": text },
        "from": SENDER,
    });
    if let Some(id) = session_id.filter(|id| !id.is_empty()) {
        user["session_id"] = json!(id);
    }
    frame.push_str(&user.to_string());
    frame.push('\n');
    Ok(frame)
}

/// Write one framed message to the inbox at `socket`, bounded by `timeout`.
#[cfg(unix)]
pub fn post(socket: &str, frame: &str, timeout: Duration) -> Result<(), PushError> {
    use std::io::Write;
    use std::os::unix::net::UnixStream;
    let unreachable = |error: std::io::Error| PushError::Unreachable(error.to_string());
    let mut stream = UnixStream::connect(socket_path(socket)).map_err(unreachable)?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(unreachable)?;
    stream.write_all(frame.as_bytes()).map_err(unreachable)?;
    stream.flush().map_err(unreachable)?;
    // Half-close so Claude parses the final line and the connection ends cleanly.
    let _ = stream.shutdown(std::net::Shutdown::Write);
    Ok(())
}

/// Native Windows uses a named pipe that Hot Sheet does not drive yet.
#[cfg(not(unix))]
pub fn post(_socket: &str, _frame: &str, _timeout: Duration) -> Result<(), PushError> {
    Err(PushError::Unsupported(
        "Claude session messaging is only supported on macOS and Linux".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn reported(fields: &[(&str, &str)], session: Option<&str>) -> ReportedSession {
        ReportedSession {
            session_id: session.map(str::to_owned),
            fields: fields
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
        }
    }

    #[test]
    fn version_comes_from_ai_agent_then_execpath() {
        assert_eq!(
            claude_version_from_env(Some("claude-code_2-1-302_agent"), None).as_deref(),
            Some("2.1.302")
        );
        assert_eq!(
            claude_version_from_env(Some("claude-code/2.1.230"), None).as_deref(),
            Some("2.1.230")
        );
        assert_eq!(
            claude_version_from_env(
                Some("cursor-agent"),
                Some("/home/u/.local/share/claude/versions/2.1.300")
            )
            .as_deref(),
            Some("2.1.300")
        );
        assert_eq!(
            claude_version_from_env(Some("claude-code_garbage"), Some("/usr/bin/claude")),
            None
        );
        assert_eq!(claude_version_from_env(None, None), None);
    }

    #[test]
    fn version_gate_refuses_only_known_old_releases() {
        assert!(version_supported(Some("2.1.302")));
        assert!(version_supported(Some("2.1.224")));
        assert!(version_supported(Some("3.0.0")));
        assert!(version_supported(None));
        assert!(version_supported(Some("not-a-version")));
        assert!(!version_supported(Some("2.1.223")));
    }

    #[test]
    fn an_adapter_needs_a_socket_and_a_supported_version() {
        let ok = ClaudeInboxPush::from_reported(&reported(
            &[
                (SOCKET_FIELD, "uds:/tmp/cc-socks/1.sock"),
                (TOKEN_FIELD, "t"),
                (VERSION_FIELD, "2.1.302"),
            ],
            Some("s-1"),
        ))
        .unwrap();
        assert_eq!(ok.socket, "/tmp/cc-socks/1.sock");
        assert_eq!(ok.session_id().as_deref(), Some("s-1"));
        assert_eq!(ok.mechanism(), MECHANISM);
        assert!(!format!("{ok:?}").contains("\"t\""), "token is redacted");
        assert!(ClaudeInboxPush::from_reported(&reported(&[(TOKEN_FIELD, "t")], None)).is_none());
        assert!(
            ClaudeInboxPush::from_reported(&reported(
                &[(SOCKET_FIELD, "/s.sock"), (VERSION_FIELD, "2.1.200")],
                None
            ))
            .is_none()
        );
        assert!(ClaudeInboxPush::from_reported(&reported(&[(SOCKET_FIELD, "  ")], None)).is_none());
    }

    #[test]
    fn frame_matches_the_verified_2_1_302_shape() {
        let frame = message_frame(Some("tok"), Some("sess-1"), "  hello there \n").unwrap();
        let lines: Vec<Value> = frame
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert!(frame.ends_with('\n'));
        assert_eq!(lines[0], json!({ "type": "auth", "token": "tok" }));
        assert_eq!(
            lines[1],
            json!({
                "type": "user",
                "message": { "role": "user", "content": "hello there" },
                "from": "hotsheet",
                "session_id": "sess-1",
            })
        );
    }

    #[test]
    fn frame_omits_missing_auth_and_session_and_rejects_bad_text() {
        let frame = message_frame(Some(""), None, "multi\nline").unwrap();
        assert_eq!(frame.lines().count(), 1, "newlines in content stay escaped");
        let user: Value = serde_json::from_str(frame.trim_end()).unwrap();
        assert_eq!(user["message"]["content"], "multi\nline");
        assert!(user.get("session_id").is_none());
        assert!(matches!(
            message_frame(None, None, " \n "),
            Err(PushError::Invalid(_))
        ));
        let big = "x".repeat(MAX_MESSAGE_CHARS + 1);
        assert!(matches!(
            message_frame(None, None, &big),
            Err(PushError::Invalid(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn message_posts_the_frame_and_reports_commands_and_dead_sockets() {
        use std::io::Read;
        use std::os::unix::net::UnixListener;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("inbox.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let reader = std::thread::spawn(move || {
            let (mut conn, _) = listener.accept().unwrap();
            let mut got = String::new();
            conn.read_to_string(&mut got).unwrap();
            got
        });
        let push = ClaudeInboxPush::from_reported(&reported(
            &[(SOCKET_FIELD, path.to_str().unwrap()), (TOKEN_FIELD, "t")],
            Some("s-1"),
        ))
        .unwrap();
        let receipt = push
            .message(&PushMessage::Prompt {
                text: "hi".into(),
                banner: Some("ignored".into()),
            })
            .unwrap();
        assert_eq!(
            receipt,
            PushReceipt {
                mechanism: MECHANISM,
                session_id: Some("s-1".into()),
                confirmed: false,
            }
        );
        assert_eq!(
            reader.join().unwrap(),
            message_frame(Some("t"), Some("s-1"), "hi").unwrap()
        );
        assert!(matches!(
            push.message(&PushMessage::Command {
                command: "x".into()
            }),
            Err(PushError::Unsupported(_))
        ));
        std::fs::remove_file(&path).unwrap();
        assert!(matches!(
            push.message(&PushMessage::Prompt {
                text: "again".into(),
                banner: None
            }),
            Err(PushError::Unreachable(_))
        ));
    }
}
