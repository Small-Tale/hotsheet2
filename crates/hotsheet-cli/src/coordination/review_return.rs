//! Exact, replayable review returns without changing the schema-1 mailbox (HS2-65ETRJ).

use super::*;
use hotsheet_model::{NoteKind, Timestamp, Ulid};

impl Coordinator {
    /// Preserve a rejected handoff, allowing a new operation to assign its corrections.
    /// Ticket and mailbox checks share the mutation lock, so a concurrent claim cannot
    /// slip between the released-claim check and the durable return.
    pub fn review_return(
        &self,
        integrator: &str,
        generation: &str,
        now: time::OffsetDateTime,
        id: &str,
        evidence_note: &str,
    ) -> Result<()> {
        let note_id = Ulid::from_string(evidence_note)?;
        ensure!(
            self.source
                .join(hotsheet_ticketing::STORE_METADATA_FILE)
                .exists(),
            "provider review-return verification is unsupported for this selected source (HS2-3ETJ26)"
        );
        let store = FsStore::open_without_maintenance(&self.source)?;
        store.with_ticket_transaction(|| {
            self.change(|state| {
                Self::integrator(state, integrator, generation, now.unix_timestamp())?;
                let a = state
                    .assignments
                    .get_mut(id)
                    .ok_or_else(|| anyhow::anyhow!("unknown assignment"))?;
                if a.returned_for_review() {
                    ensure!(
                        a.recovery_note.as_deref() == Some(evidence_note),
                        "review return already recorded with different evidence"
                    );
                    return Ok(());
                }
                ensure!(
                    a.phase == Phase::Handoff,
                    "only a handoff can be returned for review"
                );
                let commit = a
                    .commit
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("handoff has no commit"))?;
                ensure!(
                    a.ticket.connection_id == git_connection_id(&store),
                    "provider review-return verification is unsupported for this connection"
                );
                let ticket = store.read_ticket(&a.ticket.native_id.parse()?)?;
                ensure!(
                    ops::is_open(&ticket)
                        && ops::claim_available(&ticket, &Timestamp::from_datetime(now)),
                    "review return requires an open ticket without a live claim"
                );
                ensure!(
                    ticket.notes.iter().any(|note| note.id == note_id
                        && note.kind == NoteKind::Regular
                        && note.text.lines().any(|line| line == "## Review return")
                        && note.text.split_whitespace().any(|word| word == id)
                        && note.text.split_whitespace().any(|word| word == commit)),
                    "regular Review return note must exist on this exact ticket and name the assignment id and full reviewed commit"
                );
                a.phase = Phase::Blocked;
                a.recovery_note = Some(evidence_note.into());
                Ok(())
            })
        })
    }
}
