//! The `$/hello` exchange (docs/38 §38.5). It is always JSON: the host sends one
//! JSON-RPC request with id 0, the plugin answers with one response line, and both sides
//! then switch to the transport the plugin chose.

use std::io::{self, BufRead, Write};

use crate::wire::{Message, MessageReader, Payload, Transport, write_message};
use crate::{HELLO_METHOD, PROTOCOL_VERSION, PluginError, v1};

/// Why a handshake did not complete.
#[derive(Debug)]
pub enum HandshakeError {
    /// Reading or writing failed.
    Io(io::Error),
    /// The peer closed its side before answering.
    Closed,
    /// The peer sent something other than the expected handshake message.
    Unexpected(String),
    /// The plugin answered the hello with an error.
    Refused(PluginError),
    /// The two sides share no protocol version or transport, or the plugin's
    /// capabilities do not match its manifest.
    Incompatible(String),
}

impl std::fmt::Display for HandshakeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HandshakeError::Io(e) => write!(f, "handshake I/O failed: {e}"),
            HandshakeError::Closed => {
                write!(f, "the plugin closed its output before the handshake")
            }
            HandshakeError::Unexpected(what) => write!(f, "unexpected handshake message: {what}"),
            HandshakeError::Refused(e) => write!(f, "the plugin refused the handshake: {e}"),
            HandshakeError::Incompatible(why) => write!(f, "incompatible plugin: {why}"),
        }
    }
}

impl std::error::Error for HandshakeError {}

impl From<io::Error> for HandshakeError {
    fn from(e: io::Error) -> Self {
        HandshakeError::Io(e)
    }
}

/// Host side: sends `$/hello` and flushes.
pub fn write_hello(out: &mut impl Write, hello: &v1::Hello) -> io::Result<()> {
    write_message(
        out,
        Transport::Json,
        &Message::Request {
            id: 0,
            method: HELLO_METHOD.to_string(),
            payload: Payload::encode(Transport::Json, hello),
            deadline_ms: 0,
        },
    )?;
    out.flush()
}

/// Host side: reads the plugin's answer. Log lines before it are returned too, so the
/// host can record them.
pub fn read_hello_ack<R: BufRead>(
    reader: &mut MessageReader<R>,
    logs: &mut Vec<(String, String)>,
) -> Result<v1::HelloAck, HandshakeError> {
    loop {
        match reader.read()? {
            None => return Err(HandshakeError::Closed),
            Some(Message::Log { level, message }) => logs.push((level, message)),
            Some(Message::Response { id: 0, outcome }) => {
                return match outcome {
                    Ok(payload) => payload
                        .decode()
                        .map_err(|e| HandshakeError::Unexpected(e.message)),
                    Err(error) => Err(HandshakeError::Refused(error)),
                };
            }
            Some(other) => return Err(HandshakeError::Unexpected(describe(&other))),
        }
    }
}

/// Host side: checks an answer against what the host offered and what the plugin's
/// manifest declared, and returns the transport to switch to.
pub fn accept_ack(
    hello: &v1::Hello,
    ack: &v1::HelloAck,
    declared_capabilities: &[String],
) -> Result<Transport, HandshakeError> {
    if !hello.protocol_versions.contains(&ack.protocol_version) {
        return Err(HandshakeError::Incompatible(format!(
            "the plugin chose protocol version {}, the host offered {:?}",
            ack.protocol_version, hello.protocol_versions
        )));
    }
    let transport = Transport::parse(&ack.transport)
        .filter(|t| hello.transports.iter().any(|offered| offered == t.name()))
        .ok_or_else(|| {
            HandshakeError::Incompatible(format!(
                "the plugin chose transport {:?}, the host offered {:?}",
                ack.transport, hello.transports
            ))
        })?;
    let mut answered: Vec<&str> = ack.capabilities.iter().map(|c| c.name.as_str()).collect();
    let mut declared: Vec<&str> = declared_capabilities.iter().map(String::as_str).collect();
    answered.sort_unstable();
    answered.dedup();
    declared.sort_unstable();
    declared.dedup();
    if answered != declared {
        return Err(HandshakeError::Incompatible(format!(
            "the plugin reported capabilities {answered:?}, its manifest declares {declared:?}"
        )));
    }
    Ok(transport)
}

/// Plugin side: reads `$/hello`.
pub fn read_hello<R: BufRead>(reader: &mut MessageReader<R>) -> Result<v1::Hello, HandshakeError> {
    match reader.read()? {
        None => Err(HandshakeError::Closed),
        Some(Message::Request {
            id: 0,
            method,
            payload,
            ..
        }) if method == HELLO_METHOD => payload
            .decode()
            .map_err(|e| HandshakeError::Unexpected(e.message)),
        Some(other) => Err(HandshakeError::Unexpected(describe(&other))),
    }
}

/// Plugin side: the first transport in the host's preference order that the plugin
/// supports, when the host also speaks this crate's protocol version.
pub fn choose(hello: &v1::Hello, supported: &[Transport]) -> Result<Transport, PluginError> {
    if !hello.protocol_versions.contains(&PROTOCOL_VERSION) {
        return Err(PluginError::unsupported(format!(
            "protocol version {PROTOCOL_VERSION} (host offered {:?})",
            hello.protocol_versions
        )));
    }
    hello
        .transports
        .iter()
        .filter_map(|name| Transport::parse(name))
        .find(|t| supported.contains(t))
        .ok_or_else(|| {
            PluginError::unsupported(format!("any of the transports {:?}", hello.transports))
        })
}

/// Plugin side: answers the hello with `outcome` and flushes.
pub fn write_hello_ack(
    out: &mut impl Write,
    outcome: Result<&v1::HelloAck, &PluginError>,
) -> io::Result<()> {
    write_message(
        out,
        Transport::Json,
        &Message::Response {
            id: 0,
            outcome: match outcome {
                Ok(ack) => Ok(Payload::encode(Transport::Json, ack)),
                Err(error) => Err(error.clone()),
            },
        },
    )?;
    out.flush()
}

fn describe(message: &Message) -> String {
    match message {
        Message::Request { id, method, .. } => format!("request {id} {method}"),
        Message::Response { id, .. } => format!("response {id}"),
        Message::StreamItem { id, .. } => format!("stream item for {id}"),
        Message::Cancel { id } => format!("cancel {id}"),
        Message::Log { .. } => "log".to_string(),
    }
}
