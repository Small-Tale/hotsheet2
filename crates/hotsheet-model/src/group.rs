//! Ticket group ids and names (HS2-PDSHNN, HS2-9KZSD4). A ticket names at most one group
//! by its **id**; the project's group registry maps each id to a display name. A value
//! that is not id-shaped is a *legacy name* written before HS2-9KZSD4: it is read as is,
//! shown by name, and rewritten to an id by the explicit group migration. Every writer and
//! reader classifies through this module so the file format, the index, search, and the
//! API agree on what a stored value means.

use serde::{Deserialize, Deserializer};

/// Longest accepted display name, in Unicode scalar values.
pub const GROUP_NAME_MAX_CHARS: usize = 120;

/// Length of a group id: ten Crockford base32 characters, short enough for a 50-character
/// GitHub label (`hs-group:<id>`) and free of spaces for Jira (HS2-9KZSD4).
pub const GROUP_ID_LEN: usize = 10;

const CROCKFORD: &str = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Whether `value` is a group id: exactly [`GROUP_ID_LEN`] uppercase Crockford base32
/// characters (no `I`, `L`, `O`, or `U`).
pub fn is_group_id(value: &str) -> bool {
    value.len() == GROUP_ID_LEN && value.chars().all(|ch| CROCKFORD.contains(ch))
}

/// A new random group id. The characters are a ULID's random part, so two clones that
/// create groups at the same moment do not collide.
pub fn new_group_id() -> String {
    let ulid = ulid::Ulid::new().to_string();
    ulid[ulid.len() - GROUP_ID_LEN..].to_string()
}

/// The display name of a group id this project's registry does not name: a stable
/// placeholder built from the id, which a rename replaces (HS2-9KZSD4).
pub fn group_placeholder_name(id: &str) -> String {
    format!("Group {id}")
}

/// What a stored `group` value refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupValue<'a> {
    /// A registry id.
    Id(&'a str),
    /// A pre-registry display name, not yet migrated.
    Legacy(&'a str),
}

impl<'a> GroupValue<'a> {
    /// Classify a stored value.
    pub fn of(value: &'a str) -> Self {
        if is_group_id(value) {
            Self::Id(value)
        } else {
            Self::Legacy(value)
        }
    }
}

/// The comparison key of a stored value: the id itself, or a legacy name's
/// case-insensitive key. Two values with one key are the same group. A legacy key is
/// lower-cased, so it never equals an (upper-case) id: a legacy name that is itself
/// id-shaped is read as an id.
pub fn group_value_key(value: &str) -> String {
    match GroupValue::of(value) {
        GroupValue::Id(id) => id.to_string(),
        GroupValue::Legacy(name) => group_key(name),
    }
}

/// The value keys a typed group filter matches without a registry: the id itself when
/// the input is an id, and the legacy-name key of the input. A caller with the project's
/// registry adds the ids whose names match (HS2-9KZSD4). Empty when the input names no
/// group, so it matches nothing.
pub fn group_filter_keys(input: &str) -> Vec<String> {
    let trimmed = input.trim();
    let mut keys = Vec::new();
    if is_group_id(trimmed) {
        keys.push(trimmed.to_string());
    }
    keys.extend(group_key_of_input(input));
    keys
}

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
/// else (a list, a mapping, null) is treated as ungrouped. An id is kept exactly; any
/// other text is a legacy name and is normalized like one (HS2-9KZSD4).
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

    #[test]
    fn hs2_9kzsd4_ids_are_ten_crockford_characters() {
        for _ in 0..64 {
            let id = new_group_id();
            assert!(is_group_id(&id), "{id}");
            assert_eq!(GroupValue::of(&id), GroupValue::Id(&id));
            assert_eq!(group_value_key(&id), id);
        }
        assert_ne!(new_group_id(), new_group_id());
        assert!(is_group_id("7K3M9P2QXA"));
        assert!(is_group_id("0123456789"));
        for not_id in [
            "7k3m9p2qxa",  // lower case
            "7K3M9P2QX",   // too short
            "7K3M9P2QXAB", // too long
            "7K3M9P2QXI",  // I is not Crockford
            "7K3M9P2QXL",
            "7K3M9P2QXO",
            "7K3M9P2QXU",
            "Sprint",
            "Board drag",
            "",
        ] {
            assert!(!is_group_id(not_id), "{not_id}");
            assert_eq!(GroupValue::of(not_id), GroupValue::Legacy(not_id));
        }
    }

    #[test]
    fn hs2_9kzsd4_legacy_keys_never_equal_an_id_key() {
        assert_eq!(group_value_key("Sprint 12"), "sprint 12");
        assert_eq!(group_value_key("SPRINT 12"), group_value_key("sprint 12"));
        // A legacy key is lower case; an id is upper case. A legacy name that looks like an
        // id is read as that id, so the two key spaces never collide.
        let legacy = group_value_key("abcdefghjk");
        assert!(!is_group_id(&legacy));
        assert_eq!(group_placeholder_name("7K3M9P2QXA"), "Group 7K3M9P2QXA");
    }
}
