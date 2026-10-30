//! Revisioned tentative worker intent over the existing assignment authority (HS2-XCMAVT).

use std::collections::BTreeSet;

use anyhow::{Result, ensure};
use hotsheet_model::Timestamp;
use hotsheet_ticketing::{FsStore, TicketRef, git_connection_id, ops};

use super::contract::{NextPlanEntry, PlanEvent, PlanEventKind};
use super::{Assignment, Coordinator, Phase, ReservationCheck, State, scope};

const MAX_PLAN: usize = 128;
const MAX_EVENTS: usize = 128;

impl Coordinator {
    /// Replace ordered intent under the same generation fence as assignment. The
    /// caller supplies the revision it read; a stale writer receives a conflict.
    pub fn replace_next_plan(
        &self,
        authority: &str,
        generation: &str,
        clock: &dyn Fn() -> i64,
        expected_revision: u64,
        entries: Vec<NextPlanEntry>,
        all_participants_upgraded: bool,
    ) -> Result<u64> {
        ensure!(
            entries.len() <= MAX_PLAN,
            "next plan exceeds {MAX_PLAN} rows"
        );
        let mut operations = BTreeSet::new();
        let mut tickets = BTreeSet::new();
        let local_tickets = if self
            .source
            .join(hotsheet_ticketing::STORE_METADATA_FILE)
            .exists()
        {
            let store = FsStore::open_without_maintenance(&self.source)?;
            Some((git_connection_id(&store), store.list_tickets()?))
        } else {
            None
        };
        let entries = entries
            .into_iter()
            .map(|mut entry| {
                ensure!(
                    !entry.operation.trim().is_empty()
                        && !entry.worker.trim().is_empty()
                        && !entry.generation.trim().is_empty()
                        && !entry.ticket.connection_id.trim().is_empty()
                        && !entry.ticket.native_id.trim().is_empty(),
                    "next plan requires operation, worker generation, and qualified ticket"
                );
                ensure!(
                    operations.insert(entry.operation.clone()),
                    "duplicate plan operation id"
                );
                ensure!(
                    tickets.insert(entry.ticket.qualified()),
                    "ticket has duplicate planned placements"
                );
                entry.scope = scope::normalized_scope(&entry.scope)?;
                entry.priority_at_plan = None;
                entry.updated_at_plan = None;
                if let Some((connection, tickets)) = &local_tickets
                    && entry.ticket.connection_id == *connection
                    && let Some(ticket) = tickets
                        .iter()
                        .find(|ticket| ticket.id.to_string() == entry.ticket.native_id)
                {
                    entry.priority_at_plan = Some(ticket.priority);
                    entry.updated_at_plan = Some(ticket.updated_at.as_str().to_owned());
                }
                Ok(entry)
            })
            .collect::<Result<Vec<_>>>()?;
        self.change(|state| {
            Self::assignment_authority(state, authority, generation, clock())?;
            ensure!(state.stopped.is_none(), "coordination run is stopped");
            ensure!(
                state.plan_revision == expected_revision,
                "plan revision conflict: current {} expected {expected_revision}",
                state.plan_revision
            );
            if state.version < 7 {
                ensure!(all_participants_upgraded, "stop old participants and upgrade to schema seven before publishing a next plan");
                state.version = 7;
            }
            for entry in &entries {
                ensure!(
                    state.workers.get(&entry.worker).is_some_and(|session| session.generation == entry.generation),
                    "planned worker generation is unknown or replaced: {}",
                    entry.worker
                );
                ensure!(
                    !state.assignments.values().any(|assignment| assignment.ticket == entry.ticket && assignment.phase.pending() && !assignment.returned_for_review()),
                    "ticket already assigned or awaiting integration/CI"
                );
            }
            if state.next_plan != entries {
                state.next_plan = entries.clone();
                Self::record_plan_event(state, PlanEventKind::Replaced, entries);
            }
            Ok(state.plan_revision)
        })
    }

    pub fn next_plan_events(&self, after_revision: u64) -> Result<Vec<PlanEvent>> {
        let state = self.state()?;
        ensure!(
            after_revision <= state.plan_revision,
            "future plan revision"
        );
        if after_revision == state.plan_revision {
            return Ok(Vec::new());
        }
        ensure!(
            state
                .plan_events
                .first()
                .is_some_and(|event| event.revision <= after_revision + 1),
            "plan event replay overflow; fetch a fresh workers snapshot"
        );
        Ok(state
            .plan_events
            .into_iter()
            .filter(|event| event.revision > after_revision)
            .collect())
    }

    pub(super) fn record_plan_event(
        state: &mut State,
        kind: PlanEventKind,
        entries: Vec<NextPlanEntry>,
    ) {
        state.plan_revision += 1;
        state.plan_events.push(PlanEvent {
            revision: state.plan_revision,
            kind,
            entries,
        });
        let excess = state.plan_events.len().saturating_sub(MAX_EVENTS);
        state.plan_events.drain(..excess);
    }

