//! A ticket's milestone link and its own due date (HS2-A820C1, docs/38 §38.2, §38.3).
//!
//! `milestone` holds a registry id from the project's milestone registry
//! (`<checkout>/.hotsheet2/milestones/<id>.json`). An id the registry does not name is kept
//! verbatim and read as an unknown milestone, never dropped. `due_on` is a calendar date,
//! `YYYY-MM-DD`; its deadline instant is computed in the project time zone when read.
//!
//! Neither is a content edit: setting or clearing them does not change `updated_at`.

use serde::{Deserialize, Deserializer};

/// Whether `value` has the shape of a milestone id: the group id shape, ten uppercase
/// Crockford base32 characters.
pub fn is_milestone_id(value: &str) -> bool {
    crate::group::is_group_id(value)
}

/// Why a requested ticket milestone or due date was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MilestoneLinkError {
    /// The value is not a milestone id. Without a project registry only ids are accepted.
    #[error("'{0}' is not a milestone id (milestone_registry_required)")]
    NotAnId(String),
    /// The value is not a `YYYY-MM-DD` calendar date.
    #[error("due date '{0}' is not a calendar date in the form YYYY-MM-DD (invalid_due_on)")]
    InvalidDueOn(String),
}

impl MilestoneLinkError {
    /// The stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotAnId(_) => "milestone_registry_required",
            Self::InvalidDueOn(_) => "invalid_due_on",
        }
    }
}

/// Validate a requested milestone id (surrounding whitespace is ignored).
pub fn validate_milestone_id(input: &str) -> Result<String, MilestoneLinkError> {
    let id = input.trim();
    if is_milestone_id(id) {
        Ok(id.to_string())
    } else {
        Err(MilestoneLinkError::NotAnId(id.to_string()))
    }
}

/// Validate a `YYYY-MM-DD` calendar date (surrounding whitespace is ignored).
pub fn validate_due_on(input: &str) -> Result<String, MilestoneLinkError> {
    let date = input.trim();
    if parse_due_on(date).is_some() {
        Ok(date.to_string())
    } else {
        Err(MilestoneLinkError::InvalidDueOn(date.to_string()))
    }
}

/// The calendar date `text` names, strictly `YYYY-MM-DD`; `None` otherwise.
pub fn parse_due_on(text: &str) -> Option<time::Date> {
    let bytes = text.as_bytes();
    let shaped = bytes.len() == 10
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            4 | 7 => *byte == b'-',
            _ => byte.is_ascii_digit(),
        });
    if !shaped {
        return None;
    }
    let year: i32 = text[..4].parse().ok()?;
    let month = time::Month::try_from(text[5..7].parse::<u8>().ok()?).ok()?;
    let day: u8 = text[8..].parse().ok()?;
    time::Date::from_calendar_date(year, month, day).ok()
}

/// Frontmatter deserializer for `milestone` and `due_on`: a scalar is kept as trimmed
/// text, so an id this build's registry does not name (or a hand-edited date) is never
/// dropped; empty text, null, a list, or a mapping reads as unset.
pub(crate) fn deserialize_scalar_text<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_yaml::Value::deserialize(deserializer)?;
    let text = match value {
        serde_yaml::Value::String(text) => text,
        serde_yaml::Value::Number(number) => number.to_string(),
        serde_yaml::Value::Bool(flag) => flag.to_string(),
        _ => return Ok(None),
    };
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    Ok((!text.is_empty()).then_some(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hs2_a820c1_ids_and_dates_validate() {
        assert_eq!(
            validate_milestone_id(" 7K3M9P2QXA "),
            Ok("7K3M9P2QXA".into())
        );
        assert_eq!(
            validate_milestone_id("Toolchain 2.0").unwrap_err().code(),
            "milestone_registry_required"
        );
        assert_eq!(validate_due_on(" 2026-12-04 "), Ok("2026-12-04".into()));
        assert_eq!(validate_due_on("2028-02-29"), Ok("2028-02-29".into()));
        for bad in [
            "2026-02-30",
            "2026-13-01",
            "2026-1-01",
            "tomorrow",
            "",
            "2026/12/04",
        ] {
            assert_eq!(
                validate_due_on(bad).unwrap_err().code(),
                "invalid_due_on",
                "{bad:?}"
            );
        }
    }
}
