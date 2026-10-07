//! The actor behind a mutation and the rules that depend on its role (HS2-RD4M29).
//!
//! Every mutating surface (CLI, server, MCP) may say who is acting: a `human`, an `ai`, or
//! a `system` process, plus an optional stable id (a worker id, account, or tool name). A
//! missing actor is *unspecified*, never assumed to be an AI, so callers that predate the
//! field keep working unchanged.
//!
//! Rules are role-specific and their feedback is written for the actor that will read it:
//! an AI gets a machine-actionable code plus the exact retry, while humans are never held to
//! AI-only obligations such as scoring a completion.

use hotsheet_model::{AttachmentActorRole, CloseReason, Status, Ticket};
use serde::{Deserialize, Serialize};

/// The role vocabulary shared with attachment provenance (`human | ai | system`).
pub type ActorRole = AttachmentActorRole;

/// Stable error code for an AI completion without a confidence score.
pub const CONFIDENCE_REQUIRED: &str = "confidence_required";

/// Who is performing a mutation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MutationActor {
    pub role: ActorRole,
    /// Stable identity where one is available (worker id, account, or tool name).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

impl MutationActor {
    /// Parse a role name (`human`, `ai`, or `system`) and an optional id. Blank ids are
    /// dropped; an unrecognized role is an explicit error rather than a silent "unknown".
    pub fn parse(role: &str, id: Option<String>) -> Result<Self, String> {
        let role = match role.trim().to_ascii_lowercase().as_str() {
            "human" => ActorRole::Human,
            "ai" => ActorRole::Ai,
            "system" => ActorRole::System,
            other => {
                return Err(format!(
                    "invalid actor role '{other}': expected human, ai, or system"
                ));
            }
        };
        Ok(Self {
            role,
            id: id
                .map(|id| id.trim().to_owned())
                .filter(|id| !id.is_empty()),
        })
    }

    #[must_use]
    pub fn is_ai(&self) -> bool {
        self.role == ActorRole::Ai
    }

    /// The durable authorship recorded on the notes this actor writes (HS2-32QDZ3).
    #[must_use]
    pub fn note_actor(&self) -> hotsheet_model::NoteActor {
        hotsheet_model::NoteActor {
            role: self.role,
            id: self.id.clone(),
        }
    }
}

/// The optional note authorship for an optional actor.
#[must_use]
pub fn note_actor(actor: Option<&MutationActor>) -> Option<hotsheet_model::NoteActor> {
    actor.map(MutationActor::note_actor)
}

/// A role-specific rule rejected the mutation before anything was written.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct RuleViolation {
    /// Stable, machine-readable code (for example [`CONFIDENCE_REQUIRED`]).
    pub code: &'static str,
    pub message: String,
}

/// Whether moving from `current` to `next` marks the ticket completed.
#[must_use]
pub fn completes(current: Status, next: Option<Status>) -> bool {
    next == Some(Status::Completed) && current != Status::Completed
}

/// Whether closing with `reason` from `current` marks the ticket completed. Only a
/// `completed` close reports finished work; not-planned/duplicate/obsolete closes do not.
#[must_use]
pub fn close_completes(current: Status, reason: CloseReason) -> bool {
    reason == CloseReason::Completed && current.is_active()
}

/// Whether the current completion cycle already has a scored note, so completing now
/// would yield a derived `latest_confidence`.
#[must_use]
pub fn scored_in_current_cycle(ticket: &Ticket) -> bool {
    let mut completed = ticket.clone();
    completed.status = Status::Completed;
    crate::ops::latest_confidence(&completed).is_some()
}

/// [`scored_in_current_cycle`] for a provider's wire ticket (any provider route). An external
/// tracker's comment list has no reopen activity notes (its reopen history lives in native
/// events, HS2-N3RMTV), so before a completion any scored comment counts.
#[must_use]
pub fn api_scored_in_current_cycle(ticket: &crate::wire::ApiTicket) -> bool {
    let notes = ticket
        .notes
        .iter()
        .map(|note| hotsheet_model::Note {
            id: hotsheet_model::Ulid::from_string(&note.id).unwrap_or_default(),
            kind: note.kind,
            created_at: hotsheet_model::Timestamp::new(&note.created_at),
            edited_at: hotsheet_model::Timestamp::new(&note.edited_at),
            summary: note.summary.clone(),
            confidence: note
                .confidence
                .and_then(|score| hotsheet_model::Confidence::new(u64::from(score)).ok()),
            feedback_for: None,
            actor: None,
            text: note.text.clone(),
        })
        .collect::<Vec<_>>();
    crate::ops::latest_confidence_of_notes(Status::Completed, &notes).is_some()
}

/// The AI completion rule: an `ai` actor that completes a ticket must record a completion
/// confidence score (in the same request, or earlier in the current completion cycle).
/// Humans, `system` actors, and unspecified callers are never required to score.
pub fn check_completion(
    actor: Option<&MutationActor>,
    slug: &str,
    completes: bool,
    scored: bool,
) -> Result<(), RuleViolation> {
    if !completes || scored || !actor.is_some_and(MutationActor::is_ai) {
        return Ok(());
    }
    Err(RuleViolation {
        code: CONFIDENCE_REQUIRED,
        message: ai_confidence_required_message(slug),
    })
}

