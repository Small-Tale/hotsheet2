//! The two transports' encodings (docs/38 §38.5).
//!
//! * **JSON**: one JSON-RPC 2.0 object per line. `params`/`result` are the canonical
//!   proto3 JSON of the contract message; stream items, cancels and logs are the
//!   notifications `$/stream`, `$/cancel` and `$/log`; errors use code `-32000` with the
//!   [`PluginError`] in `data`.
//! * **Protobuf**: a 4-byte little-endian length, then an encoded [`v1::Frame`].
//!
//! A [`MessageReader`] keeps its buffer across [`MessageReader::set_transport`], because
//! the peer may send its first protobuf frame right behind the JSON handshake line.

use std::io::{self, BufRead, Read, Write};

use prost::Message as _;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

use crate::PluginError;
use crate::v1;

/// The largest frame or JSON line either side accepts. Larger data is chunked.
pub const MAX_FRAME_BYTES: usize = 64 << 20;

/// The JSON-RPC error code carrying a [`PluginError`] in `data`.
pub const JSON_ERROR_CODE: i64 = -32000;

/// A transport the two sides can speak after the handshake.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Transport {
    /// JSON-RPC 2.0, newline-delimited.
    Json,
    /// Length-prefixed protobuf frames.
    Protobuf,
}

impl Transport {
    /// The name used in the handshake.
    pub const fn name(self) -> &'static str {
        match self {
            Transport::Json => "json",
            Transport::Protobuf => "protobuf",
        }
    }

    /// Parses a handshake transport name.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "json" => Some(Transport::Json),
            "protobuf" => Some(Transport::Protobuf),
            _ => None,
        }
    }
}

/// An encoded contract message in the shape its transport carries.
#[derive(Clone, Debug, PartialEq)]
pub enum Payload {
    /// Canonical proto3 JSON.
    Json(Value),
    /// Protobuf bytes.
    Proto(Vec<u8>),
}

impl Payload {
    /// Encodes `message` for `transport`.
    pub fn encode<T: prost::Message + Serialize>(transport: Transport, message: &T) -> Payload {
        match transport {
            // pbjson's generated impls only build string-keyed objects, which cannot fail.
            Transport::Json => Payload::Json(
                serde_json::to_value(message).unwrap_or_else(|_| Value::Object(Default::default())),
            ),
            Transport::Protobuf => Payload::Proto(message.encode_to_vec()),
        }
    }

    /// The empty message, for calls that return nothing.
    pub fn empty(transport: Transport) -> Payload {
        Payload::encode(transport, &v1::Empty {})
    }

    /// Decodes the payload as `T`; a malformed payload is `invalid_argument`.
    pub fn decode<T: prost::Message + Default + DeserializeOwned>(&self) -> Result<T, PluginError> {
        match self {
            Payload::Json(value) => serde_json::from_value(value.clone())
                .map_err(|e| PluginError::invalid_argument(format!("malformed JSON payload: {e}"))),
            Payload::Proto(bytes) => T::decode(bytes.as_slice()).map_err(|e| {
                PluginError::invalid_argument(format!("malformed protobuf payload: {e}"))
            }),
        }
    }
}

/// One protocol message, independent of transport.
#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    /// A call. `deadline_ms` 0 means no deadline.
    Request {
        id: u64,
        method: String,
        payload: Payload,
        deadline_ms: u64,
    },
    /// The end of a call.
    Response {
        id: u64,
        outcome: Result<Payload, PluginError>,
    },
    /// One item of a streaming call, before its response.
    StreamItem { id: u64, payload: Payload },
    /// Asks the peer to stop work on request `id`.
    Cancel { id: u64 },
    /// A log line from the plugin.
    Log { level: String, message: String },
}

/// Writes one message. The caller flushes.
pub fn write_message(
    out: &mut impl Write,
    transport: Transport,
    message: &Message,
) -> io::Result<()> {
    match transport {
        Transport::Json => {
            let value = json_value(message)?;
            let mut line = serde_json::to_vec(&value).map_err(io::Error::other)?;
            if line.len() > MAX_FRAME_BYTES {
                return Err(too_large(line.len()));
            }
            line.push(b'\n');
            out.write_all(&line)
        }
        Transport::Protobuf => {
            let bytes = frame(message)?.encode_to_vec();
            if bytes.len() > MAX_FRAME_BYTES {
                return Err(too_large(bytes.len()));
            }
            out.write_all(&(bytes.len() as u32).to_le_bytes())?;
            out.write_all(&bytes)
        }
    }
}

/// Reads messages in one transport, switchable after the handshake.
pub struct MessageReader<R> {
    inner: R,
    transport: Transport,
    line: Vec<u8>,
    buf: Vec<u8>,
}

