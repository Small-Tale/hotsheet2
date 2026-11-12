//! Renewal tokens: a bearer capability for exactly one claim.
//!
//! A token names its own claim, so a holder with nothing else (a task inside a VM) can
//! renew or release it. Storage keeps only a keyed digest of the whole token.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::hmac;
use ring::rand::{SecureRandom, SystemRandom};

use crate::codec::{hex, unhex};
use crate::error::ClaimError;
use crate::model::ResourceRef;

const TOKEN_VERSION: &str = "hsrc1";
const SECRET_LEN: usize = 32;
/// Longest token a surface should accept before parsing.
pub const MAX_RENEWAL_TOKEN_LEN: usize = 512;

/// What a renewal token says about itself. Nothing here is trusted until the keyed
/// digest of the whole token matches the stored one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenewalTokenClaims {
    pub target: ResourceRef,
    pub claim_id: String,
}

/// Parse a token's self-description. Any malformed token is [`ClaimError::TokenInvalid`].
pub fn parse_renewal_token(token: &str) -> Result<RenewalTokenClaims, ClaimError> {
    if token.len() > MAX_RENEWAL_TOKEN_LEN {
        return Err(ClaimError::TokenInvalid);
    }
    let mut parts = token.split('.');
    let (Some(TOKEN_VERSION), Some(target), Some(claim_id), Some(secret), None) = (
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
    ) else {
        return Err(ClaimError::TokenInvalid);
    };
    let target = URL_SAFE_NO_PAD
        .decode(target)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|text| ResourceRef::parse(&text).ok())
        .ok_or(ClaimError::TokenInvalid)?;
    let secret_ok = URL_SAFE_NO_PAD
        .decode(secret)
        .is_ok_and(|bytes| bytes.len() == SECRET_LEN);
    if claim_id.parse::<ulid::Ulid>().is_err() || !secret_ok {
        return Err(ClaimError::TokenInvalid);
    }
    Ok(RenewalTokenClaims {
        target,
        claim_id: claim_id.to_owned(),
    })
}

pub(crate) fn mint(target: &ResourceRef, claim_id: &str) -> Result<String, ClaimError> {
    let mut secret = [0u8; SECRET_LEN];
    SystemRandom::new()
        .fill(&mut secret)
        .map_err(|_| ClaimError::Unsupported("no secure random source".into()))?;
    Ok(format!(
        "{TOKEN_VERSION}.{}.{claim_id}.{}",
        URL_SAFE_NO_PAD.encode(target.to_string()),
        URL_SAFE_NO_PAD.encode(secret)
    ))
}

pub(crate) fn digest(key: &hmac::Key, token: &str) -> String {
    hex(hmac::sign(key, token.as_bytes()).as_ref())
}

pub(crate) fn matches(key: &hmac::Key, token: &str, stored_digest: &str) -> bool {
    unhex(stored_digest).is_some_and(|tag| hmac::verify(key, token.as_bytes(), &tag).is_ok())
}
