//! Claim keys and the prefixed metadata codecs (HS2-H3WPPY, HS2-EWGPTZ).
//!
//! A backend stores metadata opaquely. Hot Sheet encodes each value before it reaches
//! the backend: `plain:` as written, `hmac-sha256:` as a keyed digest (for ids that
//! must be comparable but not readable), or `enc:v1:` encrypted (for labels a holder
//! of the key may read back). The codecs live here, below the claim crates, because
//! ticket claims (`claim_gate`) and resource claims share them.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::aead::{self, Aad, LessSafeKey, Nonce, UnboundKey};
use ring::hmac;
use ring::rand::{SecureRandom, SystemRandom};

pub const PLAIN_PREFIX: &str = "plain:";
pub const HMAC_PREFIX: &str = "hmac-sha256:";
pub const ENC_PREFIX: &str = "enc:v1:";
pub const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;

/// A codec failure. The claim crates map it onto their own error models.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CodecError {
    /// The value or key is malformed, sealed under another key, or altered.
    #[error("invalid request: {0}")]
    Invalid(String),
    /// This build cannot encode (no secure random source, key rejected).
    #[error("unsupported: {0}")]
    Unsupported(String),
}

impl CodecError {
    /// The stable code the claim surfaces report (`invalid`, `unsupported`).
    pub fn code(&self) -> &'static str {
        match self {
            Self::Invalid(_) => "invalid",
            Self::Unsupported(_) => "unsupported",
        }
    }

    fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid(message.into())
    }
}

/// The 32-byte secret of one backend configuration. Never printed or serialized.
#[derive(Clone)]
pub struct ClaimKey([u8; KEY_LEN]);

impl ClaimKey {
    pub fn from_bytes(bytes: [u8; KEY_LEN]) -> Self {
        Self(bytes)
    }

    fn derive(&self, purpose: &str) -> [u8; KEY_LEN] {
        let master = hmac::Key::new(hmac::HMAC_SHA256, &self.0);
        let tag = hmac::sign(&master, purpose.as_bytes());
        let mut out = [0u8; KEY_LEN];
        out.copy_from_slice(tag.as_ref());
        out
    }

    pub fn token_key(&self) -> hmac::Key {
        hmac::Key::new(
            hmac::HMAC_SHA256,
            &self.derive("hotsheet/resource-claims/renewal-token/v1"),
        )
    }
}

impl std::fmt::Debug for ClaimKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ClaimKey(..)")
    }
}

/// Which codec a value uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataCodec {
    Plain,
    HmacSha256,
    EncV1,
}

impl MetadataCodec {
    pub fn prefix(self) -> &'static str {
        match self {
            Self::Plain => PLAIN_PREFIX,
            Self::HmacSha256 => HMAC_PREFIX,
            Self::EncV1 => ENC_PREFIX,
        }
    }

    /// The codec of a stored value and its payload, by prefix.
    pub fn of_stored(stored: &str) -> Option<(Self, &str)> {
        [Self::Plain, Self::HmacSha256, Self::EncV1]
            .into_iter()
            .find_map(|codec| Some((codec, stored.strip_prefix(codec.prefix())?)))
    }
}

/// A stored value as its reader may see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetadataValue {
    Plain(String),
    /// A keyed digest: comparable with [`MetadataCodecs::hmac_matches`], never readable.
    Hmac(String),
    Decrypted(String),
}

/// Encodes and decodes metadata values with one backend configuration's key.
pub struct MetadataCodecs {
    hmac: hmac::Key,
    enc: LessSafeKey,
    random: SystemRandom,
}

impl std::fmt::Debug for MetadataCodecs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MetadataCodecs(..)")
    }
}

impl MetadataCodecs {
    pub fn new(key: &ClaimKey) -> Result<Self, CodecError> {
        let enc = UnboundKey::new(
            &aead::CHACHA20_POLY1305,
            &key.derive("hotsheet/resource-claims/metadata-enc/v1"),
        )
        .map_err(|_| CodecError::Unsupported("metadata encryption key rejected".into()))?;
        Ok(Self {
            hmac: hmac::Key::new(
                hmac::HMAC_SHA256,
                &key.derive("hotsheet/resource-claims/metadata-hmac/v1"),
            ),
            enc: LessSafeKey::new(enc),
            random: SystemRandom::new(),
        })
    }

