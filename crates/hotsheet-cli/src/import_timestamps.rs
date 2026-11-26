//! Resolve an HS1 export ticket's timestamps into valid HS2 timestamps (HS2-K7JG39).
//!
//! `Timestamp::new` never fails, so an export value that is null, empty, or not
//! RFC 3339 would otherwise be written verbatim (an empty `created_at` breaks
//! sorting, age, and every reader that needs an instant). A missing creation time
//! falls back to the earliest valid time recorded on the ticket, since creation
//! precedes every other event, and to the import time only when nothing else is
//! known. Malformed optional timestamps are dropped. Every substitution produces a
//! warning naming the HS1 ticket and field so the migration report records it.

use hotsheet_model::Timestamp;

use crate::import::ExportTicket;

/// The ticket's timestamps after validation, plus one warning per substitution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedTimestamps {
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub completed_at: Option<Timestamp>,
    pub verified_at: Option<Timestamp>,
    pub deleted_at: Option<Timestamp>,
    /// One entry per export note, in order; `None` keeps the note's id-derived time.
    pub note_created_at: Vec<Option<Timestamp>>,
    pub warnings: Vec<String>,
}

/// Validate `ticket`'s timestamps. `label` names the ticket in warnings (for example
/// `ticket HS-12`); `imported_at` is the last-resort creation time.
pub(crate) fn resolve(
    ticket: &ExportTicket,
    label: &str,
    imported_at: &Timestamp,
) -> ResolvedTimestamps {
    let mut warnings = Vec::new();
    let mut field = |name: &str, raw: Option<&String>| -> Option<Timestamp> {
        let raw = raw?;
        let value = Timestamp::new(raw.as_str());
        if value.instant().is_some() {
            return Some(value);
        }
        warnings.push(format!(
            "warning: HS1 {label} {name} is not an RFC 3339 timestamp; it was not imported"
        ));
        None
    };
    let completed_at = field("completed_at", ticket.completed_at.as_ref());
    let verified_at = field("verified_at", ticket.verified_at.as_ref());
    let deleted_at = field("deleted_at", ticket.deleted_at.as_ref());
    let note_created_at: Vec<Option<Timestamp>> = ticket
        .notes
        .iter()
        .map(|note| field("note created_at", note.created_at.as_ref()))
        .collect();
    let updated_source = field("updated_at", ticket.updated_at.as_ref());
    let created_source = field("created_at", ticket.created_at.as_ref());

    let created_at = match created_source {
        Some(created) => created,
        None => {
            let candidates = [
                ("updated_at", updated_source.as_ref()),
                ("completed_at", completed_at.as_ref()),
                ("verified_at", verified_at.as_ref()),
                ("deleted_at", deleted_at.as_ref()),
            ]
            .into_iter()
            .chain(
                note_created_at
                    .iter()
                    .map(|note| ("a note's created_at", note.as_ref())),
            );
            let earliest = candidates
                .filter_map(|(name, value)| value.map(|value| (name, value)))
                .min_by_key(|(_, value)| value.instant());
            let (source, created) = match earliest {
                Some((name, value)) => (
                    format!("its earliest recorded time ({name})"),
                    value.clone(),
                ),
                None => ("the import time".to_string(), imported_at.clone()),
            };
            let problem = if ticket.created_at.is_none() {
                "has no created_at"
            } else {
                "has an unusable created_at"
            };
            warnings.push(format!(
                "warning: HS1 {label} {problem}; its creation time was set to {source}"
            ));
            created
        }
    };
    let updated_at = updated_source.unwrap_or_else(|| created_at.clone());

    ResolvedTimestamps {
        created_at,
        updated_at,
        completed_at,
        verified_at,
        deleted_at,
        note_created_at,
        warnings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::ExportTicket;

    const IMPORTED_AT: &str = "2026-11-27T12:00:00Z";

    fn ticket(json: &str) -> ExportTicket {
        serde_json::from_str(json).unwrap()
    }

    fn run(json: &str) -> ResolvedTimestamps {
        resolve(&ticket(json), "ticket HS-7", &Timestamp::new(IMPORTED_AT))
    }

    #[test]
    fn hs2_k7jg39_valid_timestamps_pass_through_verbatim_without_warnings() {
        let resolved = run(r#"{
          "title": "t", "created_at": "2026-08-01T00:00:00.000Z",
          "updated_at": "2026-08-02T00:00:00Z", "completed_at": "2026-08-03T00:00:00Z",
          "verified_at": null, "deleted_at": null,
          "notes": [{ "text": "n", "created_at": "2026-08-01T01:00:00Z" }, { "text": "m" }]
        }"#);
        assert_eq!(resolved.created_at.as_str(), "2026-08-01T00:00:00.000Z");
        assert_eq!(resolved.updated_at.as_str(), "2026-08-02T00:00:00Z");
        assert_eq!(
            resolved.completed_at.as_ref().map(Timestamp::as_str),
            Some("2026-08-03T00:00:00Z")
        );
        assert_eq!(resolved.verified_at, None);
        assert_eq!(resolved.deleted_at, None);
        assert_eq!(
            resolved.note_created_at,
            vec![Some(Timestamp::new("2026-08-01T01:00:00Z")), None]
        );
        assert!(resolved.warnings.is_empty());
    }

    #[test]
    fn hs2_k7jg39_null_created_at_uses_the_earliest_recorded_time() {
        let resolved = run(r#"{
          "title": "t", "created_at": null,
          "updated_at": "2026-08-05T00:00:00Z", "completed_at": "2026-08-04T00:00:00Z",
          "verified_at": "2026-08-06T00:00:00Z", "deleted_at": null,
          "notes": [{ "text": "n", "created_at": "2026-08-03T00:00:00Z" }]
        }"#);
        assert_eq!(resolved.created_at.as_str(), "2026-08-03T00:00:00Z");
        assert_eq!(resolved.updated_at.as_str(), "2026-08-05T00:00:00Z");
        assert_eq!(
            resolved.warnings,
            vec![
                "warning: HS1 ticket HS-7 has no created_at; its creation time was set to its earliest recorded time (a note's created_at)"
            ]
        );
    }

    #[test]
    fn hs2_k7jg39_null_created_and_updated_use_a_close_time_for_both() {
        let resolved = run(r#"{
          "title": "t", "created_at": null, "updated_at": null,
          "completed_at": null, "verified_at": null, "deleted_at": "2026-08-09T00:00:00Z"
        }"#);
        assert_eq!(resolved.created_at.as_str(), "2026-08-09T00:00:00Z");
        assert_eq!(resolved.updated_at.as_str(), "2026-08-09T00:00:00Z");
        assert_eq!(resolved.warnings.len(), 1);
        assert!(resolved.warnings[0].contains("(deleted_at)"));
    }

    #[test]
    fn hs2_k7jg39_no_recorded_time_falls_back_to_the_import_time() {
        // `created_at` key absent entirely, as from a schema without the column.
        let resolved = run(r#"{ "title": "t", "updated_at": null, "completed_at": null,
          "verified_at": null, "deleted_at": null }"#);
        assert_eq!(resolved.created_at.as_str(), IMPORTED_AT);
        assert_eq!(resolved.updated_at.as_str(), IMPORTED_AT);
        assert_eq!(
            resolved.warnings,
            vec![
                "warning: HS1 ticket HS-7 has no created_at; its creation time was set to the import time"
            ]
        );
    }

    #[test]
    fn hs2_k7jg39_empty_or_malformed_values_are_never_written_verbatim() {
        let resolved = run(r#"{
          "title": "t", "created_at": "", "updated_at": "yesterday",
          "completed_at": "2026-13-45", "verified_at": null, "deleted_at": null,
          "notes": [{ "text": "n", "created_at": "" }]
        }"#);
        assert_eq!(resolved.created_at.as_str(), IMPORTED_AT);
        assert_eq!(resolved.updated_at.as_str(), IMPORTED_AT);
        assert_eq!(resolved.completed_at, None);
        assert_eq!(resolved.note_created_at, vec![None]);
        assert_eq!(
            resolved.warnings,
            vec![
                "warning: HS1 ticket HS-7 completed_at is not an RFC 3339 timestamp; it was not imported",
                "warning: HS1 ticket HS-7 note created_at is not an RFC 3339 timestamp; it was not imported",
                "warning: HS1 ticket HS-7 updated_at is not an RFC 3339 timestamp; it was not imported",
                "warning: HS1 ticket HS-7 created_at is not an RFC 3339 timestamp; it was not imported",
                "warning: HS1 ticket HS-7 has an unusable created_at; its creation time was set to the import time",
            ]
        );
    }

    #[test]
    fn hs2_k7jg39_earliest_compares_instants_not_text() {
        // An offset makes the lexically larger text the earlier instant.
        let resolved = run(r#"{
          "title": "t", "created_at": null,
          "updated_at": "2026-08-02T00:00:00Z", "completed_at": "2026-08-02T09:00:00+10:00",
          "verified_at": null, "deleted_at": null
        }"#);
        assert_eq!(resolved.created_at.as_str(), "2026-08-02T09:00:00+10:00");
        assert!(resolved.warnings[0].contains("(completed_at)"));
    }
}
