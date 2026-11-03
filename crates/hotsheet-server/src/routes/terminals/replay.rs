//! Opt-in replay framing for the terminal attach WebSocket (HS2-K93QHA).
//!
//! A viewer opts in with `replay_protocol=2` on the attach URL. The server then announces every
//! replay payload with a `{"terminal_frame":{…}}` text frame naming its kind and encoding, caps
//! it to the viewer's `replay_max_bytes` at a parser-safe start, and deflates it when the viewer
//! asked for `replay_encoding=deflate-raw`. Live output is never framed or compressed.
//!
//! Older servers ignore the parameters, and an opted-in viewer treats an unannounced binary
//! frame as an identity replay. Older viewers never opt in and keep the legacy frames.

use axum::extract::ws::Message;
use hotsheet_terminals::StreamPosition;
use serde::{Deserialize, Serialize};
use std::io::Write as _;

/// The framed protocol revision a viewer requests with `replay_protocol`.
pub(crate) const REPLAY_PROTOCOL: u32 = 2;

/// Replays smaller than this are sent uncompressed: deflate would barely help.
const MIN_DEFLATE_BYTES: usize = 1024;

/// The attach URL's replay parameters. Every field is a string so a malformed value disables
/// that option instead of rejecting the attach.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct ReplayParams {
    #[serde(default)]
    replay_protocol: Option<String>,
    #[serde(default)]
    replay_max_bytes: Option<String>,
    #[serde(default)]
    replay_encoding: Option<String>,
    #[serde(default)]
    resume_from: Option<String>,
}

/// What one viewer negotiated.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ReplayOptions {
    framed: bool,
    max_bytes: Option<usize>,
    deflate: bool,
    /// The stream position the viewer has applied output up to, on a reconnect.
    resume_from: Option<StreamPosition>,
}

impl From<&ReplayParams> for ReplayOptions {
    fn from(params: &ReplayParams) -> Self {
        let framed = params
            .replay_protocol
            .as_deref()
            .and_then(|value| value.parse::<u32>().ok())
            .is_some_and(|version| version >= REPLAY_PROTOCOL);
        if !framed {
            return Self::default();
        }
        Self {
            framed,
            max_bytes: params
                .replay_max_bytes
                .as_deref()
                .and_then(|value| value.parse::<usize>().ok())
                .filter(|bytes| *bytes > 0),
            deflate: params.replay_encoding.as_deref() == Some("deflate-raw"),
            resume_from: params.resume_from.as_deref().and_then(parse_position),
        }
    }
}

/// Which replay a payload is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReplayKind {
    /// The attach's first replay.
    Replay,
    /// A later snapshot that replaces the viewer's emulator state (a lag resync).
    Replace,
    /// Only the output after the viewer's `resume_from`, appended to its emulator.
    Resume,
}

#[derive(Serialize)]
struct FrameHeader {
    kind: ReplayKind,
    encoding: &'static str,
    /// Uncompressed payload length.
    bytes: usize,
    /// The output stream's id (16 hex digits) and the offset just past the payload, when the
    /// terminal reports them.
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    end_offset: Option<u64>,
}

/// A `resume_from` value: `<stream id in hex>:<offset>`.
fn parse_position(value: &str) -> Option<StreamPosition> {
    let (stream, offset) = value.split_once(':')?;
    Some(StreamPosition {
        stream: u64::from_str_radix(stream, 16).ok()?,
        end_offset: offset.parse().ok()?,
    })
}

fn stream_id(stream: u64) -> String {
    format!("{stream:016x}")
}

#[derive(Serialize)]
struct FrameMessage {
    terminal_frame: FrameHeader,
}

