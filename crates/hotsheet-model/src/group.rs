//! Ticket group names (HS2-PDSHNN). A ticket names at most one group; a group exists
//! while at least one non-deleted ticket names it, so there is no registry. Every writer
//! and reader normalizes through this module so the display spelling and the comparison
//! key never diverge between the file format, the index, search, and the API.

use serde::{Deserialize, Deserializer};

/// Longest accepted display name, in Unicode scalar values.
pub const GROUP_NAME_MAX_CHARS: usize = 120;

/// Why a requested group name was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum GroupNameError {
    /// The name holds a control character such as a newline or tab.
    #[error("group name must not contain control characters or line breaks")]
    ControlCharacter,
    /// The normalized name is longer than [`GROUP_NAME_MAX_CHARS`].
    #[error("group name must be at most 120 characters")]
    TooLong,
}

fn collapse_whitespace(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Normalize a requested group name into its display spelling: trimmed, with each
/// internal whitespace run collapsed to one space. `Ok(None)` means the name is empty
/// and the caller should clear the group.
pub fn normalize_group_name(input: &str) -> Result<Option<String>, GroupNameError> {
    if input.chars().any(char::is_control) {
        return Err(GroupNameError::ControlCharacter);
    }
    let name = collapse_whitespace(input);
    if name.is_empty() {
        return Ok(None);
    }
    if name.chars().count() > GROUP_NAME_MAX_CHARS {
        return Err(GroupNameError::TooLong);
    }
    Ok(Some(name))
}

/// Tolerant normalization for values read from a file: control characters become
/// whitespace and an overlong name is truncated, so a hand-edited value never fails
/// the ticket.
pub fn normalize_group_name_lossy(input: &str) -> Option<String> {
    let cleaned: String = input
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect();
    let name = collapse_whitespace(&cleaned);
    if name.is_empty() {
        return None;
    }
    if name.chars().count() <= GROUP_NAME_MAX_CHARS {
        return Some(name);
    }
    let truncated: String = name.chars().take(GROUP_NAME_MAX_CHARS).collect();
    Some(truncated.trim_end().to_string())
}

/// Comparison key of a display name: group names are unique within a project
/// regardless of letter case. Callers pass an already-normalized display name.
pub fn group_key(name: &str) -> String {
    name.to_lowercase()
}

/// Comparison key of arbitrary text (a search term, a filter value), or `None` when it
/// names no group.
pub fn group_key_of_input(input: &str) -> Option<String> {
    normalize_group_name_lossy(input).map(|name| group_key(&name))
}

/// Frontmatter deserializer for `group`: any scalar is read tolerantly and anything
/// else (a list, a mapping, null) is treated as ungrouped.
pub(crate) fn deserialize_group<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_yaml::Value::deserialize(deserializer)?;
    Ok(match value {
        serde_yaml::Value::String(text) => normalize_group_name_lossy(&text),
        serde_yaml::Value::Number(number) => normalize_group_name_lossy(&number.to_string()),
        serde_yaml::Value::Bool(flag) => normalize_group_name_lossy(&flag.to_string()),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hs2_pdshnn_normalizes_display_names() {
        assert_eq!(
            normalize_group_name("  Board   drag \u{a0} drop  "),
            Ok(Some("Board drag drop".into()))
        );
        assert_eq!(normalize_group_name("   "), Ok(None));
        assert_eq!(normalize_group_name(""), Ok(None));
        assert_eq!(
            normalize_group_name("Überarbeitung Ω"),
            Ok(Some("Überarbeitung Ω".into()))
        );
    }

    #[test]
    fn hs2_pdshnn_rejects_control_characters_and_overlong_names() {
        for bad in ["a\nb", "a\tb", "a\rb", "a\u{0}b", "a\u{85}b"] {
            assert_eq!(
                normalize_group_name(bad),
                Err(GroupNameError::ControlCharacter),
                "{bad:?}"
            );
        }
        let limit = "é".repeat(GROUP_NAME_MAX_CHARS);
        assert_eq!(normalize_group_name(&limit), Ok(Some(limit.clone())));
        assert_eq!(
            normalize_group_name(&format!("{limit}x")),
            Err(GroupNameError::TooLong)
        );
        // Length is judged after whitespace collapses.
        assert_eq!(
            normalize_group_name(&format!("  {limit}  ")),
            Ok(Some(limit))
        );
    }

    #[test]
    fn hs2_pdshnn_lossy_normalization_never_fails() {
        assert_eq!(
            normalize_group_name_lossy(" a\n\tb  c "),
            Some("a b c".into())
        );
        assert_eq!(normalize_group_name_lossy("\n\t "), None);
        let long = format!("{} tail", "x".repeat(GROUP_NAME_MAX_CHARS - 1));
        let truncated = normalize_group_name_lossy(&long).unwrap();
        assert_eq!(truncated, "x".repeat(GROUP_NAME_MAX_CHARS - 1));
        assert!(normalize_group_name(&truncated).is_ok());
    }

    #[test]
    fn hs2_pdshnn_keys_compare_without_case() {
        assert_eq!(group_key("Board Drag"), group_key("board drag"));
        assert_eq!(group_key("ÜBER"), group_key("über"));
        assert_ne!(group_key("a b"), group_key("ab"));
        assert_eq!(
            group_key_of_input("  BOARD   drag "),
            Some("board drag".into())
        );
        assert_eq!(group_key_of_input("  "), None);
    }
}