/// Feedback written for an AI reader: lead with the stable code, then the exact retry for
/// each surface and the rubric it needs, so one retry succeeds.
#[must_use]
pub fn ai_confidence_required_message(slug: &str) -> String {
    format!(
        "{CONFIDENCE_REQUIRED}: an AI actor cannot complete {slug} without a completion \
         confidence score. Nothing was changed. Retry the same request with a completing note \
         and its score: CLI `hotsheet-cli edit {slug} --status completed --note-file <note.md> \
         --note-confidence <0-100>`; MCP/HTTP `{{\"status\":\"completed\",\"note\":\"...\",\
         \"note_confidence\":<0-100>}}`. The note needs a `## Confidence` section: the score, \
         then one line per factor rated high/medium/low (clarity of the request; context \
         available; verification actually run; scope deviation or assumptions; known gaps). \
         Bands: 90-100 fully verified end to end; 70-89 verified with minor assumptions; 40-69 \
         partially verified; below 40 largely unverified."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::{self, NoteMetadataInput, TicketPatch};
    use crate::{FsStore, NewTicket, StoreMetadata};
    use hotsheet_model::{Confidence, NoteKind, Timestamp, Ulid};

    fn ai() -> MutationActor {
        MutationActor::parse("AI", Some(" codex-1 ".into())).unwrap()
    }

    #[test]
    fn parses_roles_and_rejects_unknown_ones() {
        assert_eq!(ai().role, ActorRole::Ai);
        assert_eq!(ai().id.as_deref(), Some("codex-1"));
        assert_eq!(
            MutationActor::parse("human", Some("  ".into())).unwrap(),
            MutationActor {
                role: ActorRole::Human,
                id: None
            }
        );
        assert_eq!(
            MutationActor::parse("system", None).unwrap().role,
            ActorRole::System
        );
        assert!(
            MutationActor::parse("robot", None)
                .unwrap_err()
                .contains("expected human, ai, or system")
        );
    }

    #[test]
    fn only_an_unscored_ai_completion_is_rejected() {
        let human = MutationActor::parse("human", None).unwrap();
        let system = MutationActor::parse("system", None).unwrap();
        // The transition matrix: role × completes × scored.
        for actor in [None, Some(&human), Some(&system), Some(&ai())] {
            for completes in [false, true] {
                for scored in [false, true] {
                    let result = check_completion(actor, "HS-1", completes, scored);
                    let rejected = actor.is_some_and(MutationActor::is_ai) && completes && !scored;
                    assert_eq!(result.is_err(), rejected, "{actor:?} {completes} {scored}");
                }
            }
        }
        let error = check_completion(Some(&ai()), "HS-1", true, false).unwrap_err();
        assert_eq!(error.code, CONFIDENCE_REQUIRED);
        assert!(error.message.starts_with("confidence_required: "));
        assert!(error.message.contains("--note-confidence <0-100>"));
        assert!(error.message.contains("\"note_confidence\":<0-100>"));
        assert!(error.message.contains("## Confidence"));
        assert!(error.message.contains("Nothing was changed"));
    }

    #[test]
    fn completion_transitions_and_cycle_scores() {
        assert!(completes(Status::Started, Some(Status::Completed)));
        assert!(completes(Status::Verified, Some(Status::Completed)));
        assert!(!completes(Status::Completed, Some(Status::Completed)));
        assert!(!completes(Status::Started, Some(Status::Verified)));
        assert!(!completes(Status::Started, None));
        assert!(close_completes(Status::Started, CloseReason::Completed));
        assert!(!close_completes(Status::Started, CloseReason::NotPlanned));
        assert!(!close_completes(Status::Completed, CloseReason::Completed));

        let dir = tempfile::tempdir().unwrap();
        let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
        let id = Ulid::new();
        let at = |s: &str| Timestamp::new(s);
        ops::create(
            &store,
            id,
            "HS",
            at("2026-09-01T00:00:00Z"),
            NewTicket {
                title: "scored".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let read = || store.read_ticket(&id).unwrap();
        assert!(!scored_in_current_cycle(&read()));
        // A score written before the completion flip counts for this cycle...
        ops::add_note_with_metadata(
            &store,
            &id,
            Ulid::new(),
            at("2026-09-01T00:01:00Z"),
            NoteKind::Regular,
            NoteMetadataInput {
                summary: None,
                confidence: Some(Confidence::new(80).unwrap()),
                actor: None,
            },
            "## Confidence\n80".into(),
        )
        .unwrap();
        assert!(scored_in_current_cycle(&read()));
        // ...but not after a completion and reopen start a new cycle.
        for status in [Status::Completed, Status::Started] {
            ops::update(
                &store,
                &id,
                at("2026-09-01T00:02:00Z"),
                TicketPatch {
                    status: Some(status),
                    ..Default::default()
                },
            )
            .unwrap();
        }
        assert!(!scored_in_current_cycle(&read()));
    }
}