/// The legacy control that precedes a replacement replay for viewers that did not opt in.
pub(crate) fn terminal_replay_control() -> Message {
    Message::Text(r#"{"terminal_replay":"replace"}"#.into())
}

fn deflate_raw(data: &[u8]) -> Option<Vec<u8>> {
    let mut encoder = flate2::write::DeflateEncoder::new(
        Vec::with_capacity(data.len() / 4),
        flate2::Compression::new(6),
    );
    encoder.write_all(data).ok()?;
    encoder.finish().ok()
}

/// The WebSocket messages for one replay payload, in order.
pub(crate) async fn replay_messages(
    options: ReplayOptions,
    kind: ReplayKind,
    data: Vec<u8>,
    position: Option<StreamPosition>,
) -> Vec<Message> {
    if !options.framed {
        return match kind {
            // Legacy viewers never send `resume_from`, so only callers' kinds reach here.
            ReplayKind::Replay | ReplayKind::Resume => vec![Message::Binary(data.into())],
            ReplayKind::Replace => vec![terminal_replay_control(), Message::Binary(data.into())],
        };
    }
    // A reconnecting viewer gets only what it missed while that is still retained and no larger
    // than a fresh replay would be; otherwise it falls back to a full replacement.
    let resumed = match (kind, options.resume_from, position) {
        // Only an offset in this same output stream can resume: a PTY reopened under the same
        // terminal id starts a new stream.
        (ReplayKind::Replay, Some(from), Some(at)) if from.stream == at.stream => {
            hotsheet_terminals::replay_after(&data, at.end_offset, from.end_offset)
                .filter(|missed| options.max_bytes.is_none_or(|max| missed.len() <= max))
                .map(<[u8]>::to_vec)
        }
        _ => None,
    };
    let (kind, data) = match resumed {
        Some(missed) => (ReplayKind::Resume, missed),
        None => (
            kind,
            match options.max_bytes {
                Some(max) if data.len() > max => {
                    hotsheet_terminals::replay_suffix(&data, max).to_vec()
                }
                _ => data,
            },
        ),
    };
    let bytes = data.len();
    let (encoding, payload) = if options.deflate && bytes >= MIN_DEFLATE_BYTES {
        // A large replay takes tens of milliseconds to deflate: keep it off the async workers.
        let compressed = tokio::task::spawn_blocking(move || deflate_raw(&data).ok_or(data))
            .await
            .ok();
        match compressed {
            Some(Ok(compressed)) => ("deflate-raw", compressed),
            Some(Err(data)) => ("identity", data),
            None => return Vec::new(),
        }
    } else {
        ("identity", data)
    };
    let header = FrameMessage {
        terminal_frame: FrameHeader {
            kind,
            encoding,
            bytes,
            stream: position.map(|at| stream_id(at.stream)),
            end_offset: position.map(|at| at.end_offset),
        },
    };
    vec![
        // Serializing a fixed-shape header cannot fail.
        Message::Text(serde_json::to_string(&header).unwrap_or_default().into()),
        Message::Binary(payload.into()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read as _;

    fn at(end_offset: u64) -> StreamPosition {
        StreamPosition {
            stream: 0xab,
            end_offset,
        }
    }

    fn params(query: &str) -> ReplayOptions {
        let params: ReplayParams = serde_json::from_value(serde_json::Value::Object(
            query
                .split('&')
                .filter(|pair| !pair.is_empty())
                .map(|pair| {
                    let (key, value) = pair.split_once('=').unwrap();
                    (key.to_string(), serde_json::Value::String(value.into()))
                })
                .collect(),
        ))
        .unwrap();
        ReplayOptions::from(&params)
    }

    fn text(message: &Message) -> serde_json::Value {
        match message {
            Message::Text(value) => serde_json::from_str(value.as_str()).unwrap(),
            other => panic!("expected text, got {other:?}"),
        }
    }

    fn binary(message: &Message) -> Vec<u8> {
        match message {
            Message::Binary(value) => value.to_vec(),
            other => panic!("expected binary, got {other:?}"),
        }
    }

    #[test]
    fn hs2_k93qha_only_protocol_two_opts_in_and_bad_values_disable_options() {
        assert_eq!(params(""), ReplayOptions::default());
        assert_eq!(
            params("replay_max_bytes=10&replay_encoding=deflate-raw"),
            ReplayOptions::default()
        );
        assert_eq!(params("replay_protocol=1"), ReplayOptions::default());
        assert_eq!(
            params("replay_protocol=2&replay_max_bytes=x&replay_encoding=gzip"),
            ReplayOptions {
                framed: true,
                max_bytes: None,
                deflate: false,
                resume_from: None
            }
        );
        assert_eq!(
            params("replay_protocol=2&replay_max_bytes=0"),
            ReplayOptions {
                framed: true,
                max_bytes: None,
                deflate: false,
                resume_from: None
            }
        );
        assert_eq!(
            params("replay_protocol=3&replay_max_bytes=4096&replay_encoding=deflate-raw"),
            ReplayOptions {
                framed: true,
                max_bytes: Some(4096),
                deflate: true,
                resume_from: None
            }
        );
    }

    #[tokio::test]
    async fn hs2_k93qha_legacy_viewers_keep_the_unframed_replay_frames() {
        let legacy = ReplayOptions::default();
        let replay =
            replay_messages(legacy, ReplayKind::Replay, b"abc".to_vec(), Some(at(3))).await;
        assert_eq!(replay.len(), 1);
        assert_eq!(binary(&replay[0]), b"abc");
        let replace = replay_messages(legacy, ReplayKind::Replace, b"abc".to_vec(), None).await;
        assert_eq!(
            text(&replace[0]),
            serde_json::json!({"terminal_replay": "replace"})
        );
        assert_eq!(binary(&replace[1]), b"abc");
    }

    #[tokio::test]
    async fn hs2_k93qha_framed_replays_are_capped_and_deflated() {
        let output: Vec<u8> = (0..2_000)
            .flat_map(|line| format!("\x1b[32mline {line}\x1b[0m\r\n").into_bytes())
            .collect();
        let options = params("replay_protocol=2&replay_max_bytes=8192&replay_encoding=deflate-raw");
        let messages = replay_messages(
            options,
            ReplayKind::Replace,
            output.clone(),
            Some(at(90_000)),
        )
        .await;
        let header = text(&messages[0]);
        let expected = hotsheet_terminals::replay_suffix(&output, 8192);
        assert_eq!(
            header,
            serde_json::json!({"terminal_frame": {"kind": "replace", "encoding": "deflate-raw", "bytes": expected.len(), "stream": "00000000000000ab", "end_offset": 90_000}})
        );
        let compressed = binary(&messages[1]);
        assert!(compressed.len() < expected.len() / 3);
        let mut inflated = Vec::new();
        flate2::read::DeflateDecoder::new(&compressed[..])
            .read_to_end(&mut inflated)
            .unwrap();
        assert_eq!(inflated, expected);
    }

    #[tokio::test]
    async fn hs2_k93qha_small_and_unrequested_replays_stay_identity() {
        let identity = params("replay_protocol=2");
        let messages = replay_messages(identity, ReplayKind::Replay, vec![b'x'; 4096], None).await;
        assert_eq!(
            text(&messages[0]),
            serde_json::json!({"terminal_frame": {"kind": "replay", "encoding": "identity", "bytes": 4096}})
        );
        assert_eq!(binary(&messages[1]).len(), 4096);
        let small = params("replay_protocol=2&replay_encoding=deflate-raw");
        let messages = replay_messages(small, ReplayKind::Replay, Vec::new(), Some(at(0))).await;
        assert_eq!(
            text(&messages[0]),
            serde_json::json!({"terminal_frame": {"kind": "replay", "encoding": "identity", "bytes": 0, "stream": "00000000000000ab", "end_offset": 0}})
        );
        assert!(binary(&messages[1]).is_empty());
    }

    /// HS2-K93QHA phase 3: a reconnect resumes only while its offset is retained and the gap
    /// is no larger than a fresh replay; otherwise it falls back to a full capped replay.
    #[tokio::test]
    async fn hs2_k93qha_reconnects_resume_inside_the_window_and_fall_back_outside_it() {
        let data = b"0123456789".to_vec();
        let resume = |from: &str, max: &str| {
            params(&format!(
                "replay_protocol=2&resume_from=ab:{from}&replay_max_bytes={max}"
            ))
        };
        // Retained, small gap: only the missed bytes, appended.
        let messages = replay_messages(
            resume("107", "64"),
            ReplayKind::Replay,
            data.clone(),
            Some(at(110)),
        )
        .await;
        assert_eq!(
            text(&messages[0]),
            serde_json::json!({"terminal_frame": {"kind": "resume", "encoding": "identity", "bytes": 3, "stream": "00000000000000ab", "end_offset": 110}})
        );
        assert_eq!(binary(&messages[1]), b"789");
        // Fully caught up: an empty resume is still the attach boundary.
        let messages = replay_messages(
            resume("110", "64"),
            ReplayKind::Replay,
            data.clone(),
            Some(at(110)),
        )
        .await;
        assert_eq!(text(&messages[0])["terminal_frame"]["kind"], "resume");
        assert!(binary(&messages[1]).is_empty());
        // Evicted, future, unknown-offset, too-large gap, and replace: full replay instead.
        for (options, end, kind) in [
            (resume("99", "64"), Some(at(110)), ReplayKind::Replay),
            (resume("111", "64"), Some(at(110)), ReplayKind::Replay),
            (resume("105", "64"), None, ReplayKind::Replay),
            (resume("100", "4"), Some(at(110)), ReplayKind::Replay),
            (resume("107", "64"), Some(at(110)), ReplayKind::Replace),
            // The same terminal id reopened as a new PTY: another stream never resumes.
            (
                resume("107", "64"),
                Some(StreamPosition {
                    stream: 0xcd,
                    end_offset: 110,
                }),
                ReplayKind::Replay,
            ),
        ] {
            let messages = replay_messages(options, kind, data.clone(), end).await;
            let header = text(&messages[0]);
            assert_ne!(
                header["terminal_frame"]["kind"], "resume",
                "{options:?} {end:?}"
            );
            let expected: &[u8] = if options.max_bytes == Some(4) {
                b"6789"
            } else {
                &data
            };
            assert_eq!(binary(&messages[1]), expected, "{options:?}");
        }
        // A malformed position is ignored, never an error.
        for bad in ["-1", "107", "zz:1", "ab:-1", "ab:"] {
            assert_eq!(
                params(&format!("replay_protocol=2&resume_from={bad}")).resume_from,
                None,
                "{bad}"
            );
        }
    }
}
