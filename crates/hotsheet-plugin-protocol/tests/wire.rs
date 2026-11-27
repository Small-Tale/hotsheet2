//! HS2-XY0VTN: both transports encode every message the same way and survive the
//! JSON-to-protobuf switch right after the handshake (docs/38 §38.5).

use std::io::{self, Cursor};

use hotsheet_plugin_protocol::wire::{MAX_FRAME_BYTES, MessageReader, write_message};
use hotsheet_plugin_protocol::{Message, Payload, PluginError, Transport, code, v1};
use serde_json::{Value, json};

fn messages(t: Transport) -> Vec<Message> {
    let echo = Payload::encode(
        t,
        &v1::EchoRequest {
            text: "hi".into(),
            data: vec![1, 2, 3],
        },
    );
    vec![
        Message::Request {
            id: 7,
            method: "diagnostics.Echo".into(),
            payload: echo.clone(),
            deadline_ms: 0,
        },
        Message::Request {
            id: u64::MAX,
            method: "diagnostics.Echo".into(),
            payload: echo.clone(),
            deadline_ms: 30_000,
        },
        Message::Response {
            id: 7,
            outcome: Ok(echo.clone()),
        },
        Message::Response {
            id: 8,
            outcome: Err(PluginError {
                code: code::RATE_LIMITED.into(),
                message: "slow down".into(),
                retry_after_seconds: Some(30),
                stale: false,
                capability: String::new(),
            }),
        },
        Message::Response {
            id: 9,
            outcome: Err(PluginError {
                code: code::CONFLICT.into(),
                message: "changed since read".into(),
                stale: true,
                ..Default::default()
            }),
        },
        Message::StreamItem {
            id: 7,
            payload: Payload::encode(t, &v1::CountItem { seq: 3 }),
        },
        Message::Cancel { id: 7 },
        Message::Log {
            level: "warn".into(),
            message: "careful".into(),
        },
    ]
}

fn encode_all(t: Transport, messages: &[Message]) -> Vec<u8> {
    let mut out = Vec::new();
    for message in messages {
        write_message(&mut out, t, message).expect("encodes");
    }
    out
}

fn decode_all(t: Transport, bytes: Vec<u8>) -> Vec<Message> {
    let mut reader = MessageReader::new(Cursor::new(bytes), t);
    let mut out = Vec::new();
    while let Some(message) = reader.read().expect("decodes") {
        out.push(message);
    }
    out
}

#[test]
fn hs2_xy0vtn_every_message_round_trips_on_both_transports() {
    for t in [Transport::Json, Transport::Protobuf] {
        let sent = messages(t);
        assert_eq!(decode_all(t, encode_all(t, &sent)), sent, "{t:?}");
    }
}

#[test]
fn hs2_xy0vtn_json_payloads_use_the_canonical_proto3_mapping() {
    let request = Message::Request {
        id: 1,
        method: "diagnostics.Count".into(),
        payload: Payload::encode(Transport::Json, &v1::CountRequest { count: 5 }),
        deadline_ms: 250,
    };
    let line = encode_all(Transport::Json, &[request]);
    let value: Value = serde_json::from_slice(&line).expect("one JSON line");
    // 64-bit integers are strings in the canonical mapping.
    assert_eq!(
        value,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "diagnostics.Count",
            "params": { "count": "5" },
            "meta": { "deadlineMs": 250 },
        })
    );
    // Bytes are base64, field names lowerCamelCase.
    let echo = Payload::encode(
        Transport::Json,
        &v1::EchoRequest {
            text: String::new(),
            data: vec![1, 2, 3],
        },
    );
    assert_eq!(echo, Payload::Json(json!({ "data": "AQID" })));
    let error = Payload::encode(
        Transport::Json,
        &PluginError {
            code: code::RATE_LIMITED.into(),
            retry_after_seconds: Some(9),
            ..Default::default()
        },
    );
    assert_eq!(
        error,
        Payload::Json(json!({ "code": "rate_limited", "retryAfterSeconds": "9" }))
    );
    // Plugins written against other JSON libraries may send numbers and snake_case.
    let lenient = Payload::Json(json!({ "count": 5 }));
    assert_eq!(
        lenient.decode::<v1::CountRequest>().expect("decodes").count,
        5
    );
    let snake = Payload::Json(json!({ "retry_after_seconds": 4, "code": "rate_limited" }));
    assert_eq!(
        snake
            .decode::<PluginError>()
            .expect("decodes")
            .retry_after_seconds,
        Some(4)
    );
}

