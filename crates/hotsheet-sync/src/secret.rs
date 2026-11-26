//! The one redacting secret string (HS2-A8T481).
//!
//! A credential held in a struct that derives `Debug` leaks the moment someone writes
//! `dbg!`, `{:?}` in an error, or a failing `assert_eq!`. [`SecretString`] prints
//! `<redacted>` under `Debug`, has no `Display` and no `Deref`, and hands out the value only
//! through [`SecretString::expose`], so every read is explicit at the call site. Serde is
//! `transparent`: a field retyped from `String` keeps its stored and wire format exactly.
//! `tests/secret_debug_policy.rs` rejects a derived `Debug` over any other secret-named
//! string field.
//!
//! HTTP header collections carry credentials too (`Authorization`, `Cookie`, signature and
//! API-key headers). [`RedactedHeaders`] is the `Debug` adapter a hand-written `Debug` uses
//! for them: every header name stays visible, and the value of each [`sensitive_header`]
//! prints `<redacted>` (HS2-CQNE4E). The policy test also rejects a derived `Debug` over a
//! plain-string header collection.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A secret string whose `Debug` output is `<redacted>`.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SecretString(String);

impl SecretString {
    /// Wrap a secret value.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The secret value. Callers pass it straight to the transport that needs it.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Unwrap the secret value.
    pub fn into_inner(self) -> String {
        self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

impl From<String> for SecretString {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for SecretString {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

/// Header names whose value is a credential. Case-insensitive; deliberately broad.
const SENSITIVE_HEADERS: &[&str] = &[
    "authorization",
    "proxy-authorization",
    "cookie",
    "set-cookie",
];
const SENSITIVE_HEADER_WORDS: &[&str] = &[
    "token",
    "secret",
    "key",
    "auth",
    "signature",
    "password",
    "session",
    "credential",
];

/// Whether `name` is a header whose value must never be printed (HS2-CQNE4E).
pub fn sensitive_header(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    SENSITIVE_HEADERS.contains(&name.as_str())
        || SENSITIVE_HEADER_WORDS
            .iter()
            .any(|word| name.contains(word))
}

/// `Debug` view of a header collection that keeps names and redacts credential values.
pub struct RedactedHeaders<'a>(Vec<(&'a str, &'a str)>);

impl<'a> RedactedHeaders<'a> {
    /// Wrap headers in their own order.
    pub fn new(headers: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        Self(headers.into_iter().collect())
    }

    /// Wrap headers sorted by name, for unordered maps whose `Debug` should be stable.
    pub fn sorted(headers: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        let mut headers: Vec<_> = headers.into_iter().collect();
        headers.sort_unstable();
        Self(headers)
    }
}

impl fmt::Debug for RedactedHeaders<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        struct Value<'v>(&'v str, bool);
        impl fmt::Debug for Value<'_> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                if self.1 {
                    f.write_str("<redacted>")
                } else {
                    fmt::Debug::fmt(self.0, f)
                }
            }
        }
        f.debug_list()
            .entries(
                self.0
                    .iter()
                    .map(|(name, value)| (name, Value(value, sensitive_header(name)))),
            )
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::{RedactedHeaders, SecretString, sensitive_header};
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct Holder {
        token: SecretString,
        #[serde(default)]
        refresh: Option<SecretString>,
    }

    #[test]
    fn hs2_a8t481_debug_never_prints_the_value() {
        let holder = Holder {
            token: "ghp_synthetic".into(),
            refresh: Some(SecretString::new("ghr_synthetic")),
        };
        let printed = format!("{holder:?} {holder:#?}");
        assert!(!printed.contains("synthetic"), "{printed}");
        assert_eq!(format!("{:?}", holder.token), "<redacted>");
        assert!(printed.contains("Some(<redacted>)"), "{printed}");
    }

    #[test]
    fn hs2_a8t481_serde_matches_a_plain_string_field() {
        #[derive(Serialize, Deserialize)]
        struct Plain {
            token: String,
            refresh: Option<String>,
        }
        let plain = r#"{"token":"t-1","refresh":"r-1"}"#;
        let holder: Holder = serde_json::from_str(plain).unwrap();
        assert_eq!(holder.token.expose(), "t-1");
        assert_eq!(serde_json::to_string(&holder).unwrap(), plain);
        let back: Plain = serde_json::from_str(&serde_json::to_string(&holder).unwrap()).unwrap();
        assert_eq!(
            (back.token.as_str(), back.refresh.as_deref()),
            ("t-1", Some("r-1"))
        );

        let missing: Holder = serde_json::from_str(r#"{"token":""}"#).unwrap();
        assert!(missing.token.is_empty());
        assert_eq!(missing.refresh, None);
        assert_eq!(missing.token.into_inner(), "");
    }

    #[test]
    fn hs2_cqne4e_sensitive_header_names() {
        for name in [
            "Authorization",
            "proxy-authorization",
            "Cookie",
            "Set-Cookie",
            "X-Api-Key",
            "X-Auth-Token",
            "X-Hotsheet-Signature",
            "X-Session-Id",
            "X-Client-Secret",
        ] {
            assert!(sensitive_header(name), "{name}");
        }
        for name in [
            "Content-Type",
            "Title",
            "Priority",
            "Click",
            "X-Hotsheet-Timestamp",
            "Retry-After",
            "Keep-Alive",
        ] {
            assert!(!sensitive_header(name), "{name}");
        }
    }

    #[test]
    fn hs2_cqne4e_redacted_headers_keep_names_and_hide_values() {
        let headers = [
            ("Authorization", "Basic c3ludGhldGljOnBhc3M="),
            ("Content-Type", "application/json"),
            ("X-Api-Key", "synthetic-key"),
        ];
        let printed = format!("{:?}", RedactedHeaders::new(headers));
        assert_eq!(
            printed,
            r#"[("Authorization", <redacted>), ("Content-Type", "application/json"), ("X-Api-Key", <redacted>)]"#
        );
        let sorted = format!(
            "{:?}",
            RedactedHeaders::sorted([("b", "2"), ("a", "1"), ("cookie", "synthetic")])
        );
        assert_eq!(
            sorted,
            r#"[("a", "1"), ("b", "2"), ("cookie", <redacted>)]"#
        );
    }
}
