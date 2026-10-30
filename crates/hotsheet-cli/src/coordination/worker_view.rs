//! Read-only, source-scoped worker projection (HS2-DBECD0).
//! The coordination state and ticket claims remain the only authorities.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use hotsheet_model::StartedPhase;
use hotsheet_ticketing::TicketRef;
use notify::{RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::contract::{
    HandoffEvidence, IntegrationSuspension, PlanEvent, PlannedTicket, SourceSupport,
};
use super::{Authority, Coordinator, Phase, ReturnRoute, Role, Snapshot};

/// A host-observed connection explicitly bound to a coordination session. The host must
/// resolve this from a registered connection/terminal id, never a process scan or name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ConnectionObservation {
    pub connection_id: String,
    pub tool: String,
    pub kind: String,
    pub role: Option<String>,
    pub busy: Option<bool>,
    pub observed_alive: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    Live,
    Disconnected,
    UnresolvedClaimHolder,
    UnboundConnection,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerTicket {
    pub ticket: TicketRef,
    pub slug: Option<String>,
    /// A durable reservation/handoff is separate from the ticket's actual claim.
    pub assignment_id: Option<String>,
    pub assignment_phase: Option<Phase>,
    pub assignment_generation: Option<String>,
    pub handoff: Option<HandoffEvidence>,
    pub integration_suspension: Option<IntegrationSuspension>,
    pub claim_expires_at: Option<i64>,
    pub started_phase: Option<StartedPhase>,
    pub eta_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerRow {
    pub id: String,
    pub role: Option<Role>,
    pub generation: Option<String>,
    pub parent: Option<String>,
    pub label: String,
    pub tool: Option<String>,
    pub connection_kind: Option<String>,
    pub connection_role: Option<String>,
    pub connection_id: Option<String>,
    pub connection_available: Option<bool>,
    pub observed_alive: Option<bool>,
    pub process_busy: Option<bool>,
    pub availability: Availability,
    pub lease_expires_at: Option<i64>,
    pub tickets: Vec<WorkerTicket>,
    pub planned: Vec<PlannedTicket>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerView {
    pub cursor: String,
    pub project_id: String,
    pub source_id: String,
    pub authority: Authority,
    pub workers: Vec<WorkerRow>,
    pub plan_revision: u64,
    pub plan_events: Vec<PlanEvent>,
    pub uncovered_sources: Vec<String>,
    pub source_support: Vec<SourceSupport>,
    /// A headless read has no host connection registry to inspect.
    pub connection_coverage: bool,
}

impl WorkerView {
    /// The first component is the authoritative coordination cursor. The second
    /// covers only explicitly observed host connections.
    pub fn coordination_cursor(after: &str) -> &str {
        after.split_once('.').map_or(after, |(cursor, _)| cursor)
    }

    /// The shared status cursor intentionally normalizes live lease deadlines.
    /// Worker consumers display exact deadlines, so renewals need their own cursor.
    pub fn worker_state_cursor(snapshot: &Snapshot) -> String {
        format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&(
                    &snapshot.cursor,
                    &snapshot.state.integrator,
                    &snapshot.state.dispatcher,
                    &snapshot.state.workers,
                ))
                .expect("worker state serializes")
            )
        )
    }

    /// Compose one worker per exact coordination identity, including disconnected
    /// sessions with retained assignments and claim holders without a session.
    pub fn from_snapshot(
        snapshot: &Snapshot,
        now: i64,
        connections: &BTreeMap<String, ConnectionObservation>,
        connection_coverage: bool,
    ) -> Self {
        let state = &snapshot.state;
        let mut rows = BTreeMap::<String, WorkerRow>::new();
        for (role, session) in state
            .integrator
            .iter()
            .map(|s| (Role::Integrator, s))
            .chain(state.dispatcher.iter().map(|s| (Role::Dispatcher, s)))
            .chain(state.workers.values().map(|s| (Role::Worker, s)))
        {
            let connection_id = match &session.return_route {
                ReturnRoute::Terminal(id) => Some(id.clone()),
                _ => None,
            };
            let observed = connection_id.as_ref().and_then(|id| connections.get(id));
            let parent = match &session.return_route {
                ReturnRoute::NativeParent(id) => Some(id.clone()),
                _ => None,
            };
            rows.insert(
                session.id.clone(),
                WorkerRow {
                    id: session.id.clone(),
                    role: Some(role),
                    generation: Some(session.generation.clone()),
                    parent,
                    label: session.id.clone(),
                    tool: observed.map(|c| c.tool.clone()),
                    connection_kind: observed.map(|c| c.kind.clone()),
                    connection_role: observed.and_then(|c| c.role.clone()),
                    connection_id: connection_id.clone(),
                    connection_available: connection_id
                        .as_ref()
                        .and_then(|_| connection_coverage.then_some(observed.is_some())),
                    process_busy: observed.and_then(|c| c.busy),
                    observed_alive: observed.and_then(|c| c.observed_alive),
                    availability: if session.expires_at > now {
                        Availability::Live
                    } else {
                        Availability::Disconnected
                    },
                    lease_expires_at: Some(session.expires_at),
                    tickets: Vec::new(),
                    planned: Vec::new(),
                },
            );
        }
        let queue: BTreeMap<String, _> = snapshot
            .local_queue
            .iter()
            .flatten()
            .map(|entry| (entry.ticket.qualified(), entry))
            .collect();
        let mut tickets = BTreeMap::<String, Vec<WorkerTicket>>::new();
        for assignment in state.assignments.values().filter(|a| a.phase.pending()) {
            let key = assignment.ticket.qualified();
            let detail = queue.get(&key);
            tickets
                .entry(assignment.worker.clone())
                .or_default()
                .push(WorkerTicket {
                    ticket: assignment.ticket.clone(),
                    slug: Some(assignment.slug.clone()),
                    assignment_id: Some(assignment.id.clone()),
                    assignment_phase: Some(assignment.phase),
                    assignment_generation: Some(assignment.generation.clone()),
                    handoff: assignment.handoff.clone().or_else(|| {
                        assignment
                            .commit
                            .as_ref()
                            .map(|commit| HandoffEvidence::Code {
                                commit: commit.clone(),
                            })
                    }),
                    integration_suspension: assignment.suspension.clone(),
                    claim_expires_at: None,
                    started_phase: detail.and_then(|d| d.phase),
                    eta_at: None,
                });
        }
        for claim in &snapshot.live_claims {
            let key = claim.ticket.qualified();
            let detail = queue.get(&key);
            tickets
                .entry(claim.worker.clone())
                .or_default()
                .push(WorkerTicket {
                    ticket: claim.ticket.clone(),
                    slug: detail.map(|d| d.slug.clone()),
                    assignment_id: None,
                    assignment_phase: None,
                    assignment_generation: None,
                    handoff: None,
                    integration_suspension: None,
                    claim_expires_at: Some(claim.expires_at),
                    started_phase: claim.started_phase.or_else(|| detail.and_then(|d| d.phase)),
                    eta_at: claim.eta_at.clone(),
                });
        }
        for (worker, mut held) in tickets {
            let row = rows.entry(worker.clone()).or_insert_with(|| WorkerRow {
                id: worker.clone(),
                role: None,
                generation: None,
                parent: None,
                label: worker,
                tool: None,
                connection_kind: None,
                connection_role: None,
                connection_id: None,
                connection_available: None,
                observed_alive: None,
                process_busy: None,
                availability: Availability::UnresolvedClaimHolder,
                lease_expires_at: None,
                tickets: Vec::new(),
                planned: Vec::new(),
            });
            held.sort_by_key(|ticket| (ticket.ticket.qualified(), ticket.assignment_id.clone()));
            row.tickets = held;
        }
        // A host-registered external session can exist before a coordination session.
        // Only its explicit connection identity is known; keep its role/worker id
        // unresolved and suppress it when a session binds that same terminal id.
        let bound: BTreeSet<_> = rows
            .values()
            .filter_map(|row| row.connection_id.clone())
            .collect();
        for connection in connections
            .values()
            .filter(|c| !bound.contains(&c.connection_id))
        {
            let id = format!("connection:{}", connection.connection_id);
            rows.insert(
                id.clone(),
                WorkerRow {
                    id,
                    role: None,
                    generation: None,
                    parent: None,
                    label: connection.tool.clone(),
                    tool: Some(connection.tool.clone()),
                    connection_kind: Some(connection.kind.clone()),
                    connection_role: connection.role.clone(),
                    connection_id: Some(connection.connection_id.clone()),
                    connection_available: Some(true),
                    observed_alive: connection.observed_alive,
                    process_busy: connection.busy,
                    availability: Availability::UnboundConnection,
                    lease_expires_at: None,
                    tickets: Vec::new(),
                    planned: Vec::new(),
                },
            );
        }
        for planned in &snapshot.planned {
            let row = rows
                .entry(planned.entry.worker.clone())
                .or_insert_with(|| WorkerRow {
                    id: planned.entry.worker.clone(),
                    role: None,
                    generation: None,
                    parent: None,
                    label: planned.entry.worker.clone(),
                    tool: None,
                    connection_kind: None,
                    connection_role: None,
                    connection_id: None,
                    connection_available: None,
                    observed_alive: None,
                    process_busy: None,
                    availability: Availability::Disconnected,
                    lease_expires_at: None,
                    tickets: Vec::new(),
                    planned: Vec::new(),
                });
            row.planned.push(planned.clone());
        }
        // Explicit role order is an interaction contract; stable ids break ties.
        let mut workers: Vec<_> = rows.into_values().collect();
        workers.sort_by_key(|row| {
            let order = match row.role {
                Some(Role::Integrator) => 0,
                Some(Role::Dispatcher) => 1,
                Some(Role::Worker) => 2,
                None => 3,
            };
            (
                order,
                row.parent.clone().unwrap_or_default(),
                row.id.clone(),
            )
        });
        // The snapshot names unsupported/unknown sources; never infer an empty source
        // from its absence in local_queue.
        let uncovered_sources: Vec<_> = snapshot
            .uncovered_sources
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let observation_hash = Sha256::digest(
            serde_json::to_vec(&(connections, connection_coverage))
                .expect("connection observations serialize"),
        );
        Self {
            cursor: format!(
                "{}.{:x}",
                Self::worker_state_cursor(snapshot),
                observation_hash
            ),
            project_id: state.project_id.clone(),
            source_id: state.source_id.clone(),
            authority: state.authority,
            workers,
            plan_revision: state.plan_revision,
            plan_events: state.plan_events.clone(),
            uncovered_sources,
            source_support: snapshot.source_support.clone(),
            connection_coverage,
        }
    }
}

