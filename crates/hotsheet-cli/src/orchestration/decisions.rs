//! HS2-XHQ1EH: judgment calls on the orchestration ticket.
//!
//! When the orchestrator itself (an AI session or the scheduling loop) wants to do
//! something only a person should judge, such as dispatching work the plan says is
//! not ready, it does not file an approval. It adds a `FEEDBACK NEEDED` note with
//! `CHOICE` options to the orchestration (goal) ticket, as AI sessions already do, and
//! waits on the existing feedback flow: a person's response to that note decides.
//! One note per operation; asking again reads the response instead of adding a note.
use hotsheet_model::NoteKind;
use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::ops;
use hotsheet_ticketing::orchestration::{DecisionAsked, RunTicket};

use super::tickets::timestamp;
use super::*;

/// Where a judgment call stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Decided {
    /// The note waits for a person's response.
    Waiting { note_id: String },
    /// A person's response names this option (its index in the options asked).
    Chosen { option: usize, note_id: String },
}

/// One judgment call to ask.
pub(super) struct Judgment<'a> {
    /// A stable code, such as `readiness_override`.
    pub kind: &'a str,
    pub target: &'a TargetRef,
    pub operation_id: &'a str,
    /// The orchestration ticket the note goes on.
    pub ticket: &'a RunTicket,
    /// Identifiers and codes only, never ticket content.
    pub context: String,
    pub question: String,
    /// The options, each a distinct phrase none of the others contains.
    pub options: &'a [&'a str],
}

impl OrchestrationRuntime {
    /// Ask a judgment call once per operation, or read the answer to the one asked.
    pub(super) fn judgment(
        &self,
        owner_id: &str,
        judgment: &Judgment<'_>,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<Decided, OrchestrationError> {
        let ledger = self.permission_ledger();
        let ticket = OrchestrationTicket {
            connection_id: judgment.ticket.connection_id.clone(),
            native_id: judgment.ticket.native_id.clone(),
        };
        let (store, id, connection_id) = self.orchestration_store(owner_id, &ticket)?;
        if let Some(asked) =
            ledger.decision(judgment.target, judgment.kind, judgment.operation_id)?
        {
            let note =
                ulid::Ulid::from_string(&asked.note_id).map_err(|_| OrchestrationError::Corrupt)?;
            let read = store.read_ticket(&id).map_err(|_| {
                OrchestrationError::TargetOperation(
                    hotsheet_ticketing::orchestration::TargetOpError::NotFound,
                )
            })?;
            // The newest response to the note decides.
            let chosen = read
                .notes
                .iter()
                .rev()
                .filter(|candidate| candidate.response_to == Some(note))
                .find_map(|response| {
                    let text = response.text.to_lowercase();
                    judgment
                        .options
                        .iter()
                        .position(|option| text.contains(&option.to_lowercase()))
                });
            return Ok(match chosen {
                Some(option) => Decided::Chosen {
                    option,
                    note_id: asked.note_id,
                },
                None => Decided::Waiting {
                    note_id: asked.note_id,
                },
            });
        }
        let note_id = hotsheet_ticketing::mint_ulid(tx.clock, tx.rng);
        let mut text = format!("{}\n\nCHOICE: {}\n", judgment.context, judgment.question);
        for option in judgment.options {
            text.push_str(&format!("- {option}\n"));
        }
        ops::add_note_with_metadata(
            &store,
            &id,
            note_id,
            timestamp(tx),
            NoteKind::FeedbackNeeded,
            ops::NoteMetadataInput {
                kind_explicit: true,
                summary: Some(format!("Decision needed: {}", judgment.question)),
                actor: actor.map(MutationActor::note_actor),
                ..ops::NoteMetadataInput::default()
            },
            text,
        )
        .map_err(|_| {
            OrchestrationError::TargetOperation(
                hotsheet_ticketing::orchestration::TargetOpError::Failed,
            )
        })?;
        let recorded = ledger.record_decision(DecisionAsked {
            decision_id: hotsheet_ticketing::mint_ulid(tx.clock, tx.rng).to_string(),
            target: judgment.target.clone(),
            kind: judgment.kind.into(),
            operation_id: judgment.operation_id.into(),
            connection_id,
            native_id: id.to_string(),
            note_id: note_id.to_string(),
            actor_id: actor
                .and_then(|actor| actor.id.clone())
                .unwrap_or_else(|| "orchestrator".into()),
            asked_at_ms: tx.clock.now_ms(),
        })?;
        Ok(Decided::Waiting {
            note_id: recorded.note_id,
        })
    }
}
