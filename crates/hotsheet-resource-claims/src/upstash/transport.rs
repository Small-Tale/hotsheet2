//! The Upstash Redis REST wire (HS2-K9J1RK): `POST <url>` with a JSON array command,
//! `POST <url>/pipeline` with an array of them, and `POST <url>/subscribe/<channel>` for a
//! server-sent-event stream. Replies are `{"result": …}` or `{"error": "…"}`; a missing or
//! bad token is 401, an unsupported command 400, a rate limit 429.
//!
//! Blocking list pops (`BLPOP`, `BRPOP`…) are not available over REST, so a wait cannot use
//! them; it holds one `subscribe` stream open instead.

use std::io::{BufRead, BufReader, ErrorKind};
use std::time::Duration;

use serde_json::Value;

/// Why one REST call failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RestError {
    /// The service could not be reached, or answered with a server error.
    #[error("{0}")]
    Unreachable(String),
    /// 401: the token is missing, wrong or read-only for this command.
    #[error("the service rejected the token (HTTP 401)")]
    Unauthorized,
    /// 429: back off, then retry.
    #[error("rate limited (HTTP 429)")]
    RateLimited { retry_after: Option<Duration> },
    /// Redis refused the command (a script error, a bad argument).
    #[error("{0}")]
    Command(String),
    /// The reply was not the documented shape.
    #[error("unexpected reply: {0}")]
    Protocol(String),
}

/// One thing a subscription stream delivered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The service confirmed the subscription: from here on no message is missed.
    Subscribed,
    Message(String),
    /// The stream's time ran out with nothing more to read.
    Timeout,
    /// The service closed the stream.
    Closed,
}

pub trait Subscription: Send {
    fn next(&mut self) -> Result<Event, RestError>;
}

/// The three calls the backend makes. Injected so a fake can stand in for the service.
pub trait RestTransport: Send + Sync {
    fn command(&self, args: &[Value]) -> Result<Value, RestError>;
    fn pipeline(&self, commands: &[Vec<Value>]) -> Result<Vec<Value>, RestError>;
    /// Open a stream that lasts at most `lifetime`.
    fn subscribe(
        &self,
        channel: &str,
        lifetime: Duration,
    ) -> Result<Box<dyn Subscription>, RestError>;
}

/// Hosts the transport may use without TLS: loopback only, for development and tests.
fn plain_http_allowed(url: &str) -> bool {
    ["http://127.0.0.1", "http://localhost", "http://[::1]"]
        .iter()
        .any(|prefix| url.starts_with(prefix))
}

/// A REST URL: `https://…`, or plain `http://` to a loopback address only. Never carries
/// credentials, a query or a fragment, since the token travels in a header.
pub fn validate_url(url: &str) -> Result<(), String> {
    let ok_scheme = url.starts_with("https://") || plain_http_allowed(url);
    if !ok_scheme {
        return Err("the REST URL must be https:// (plain http only to a loopback address)".into());
    }
    let after = url.split_once("://").map_or("", |(_, rest)| rest);
    if after.is_empty()
        || after.contains('@')
        || url.contains('?')
        || url.contains('#')
        || url.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return Err(
            "the REST URL must be a plain address without credentials, query or fragment".into(),
        );
    }
    Ok(())
}

/// The real transport over `ureq`.
pub struct UreqTransport {
    base: String,
    token: String,
    agent: ureq::Agent,
}

impl std::fmt::Debug for UreqTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UreqTransport")
            .field("base", &self.base)
            .finish_non_exhaustive()
    }
}

impl UreqTransport {
    pub fn new(url: &str, token: &str) -> Self {
        Self {
            base: url.trim_end_matches('/').to_owned(),
            token: token.to_owned(),
            agent: ureq::AgentBuilder::new()
                .timeout_connect(Duration::from_secs(10))
                .timeout(Duration::from_secs(30))
                .build(),
        }
    }

    fn post(&self, path: &str) -> ureq::Request {
        self.agent
            .post(&format!("{}{path}", self.base))
            .set("Authorization", &format!("Bearer {}", self.token))
    }