    /// Encode `text` for storage with the chosen codec.
    pub fn encode(&self, codec: MetadataCodec, text: &str) -> Result<String, CodecError> {
        match codec {
            MetadataCodec::Plain => Ok(format!("{PLAIN_PREFIX}{text}")),
            MetadataCodec::HmacSha256 => {
                let tag = hmac::sign(&self.hmac, text.as_bytes());
                Ok(format!("{HMAC_PREFIX}{}", hex(tag.as_ref())))
            }
            MetadataCodec::EncV1 => {
                let mut nonce = [0u8; NONCE_LEN];
                self.random
                    .fill(&mut nonce)
                    .map_err(|_| CodecError::Unsupported("no secure random source".into()))?;
                let mut sealed = text.as_bytes().to_vec();
                self.enc
                    .seal_in_place_append_tag(
                        Nonce::assume_unique_for_key(nonce),
                        Aad::from(ENC_PREFIX.as_bytes()),
                        &mut sealed,
                    )
                    .map_err(|_| CodecError::Unsupported("metadata encryption failed".into()))?;
                let mut out = nonce.to_vec();
                out.extend_from_slice(&sealed);
                Ok(format!("{ENC_PREFIX}{}", URL_SAFE_NO_PAD.encode(out)))
            }
        }
    }

    /// Encode a value a person typed: a leading codec prefix picks the codec for the text
    /// after it, and a value with no prefix is stored `plain:`.
    pub fn encode_input(&self, input: &str) -> Result<String, CodecError> {
        match MetadataCodec::of_stored(input) {
            Some((codec, text)) => self.encode(codec, text),
            None => self.encode(MetadataCodec::Plain, input),
        }
    }

    /// Decode a stored value. An `enc:v1:` value sealed under another key, or altered,
    /// fails as [`CodecError::Invalid`].
    pub fn decode(&self, stored: &str) -> Result<MetadataValue, CodecError> {
        match MetadataCodec::of_stored(stored) {
            Some((MetadataCodec::Plain, text)) => Ok(MetadataValue::Plain(text.to_owned())),
            Some((MetadataCodec::HmacSha256, digest)) => Ok(MetadataValue::Hmac(digest.to_owned())),
            Some((MetadataCodec::EncV1, payload)) => {
                let undecodable = || CodecError::invalid("enc:v1: value cannot be decrypted");
                let mut bytes = URL_SAFE_NO_PAD.decode(payload).map_err(|_| undecodable())?;
                if bytes.len() < NONCE_LEN {
                    return Err(undecodable());
                }
                let mut sealed = bytes.split_off(NONCE_LEN);
                let nonce: [u8; NONCE_LEN] = bytes.try_into().map_err(|_| undecodable())?;
                let opened = self
                    .enc
                    .open_in_place(
                        Nonce::assume_unique_for_key(nonce),
                        Aad::from(ENC_PREFIX.as_bytes()),
                        &mut sealed,
                    )
                    .map_err(|_| undecodable())?;
                String::from_utf8(opened.to_vec())
                    .map(MetadataValue::Decrypted)
                    .map_err(|_| undecodable())
            }
            None => Err(CodecError::invalid(
                "metadata value needs a plain:, hmac-sha256: or enc:v1: prefix",
            )),
        }
    }

    /// Whether a stored `hmac-sha256:` value is the digest of `candidate`, in constant time.
    pub fn hmac_matches(&self, stored: &str, candidate: &str) -> bool {
        let Some((MetadataCodec::HmacSha256, digest)) = MetadataCodec::of_stored(stored) else {
            return false;
        };
        unhex(digest)
            .is_some_and(|tag| hmac::verify(&self.hmac, candidate.as_bytes(), &tag).is_ok())
    }
}

pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

pub fn unhex(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 || !text.is_ascii() {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&text[at..at + 2], 16).ok())
        .collect()
}
