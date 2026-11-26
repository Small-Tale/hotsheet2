//! The Ably adapter (HS2-BWJWWX, `docs/37` §37.9): a server-owned, blocking Ably Realtime
//! connection over a JSON WebSocket that attaches the bound channels and turns a GitHub push
//! payload delivered through an Ably incoming webhook into a remote-change hint.
//!
//! The credential is an Ably API key kept in the OS credential store. It is sent once, in the
//! connect URL's `key` parameter over `wss`, and never printed: every error message has it
//! stripped. Give the key the narrowest capability, subscribe on exactly the bound channels.
//! The adapter reads only the repository, ref and tip of a push; the payload is never kept.

use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde_json::{Value, json};
use tungstenite::client::IntoClientRequest;
use tungstenite::protocol::WebSocketConfig;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Connector, HandshakeError, Message, WebSocket, client_tls_with_config};

use super::github::{Mapped, Skip, map_push, map_value};
use super::model::{
    AdapterDescriptor, AdapterError, CONTRACT_VERSION, ChangeAdapter, ConnectionContext, Event,
    SettingSpec,
};

pub const ADAPTER_ID: &str = "ably";
/// Ably's realtime endpoint. A connection's `endpoint` setting overrides it (a private or
/// regional endpoint, or a loopback server in development).
pub const DEFAULT_ENDPOINT: &str = "wss://main.realtime.ably.net";

// ProtocolMessage actions (Ably protocol v2, JSON).
const HEARTBEAT: u8 = 0;
const CONNECTED: u8 = 4;
const DISCONNECT_ED: u8 = 6;
const CLOSE: u8 = 7;
const CLOSED: u8 = 8;
const ERROR: u8 = 9;
const ATTACH: u8 = 10;
const ATTACHED: u8 = 11;
const DETACHED: u8 = 13;
const MESSAGE: u8 = 15;

const MAX_FRAME_BYTES: usize = 1024 * 1024;

#[derive(Debug, Deserialize)]
struct ErrorInfo {
    #[serde(default)]
    code: u32,
    #[serde(default, rename = "statusCode")]
    status_code: u32,
    #[serde(default)]
    message: String,
}

#[derive(Debug, Deserialize)]
struct WireMessage {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    data: Value,
}

#[derive(Debug, Deserialize)]
struct Protocol {
    action: u8,
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    messages: Vec<WireMessage>,
    #[serde(default)]
    error: Option<ErrorInfo>,
}

/// What the adapter skipped, by reason. Counts only: a payload is never kept.
#[derive(Debug, Default)]
pub struct Stats {
    pub hints: AtomicU64,
    pub pings: AtomicU64,
    pub not_push: AtomicU64,
    pub ref_deleted: AtomicU64,
    pub oversized: AtomicU64,
    pub malformed: AtomicU64,
    pub missing_field: AtomicU64,
}

impl Stats {
    fn skipped(&self, skip: Skip) {
        let counter = match skip {
            Skip::Ping => &self.pings,
            Skip::NotAPush => &self.not_push,
            Skip::RefDeleted => &self.ref_deleted,
            Skip::Oversized => &self.oversized,
            Skip::Malformed => &self.malformed,
            Skip::MissingField(_) => &self.missing_field,
        };
        counter.fetch_add(1, Ordering::SeqCst);
    }

    /// A plain snapshot, for diagnostics.
    pub fn snapshot(&self) -> [(&'static str, u64); 7] {
        let get = |c: &AtomicU64| c.load(Ordering::SeqCst);
        [
            ("hints", get(&self.hints)),
            ("pings", get(&self.pings)),
            ("not_push", get(&self.not_push)),
            ("ref_deleted", get(&self.ref_deleted)),
            ("oversized", get(&self.oversized)),
            ("malformed", get(&self.malformed)),
            ("missing_field", get(&self.missing_field)),
        ]
    }
}

/// How long the adapter waits at each stage. The defaults suit the real service; tests shorten
/// them.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    /// The read timeout that lets a connection notice a stop request. A local wake only.
    pub tick: Duration,
    /// How long to wait for the service to confirm the connection and the attachments.
    pub handshake: Duration,
    /// How long a connection may be silent before it is treated as dead. Ably sends a
    /// heartbeat every 15 s, so a minute of silence is a lost connection.
    pub silence: Duration,
}

impl Timing {
    pub const DEFAULT: Timing = Timing {
        tick: Duration::from_secs(1),
        handshake: Duration::from_secs(15),
        silence: Duration::from_secs(60),
    };
}

