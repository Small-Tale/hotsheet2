//! A lenient RFC3339 timestamp.
//!
//! Ticket files carry timestamps as RFC3339 text. We want typed, chronological
//! comparison (for last-writer-wins merges, `docs/02` §2.7) **without** losing the
//! round-trip guarantee or panicking on a malformed value (`docs/17` §17.4). So a
//! [`Timestamp`] always keeps the **raw** text verbatim (what serializes back to
//! disk) and, *when the text is valid RFC3339*, also an [`OffsetDateTime`] for
//! comparison. An unparseable value degrades to `instant() == None` rather than
//! erroring — the raw text is preserved untouched.

use std::cmp::Ordering;
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// An RFC3339 timestamp that preserves its exact on-disk text and, when valid, a
/// parsed instant. Equality is textual (so parse→serialize round-trips); use
/// [`Timestamp::instant`] / [`Timestamp::is_after`] for chronological order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Timestamp {
    raw: String,
    parsed: Option<OffsetDateTime>,
}

impl Timestamp {
    /// Build from raw text, parsing it as RFC3339 if possible (invalid → preserved
    /// verbatim with no instant). Never fails.
    pub fn new(raw: impl Into<String>) -> Self {
        let raw = raw.into();
        let parsed = OffsetDateTime::parse(&raw, &Rfc3339).ok();
        Self { raw, parsed }
    }

    /// Build from a parsed instant, rendering canonical RFC3339 text.
    pub fn from_datetime(dt: OffsetDateTime) -> Self {
        let raw = dt.format(&Rfc3339).unwrap_or_default();
        Self {
            raw,
            parsed: Some(dt),
        }
    }

    /// The exact on-disk text.
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// The parsed instant, or `None` if the text isn't valid RFC3339.
    pub fn instant(&self) -> Option<OffsetDateTime> {
        self.parsed
    }

    /// Whether the text parsed as valid RFC3339.
    pub fn is_valid(&self) -> bool {
        self.parsed.is_some()
    }

    /// Whether the raw text is empty.
    pub fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }

    /// Chronological order **when both are valid**; `None` if either is unparseable
    /// (callers decide how to break the tie — e.g. last-writer-wins keeps the current
    /// value). Distinct from `Eq`, which is textual.
    pub fn chronological_cmp(&self, other: &Self) -> Option<Ordering> {
        match (self.parsed, other.parsed) {
            (Some(a), Some(b)) => Some(a.cmp(&b)),
            _ => None,
        }
    }

    /// True when `self` is strictly later than `other` (both must be valid).
    pub fn is_after(&self, other: &Self) -> bool {
        self.chronological_cmp(other) == Some(Ordering::Greater)
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.raw)
    }
}

impl From<String> for Timestamp {
    fn from(raw: String) -> Self {
        Self::new(raw)
    }
}

impl From<&str> for Timestamp {
    fn from(raw: &str) -> Self {
        Self::new(raw)
    }
}

impl From<OffsetDateTime> for Timestamp {
    fn from(dt: OffsetDateTime) -> Self {
        Self::from_datetime(dt)
    }
}

impl Serialize for Timestamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.raw)
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // Accept any scalar as text — YAML may hand us a bare (unquoted) timestamp.
        let raw = String::deserialize(deserializer)?;
        Ok(Self::new(raw))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_rfc3339_parses_and_preserves_text() {
        let t = Timestamp::new("2026-08-19T14:03:11Z");
        assert!(t.is_valid());
        assert_eq!(t.as_str(), "2026-08-19T14:03:11Z");
        assert!(t.instant().is_some());
    }

    #[test]
    fn invalid_text_degrades_but_is_preserved() {
        let t = Timestamp::new("not a timestamp");
        assert!(!t.is_valid());
        assert_eq!(t.as_str(), "not a timestamp");
        assert_eq!(t.instant(), None);
    }

    #[test]
    fn fractional_seconds_and_offsets_parse() {
        assert!(Timestamp::new("2026-08-19T01:30:27.831Z").is_valid());
        assert!(Timestamp::new("2026-08-19T14:03:11+02:00").is_valid());
    }

    #[test]
    fn chronological_order_uses_the_instant_not_the_text() {
        let earlier = Timestamp::new("2026-08-19T14:03:11Z");
        let later = Timestamp::new("2026-08-19T15:20:44Z");
        assert!(later.is_after(&earlier));
        assert!(!earlier.is_after(&later));
        // An unparseable side yields no ordering.
        assert_eq!(later.chronological_cmp(&Timestamp::new("x")), None);
    }

    #[test]
    fn equality_is_textual() {
        // Same instant, different text ⇒ not equal (round-trip identity).
        let z = Timestamp::new("2026-08-19T14:03:11Z");
        let plus = Timestamp::new("2026-08-19T14:03:11+00:00");
        assert_ne!(z, plus);
        assert_eq!(z.chronological_cmp(&plus), Some(Ordering::Equal));
    }
}