#[test]
fn hs2_xy0vtn_reader_keeps_buffered_frames_across_the_transport_switch() {
    let mut bytes = encode_all(
        Transport::Json,
        &[Message::Response {
            id: 0,
            outcome: Ok(Payload::Json(json!({ "transport": "protobuf" }))),
        }],
    );
    let frame = Message::StreamItem {
        id: 3,
        payload: Payload::encode(Transport::Protobuf, &v1::CountItem { seq: 1 }),
    };
    bytes.extend(encode_all(
        Transport::Protobuf,
        std::slice::from_ref(&frame),
    ));
    // One cursor holds both, as a pipe would after the plugin writes them back to back.
    let mut reader = MessageReader::new(io::BufReader::new(Cursor::new(bytes)), Transport::Json);
    assert!(matches!(
        reader.read(),
        Ok(Some(Message::Response { id: 0, .. }))
    ));
    reader.set_transport(Transport::Protobuf);
    assert_eq!(reader.read().expect("frame"), Some(frame));
    assert_eq!(reader.read().expect("clean end"), None);
}

#[test]
fn hs2_xy0vtn_unknown_notifications_and_blank_lines_are_skipped() {
    let input = b"\n{\"jsonrpc\":\"2.0\",\"method\":\"$/future\",\"params\":{}}\n{\"jsonrpc\":\"2.0\",\"method\":\"$/cancel\",\"params\":{\"id\":\"12\"}}\n";
    assert_eq!(
        decode_all(Transport::Json, input.to_vec()),
        vec![Message::Cancel { id: 12 }]
    );
    // A frame with no kind (a newer peer's) is skipped too.
    let mut frames = Vec::new();
    frames.extend(0u32.to_le_bytes());
    frames.extend(encode_all(
        Transport::Protobuf,
        &[Message::Cancel { id: 4 }],
    ));
    assert_eq!(
        decode_all(Transport::Protobuf, frames),
        vec![Message::Cancel { id: 4 }]
    );
}

#[test]
fn hs2_xy0vtn_plain_json_rpc_errors_map_to_plugin_errors() {
    let decode = |error: Value| {
        let line = json!({ "jsonrpc": "2.0", "id": 5, "error": error }).to_string() + "\n";
        match decode_all(Transport::Json, line.into_bytes()).pop() {
            Some(Message::Response {
                outcome: Err(e), ..
            }) => e,
            other => panic!("expected an error response, got {other:?}"),
        }
    };
    assert_eq!(
        decode(json!({ "code": -32601, "message": "no such method" })).code,
        code::UNSUPPORTED
    );
    assert_eq!(
        decode(json!({ "code": -32602, "message": "bad params" })).code,
        code::INVALID_ARGUMENT
    );
    assert_eq!(
        decode(json!({ "code": 17, "message": "boom" })).code,
        code::INTERNAL
    );
    let exact = decode(json!({
        "code": -32000,
        "message": "gone",
        "data": { "code": "not_found", "message": "gone" },
    }));
    assert_eq!(exact, PluginError::new(code::NOT_FOUND, "gone"));
}

#[test]
fn hs2_xy0vtn_payloads_must_match_the_transport() {
    let proto = Message::StreamItem {
        id: 1,
        payload: Payload::Proto(vec![]),
    };
    let error = write_message(&mut Vec::new(), Transport::Json, &proto).expect_err("mismatch");
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    let json = Message::StreamItem {
        id: 1,
        payload: Payload::Json(json!({})),
    };
    let error = write_message(&mut Vec::new(), Transport::Protobuf, &json).expect_err("mismatch");
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn hs2_xy0vtn_oversized_and_truncated_input_is_an_error() {
    let mut huge = Vec::new();
    huge.extend(((MAX_FRAME_BYTES + 1) as u32).to_le_bytes());
    let mut reader = MessageReader::new(Cursor::new(huge), Transport::Protobuf);
    assert_eq!(
        reader.read().expect_err("too large").kind(),
        io::ErrorKind::InvalidData
    );

    let mut truncated = encode_all(Transport::Protobuf, &[Message::Cancel { id: 1 }]);
    truncated.pop();
    let mut reader = MessageReader::new(Cursor::new(truncated), Transport::Protobuf);
    assert_eq!(
        reader.read().expect_err("truncated").kind(),
        io::ErrorKind::UnexpectedEof
    );

    let mut reader = MessageReader::new(Cursor::new(b"[1,2]\n".to_vec()), Transport::Json);
    assert_eq!(
        reader.read().expect_err("not an object").kind(),
        io::ErrorKind::InvalidData
    );
}

#[test]
fn hs2_xy0vtn_payload_decode_failures_are_invalid_argument() {
    let bad = Payload::Proto(vec![0xff, 0xff, 0xff]);
    assert_eq!(
        bad.decode::<v1::EchoRequest>().expect_err("bad").code,
        code::INVALID_ARGUMENT
    );
    let bad = Payload::Json(json!({ "count": "lots" }));
    assert_eq!(
        bad.decode::<v1::CountRequest>().expect_err("bad").code,
        code::INVALID_ARGUMENT
    );
    assert!(PluginError::unavailable("x").is_retryable());
    assert!(!PluginError::internal("x").is_retryable());
}