pub struct AblyAdapter {
    pub stats: Arc<Stats>,
    timing: Timing,
}

impl Default for AblyAdapter {
    fn default() -> Self {
        Self::new()
    }
}

type Socket = WebSocket<MaybeTlsStream<TcpStream>>;

fn now_ms() -> u64 {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis()),
    )
    .unwrap_or(0)
}

/// An endpoint: `wss://host[:port]`, or plain `ws://` to a loopback address for development.
/// No credentials, path, query or fragment: the key is added to the query here, never read from
/// a setting.
pub fn validate_endpoint(endpoint: &str) -> Result<(), String> {
    let loopback = ["ws://127.0.0.1", "ws://localhost", "ws://[::1]"]
        .iter()
        .any(|p| endpoint.starts_with(p));
    if !(endpoint.starts_with("wss://") || loopback) {
        return Err("the endpoint must be wss:// (plain ws:// only to a loopback address)".into());
    }
    let rest = endpoint.split_once("://").map_or("", |(_, r)| r);
    let bare = !rest.is_empty()
        && !rest.contains(['@', '?', '#', '/'])
        && !endpoint
            .chars()
            .any(|c| c.is_control() || c.is_whitespace());
    if bare {
        Ok(())
    } else {
        Err(
            "the endpoint is a bare host (and port), without credentials, path, query or fragment"
                .into(),
        )
    }
}

fn redact(message: &str, key: &str) -> String {
    let mut text = message.replace(key, "<redacted>");
    // The key's secret half alone is as sensitive as the whole.
    if let Some((_, secret)) = key.split_once(':') {
        text = text.replace(secret, "<redacted>");
    }
    text.chars().filter(|c| !c.is_control()).take(300).collect()
}

fn auth_status(info: &ErrorInfo) -> bool {
    matches!(info.status_code, 401 | 403)
        || (40100..40200).contains(&info.code)
        || (40300..40400).contains(&info.code)
}

impl AblyAdapter {
    pub fn new() -> Self {
        Self::with_timing(Timing::DEFAULT)
    }

    pub fn with_timing(timing: Timing) -> Self {
        Self {
            stats: Arc::new(Stats::default()),
            timing,
        }
    }

    /// The TLS configuration for `wss`, built with an explicit crypto provider.
    pub fn tls_config() -> Result<Arc<rustls::ClientConfig>, AdapterError> {
        let mut roots = rustls::RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(|e| AdapterError::Config(format!("TLS setup: {e}")))?
        .with_root_certificates(roots)
        .with_no_client_auth();
        Ok(Arc::new(config))
    }

    /// Connect and finish the WebSocket handshake. A refused handshake is an auth failure.
    fn open(&self, context: &ConnectionContext, key: &str) -> Result<Socket, AdapterError> {
        let endpoint = context
            .settings
            .get("endpoint")
            .map_or(DEFAULT_ENDPOINT, String::as_str);
        validate_endpoint(endpoint).map_err(AdapterError::Config)?;
        if key.is_empty() || key.chars().any(|c| c.is_control() || c.is_whitespace()) {
            return Err(AdapterError::Config(
                "the stored credential is not an Ably API key".into(),
            ));
        }
        let encoded: String =
            percent_encoding::utf8_percent_encode(key, percent_encoding::NON_ALPHANUMERIC)
                .collect();
        let url = format!("{endpoint}/?key={encoded}&format=json&v=2&heartbeats=true");
        let request = url
            .as_str()
            .into_client_request()
            .map_err(|e| AdapterError::Config(redact(&e.to_string(), key)))?;
        let uri = request.uri().clone();
        let tls = uri.scheme_str() == Some("wss");
        let host = uri
            .host()
            .ok_or_else(|| AdapterError::Config("the endpoint has no host".into()))?
            .to_owned();
        let port = uri.port_u16().unwrap_or(if tls { 443 } else { 80 });
        let transient =
            |e: &dyn std::fmt::Display| AdapterError::Transient(redact(&e.to_string(), key));
        let addr = (host.as_str(), port)
            .to_socket_addrs()
            .map_err(|e| transient(&e))?
            .next()
            .ok_or_else(|| AdapterError::Transient(format!("{host} did not resolve")))?;
        let stream =
            TcpStream::connect_timeout(&addr, self.timing.handshake).map_err(|e| transient(&e))?;
        let _ = stream.set_read_timeout(Some(self.timing.handshake));
        let _ = stream.set_write_timeout(Some(self.timing.handshake));
        let _ = stream.set_nodelay(true);
        let connector = if tls {
            Connector::Rustls(Self::tls_config()?)
        } else {
            Connector::Plain
        };
        let config = WebSocketConfig::default()
            .max_message_size(Some(MAX_FRAME_BYTES))
            .max_frame_size(Some(MAX_FRAME_BYTES));
        match client_tls_with_config(request, stream, Some(config), Some(connector)) {
            Ok((mut socket, _)) => {
                set_tick(&mut socket, self.timing.tick);
                Ok(socket)
            }
            Err(HandshakeError::Failure(tungstenite::Error::Http(response)))
                if matches!(response.status().as_u16(), 401 | 403) =>
            {
                Err(AdapterError::Auth(format!(
                    "the service refused the connection (HTTP {})",
                    response.status().as_u16()
                )))
            }
            Err(HandshakeError::Failure(e)) => Err(transient(&e)),
            Err(HandshakeError::Interrupted(_)) => Err(AdapterError::Transient(
                "the handshake was interrupted".into(),
            )),
        }
    }