    fn map(error: ureq::Error) -> RestError {
        match error {
            ureq::Error::Status(401, _) => RestError::Unauthorized,
            ureq::Error::Status(429, response) => RestError::RateLimited {
                retry_after: response
                    .header("Retry-After")
                    .and_then(|v| v.trim().parse::<u64>().ok())
                    .map(Duration::from_secs),
            },
            ureq::Error::Status(400, response) => {
                let body: Value = response
                    .into_string()
                    .ok()
                    .and_then(|text| serde_json::from_str(&text).ok())
                    .unwrap_or(Value::Null);
                RestError::Command(
                    body.get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("the command was refused")
                        .to_owned(),
                )
            }
            ureq::Error::Status(code, _) => RestError::Unreachable(format!("HTTP {code}")),
            // The transport error names the host and the failure, never a header.
            ureq::Error::Transport(t) => RestError::Unreachable(t.to_string()),
        }
    }

    /// POST a JSON body and read the JSON reply.
    fn send(&self, path: &str, body: &Value) -> Result<Value, RestError> {
        let text = self
            .post(path)
            .set("Content-Type", "application/json")
            .send_string(&body.to_string())
            .map_err(Self::map)?
            .into_string()
            .map_err(|e| RestError::Unreachable(e.to_string()))?;
        serde_json::from_str(&text).map_err(|e| RestError::Protocol(e.to_string()))
    }

    fn result_of(value: &Value) -> Result<Value, RestError> {
        if let Some(error) = value.get("error").and_then(Value::as_str) {
            return Err(RestError::Command(error.to_owned()));
        }
        value
            .get("result")
            .cloned()
            .ok_or_else(|| RestError::Protocol("no result field".into()))
    }
}

impl RestTransport for UreqTransport {
    fn command(&self, args: &[Value]) -> Result<Value, RestError> {
        let body = self.send("", &Value::Array(args.to_vec()))?;
        Self::result_of(&body)
    }

    fn pipeline(&self, commands: &[Vec<Value>]) -> Result<Vec<Value>, RestError> {
        let body = self.send(
            "/pipeline",
            &Value::Array(commands.iter().cloned().map(Value::Array).collect()),
        )?;
        let Value::Array(items) = body else {
            return Err(RestError::Protocol(
                "a pipeline answers with an array".into(),
            ));
        };
        items.iter().map(Self::result_of).collect()
    }

    fn subscribe(
        &self,
        channel: &str,
        lifetime: Duration,
    ) -> Result<Box<dyn Subscription>, RestError> {
        // The overall timeout bounds connecting and reading, so it is the stream's lifetime:
        // when it passes mid-read the reader reports `Timeout`.
        let response = self
            .post(&format!("/subscribe/{channel}"))
            .set("Accept", "text/event-stream")
            .timeout(lifetime.max(Duration::from_millis(50)))
            .send_string("")
            .map_err(Self::map)?;
        Ok(Box::new(SseStream {
            reader: BufReader::new(response.into_reader()),
        }))
    }
}

struct SseStream {
    reader: BufReader<Box<dyn std::io::Read + Send + Sync + 'static>>,
}

/// One `data:` payload of the stream. `subscribe,<channel>,<count>` confirms the
/// subscription; `message,<channel>,<payload>` carries a published message.
pub fn parse_event(line: &str) -> Option<Event> {
    let data = line.trim_end().strip_prefix("data:")?.trim_start();
    let mut parts = data.splitn(3, ',');
    match (parts.next()?, parts.next(), parts.next()) {
        ("subscribe", Some(_), _) => Some(Event::Subscribed),
        ("message", Some(_), Some(payload)) => Some(Event::Message(payload.to_owned())),
        _ => None,
    }
}

impl Subscription for SseStream {
    fn next(&mut self) -> Result<Event, RestError> {
        let mut line = String::new();
        loop {
            line.clear();
            match self.reader.read_line(&mut line) {
                Ok(0) => return Ok(Event::Closed),
                Ok(_) => {
                    if let Some(event) = parse_event(&line) {
                        return Ok(event);
                    }
                }
                Err(e) if matches!(e.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock) => {
                    return Ok(Event::Timeout);
                }
                Err(e) => return Err(RestError::Unreachable(e.to_string())),
            }
        }
    }
}