impl Coordinator {
    /// Long-poll the authoritative coordination snapshot without registering or
    /// renewing a session. Subscribing before the first read closes the race with
    /// concurrent state and ticket writes (HS2-DBECD0).
    pub fn watch_worker_snapshot(
        &self,
        after: &str,
        max_wait: Duration,
    ) -> anyhow::Result<Snapshot> {
        self.watch_worker_snapshot_with_cancel(after, max_wait, None)
    }

    /// A server request can cancel its filesystem watcher on disconnect or an
    /// independent connection event; idle checks never rescan the source.
    pub fn watch_worker_snapshot_with_cancel(
        &self,
        after: &str,
        max_wait: Duration,
        cancelled: Option<&AtomicBool>,
    ) -> anyhow::Result<Snapshot> {
        let (sender, receiver) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |event| {
            let _ = sender.send(event);
        })?;
        watcher.watch(&self.directory, RecursiveMode::NonRecursive)?;
        watcher.watch(&self.source, RecursiveMode::Recursive)?;
        std::fs::create_dir_all(&self.registry_home)?;
        watcher.watch(&self.registry_home, RecursiveMode::NonRecursive)?;
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let mut snapshot = self.snapshot(now)?;
        if WorkerView::worker_state_cursor(&snapshot) != after || max_wait.is_zero() {
            return Ok(snapshot);
        }
        let deadline = Instant::now() + max_wait;
        loop {
            if cancelled.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
                return Ok(snapshot);
            }
            let now = time::OffsetDateTime::now_utc().unix_timestamp();
            let next_expiry = snapshot
                .state
                .integrator
                .iter()
                .chain(snapshot.state.dispatcher.iter())
                .chain(snapshot.state.workers.values())
                .map(|session| session.expires_at)
                .chain(snapshot.live_claims.iter().map(|claim| claim.expires_at))
                .filter(|at| *at > now)
                .min();
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(snapshot);
            }
            let wait = next_expiry.map_or(remaining, |at| {
                remaining.min(Duration::from_secs(
                    u64::try_from(at - now).unwrap_or_default(),
                ))
            });
            let slice = if cancelled.is_some() {
                wait.min(Duration::from_millis(250))
            } else {
                wait
            };
            let relevant = match receiver.recv_timeout(slice) {
                Ok(Ok(event)) => {
                    let tickets = self.source.join("tickets");
                    let providers = self.source.join("providers.json");
                    let checkouts = self.registry_home.join("checkouts.json");
                    let state = self.directory.join("state.json");
                    event.need_rescan()
                        || event.paths.iter().any(|path| {
                            path == &state
                                || path == &providers
                                || path == &checkouts
                                || path.starts_with(&tickets)
                        })
                }
                Err(mpsc::RecvTimeoutError::Timeout) => slice == wait,
                Ok(Err(error)) => anyhow::bail!("worker view watch failed: {error}"),
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    anyhow::bail!("worker view watch disconnected; reconnect from cursor")
                }
            };
            if relevant {
                snapshot = self.snapshot(time::OffsetDateTime::now_utc().unix_timestamp())?;
                if WorkerView::worker_state_cursor(&snapshot) != after {
                    return Ok(snapshot);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coordination::contract::{Assignment, LiveClaim, State};
    use crate::coordination::{ReturnRoute, Session, Snapshot};

    fn ticket(native_id: &str) -> TicketRef {
        TicketRef {
            connection_id: "source-a".into(),
            native_id: native_id.into(),
        }
    }

    fn snapshot() -> Snapshot {
        Snapshot {
            cursor: "cursor-a".into(),
            state: State::new("project-a".into(), "source-a".into()),
            live_workers: Vec::new(),
            live_integrator: false,
            live_dispatcher: false,
            dispatch_blockers: Vec::new(),
            local_queue: Some(Vec::new()),
            queue_support: "selected_local_source_only".into(),
            uncovered_sources: vec!["external-b".into()],
            live_claims: Vec::new(),
            source_support: Vec::new(),
            planned: Vec::new(),
        }
    }

    fn session(id: &str, generation: &str, expires_at: i64, route: ReturnRoute) -> Session {
        Session {
            id: id.into(),
            generation: generation.into(),
            expires_at,
            return_route: route,
        }
    }

    fn assignment(worker: &str, generation: &str, id: &str, phase: Phase) -> Assignment {
        Assignment {
            id: format!("operation-{id}"),
            worker: worker.into(),
            generation: generation.into(),
            ticket: ticket(id),
            slug: format!("HS2-{id}"),
            base: "0".repeat(40),
            phase,
            commit: None,
            handoff: None,
            return_route: ReturnRoute::HumanRelay,
            recovery_note: None,
            suspension: None,
            suspension_history: Vec::new(),
            scope: Vec::new(),
            transitions: Vec::new(),
        }
    }

    #[test]
    fn hs2_dbecd0_distinguishes_live_claim_handoff_and_disconnected_work() {
        let mut snapshot = snapshot();
        snapshot.state.integrator =
            Some(session("integrator", "gen-1", 300, ReturnRoute::HumanRelay));
        snapshot.state.workers.insert(
            "worker-a".into(),
            session(
                "worker-a",
                "gen-2",
                200,
                ReturnRoute::NativeParent("integrator".into()),
            ),
        );
        snapshot.state.assignments.insert(
            "offered".into(),
            assignment("worker-a", "gen-2", "A", Phase::Offered),
        );
        let mut handoff = assignment("worker-a", "gen-2", "B", Phase::Ci);
        handoff.handoff = Some(HandoffEvidence::Code {
            commit: "a".repeat(40),
        });
        handoff.suspension = Some(IntegrationSuspension {
            main_commit: "b".repeat(40),
            prerequisite: ticket("P"),
            failure_note: "note-p".into(),
            resumed: None,
        });
        snapshot.state.assignments.insert("handoff".into(), handoff);
        snapshot.live_claims.push(LiveClaim {
            ticket: ticket("A"),
            worker: "worker-a".into(),
            expires_at: 260,
            started_phase: None,
            eta_at: Some("2026-10-30T00:00:00Z".into()),
        });
        snapshot.live_claims.push(LiveClaim {
            ticket: ticket("C"),
            worker: "unknown-holder".into(),
            expires_at: 260,
            started_phase: None,
            eta_at: None,
        });
        let view = WorkerView::from_snapshot(&snapshot, 250, &BTreeMap::new(), false);
        assert_eq!(view.workers.len(), 3);
        assert_eq!(view.workers[0].id, "integrator");
        let worker = &view.workers[1];
        assert_eq!(worker.availability, Availability::Disconnected);
        assert_eq!(worker.parent.as_deref(), Some("integrator"));
        assert_eq!(
            worker.tickets.len(),
            3,
            "claim and exact assignment stay separate"
        );
        let offered = worker
            .tickets
            .iter()
            .find(|row| row.assignment_id.as_deref() == Some("operation-A"))
            .unwrap();
        assert_eq!(offered.assignment_phase, Some(Phase::Offered));
        assert_eq!(offered.claim_expires_at, None);
        assert!(worker.tickets.iter().any(|row| row.ticket.native_id == "A"
            && row.assignment_id.is_none()
            && row.claim_expires_at == Some(260)));
        assert!(worker.tickets.iter().any(|row| {
            row.assignment_phase == Some(Phase::Ci)
                && row.handoff
                    == Some(HandoffEvidence::Code {
                        commit: "a".repeat(40),
                    })
                && row
                    .integration_suspension
                    .as_ref()
                    .is_some_and(|suspension| suspension.main_commit == "b".repeat(40))
        }));
        assert_eq!(
            view.workers[2].availability,
            Availability::UnresolvedClaimHolder
        );
        assert_eq!(view.uncovered_sources, vec!["external-b"]);
        assert_eq!(view.workers[2].process_busy, None);
        snapshot
            .state
            .workers
            .get_mut("worker-a")
            .unwrap()
            .expires_at = 300;
        let reconnected = WorkerView::from_snapshot(&snapshot, 250, &BTreeMap::new(), false);
        assert_eq!(reconnected.workers[1].availability, Availability::Live);
        assert_eq!(reconnected.workers[1].tickets.len(), 3);
    }

    #[test]
    fn hs2_dbecd0_only_explicit_terminal_binding_supplies_tool_and_busy_state() {
        let mut snapshot = snapshot();
        snapshot.state.workers.insert(
            "worker-a".into(),
            session(
                "worker-a",
                "gen-1",
                300,
                ReturnRoute::Terminal("connection-a".into()),
            ),
        );
        let observations = BTreeMap::from([
            (
                "connection-a".into(),
                ConnectionObservation {
                    connection_id: "connection-a".into(),
                    tool: "claude".into(),
                    kind: "pty".into(),
                    role: Some("worker".into()),
                    busy: Some(true),
                    observed_alive: Some(true),
                },
            ),
            (
                "connection-b".into(),
                ConnectionObservation {
                    connection_id: "connection-b".into(),
                    tool: "codex".into(),
                    kind: "app_server".into(),
                    role: Some("worker".into()),
                    busy: Some(false),
                    observed_alive: None,
                },
            ),
        ]);
        let view = WorkerView::from_snapshot(&snapshot, 250, &observations, true);
        assert_eq!(view.workers.len(), 2);
        let bound = view
            .workers
            .iter()
            .find(|row| row.id == "worker-a")
            .unwrap();
        assert_eq!(bound.connection_id.as_deref(), Some("connection-a"));
        assert_eq!(bound.tool.as_deref(), Some("claude"));
        assert_eq!(bound.process_busy, Some(true));
        assert_eq!(bound.connection_available, Some(true));
        assert_eq!(bound.connection_kind.as_deref(), Some("pty"));
        assert_eq!(bound.observed_alive, Some(true));
        let missing = WorkerView::from_snapshot(&snapshot, 250, &BTreeMap::new(), true);
        assert_eq!(missing.workers[0].connection_available, Some(false));
        assert!(
            view.workers
                .iter()
                .any(|row| row.id == "connection:connection-b")
        );
        snapshot
            .state
            .workers
            .get_mut("worker-a")
            .unwrap()
            .return_route = ReturnRoute::HumanRelay;
        let no_binding = WorkerView::from_snapshot(&snapshot, 250, &observations, true);
        let unbound = no_binding
            .workers
            .iter()
            .find(|row| row.id == "worker-a")
            .unwrap();
        assert_eq!(unbound.tool, None);
        assert_eq!(unbound.process_busy, None);
        assert!(
            no_binding
                .workers
                .iter()
                .any(|row| row.id == "connection:connection-a")
        );
    }

    #[test]
    fn hs2_dbecd0_cursor_replays_late_registration_without_network_polling() {
        let (root, coordinator) = crate::coordination::tests::fixture();
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let initial = coordinator.snapshot(now).unwrap();
        let observer =
            Coordinator::open(&root.path().join("project"), &root.path().join("store")).unwrap();
        let cursor = WorkerView::worker_state_cursor(&initial);
        let wait = std::thread::spawn(move || {
            observer
                .watch_worker_snapshot(&cursor, Duration::from_secs(2))
                .unwrap()
        });
        coordinator
            .register(
                Role::Worker,
                "late-worker",
                "gen-1",
                now,
                120,
                ReturnRoute::HumanRelay,
            )
            .unwrap();
        let changed = wait.join().unwrap();
        assert_ne!(
            WorkerView::worker_state_cursor(&changed),
            WorkerView::worker_state_cursor(&initial)
        );
        assert!(changed.state.workers.contains_key("late-worker"));
        let immediate = coordinator
            .watch_worker_snapshot(&WorkerView::worker_state_cursor(&changed), Duration::ZERO)
            .unwrap();
        assert_eq!(immediate.cursor, changed.cursor);
    }

    #[test]
    fn hs2_dbecd0_exact_lease_renewal_changes_cursor_and_wakes_waiter() {
        let (root, coordinator) = crate::coordination::tests::fixture();
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        coordinator
            .register(
                Role::Worker,
                "worker-a",
                "gen-a",
                now,
                120,
                ReturnRoute::HumanRelay,
            )
            .unwrap();
        let first = coordinator.snapshot(now).unwrap();
        let first_cursor = WorkerView::worker_state_cursor(&first);
        let observer =
            Coordinator::open(&root.path().join("project"), &root.path().join("store")).unwrap();
        let waiting = std::thread::spawn(move || {
            observer
                .watch_worker_snapshot(&first_cursor, Duration::from_secs(2))
                .unwrap()
        });
        coordinator
            .register(
                Role::Worker,
                "worker-a",
                "gen-a",
                now + 1,
                120,
                ReturnRoute::HumanRelay,
            )
            .unwrap();
        let renewed = waiting.join().unwrap();
        assert_eq!(
            renewed.cursor, first.cursor,
            "shared status cursor intentionally normalizes live leases"
        );
        assert_ne!(
            WorkerView::worker_state_cursor(&renewed),
            WorkerView::worker_state_cursor(&first)
        );
        assert!(
            renewed.state.workers["worker-a"].expires_at
                > first.state.workers["worker-a"].expires_at
        );
    }

    #[test]
    fn hs2_dbecd0_same_ticket_retains_distinct_blocked_and_offered_operations() {
        let mut snapshot = snapshot();
        snapshot.state.workers.insert(
            "worker-a".into(),
            session("worker-a", "gen-a", 300, ReturnRoute::HumanRelay),
        );
        let mut blocked = assignment("worker-a", "gen-a", "A", Phase::Blocked);
        blocked.id = "blocked-old".into();
        let mut offered = assignment("worker-a", "gen-a", "A", Phase::Offered);
        offered.id = "offered-new".into();
        snapshot
            .state
            .assignments
            .insert(blocked.id.clone(), blocked);
        snapshot
            .state
            .assignments
            .insert(offered.id.clone(), offered);
        let view = WorkerView::from_snapshot(&snapshot, 250, &BTreeMap::new(), false);
        let tickets = &view.workers[0].tickets;
        assert_eq!(tickets.len(), 2);
        assert!(tickets.iter().any(|ticket| ticket.assignment_id.as_deref()
            == Some("blocked-old")
            && ticket.assignment_phase == Some(Phase::Blocked)));
        assert!(tickets.iter().any(|ticket| ticket.assignment_id.as_deref()
            == Some("offered-new")
            && ticket.assignment_phase == Some(Phase::Offered)));
    }

    #[test]
    fn hs2_dbecd0_cancelled_wait_releases_watcher_without_idle_rescan() {
        let (root, coordinator) = crate::coordination::tests::fixture();
        let initial = coordinator
            .snapshot(time::OffsetDateTime::now_utc().unix_timestamp())
            .unwrap();
        let cursor = WorkerView::worker_state_cursor(&initial);
        let cancelled = std::sync::Arc::new(AtomicBool::new(false));
        let flag = cancelled.clone();
        let observer =
            Coordinator::open(&root.path().join("project"), &root.path().join("store")).unwrap();
        let started = Instant::now();
        let waiting = std::thread::spawn(move || {
            observer
                .watch_worker_snapshot_with_cancel(&cursor, Duration::from_secs(30), Some(&flag))
                .unwrap()
        });
        std::thread::sleep(Duration::from_millis(100));
        cancelled.store(true, Ordering::Relaxed);
        let resumed = waiting.join().unwrap();
        assert_eq!(resumed.cursor, initial.cursor);
        // Leave room for scheduler contention in the full suite while still
        // proving cancellation returns well before the watch timeout.
        assert!(started.elapsed() < Duration::from_secs(15));
    }
}