impl<R: BufRead> MessageReader<R> {
    /// A reader that starts in `transport` (the handshake always starts in JSON).
    pub fn new(inner: R, transport: Transport) -> Self {
        MessageReader {
            inner,
            transport,
            line: Vec::new(),
            buf: Vec::new(),
        }
    }

    /// Switches the transport for every later message; buffered bytes are kept.
    pub fn set_transport(&mut self, transport: Transport) {
        self.transport = transport;
    }

    /// The current transport.
    pub fn transport(&self) -> Transport {
        self.transport
    }

    /// The next message, or `None` at a clean end of input. Unknown JSON notifications
    /// and unknown protobuf frame kinds are skipped, so a newer peer stays compatible.
    pub fn read(&mut self) -> io::Result<Option<Message>> {
        loop {
            let parsed = match self.transport {
                Transport::Json => {
                    let Some(value) = self.read_json_line()? else {
                        return Ok(None);
                    };
                    parse_json(value)?
                }
                Transport::Protobuf => {
                    let Some(frame) = self.read_frame()? else {
                        return Ok(None);
                    };
                    from_frame(frame)
                }
            };
            if let Some(message) = parsed {
                return Ok(Some(message));
            }
        }
    }

    fn read_json_line(&mut self) -> io::Result<Option<Value>> {
        loop {
            self.line.clear();
            let read = (&mut self.inner)
                .take(MAX_FRAME_BYTES as u64 + 1)
                .read_until(b'\n', &mut self.line)?;
            if read == 0 {
                return Ok(None);
            }
            if self.line.len() > MAX_FRAME_BYTES {
                return Err(too_large(self.line.len()));
            }
            let text = std::str::from_utf8(&self.line).map_err(invalid)?.trim();
            if text.is_empty() {
                continue;
            }
            return serde_json::from_str(text).map(Some).map_err(invalid);
        }
    }

    fn read_frame(&mut self) -> io::Result<Option<v1::Frame>> {
        let mut len = [0u8; 4];
        if !read_exact_or_eof(&mut self.inner, &mut len)? {
            return Ok(None);
        }
        let len = u32::from_le_bytes(len) as usize;
        if len > MAX_FRAME_BYTES {
            return Err(too_large(len));
        }
        self.buf.resize(len, 0);
        self.inner.read_exact(&mut self.buf)?;
        v1::Frame::decode(self.buf.as_slice())
            .map(Some)
            .map_err(invalid)
    }
}

/// Fills `buf`, or returns `false` when the input ends before its first byte.
fn read_exact_or_eof(r: &mut impl io::Read, buf: &mut [u8]) -> io::Result<bool> {
    let mut filled = 0;
    while filled < buf.len() {
        match r.read(&mut buf[filled..]) {
            Ok(0) if filled == 0 => return Ok(false),
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(n) => filled += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(true)
}

fn json_payload(payload: &Payload) -> io::Result<Value> {
    match payload {
        Payload::Json(value) => Ok(value.clone()),
        Payload::Proto(_) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a protobuf payload cannot be sent on the JSON transport",
        )),
    }
}

fn proto_payload(payload: &Payload) -> io::Result<Vec<u8>> {
    match payload {
        Payload::Proto(bytes) => Ok(bytes.clone()),
        Payload::Json(_) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a JSON payload cannot be sent on the protobuf transport",
        )),
    }
}

fn json_value(message: &Message) -> io::Result<Value> {
    Ok(match message {
        Message::Request {
            id,
            method,
            payload,
            deadline_ms,
        } => {
            let mut value = json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": method,
                "params": json_payload(payload)?,
            });
            if *deadline_ms > 0 {
                value["meta"] = json!({ "deadlineMs": deadline_ms });
            }
            value
        }
        Message::Response {
            id,
            outcome: Ok(payload),
        } => json!({ "jsonrpc": "2.0", "id": id, "result": json_payload(payload)? }),
        Message::Response {
            id,
            outcome: Err(error),
        } => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": JSON_ERROR_CODE,
                "message": error.message,
                "data": serde_json::to_value(error).map_err(io::Error::other)?,
            },
        }),
        Message::StreamItem { id, payload } => json!({
            "jsonrpc": "2.0",
            "method": "$/stream",
            "params": { "id": id, "item": json_payload(payload)? },
        }),
        Message::Cancel { id } => {
            json!({ "jsonrpc": "2.0", "method": "$/cancel", "params": { "id": id } })
        }
        Message::Log { level, message } => json!({
            "jsonrpc": "2.0",
            "method": "$/log",
            "params": { "level": level, "message": message },
        }),
    })
}

