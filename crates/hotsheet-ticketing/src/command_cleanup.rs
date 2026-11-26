//! Auto-cleanup of AI command tickets after completion (HS2-27C161).
//!
//! An AI command's `cleanup` option archives its tickets, or moves them to Trash, as soon as
//! they reach Completed or Verified, whichever comes first. Cleanup appends a system note,
//! and a ticket carrying that note is never cleaned up again, so restoring it from Archive or
//! Trash keeps it. This module holds the provider-neutral decision; the server applies it.

use crate::command_schedules::COMMAND_TAG_PREFIX;
use hotsheet_model::Status;
use serde::{Deserialize, Serialize};

/// What happens to a command's tickets once they are completed. `None` (the default) is
/// stored as an absent field.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CommandCleanup {
    #[default]
    None,
    Archive,
    /// Move to Trash: the restorable delete every provider's Trash already implements.
    Delete,
}

impl CommandCleanup {
    pub fn is_none(&self) -> bool {
        *self == Self::None
    }

    /// The status cleanup moves a ticket to, or `None` when it leaves tickets alone.
    pub fn target_status(self) -> Option<Status> {
        match self {
            Self::None => None,
            Self::Archive => Some(Status::Archive),
            Self::Delete => Some(Status::Deleted),
        }
    }

    /// The system note cleanup appends, which also marks the ticket as cleaned up.
    pub fn note(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::Archive => Some(
                "Auto-Cleanup archived this command ticket after it was completed. It stays in Archive; restoring it keeps it.",
            ),
            Self::Delete => Some(
                "Auto-Cleanup moved this command ticket to Trash after it was completed. Restore it from Trash to keep it; it will not be cleaned up again.",
            ),
        }
    }
}

/// The actor id on every cleanup note.
pub const CLEANUP_ACTOR_ID: &str = "hotsheet-command-cleanup";
/// The timeline headline on every cleanup note.
pub const CLEANUP_NOTE_SUMMARY: &str = "Command auto-cleanup";
/// Every cleanup note's text starts with this, so a provider that drops note metadata still
/// recognizes it.
pub const CLEANUP_NOTE_PREFIX: &str = "Auto-Cleanup ";

/// The command a ticket came from: its first `hs2-command:<id>` provenance tag.
pub fn command_id_from_tags(tags: &[String]) -> Option<&str> {
    tags.iter()
        .find_map(|tag| tag.strip_prefix(COMMAND_TAG_PREFIX))
        .filter(|id| !id.is_empty())
}

/// Whether a note is a cleanup note, by its actor id, its headline, or its text.
pub fn is_cleanup_note(actor_id: Option<&str>, summary: Option<&str>, text: &str) -> bool {
    actor_id == Some(CLEANUP_ACTOR_ID)
        || summary == Some(CLEANUP_NOTE_SUMMARY)
        || text.trim_start().starts_with(CLEANUP_NOTE_PREFIX)
}

/// Completed and Verified are the statuses that trigger cleanup.
pub fn is_cleanup_trigger(status: Status) -> bool {
    matches!(status, Status::Completed | Status::Verified)
}

/// The command field recording when its cleanup was turned on (HS2-27C161). The settings
/// writer owns it: turning cleanup on from None stamps it, switching between Archive and
/// Delete keeps it, and turning it off removes it.
pub const CLEANUP_SINCE_FIELD: &str = "cleanup_since";

/// A command's cleanup and when it was turned on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CleanupPolicy {
    pub cleanup: CommandCleanup,
    pub since: Option<String>,
}

/// Parse an RFC 3339 instant; anything else is `None`.
pub fn parse_instant(raw: &str) -> Option<time::OffsetDateTime> {
    time::OffsetDateTime::parse(raw.trim(), &time::format_description::well_known::Rfc3339).ok()
}

/// The current instant as RFC 3339 UTC, the form `cleanup_since` is stored in.
pub fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

