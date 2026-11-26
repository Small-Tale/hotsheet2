//! The one redacting secret string (HS2-A8T481).
//!
//! A credential held in a struct that derives `Debug` leaks the moment someone writes
//! `dbg!`, `{:?}` in an error, or a failing `assert_eq!`. [`SecretString`] prints
//! `<redacted>` under `Debug`, has no `Display` and no `Deref`, and hands out the value only
//! through [`SecretString::expose`], so every read is explicit at the call site. Serde is
//! `transparent`: a field retyped from `String` keeps its stored and wire format exactly.
//! `tests/secret_debug_policy.rs` rejects a derived `Debug` over any other secret-named
//! string field.

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

#[cfg(test)]
mod tests {
    use super::SecretString;
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
}