/// A JSON number, or a decimal string as the canonical mapping writes 64-bit integers.
fn json_u64(value: Option<&Value>) -> Option<u64> {
    match value? {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

fn parse_json(value: Value) -> io::Result<Option<Message>> {
    let Value::Object(object) = value else {
        return Err(invalid("a JSON-RPC message must be an object"));
    };
    let id = json_u64(object.get("id"));
    let method = object.get("method").and_then(Value::as_str);
    let params = object.get("params");
    Ok(match (id, method) {
        (Some(id), Some(method)) => Some(Message::Request {
            id,
            method: method.to_string(),
            payload: Payload::Json(params.cloned().unwrap_or_else(|| json!({}))),
            deadline_ms: json_u64(object.get("meta").and_then(|m| m.get("deadlineMs")))
                .unwrap_or(0),
        }),
        (None, Some("$/stream")) => {
            let params = params.ok_or_else(|| invalid("$/stream without params"))?;
            Some(Message::StreamItem {
                id: json_u64(params.get("id")).ok_or_else(|| invalid("$/stream without id"))?,
                payload: Payload::Json(params.get("item").cloned().unwrap_or_else(|| json!({}))),
            })
        }
        (None, Some("$/cancel")) => Some(Message::Cancel {
            id: json_u64(params.and_then(|p| p.get("id")))
                .ok_or_else(|| invalid("$/cancel without id"))?,
        }),
        (None, Some("$/log")) => {
            let text = |key| {
                params
                    .and_then(|p| p.get(key))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string()
            };
            Some(Message::Log {
                level: text("level"),
                message: text("message"),
            })
        }
        (None, Some(_)) => None,
        (Some(id), None) => {
            if let Some(result) = object.get("result") {
                Some(Message::Response {
                    id,
                    outcome: Ok(Payload::Json(result.clone())),
                })
            } else if let Some(error) = object.get("error") {
                Some(Message::Response {
                    id,
                    outcome: Err(json_error(error)),
                })
            } else {
                return Err(invalid("a JSON-RPC response needs result or error"));
            }
        }
        (None, None) => return Err(invalid("a JSON-RPC message needs a method or an id")),
    })
}

/// A [`PluginError`] from a JSON-RPC error object, also accepting plain JSON-RPC errors
/// from generic libraries.
fn json_error(error: &Value) -> PluginError {
    if let Some(data) = error.get("data")
        && let Ok(plugin_error) = serde_json::from_value::<PluginError>(data.clone())
        && !plugin_error.code.is_empty()
    {
        return plugin_error;
    }
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("error without a message")
        .to_string();
    match error.get("code").and_then(Value::as_i64) {
        Some(-32601) => PluginError::new(crate::code::UNSUPPORTED, message),
        Some(-32700 | -32600 | -32602) => PluginError::invalid_argument(message),
        _ => PluginError::internal(message),
    }
}

fn frame(message: &Message) -> io::Result<v1::Frame> {
    use v1::frame::Kind;
    let kind = match message {
        Message::Request {
            id,
            method,
            payload,
            deadline_ms,
        } => Kind::Request(v1::Request {
            id: *id,
            method: method.clone(),
            payload: proto_payload(payload)?,
            deadline_ms: *deadline_ms,
        }),
        Message::Response { id, outcome } => Kind::Response(v1::Response {
            id: *id,
            outcome: Some(match outcome {
                Ok(payload) => v1::response::Outcome::Payload(proto_payload(payload)?),
                Err(error) => v1::response::Outcome::Error(error.clone()),
            }),
        }),
        Message::StreamItem { id, payload } => Kind::StreamItem(v1::StreamItem {
            id: *id,
            payload: proto_payload(payload)?,
        }),
        Message::Cancel { id } => Kind::Cancel(v1::Cancel { id: *id }),
        Message::Log { level, message } => Kind::Log(v1::Log {
            level: level.clone(),
            message: message.clone(),
        }),
    };
    Ok(v1::Frame { kind: Some(kind) })
}

fn from_frame(frame: v1::Frame) -> Option<Message> {
    use v1::frame::Kind;
    Some(match frame.kind? {
        Kind::Request(r) => Message::Request {
            id: r.id,
            method: r.method,
            payload: Payload::Proto(r.payload),
            deadline_ms: r.deadline_ms,
        },
        Kind::Response(r) => Message::Response {
            id: r.id,
            outcome: match r.outcome {
                Some(v1::response::Outcome::Error(error)) => Err(error),
                Some(v1::response::Outcome::Payload(bytes)) => Ok(Payload::Proto(bytes)),
                None => Ok(Payload::Proto(Vec::new())),
            },
        },
        Kind::StreamItem(s) => Message::StreamItem {
            id: s.id,
            payload: Payload::Proto(s.payload),
        },
        Kind::Cancel(c) => Message::Cancel { id: c.id },
        Kind::Log(l) => Message::Log {
            level: l.level,
            message: l.message,
        },
    })
}

fn invalid(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

fn too_large(len: usize) -> io::Error {
    invalid(format!(
        "message of {len} bytes exceeds the {MAX_FRAME_BYTES}-byte limit"
    ))
}
