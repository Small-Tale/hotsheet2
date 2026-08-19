//! Ticket enumerations. Wire values match `docs/17-ticket-file-format.md`.

use serde::{Deserialize, Serialize};

/// One of five priority levels (highest → lowest).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    Highest,
    High,
    #[default]
    Default,
    Low,
    Lowest,
}

/// Workflow status. HS1's set is kept unchanged, plus the `moved` tombstone
/// (see `docs/02-ticket-storage.md` §2.13 and HS2-24).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    #[default]
    NotStarted,
    Started,
    Completed,
    Verified,
    Backlog,
    Archive,
    Deleted,
    /// A cross-store move tombstone/redirect (§2.13).
    Moved,
}

/// Why a ticket was closed. A **separate optional** field, orthogonal to `Status`
/// (HS2-24): setting it annotates a closure, it does not change the status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloseReason {
    Completed,
    NotPlanned,
    Duplicate,
    Obsolete,
}

/// The kind of a note (`docs/02` §2.6). `FeedbackDraft` is stored **locally**
/// (per-user overlay), the others are shared in the ticket file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum NoteKind {
    #[default]
    Regular,
    FeedbackNeeded,
    FeedbackDraft,
    Status,
}

/// The kind of a human review request (`docs/10` §10.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReviewKind {
    Work,
    Feedback,
    Review,
    Fyi,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn priority_wire_values_are_lowercase() {
        assert_eq!(
            serde_json::to_string(&Priority::Highest).unwrap(),
            "\"highest\""
        );
        assert_eq!(Priority::default(), Priority::Default);
    }

    #[test]
    fn status_wire_values_are_snake_case() {
        assert_eq!(
            serde_json::to_string(&Status::NotStarted).unwrap(),
            "\"not_started\""
        );
        assert_eq!(
            serde_json::from_str::<Status>("\"moved\"").unwrap(),
            Status::Moved
        );
    }

    #[test]
    fn close_reason_round_trips() {
        for r in [
            CloseReason::Completed,
            CloseReason::NotPlanned,
            CloseReason::Duplicate,
            CloseReason::Obsolete,
        ] {
            let s = serde_json::to_string(&r).unwrap();
            assert_eq!(serde_json::from_str::<CloseReason>(&s).unwrap(), r);
        }
    }
}