    pub(super) fn consume_planned_assignment(state: &mut State, assignment: &Assignment) {
        if let Some(index) = state
            .next_plan
            .iter()
            .position(|entry| entry.ticket == assignment.ticket)
        {
            let entry = state.next_plan.remove(index);
            Self::record_plan_event(state, PlanEventKind::Assigned, vec![entry]);
        }
    }

    /// Promote one matching plan row through the existing ticket-transaction and
    /// verified reservation path. The worker still claims the exact ticket itself.
    pub fn dispatch_next_plan(
        &self,
        authority: &str,
        generation: &str,
        clock: &dyn Fn() -> i64,
        expected_revision: u64,
        operation: &str,
        base: &str,
    ) -> Result<Assignment> {
        let state = self.state()?;
        let entry = if state.plan_revision == expected_revision {
            state
                .next_plan
                .iter()
                .find(|entry| entry.operation == operation)
        } else {
            // A lost response can retry the same promotion. Its exact consumed
            // event proves which revision and operation the caller observed.
            state
                .plan_events
                .iter()
                .find(|event| {
                    Some(event.revision) == expected_revision.checked_add(1)
                        && event.kind == PlanEventKind::Assigned
                })
                .and_then(|event| {
                    event
                        .entries
                        .iter()
                        .find(|entry| entry.operation == operation)
                })
        }
        .ok_or_else(|| anyhow::anyhow!("plan revision conflict or unknown planned operation"))?
        .clone();
        ensure!(
            self.source
                .join(hotsheet_ticketing::STORE_METADATA_FILE)
                .exists(),
            "provider dispatch verification is unsupported for this source (HS2-3ETJ26)"
        );
        let store = FsStore::open_without_maintenance(&self.source)?;
        ensure!(
            entry.ticket.connection_id == git_connection_id(&store),
            "provider dispatch verification is unsupported for this connection (HS2-3ETJ26)"
        );
        let ticket_id: hotsheet_model::Ulid = entry.ticket.native_id.parse()?;
        let route = state
            .integrator
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("register integrator first"))?
            .return_route
            .clone();
        let check_entry = entry.clone();
        let check: ReservationCheck<'_> = Box::new(move |current, replayed| {
            ensure!(
                current.plan_revision == expected_revision || replayed.is_some(),
                "plan revision conflict"
            );
            ensure!(
                current
                    .next_plan
                    .iter()
                    .any(|planned| planned == &check_entry)
                    || replayed.is_some(),
                "planned operation changed before dispatch"
            );
            let ticket = store.read_ticket(&ticket_id)?;
            if replayed.is_none() {
                let tickets = store.list_tickets()?;
                let now = time::OffsetDateTime::from_unix_timestamp(clock())?;
                let at = Timestamp::from_datetime(now);
                ensure!(
                    !tickets
                        .iter()
                        .any(|candidate| candidate.claimed_by.as_deref()
                            == Some(&check_entry.worker)
                            && !ops::claim_available(candidate, &at)),
                    "worker already holds a live ticket claim"
                );
                ensure!(
                    ticket.up_next
                        && ops::is_open(&ticket)
                        && ticket
                            .blocked_reason
                            .as_ref()
                            .is_none_or(|reason| reason.trim().is_empty())
                        && ops::claim_available(&ticket, &at),
                    "ticket is not eligible for a new assignment"
                );
                ensure!(
                    check_entry
                        .priority_at_plan
                        .is_none_or(|priority| priority == ticket.priority),
                    "ticket priority changed; replace the next plan before dispatch"
                );
                ensure!(
                    ticket.slug == check_entry.slug,
                    "ticket slug changed; replace the next plan before dispatch"
                );
                for dependency in &ticket.blocked_by {
                    ensure!(
                        tickets
                            .iter()
                            .find(|candidate| candidate.id == *dependency)
                            .is_some_and(|candidate| !ops::is_open(candidate)),
                        "ticket has an open or unavailable dependency {dependency}"
                    );
                }
            }
            Ok(())
        });
        self.assign_verified(
            authority,
            generation,
            clock,
            Assignment {
                id: entry.operation,
                worker: entry.worker,
                generation: entry.generation,
                ticket: TicketRef {
                    connection_id: entry.ticket.connection_id,
                    native_id: entry.ticket.native_id,
                },
                slug: entry.slug,
                base: base.into(),
                phase: Phase::Offered,
                commit: None,
                handoff: None,
                return_route: route,
                recovery_note: None,
                suspension: None,
                suspension_history: Vec::new(),
                scope: entry.scope,
                transitions: Vec::new(),
            },
            check,
        )
    }
}