    /// Read until the next protocol message, a stop, or `deadline`.
    fn read(&self, socket: &mut Socket, key: &str) -> Result<Option<Protocol>, AdapterError> {
        match socket.read() {
            Ok(Message::Text(text)) => serde_json::from_str::<Protocol>(text.as_str())
                .map(Some)
                // A frame that is not a protocol message is skipped, never fatal.
                .or(Ok(None)),
            Ok(Message::Close(_)) => Err(AdapterError::Transient(
                "the service closed the connection".into(),
            )),
            Ok(_) => Ok(None),
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                Ok(None)
            }
            Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => {
                Err(AdapterError::Transient("the connection closed".into()))
            }
            Err(e) => Err(AdapterError::Transient(redact(&e.to_string(), key))),
        }
    }

    fn failure(info: Option<&ErrorInfo>, what: &str, key: &str) -> AdapterError {
        match info {
            Some(info) if auth_status(info) => AdapterError::Auth(format!(
                "{what}: {} (code {}, HTTP {})",
                redact(&info.message, key),
                info.code,
                info.status_code
            )),
            Some(info) => AdapterError::Transient(format!(
                "{what}: {} (code {})",
                redact(&info.message, key),
                info.code
            )),
            None => AdapterError::Transient(what.to_owned()),
        }
    }

    /// Wait for the service to confirm the connection.
    fn await_connected(
        &self,
        socket: &mut Socket,
        key: &str,
        stop: &AtomicBool,
    ) -> Result<(), AdapterError> {
        let deadline = Instant::now() + self.timing.handshake;
        loop {
            if stop.load(Ordering::SeqCst) {
                return Ok(());
            }
            match self.read(socket, key)? {
                Some(p) if p.action == CONNECTED => return Ok(()),
                Some(p) if matches!(p.action, ERROR | DISCONNECT_ED | CLOSED) => {
                    return Err(Self::failure(
                        p.error.as_ref(),
                        "the service refused the connection",
                        key,
                    ));
                }
                _ => {}
            }
            if Instant::now() >= deadline {
                return Err(AdapterError::Transient(
                    "the service did not confirm the connection in time".into(),
                ));
            }
        }
    }

    fn send(&self, socket: &mut Socket, value: &Value, key: &str) -> Result<(), AdapterError> {
        socket
            .send(Message::text(value.to_string()))
            .map_err(|e| AdapterError::Transient(redact(&e.to_string(), key)))
    }

    /// Turn the messages of one MESSAGE frame into hints.
    fn deliver(&self, frame: &Protocol, emit: &mut dyn FnMut(Event)) {
        let channel = frame.channel.clone().unwrap_or_default();
        for (index, message) in frame.messages.iter().enumerate() {
            let id = message
                .id
                .clone()
                .or_else(|| frame.id.as_ref().map(|id| format!("{id}:{index}")));
            let mapped = match &message.data {
                Value::String(text) => map_push(text, id.as_deref(), now_ms()),
                Value::Object(_) => {
                    if message.data.to_string().len() > super::github::MAX_PAYLOAD_BYTES {
                        Mapped::Skipped(Skip::Oversized)
                    } else {
                        map_value(&message.data, id.as_deref(), now_ms())
                    }
                }
                _ => Mapped::Skipped(Skip::Malformed),
            };
            match mapped {
                Mapped::Hint(hint) => {
                    self.stats.hints.fetch_add(1, Ordering::SeqCst);
                    emit(Event::Hint {
                        channel: channel.clone(),
                        hint,
                    });
                }
                Mapped::Skipped(skip) => self.stats.skipped(skip),
            }
        }
    }
}

