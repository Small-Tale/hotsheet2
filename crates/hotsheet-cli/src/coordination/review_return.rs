//! Exact, replayable code and findings review returns (HS2-65ETRJ/HS2-AEBJYN).

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
                ensure!(
                    a.ticket.connection_id == git_connection_id(&store),
                    "provider review-return verification is unsupported for this connection; {}",
                    self.unsupported_source(&a.ticket.connection_id)?
                );
                if a.returned_for_review() {
                    ensure!(
                        a.recovery_note.as_deref() == Some(evidence_note),
                        "review return already recorded with different evidence"
                    );
                    return Ok(());
                }
                let original_evidence = match a.handoff.as_ref() {
                    Some(HandoffEvidence::Findings { note_id }) => {
                        ensure!(
                            matches!(a.phase, Phase::Handoff | Phase::Verifying),
                            "only a findings handoff or verification can be returned for review"
                        );
                        note_id.as_str()
                    }
                    _ => {
                        ensure!(a.phase == Phase::Handoff, "only a handoff can be returned for review");
                        a.commit.as_deref().ok_or_else(|| anyhow::anyhow!("handoff has no commit"))?
                    }
                };
                let ticket = store.read_ticket(&a.ticket.native_id.parse()?)?;
                ensure!(
                    ops::is_open(&ticket)
                        && ops::claim_available(&ticket, &Timestamp::from_datetime(now)),
                    "review return requires an open ticket without a live claim"
                );
                if matches!(a.handoff, Some(HandoffEvidence::Findings { .. })) {
                    if a.phase == Phase::Verifying {
                        ensure!(ticket.claimed_by.is_none(), "release the integrator claim before returning findings verification");
                    }
                    let original_note = Ulid::from_string(original_evidence)?;
                    ensure!(
                        ticket.notes.iter().any(|note| note.id == original_note
                            && note.kind == NoteKind::Regular
                            && note.text.lines().next() == Some("## Findings handoff")
                            && note.text.lines().skip(1).any(|line| !line.trim().is_empty())),
                        "original regular Findings handoff note must exist with findings on this exact ticket"
                    );
                }
                ensure!(
                    ticket.notes.iter().any(|note| note.id == note_id
                        && note.kind == NoteKind::Regular
                        && note.text.lines().any(|line| line == "## Review return")
                        && note.text.split_whitespace().any(|word| word == id)
                        && note.text.split_whitespace().any(|word| word == original_evidence)),
                    "regular Review return note must exist on this exact ticket and name the assignment id and exact original handoff evidence"
                );
                a.phase = Phase::Blocked;
                a.recovery_note = Some(evidence_note.into());
                Ok(())
            })
        })
    }
}