/// When a ticket reached Completed or Verified: its completion time, or for a ticket
/// verified without one, its verification time. Missing or unparseable is `None`.
pub fn reached_done_at(
    completed_at: Option<&str>,
    verified_at: Option<&str>,
) -> Option<time::OffsetDateTime> {
    parse_instant(completed_at.or(verified_at)?)
}

/// The cleanup a ticket is due, if any: it is completed or verified, came from a command
/// whose policy is not `None`, reached that status at or after the policy's `since`, and
/// was never cleaned up before. A missing or unparseable time on either side cleans nothing.
pub fn cleanup_due(
    status: Status,
    tags: &[String],
    done_at: Option<time::OffsetDateTime>,
    already_cleaned: bool,
    policy: impl FnOnce(&str) -> CleanupPolicy,
) -> Option<(String, CommandCleanup)> {
    if !is_cleanup_trigger(status) || already_cleaned {
        return None;
    }
    let command_id = command_id_from_tags(tags)?;
    let policy = policy(command_id);
    if policy.cleanup.is_none() {
        return None;
    }
    let since = parse_instant(policy.since.as_deref()?)?;
    (done_at? >= since).then(|| (command_id.to_owned(), policy.cleanup))
}

fn cleanup_enabled(command: &serde_json::Value) -> bool {
    matches!(
        command.get("cleanup").and_then(serde_json::Value::as_str),
        Some("archive" | "delete")
    )
}

fn valid_since(command: &serde_json::Value) -> Option<&str> {
    command
        .get(CLEANUP_SINCE_FIELD)
        .and_then(serde_json::Value::as_str)
        .filter(|raw| parse_instant(raw).is_some())
}

/// Whether any command in a `commands` value has cleanup on without a valid `cleanup_since`.
pub fn needs_cleanup_since(commands: &serde_json::Value) -> bool {
    commands.as_array().is_some_and(|items| {
        items
            .iter()
            .any(|command| cleanup_enabled(command) && valid_since(command).is_none())
    })
}