fn set_tick(socket: &mut Socket, tick: Duration) {
    match socket.get_mut() {
        MaybeTlsStream::Plain(stream) => {
            let _ = stream.set_read_timeout(Some(tick));
        }
        MaybeTlsStream::Rustls(stream) => {
            let _ = stream.sock.set_read_timeout(Some(tick));
        }
        _ => {}
    }
}

impl ChangeAdapter for AblyAdapter {
    fn descriptor(&self) -> AdapterDescriptor {
        AdapterDescriptor {
            id: ADAPTER_ID,
            display_name: "Ably Realtime",
            contract: CONTRACT_VERSION,
            settings: vec![SettingSpec {
                key: "endpoint",
                label: "Realtime endpoint",
                required: false,
                help: "wss://host[:port]; default wss://main.realtime.ably.net",
            }],
            credential: true,
            credential_help: "an Ably API key with only the subscribe capability on the bound channels",
        }
    }

    fn run(
        &self,
        context: &ConnectionContext,
        channels: &[String],
        emit: &mut dyn FnMut(Event),
        stop: &AtomicBool,
    ) -> Result<(), AdapterError> {
        let key = context
            .credential
            .as_ref()
            .map(hotsheet_sync::secret::SecretString::expose)
            .ok_or_else(|| {
                AdapterError::Config("no credential is stored for this connection".into())
            })?;
        let mut socket = self.open(context, key)?;
        self.await_connected(&mut socket, key, stop)?;
        for channel in channels {
            self.send(
                &mut socket,
                &json!({ "action": ATTACH, "channel": channel }),
                key,
            )?;
        }
        let mut pending: Vec<&String> = channels.iter().collect();
        let deadline = Instant::now() + self.timing.handshake;
        let mut last_seen = Instant::now();
        let mut announced = pending.is_empty();
        if announced {
            emit(Event::Connected { resumed: false });
        }
        loop {
            if stop.load(Ordering::SeqCst) {
                let _ = self.send(&mut socket, &json!({ "action": CLOSE }), key);
                let _ = socket.close(None);
                return Ok(());
            }
            let Some(frame) = self.read(&mut socket, key)? else {
                if !announced && Instant::now() >= deadline {
                    return Err(AdapterError::Transient(
                        "the channels were not attached in time".into(),
                    ));
                }
                if last_seen.elapsed() > self.timing.silence {
                    return Err(AdapterError::Transient(format!(
                        "no traffic for {} s",
                        self.timing.silence.as_secs()
                    )));
                }
                continue;
            };
            last_seen = Instant::now();
            match frame.action {
                ATTACHED => {
                    pending.retain(|c| Some(c.as_str()) != frame.channel.as_deref());
                    if pending.is_empty() && !announced {
                        announced = true;
                        // Every connection is fresh, so a gap is possible: ask for a check.
                        emit(Event::Connected { resumed: false });
                    }
                }
                MESSAGE => self.deliver(&frame, emit),
                HEARTBEAT => {}
                ERROR | DETACHED | DISCONNECT_ED | CLOSED => {
                    let what = match (frame.action, &frame.channel) {
                        (DETACHED, Some(channel)) => format!("channel '{channel}' was detached"),
                        (ERROR, Some(channel)) => format!("channel '{channel}' failed"),
                        _ => "the service ended the connection".to_owned(),
                    };
                    return Err(Self::failure(frame.error.as_ref(), &what, key));
                }
                _ => {}
            }
        }
    }

    fn test(&self, context: &ConnectionContext) -> Result<(), AdapterError> {
        let key = context
            .credential
            .as_ref()
            .map(hotsheet_sync::secret::SecretString::expose)
            .ok_or_else(|| {
                AdapterError::Config("no credential is stored for this connection".into())
            })?;
        let mut socket = self.open(context, key)?;
        let never = AtomicBool::new(false);
        self.await_connected(&mut socket, key, &never)?;
        let _ = self.send(&mut socket, &json!({ "action": CLOSE }), key);
        let _ = socket.close(None);
        Ok(())
    }
}
