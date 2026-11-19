//! Mapping a GitHub `push` webhook payload to a remote-change hint (HS2-BWJWWX, `docs/37`
//! §37.9). The payload reaches Hot Sheet through an event service as untrusted text. Only the
//! repository identity, the ref and the tip are read; the payload is never stored, logged or
//! forwarded, and every unusable case is a named [`Skip`], not an error.

use serde_json::Value;

use super::model::{ChangeHint, RepoIdentity};

/// The largest payload read. A push naming thousands of commits can exceed a free realtime
/// plan's message limit anyway; anything bigger than this is skipped, not parsed.
pub const MAX_PAYLOAD_BYTES: usize = 256 * 1024;

/// Why a payload produced no hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skip {
    /// GitHub's `ping` when a webhook is created: it carries no ref.
    Ping,
    /// A delivery for another event (an issue, a pull request): no `ref` and no `after`.
    NotAPush,
    /// A deleted branch or tag: the ref no longer advances anything to fetch.
    RefDeleted,
    /// Larger than [`MAX_PAYLOAD_BYTES`].
    Oversized,
    /// Not JSON, or not an object.
    Malformed,
    /// A push payload without a usable `ref`, `after` or `repository`.
    MissingField(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mapped {
    Hint(ChangeHint),
    Skipped(Skip),
}

fn is_object_id(text: &str) -> bool {
    matches!(text.len(), 40 | 64) && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Map one payload, given as the text the service delivered. `event_id` is the service's
/// own message id: GitHub's delivery id is not in the body, and a retried delivery collapses on
/// (ref, tip) anyway.
pub fn map_push(text: &str, event_id: Option<&str>, now_ms: u64) -> Mapped {
    if text.len() > MAX_PAYLOAD_BYTES {
        return Mapped::Skipped(Skip::Oversized);
    }
    match serde_json::from_str::<Value>(text) {
        Ok(value) => map_value(&value, event_id, now_ms),
        Err(_) => Mapped::Skipped(Skip::Malformed),
    }
}

/// Map a payload the service already decoded to JSON.
pub fn map_value(value: &Value, event_id: Option<&str>, now_ms: u64) -> Mapped {
    let Some(object) = value.as_object() else {
        return Mapped::Skipped(Skip::Malformed);
    };
    let Some(reference) = object.get("ref") else {
        return Mapped::Skipped(
            if object.contains_key("zen") || object.contains_key("hook_id") {
                Skip::Ping
            } else {
                Skip::NotAPush
            },
        );
    };
    let Some(reference) = reference
        .as_str()
        .filter(|r| r.starts_with("refs/") && r.len() <= 255 && !r.chars().any(char::is_control))
    else {
        return Mapped::Skipped(Skip::MissingField("ref"));
    };
    let Some(after) = object
        .get("after")
        .and_then(Value::as_str)
        .filter(|a| is_object_id(a))
    else {
        return Mapped::Skipped(Skip::MissingField("after"));
    };
    let repository = object.get("repository").and_then(Value::as_object);
    let identity = RepoIdentity {
        id: repository.and_then(|r| r.get("id")).and_then(Value::as_u64),
        full_name: repository
            .and_then(|r| r.get("full_name"))
            .and_then(Value::as_str)
            .filter(|n| n.len() <= 200)
            .map(str::to_owned),
    };
    if identity.is_empty() {
        return Mapped::Skipped(Skip::MissingField("repository"));
    }
    if object.get("deleted").and_then(Value::as_bool) == Some(true)
        || after.bytes().all(|b| b == b'0')
    {
        return Mapped::Skipped(Skip::RefDeleted);
    }
    Mapped::Hint(ChangeHint {
        repository: identity,
        ref_name: reference.to_owned(),
        advertised_tip: Some(after.to_owned()),
        event_id: event_id.map(str::to_owned),
        at_ms: now_ms,
    })
}