/// Stamp `cleanup_since` on a `commands` value about to be written, given the value it
/// replaces. A command whose cleanup was already on keeps its earlier stamp; one turned on
/// now (or carrying no valid stamp) gets `now`; a command with cleanup off loses the field.
/// Other fields are left exactly as written.
pub fn stamp_cleanup_since(
    prior: Option<&serde_json::Value>,
    next: &mut serde_json::Value,
    now: &str,
) {
    let Some(items) = next.as_array_mut() else {
        return;
    };
    let prior_since = |id: &str| -> Option<String> {
        prior?
            .as_array()?
            .iter()
            .find(|command| command.get("id").and_then(serde_json::Value::as_str) == Some(id))
            .filter(|command| cleanup_enabled(command))
            .and_then(valid_since)
            .map(str::to_owned)
    };
    for command in items {
        let enabled = cleanup_enabled(command);
        let id = command
            .get("id")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        let Some(object) = command.as_object_mut() else {
            continue;
        };
        if !enabled {
            object.remove(CLEANUP_SINCE_FIELD);
            continue;
        }
        let since = id
            .as_deref()
            .and_then(prior_since)
            .unwrap_or_else(|| now.to_owned());
        object.insert(CLEANUP_SINCE_FIELD.into(), since.into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn hs2_27c161_cleanup_serializes_snake_case_and_defaults_to_none() {
        assert_eq!(CommandCleanup::default(), CommandCleanup::None);
        for (value, raw) in [
            (CommandCleanup::None, "none"),
            (CommandCleanup::Archive, "archive"),
            (CommandCleanup::Delete, "delete"),
        ] {
            assert_eq!(serde_json::to_value(value).unwrap(), raw);
            assert_eq!(
                serde_json::from_value::<CommandCleanup>(serde_json::json!(raw)).unwrap(),
                value
            );
        }
        assert!(serde_json::from_value::<CommandCleanup>(serde_json::json!("purge")).is_err());
    }

    #[test]
    fn hs2_27c161_targets_archive_or_trash_and_never_a_permanent_delete() {
        assert_eq!(CommandCleanup::None.target_status(), None);
        assert_eq!(
            CommandCleanup::Archive.target_status(),
            Some(Status::Archive)
        );
        assert_eq!(
            CommandCleanup::Delete.target_status(),
            Some(Status::Deleted)
        );
        assert_eq!(CommandCleanup::None.note(), None);
        for cleanup in [CommandCleanup::Archive, CommandCleanup::Delete] {
            let note = cleanup.note().unwrap();
            assert!(note.starts_with(CLEANUP_NOTE_PREFIX));
            assert!(is_cleanup_note(None, None, note));
        }
    }

    #[test]
    fn hs2_27c161_reads_the_command_provenance_tag() {
        assert_eq!(
            command_id_from_tags(&tags(&["bug", "hs2-command:nightly", "hs2-occurrence:x"])),
            Some("nightly")
        );
        assert_eq!(command_id_from_tags(&tags(&["bug"])), None);
        assert_eq!(command_id_from_tags(&tags(&["hs2-command:"])), None);
    }

    #[test]
    fn hs2_27c161_recognizes_cleanup_notes_by_actor_summary_or_text() {
        assert!(is_cleanup_note(Some(CLEANUP_ACTOR_ID), None, "anything"));
        assert!(is_cleanup_note(
            None,
            Some(CLEANUP_NOTE_SUMMARY),
            "anything"
        ));
        assert!(is_cleanup_note(None, None, "  Auto-Cleanup moved it"));
        assert!(!is_cleanup_note(
            Some("someone"),
            Some("Done"),
            "Finished the review."
        ));
    }

    /// Every status against every policy, plus the already-cleaned guard.
    #[test]
    fn hs2_27c161_cleanup_is_due_only_for_done_uncleaned_command_tickets() {
        let command = tags(&["hs2-command:nightly"]);
        let since = "2026-11-26T08:00:00Z";
        let after = reached_done_at(Some("2026-11-26T09:00:00Z"), None);
        let policy = |cleanup| CleanupPolicy {
            cleanup,
            since: Some(since.into()),
        };
        let statuses = [
            Status::NotStarted,
            Status::Started,
            Status::Completed,
            Status::Verified,
            Status::Backlog,
            Status::Archive,
            Status::Deleted,
            Status::Moved,
        ];
        for status in statuses {
            for cleanup in [
                CommandCleanup::None,
                CommandCleanup::Archive,
                CommandCleanup::Delete,
            ] {
                let due = cleanup_due(status, &command, after, false, |id| {
                    assert_eq!(id, "nightly");
                    policy(cleanup)
                });
                let expected = (matches!(status, Status::Completed | Status::Verified)
                    && !cleanup.is_none())
                .then(|| ("nightly".to_owned(), cleanup));
                assert_eq!(due, expected, "{status:?} {cleanup:?}");
                assert_eq!(
                    cleanup_due(status, &command, after, true, |_| policy(cleanup)),
                    None,
                    "an already cleaned ticket is kept"
                );
            }
        }
        assert_eq!(
            cleanup_due(Status::Completed, &tags(&["bug"]), after, false, |_| {
                policy(CommandCleanup::Delete)
            }),
            None,
            "a ticket without command provenance is never cleaned up"
        );
    }

    /// Only completions at or after `cleanup_since` count; a missing or unparseable time on
    /// either side cleans nothing.
    #[test]
    fn hs2_27c161_cleanup_counts_only_completions_since_it_was_turned_on() {
        let command = tags(&["hs2-command:nightly"]);
        let due = |done: Option<time::OffsetDateTime>, since: Option<&str>| {
            cleanup_due(Status::Completed, &command, done, false, |_| {
                CleanupPolicy {
                    cleanup: CommandCleanup::Archive,
                    since: since.map(str::to_owned),
                }
            })
            .is_some()
        };
        let since = Some("2026-11-26T08:00:00Z");
        assert!(!due(
            reached_done_at(Some("2026-11-26T07:59:59Z"), None),
            since
        ));
        assert!(due(
            reached_done_at(Some("2026-11-26T08:00:00Z"), None),
            since
        ));
        assert!(due(
            reached_done_at(Some("2026-11-26T08:00:00.5+00:00"), None),
            since
        ));
        // A ticket verified without a completion time uses the verification time.
        assert!(due(
            reached_done_at(None, Some("2026-11-26T09:00:00Z")),
            since
        ));
        // Completion time wins over a later verification.
        assert!(!due(
            reached_done_at(Some("2026-11-25T09:00:00Z"), Some("2026-11-27T09:00:00Z")),
            since
        ));
        assert!(!due(reached_done_at(None, None), since));
        assert!(!due(reached_done_at(Some("yesterday"), None), since));
        let later = reached_done_at(Some("2026-11-27T00:00:00Z"), None);
        assert!(!due(later, None));
        assert!(!due(later, Some("not a time")));
    }

    fn since_of(value: &serde_json::Value, index: usize) -> Option<&str> {
        value[index][CLEANUP_SINCE_FIELD].as_str()
    }

    /// Turning cleanup on stamps it, Archive ⇄ Delete keeps it, None drops it, and turning
    /// it on again restamps; other fields and commands are untouched.
    #[test]
    fn hs2_27c161_settings_writer_stamps_cleanup_since() {
        let command = |cleanup: Option<&str>, since: Option<&str>| {
            let mut value = serde_json::json!({"id": "nightly", "title": "Nightly", "kind": "ai",
                "prompt": "Review", "future_field": 7});
            if let Some(cleanup) = cleanup {
                value["cleanup"] = cleanup.into();
            }
            if let Some(since) = since {
                value[CLEANUP_SINCE_FIELD] = since.into();
            }
            serde_json::json!([value, {"id": "lint", "title": "Lint", "kind": "shell", "command": "true"}])
        };
        let t1 = "2026-11-26T08:00:00Z";
        let t2 = "2026-11-27T08:00:00Z";
        // None -> Archive stamps now, ignoring any incoming stamp.
        let mut next = command(Some("archive"), Some("2020-01-01T00:00:00Z"));
        stamp_cleanup_since(Some(&command(None, None)), &mut next, t1);
        assert_eq!(since_of(&next, 0), Some(t1));
        assert_eq!(next[0]["future_field"], 7);
        assert!(next[1].get(CLEANUP_SINCE_FIELD).is_none());
        // Archive -> Delete keeps it, even when the writer omits or changes it.
        let archived = next.clone();
        for incoming in [None, Some(t2)] {
            let mut next = command(Some("delete"), incoming);
            stamp_cleanup_since(Some(&archived), &mut next, t2);
            assert_eq!(since_of(&next, 0), Some(t1));
        }
        // Delete -> None removes it; None -> Archive again restamps.
        let mut off = command(Some("none"), Some(t1));
        stamp_cleanup_since(Some(&archived), &mut off, t2);
        assert!(off[0].get(CLEANUP_SINCE_FIELD).is_none());
        let mut again = command(Some("archive"), None);
        stamp_cleanup_since(Some(&off), &mut again, t2);
        assert_eq!(since_of(&again, 0), Some(t2));
        // A first write, and an enabled command stored without a stamp, get now.
        let mut first = command(Some("delete"), None);
        stamp_cleanup_since(None, &mut first, t2);
        assert_eq!(since_of(&first, 0), Some(t2));
        let legacy = command(Some("archive"), None);
        assert!(needs_cleanup_since(&legacy));
        let mut stamped = legacy.clone();
        stamp_cleanup_since(Some(&legacy), &mut stamped, t2);
        assert_eq!(since_of(&stamped, 0), Some(t2));
        assert!(!needs_cleanup_since(&stamped));
    }
}
