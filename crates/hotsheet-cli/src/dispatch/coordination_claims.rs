//! Ticket authority uses a precise clock, independently of mailbox availability (HS2-35R7M3).

use anyhow::{Result, ensure};
use hotsheet_model::{Ticket, Timestamp};
use hotsheet_ticketing::ops;
use time::OffsetDateTime;

pub(super) fn assignable(
    ticket: &Ticket,
    tickets: &[Ticket],
    worker: &str,
    now: OffsetDateTime,
) -> Result<()> {
    let at = Timestamp::from_datetime(now);
    ensure!(
        !tickets
            .iter()
            .any(|t| t.claimed_by.as_deref() == Some(worker) && !ops::claim_available(t, &at)),
        "worker already holds a live ticket claim; finish or release it before another assignment"
    );
    ensure!(
        ticket.up_next
            && ops::is_open(ticket)
            && ticket
                .blocked_reason
                .as_ref()
                .is_none_or(|r| r.trim().is_empty())
            && ops::claim_available(ticket, &at),
        "ticket is not eligible for a new assignment; inspect status, blockers and live claim"
    );
    Ok(())
}

/// A dispatcher reserves a ticket only once every `blocked_by` dependency is closed
/// (HS2-TWZW7X). A dependency missing from this source is unknown, not satisfied.
pub(super) fn dependencies_closed(ticket: &Ticket, tickets: &[Ticket]) -> Result<()> {
    for dependency in &ticket.blocked_by {
        let closed = tickets
            .iter()
            .find(|t| t.id == *dependency)
            .is_some_and(|t| !ops::is_open(t));
        ensure!(
            closed,
            "ticket depends on {dependency}, which is open or not in this source"
        );
    }
    Ok(())
}

pub(super) fn accepted(
    ticket: &Ticket,
    tickets: &[Ticket],
    worker: &str,
    now: OffsetDateTime,
) -> Result<()> {
    let at = Timestamp::from_datetime(now);
    ensure!(
        !tickets.iter().any(|t| t.id != ticket.id
            && t.claimed_by.as_deref() == Some(worker)
            && !ops::claim_available(t, &at)),
        "worker holds another live ticket claim"
    );
    ensure!(
        ticket.claimed_by.as_deref() == Some(worker) && !ops::claim_available(ticket, &at),
        "claim this exact ticket with the assigned worker before acknowledging acceptance"
    );
    Ok(())
}

pub(super) fn recoverable(ticket: &Ticket, now: OffsetDateTime) -> Result<()> {
    ensure!(
        ops::claim_available(ticket, &Timestamp::from_datetime(now)),
        "a live ticket claim prevents recovery"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hotsheet_model::{Status, Ulid};
    use time::Duration;

    fn at(milliseconds: i64) -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap()
            + Duration::milliseconds(milliseconds)
    }

    fn claimed(worker: &str, expiry_ms: i64) -> Ticket {
        Ticket {
            id: Ulid::new(),
            up_next: true,
            claimed_by: Some(worker.into()),
            claim_lease_expires_at: Some(Timestamp::from_datetime(at(expiry_ms))),
            ..Ticket::default()
        }
    }

    #[test]
    fn hs2_35r7m3_same_second_authority_on_initial_and_replayed_checks() {
        let mut ticket = claimed("worker", 200);
        for _ in 0..2 {
            for (now, live) in [(100, true), (200, false), (800, false)] {
                let tickets = [ticket.clone()];
                assert_eq!(accepted(&ticket, &tickets, "worker", at(now)).is_ok(), live);
                assert_eq!(recoverable(&ticket, at(now)).is_ok(), !live);
                assert_eq!(
                    assignable(&ticket, &tickets, "next", at(now)).is_ok(),
                    !live
                );
                assert_eq!(
                    assignable(&ticket, &tickets, "worker", at(now)).is_ok(),
                    !live
                );
            }
        }
        for (now, live) in [
            (at(200) - Duration::nanoseconds(1), true),
            (at(200), false),
            (at(200) + Duration::nanoseconds(1), false),
        ] {
            assert_eq!(
                accepted(&ticket, std::slice::from_ref(&ticket), "worker", now).is_ok(),
                live
            );
            assert_eq!(recoverable(&ticket, now).is_ok(), !live);
            assert_eq!(
                assignable(&ticket, std::slice::from_ref(&ticket), "next", now).is_ok(),
                !live
            );
        }
        // A renewal within this same second restores live authority without changing availability clocks.
        ticket.claim_lease_expires_at = Some(Timestamp::from_datetime(at(900)));
        assert!(accepted(&ticket, std::slice::from_ref(&ticket), "worker", at(800)).is_ok());
        assert!(recoverable(&ticket, at(800)).is_err());
        assert!(assignable(&ticket, std::slice::from_ref(&ticket), "next", at(800)).is_err());

        ticket.claimed_by = Some("replacement".into());
        assert!(accepted(&ticket, std::slice::from_ref(&ticket), "worker", at(800)).is_err());
        assert!(
            accepted(
                &ticket,
                std::slice::from_ref(&ticket),
                "replacement",
                at(800)
            )
            .is_ok()
        );
        assert!(recoverable(&ticket, at(800)).is_err());
        ticket.claimed_by = None;
        assert!(accepted(&ticket, std::slice::from_ref(&ticket), "worker", at(800)).is_err());
        assert!(recoverable(&ticket, at(800)).is_ok());
        assert!(assignable(&ticket, std::slice::from_ref(&ticket), "worker", at(800)).is_ok());
        ticket.claimed_by = Some("worker".into());
        for expiry in [None, Some(Timestamp::new("invalid"))] {
            ticket.claim_lease_expires_at = expiry;
            assert!(accepted(&ticket, std::slice::from_ref(&ticket), "worker", at(800)).is_err());
            assert!(recoverable(&ticket, at(800)).is_ok());
            assert!(assignable(&ticket, std::slice::from_ref(&ticket), "worker", at(800)).is_ok());
        }
    }

    #[test]
    fn hs2_35r7m3_other_claims_expire_at_full_precision_without_authorizing_wrong_owner() {
        let ticket = claimed("worker", 900);
        let mut other = claimed("worker", 200);
        for (now, live_other) in [(100, true), (200, false), (800, false)] {
            assert_eq!(
                accepted(&ticket, &[ticket.clone(), other.clone()], "worker", at(now)).is_ok(),
                !live_other
            );
            assert_eq!(
                assignable(
                    &Ticket {
                        up_next: true,
                        ..Ticket::default()
                    },
                    std::slice::from_ref(&other),
                    "worker",
                    at(now)
                )
                .is_ok(),
                !live_other
            );
        }
        other.claimed_by = Some("different".into());
        assert!(accepted(&ticket, &[ticket.clone(), other], "worker", at(100)).is_ok());
        assert!(accepted(&ticket, std::slice::from_ref(&ticket), "different", at(100)).is_err());
    }

    #[test]
    fn hs2_35r7m3_claim_expiry_does_not_remove_dispatch_eligibility_guards() {
        let mut ticket = claimed("worker", 200);
        ticket.up_next = false;
        assert!(assignable(&ticket, &[], "next", at(800)).is_err());
        ticket.up_next = true;
        ticket.status = Status::Completed;
        assert!(assignable(&ticket, &[], "next", at(800)).is_err());
        ticket.status = Status::Started;
        ticket.blocked_reason = Some("synthetic blocker".into());
        assert!(assignable(&ticket, &[], "next", at(800)).is_err());
        ticket.blocked_reason = Some(" ".into());
        assert!(assignable(&ticket, &[], "next", at(800)).is_ok());
    }
}
