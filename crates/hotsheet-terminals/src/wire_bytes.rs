//! Terminal bytes on the detached-broker wire (HS2-9R751Y).
//!
//! The original protocol carried every byte payload as a JSON array of numbers: a 256 KiB
//! scrollback replay was roughly 850 KB of JSON, and each element went through serde's
//! buffered `Content` path for the internally tagged frame enums. [`WireBytes`] encodes as a
//! base64 string when the peer negotiated it ([`BytesEncoding::Base64`]) and as the legacy
//! number array otherwise, and it **always decodes either form**, so a broker that outlives
//! a server upgrade (or the reverse) keeps working in both directions.

use std::ops::Deref;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde::de::{self, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// How a peer asks for byte payloads to be encoded on one connection. Brokers that predate
/// it ignore the request field and keep sending [`BytesEncoding::Array`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BytesEncoding {
    /// A JSON array of numbers: the original encoding every broker and server understands.
    #[default]
    Array,
    /// A standard-alphabet base64 string.
    Base64,
}

impl BytesEncoding {
    /// Whether this is the default (legacy) encoding, for `skip_serializing_if`.
    pub fn is_array(&self) -> bool {
        *self == Self::Array
    }
}

/// A byte payload plus the encoding it travels in. Decoding records which form arrived,
/// so a receiver can learn that its peer speaks base64 ([`WireBytes::encoding`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WireBytes {
    bytes: Vec<u8>,
    encoding: BytesEncoding,
}

impl WireBytes {
    /// `bytes` to be sent in `encoding`.
    pub fn new(bytes: Vec<u8>, encoding: BytesEncoding) -> Self {
        Self { bytes, encoding }
    }

    /// The encoding this payload was decoded from (or will be encoded in).
    pub fn encoding(&self) -> BytesEncoding {
        self.encoding
    }

    /// The raw bytes.
    pub fn into_vec(self) -> Vec<u8> {
        self.bytes
    }
}

/// Legacy-safe default: an unannotated payload travels as a number array.
impl From<Vec<u8>> for WireBytes {
    fn from(bytes: Vec<u8>) -> Self {
        Self::new(bytes, BytesEncoding::Array)
    }
}

impl Deref for WireBytes {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        &self.bytes
    }
}

impl Serialize for WireBytes {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.encoding {
            BytesEncoding::Base64 => serializer.serialize_str(&STANDARD.encode(&self.bytes)),
            BytesEncoding::Array => serializer.collect_seq(&self.bytes),
        }
    }
}

impl<'de> Deserialize<'de> for WireBytes {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct WireBytesVisitor;

        impl<'de> Visitor<'de> for WireBytesVisitor {
            type Value = WireBytes;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a base64 string or an array of bytes")
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<WireBytes, E> {
                STANDARD
                    .decode(value)
                    .map(|bytes| WireBytes::new(bytes, BytesEncoding::Base64))
                    .map_err(|error| E::custom(format!("invalid base64 bytes: {error}")))
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<WireBytes, A::Error> {
                let mut bytes = Vec::with_capacity(seq.size_hint().unwrap_or(0));
                while let Some(byte) = seq.next_element::<u8>()? {
                    bytes.push(byte);
                }
                Ok(WireBytes::new(bytes, BytesEncoding::Array))
            }
        }

        deserializer.deserialize_any(WireBytesVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_forms_round_trip_and_decode_whichever_form_arrives() {
        let bytes = b"\x1b[31mred\x00\xff".to_vec();
        let compact = WireBytes::new(bytes.clone(), BytesEncoding::Base64);
        let text = serde_json::to_string(&compact).unwrap();
        assert!(text.starts_with('"'), "base64 travels as a string: {text}");
        let back: WireBytes = serde_json::from_str(&text).unwrap();
        assert_eq!(
            (&*back, back.encoding()),
            (&bytes[..], BytesEncoding::Base64)
        );

        let legacy = WireBytes::from(bytes.clone());
        let text = serde_json::to_string(&legacy).unwrap();
        assert_eq!(text, "[27,91,51,49,109,114,101,100,0,255]");
        let back: WireBytes = serde_json::from_str(&text).unwrap();
        assert_eq!(
            (&*back, back.encoding()),
            (&bytes[..], BytesEncoding::Array)
        );

        let empty: WireBytes = serde_json::from_str(r#""""#).unwrap();
        assert!(empty.is_empty());
        assert_eq!(empty.encoding(), BytesEncoding::Base64);
    }

    #[test]
    fn malformed_payloads_are_rejected() {
        assert!(serde_json::from_str::<WireBytes>(r#""not base64!""#).is_err());
        assert!(serde_json::from_str::<WireBytes>("[256]").is_err());
        assert!(serde_json::from_str::<WireBytes>("7").is_err());
    }

    /// The HS2-9R751Y measurement: a full 256 KiB scrollback replay frame, encoded and parsed
    /// through the internally tagged frame enum, in both encodings. Run with
    /// `cargo nextest run -p hotsheet-terminals --run-ignored only wire_bytes_replay_cost
    /// --no-capture` (optimized numbers need `--release`).
    #[test]
    #[ignore = "measurement, not a correctness check"]
    fn wire_bytes_replay_cost() {
        use crate::StreamOut;
        let replay: Vec<u8> = (0..256 * 1024).map(|i| (i % 251) as u8).collect();
        for encoding in [BytesEncoding::Array, BytesEncoding::Base64] {
            let frame = StreamOut::Scrollback {
                data: WireBytes::new(replay.clone(), encoding),
            };
            let started = std::time::Instant::now();
            let line = serde_json::to_string(&frame).unwrap();
            let encoded = started.elapsed();
            let started = std::time::Instant::now();
            for _ in 0..10 {
                let parsed: StreamOut = serde_json::from_str(&line).unwrap();
                assert!(
                    matches!(parsed, StreamOut::Scrollback { ref data } if data.len() == replay.len())
                );
            }
            let parsed = started.elapsed() / 10;
            println!(
                "{encoding:?}: {} bytes on the wire, encode {encoded:?}, parse {parsed:?}",
                line.len()
            );
        }
    }
}
