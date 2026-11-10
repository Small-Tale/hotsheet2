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
    CoverageGap, HandoffEvidence, IntegrationSuspension, PlanEvent, PlannedTicket, SourceSupport,
};
use super::{
    Authority, Coordinator, IntegrationStrategy, Phase, ReturnRoute, Role, Snapshot, WorkerMatch,
    WorkerPreferences,
};

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
    /// The recorded strategy of an integrator session; None for other rows (HS2-NEY1QH).
    #[serde(default)]
    pub integration_strategy: Option<IntegrationStrategy>,
    /// A worker session's stated preferences, request and refusal; None when it states
    /// none (HS2-6WFCZR).
    #[serde(default)]
    pub preferences: Option<WorkerPreferences>,
    /// That worker's candidates, idle reason and request check (HS2-6WFCZR).
    #[serde(default)]
    pub matching: Option<WorkerMatch>,
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
    /// `uncovered_sources`, one entry each with a name and reason (HS2-83KFB5).
    #[serde(default)]
    pub coverage_gaps: Vec<CoverageGap>,
    pub source_support: Vec<SourceSupport>,
    /// A headless read has no host connection registry to inspect.
    pub connection_coverage: bool,
    /// Expired sessions holding no non-terminal assignment, live claim or plan entry
    /// (HS2-V9BXWF). The default view omits them and reports only this count; a view
    /// built with `include_past` lists them in `workers` as well.
    #[serde(default)]
    pub past_workers: usize,
    /// The live integrator's strategy, None without a live integrator (HS2-NEY1QH).
    #[serde(default)]
    pub integration_strategy: Option<IntegrationStrategy>,
    /// Durable Steady Main goal and staged transition facts (HS2-0BJ1A3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cadence: Option<super::batch_staged::CadenceStatus>,
    /// Ready tickets no live idle worker's strict preferences admit (HS2-6WFCZR).
    #[serde(default)]
    pub unmatched_tickets: Vec<String>,
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
    /// Past workers are omitted (HS2-V9BXWF); see [`Self::from_snapshot_including_past`].
    pub fn from_snapshot(
        snapshot: &Snapshot,
        now: i64,
        connections: &BTreeMap<String, ConnectionObservation>,
        connection_coverage: bool,
    ) -> Self {
        Self::project(snapshot, now, connections, connection_coverage, false)
    }

    /// The same projection, also listing expired sessions that hold nothing (HS2-V9BXWF).
    pub fn from_snapshot_including_past(
        snapshot: &Snapshot,
        now: i64,
        connections: &BTreeMap<String, ConnectionObservation>,
        connection_coverage: bool,
    ) -> Self {
        Self::project(snapshot, now, connections, connection_coverage, true)
    }

    fn project(
        snapshot: &Snapshot,
        now: i64,
        connections: &BTreeMap<String, ConnectionObservation>,
        connection_coverage: bool,
        include_past: bool,
    ) -> Self {
        let state = &snapshot.state;
        // HS2-V9BXWF: an assignment on a closed ticket is terminal for display; the
        // pool keeps the record for audit.
        let finished: BTreeSet<&str> = snapshot
            .finished_assignments
            .iter()
            .map(String::as_str)
            .collect();
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
                    integration_strategy: (role == Role::Integrator).then_some(session.strategy),
                    preferences: (!session.preferences.is_empty())
                        .then(|| session.preferences.clone()),
                    matching: snapshot.worker_matches.get(&session.id).cloned(),
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
        for assignment in state
            .assignments
            .values()
            .filter(|a| a.phase.pending() && !finished.contains(a.id.as_str()))
        {
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
                integration_strategy: None,
                preferences: None,
                matching: None,
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
                    integration_strategy: None,
                    preferences: None,
                    matching: None,
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
                    integration_strategy: None,
                    preferences: None,
                    matching: None,
                });
            row.planned.push(planned.clone());
        }
        // HS2-V9BXWF: an expired session that holds nothing and has no observed live
        // connection is a past worker, hidden from the default view.
        let (past, current): (Vec<_>, Vec<_>) = rows.into_values().partition(|row| {
            row.role.is_some()
                && row.availability == Availability::Disconnected
                && row.connection_available != Some(true)
                && row.tickets.is_empty()
                && row.planned.is_empty()
        });
        let past_workers = past.len();
        let mut workers = current;
        if include_past {
            workers.extend(past);
        }
        // Explicit role order is an interaction contract; stable ids break ties.
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
            coverage_gaps: snapshot.coverage_gaps.clone(),
            source_support: snapshot.source_support.clone(),
            connection_coverage,
            past_workers,
            integration_strategy: snapshot.integration_strategy,
            cadence: snapshot
                .batches
                .as_ref()
                .and_then(|batches| batches.cadence.clone()),
            unmatched_tickets: snapshot.unmatched_tickets.clone(),
        }
    }
}

/// Observable progress of one worker-snapshot wait (HS2-QVWZDP).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WatchStep {
    /// The watcher is registered and the initial cursor still matches.
    Armed,
    /// A cancellation-check slice elapsed without reading the source.
    IdleSlice,
    /// A relevant filesystem event or lease/deadline expiry re-read the source.
    Rescanned,
    /// The cancel flag ended the wait; the watcher drops on return.
    Cancelled,
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
        self.watch_worker_snapshot_observed(after, max_wait, cancelled, &mut |_| {})
    }

    /// The wait loop, reporting each [`WatchStep`] so tests can synchronize on
    /// the watcher's real progress instead of wall time (HS2-QVWZDP).
    fn watch_worker_snapshot_observed(
        &self,
        after: &str,
        max_wait: Duration,
        cancelled: Option<&AtomicBool>,
        observe: &mut dyn FnMut(WatchStep),
    ) -> anyhow::Result<Snapshot> {
        let (sender, receiver) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |event| {
            let _ = sender.send(event);
        })?;
        watcher.watch(&self.directory, RecursiveMode::NonRecursive)?;
        watcher.watch(&self.source, RecursiveMode::Recursive)?;
        std::fs::create_dir_all(&self.registry_home)?;
        watcher.watch(&self.registry_home, RecursiveMode::NonRecursive)?;
        let settings_directory = self.project.join(".hotsheet2");
        let mut settings_directory_watched = settings_directory.is_dir();
        if settings_directory_watched {
            watcher.watch(&settings_directory, RecursiveMode::NonRecursive)?;
        } else {
            watcher.watch(&self.project, RecursiveMode::NonRecursive)?;
        }
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let mut snapshot = self.snapshot(now)?;
        if WorkerView::worker_state_cursor(&snapshot) != after || max_wait.is_zero() {
            return Ok(snapshot);
        }
        let deadline = Instant::now() + max_wait;
        observe(WatchStep::Armed);
        loop {
            if cancelled.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
                observe(WatchStep::Cancelled);
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
                    let settings = settings_directory.join("settings.json");
                    event.need_rescan()
                        || event.paths.iter().any(|path| {
                            path == &state
                                || path == &settings
                                || path == &settings_directory
                                || path == &providers
                                || path == &checkouts
                                || path.starts_with(&tickets)
                        })
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    let expired = slice == wait;
                    if !expired {
                        observe(WatchStep::IdleSlice);
                    }
                    expired
                }
                Ok(Err(error)) => anyhow::bail!("worker view watch failed: {error}"),
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    anyhow::bail!("worker view watch disconnected; reconnect from cursor")
                }
            };
            if relevant {
                if !settings_directory_watched && settings_directory.is_dir() {
                    watcher.watch(&settings_directory, RecursiveMode::NonRecursive)?;
                    settings_directory_watched = true;
                }
                observe(WatchStep::Rescanned);
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
            coverage_gaps: Vec::new(),
            live_claims: Vec::new(),
            source_support: Vec::new(),
            planned: Vec::new(),
            finished_assignments: Vec::new(),
            integration_strategy: None,
            worker_matches: Default::default(),
            unmatched_tickets: Vec::new(),
            batches: None,
        }
    }

    fn session(id: &str, generation: &str, expires_at: i64, route: ReturnRoute) -> Session {
        Session {
            id: id.into(),
            generation: generation.into(),
            expires_at,
            return_route: route,
            strategy: Default::default(),
            preferences: Default::default(),
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
            retirement_note: None,
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

    /// Hang guard for each step of an observed wait (HS2-QVWZDP). It bounds only
    /// a regression (a wait that never wakes) and is not a latency claim: setup
    /// under shared-host load has taken up to 28 s, so it allows twice that.
    const HANG_GUARD: Duration = Duration::from_secs(60);

    /// The longest a change-driven wait may run. Success returns on the event;
    /// this only bounds a missed wake-up, and stays below [`HANG_GUARD`].
    const CHANGE_WAIT: Duration = Duration::from_secs(30);

    struct ObservedWait {
        steps: mpsc::Receiver<WatchStep>,
        done: mpsc::Receiver<Snapshot>,
    }

    impl ObservedWait {
        fn start(
            observer: Coordinator,
            cursor: String,
            max_wait: Duration,
            cancelled: Option<std::sync::Arc<AtomicBool>>,
        ) -> Self {
            let (step_sender, steps) = mpsc::channel();
            let (done_sender, done) = mpsc::channel();
            std::thread::spawn(move || {
                let snapshot = observer
                    .watch_worker_snapshot_observed(
                        &cursor,
                        max_wait,
                        cancelled.as_deref(),
                        &mut |step| step_sender.send(step).unwrap(),
                    )
                    .unwrap();
                done_sender.send(snapshot).unwrap();
            });
            Self { steps, done }
        }

        fn next_step(&self) -> WatchStep {
            self.steps
                .recv_timeout(HANG_GUARD)
                .expect("watch step within the hang guard")
        }

        /// The returned snapshot plus every step after those already read. The
        /// step channel closes only when the waiter's frame, including its
        /// filesystem watcher, has been dropped.
        fn finish(self) -> (Snapshot, Vec<WatchStep>) {
            let snapshot = self
                .done
                .recv_timeout(HANG_GUARD)
                .expect("wait returns within the hang guard");
            let mut rest = Vec::new();
            loop {
                match self.steps.recv_timeout(HANG_GUARD) {
                    Ok(step) => rest.push(step),
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        panic!("waiter did not release its watcher")
                    }
                }
            }
            (snapshot, rest)
        }
    }

    #[test]
    fn hs2_dbecd0_cursor_replays_late_registration_without_network_polling() {
        let (root, coordinator) = crate::coordination::tests::fixture();
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let initial = coordinator.snapshot(now).unwrap();
        let observer =
            Coordinator::open(&root.path().join("project"), &root.path().join("store")).unwrap();
        let cursor = WorkerView::worker_state_cursor(&initial);
        let wait = ObservedWait::start(observer, cursor, CHANGE_WAIT, None);
        // Registering only after the watcher is armed proves the event wake-up
        // rather than the initial-read fast path (HS2-QVWZDP).
        assert_eq!(wait.next_step(), WatchStep::Armed);
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
        let (changed, steps) = wait.finish();
        assert_woken_by_rescan(&steps);
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
    fn hs2_0bj1a3_policy_only_goal_change_wakes_worker_view_without_a_batch() {
        use crate::coordination::development_strategy::{DevelopmentPolicy, DevelopmentPreset};
        use crate::coordination::strategy::StrategyPolicy;
        let (root, coordinator) = crate::coordination::tests::fixture();
        let settings = hotsheet_ticketing::Settings::for_project(root.path().join("project"));
        let mut integration = StrategyPolicy::default();
        integration
            .allow(IntegrationStrategy::CiStaged, "synthetic cadence")
            .unwrap();
        integration.write(&settings).unwrap();
        let mut policy = DevelopmentPolicy {
            default: DevelopmentPreset::SteadyMain,
            configured: true,
            ..DevelopmentPolicy::default()
        };
        policy.write(&settings).unwrap();
        let first = coordinator
            .snapshot(time::OffsetDateTime::now_utc().unix_timestamp())
            .unwrap();
        assert_eq!(
            first
                .batches
                .as_ref()
                .unwrap()
                .cadence
                .as_ref()
                .unwrap()
                .goal_minutes,
            360
        );
        let observer =
            Coordinator::open(&root.path().join("project"), &root.path().join("store")).unwrap();
        let wait = ObservedWait::start(
            observer,
            WorkerView::worker_state_cursor(&first),
            CHANGE_WAIT,
            None,
        );
        assert_eq!(wait.next_step(), WatchStep::Armed);
        policy.green_main_goal_minutes = 90;
        policy.write(&settings).unwrap();
        let (changed, steps) = wait.finish();
        assert_woken_by_rescan(&steps);
        assert_eq!(
            changed
                .batches
                .as_ref()
                .unwrap()
                .cadence
                .as_ref()
                .unwrap()
                .goal_minutes,
            90
        );
        assert_ne!(
            WorkerView::worker_state_cursor(&first),
            WorkerView::worker_state_cursor(&changed)
        );
    }

    #[test]
    fn hs2_0bj1a3_first_policy_file_creation_wakes_worker_view() {
        use crate::coordination::development_strategy::{DevelopmentPolicy, DevelopmentPreset};
        use crate::coordination::strategy::StrategyPolicy;
        let (root, coordinator) = crate::coordination::tests::fixture();
        let initial = coordinator
            .snapshot(time::OffsetDateTime::now_utc().unix_timestamp())
            .unwrap();
        let observer =
            Coordinator::open(&root.path().join("project"), &root.path().join("store")).unwrap();
        let wait = ObservedWait::start(
            observer,
            WorkerView::worker_state_cursor(&initial),
            CHANGE_WAIT,
            None,
        );
        assert_eq!(wait.next_step(), WatchStep::Armed);
        let settings = hotsheet_ticketing::Settings::for_project(root.path().join("project"));
        let mut integration = StrategyPolicy::default();
        integration
            .allow(IntegrationStrategy::CiStaged, "synthetic first policy")
            .unwrap();
        integration.write(&settings).unwrap();
        DevelopmentPolicy {
            default: DevelopmentPreset::SteadyMain,
            configured: true,
            ..DevelopmentPolicy::default()
        }
        .write(&settings)
        .unwrap();
        let (changed, steps) = wait.finish();
        assert_woken_by_rescan(&steps);
        assert_eq!(
            changed
                .batches
                .as_ref()
                .unwrap()
                .cadence
                .as_ref()
                .unwrap()
                .goal_minutes,
            360
        );
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
        let waiting = ObservedWait::start(observer, first_cursor, CHANGE_WAIT, None);
        assert_eq!(waiting.next_step(), WatchStep::Armed);
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
        let (renewed, steps) = waiting.finish();
        assert_woken_by_rescan(&steps);
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
        let observer =
            Coordinator::open(&root.path().join("project"), &root.path().join("store")).unwrap();
        // An hour-long wait can return with an unchanged cursor within the hang
        // guard only through cancellation, so no wall-clock bound is needed to
        // tell cancellation from timeout under host load (HS2-QVWZDP).
        let waiting = ObservedWait::start(
            observer,
            cursor,
            Duration::from_secs(3600),
            Some(cancelled.clone()),
        );
        assert_eq!(waiting.next_step(), WatchStep::Armed);
        // Cancel only once the armed wait has idled through a check slice, so
        // the flag interrupts a real wait and the idle slice is shown not to
        // rescan.
        assert_eq!(waiting.next_step(), WatchStep::IdleSlice);
        cancelled.store(true, Ordering::Relaxed);
        let (resumed, steps) = waiting.finish();
        assert_eq!(resumed.cursor, initial.cursor);
        let (last, idle) = steps.split_last().expect("cancellation step");
        assert_eq!(*last, WatchStep::Cancelled);
        assert!(
            idle.iter().all(|step| *step == WatchStep::IdleSlice),
            "idle checks never rescan the source: {steps:?}"
        );
    }

    fn assert_woken_by_rescan(steps: &[WatchStep]) {
        assert!(
            !steps.is_empty() && steps.iter().all(|step| *step == WatchStep::Rescanned),
            "an armed wait wakes only by rescanning on the change: {steps:?}"
        );
    }

    #[test]
    fn hs2_v9bxwf_finished_assignments_and_past_workers_leave_the_default_view() {
        let mut snapshot = snapshot();
        snapshot.state.integrator =
            Some(session("integrator", "gen-1", 300, ReturnRoute::HumanRelay));
        for (id, expires) in [
            ("stale", 100),
            ("active", 100),
            ("idle", 100),
            ("live", 300),
        ] {
            snapshot.state.workers.insert(
                id.into(),
                session(id, "gen-1", expires, ReturnRoute::HumanRelay),
            );
        }
        // A review-returned code handoff on a closed ticket.
        let mut returned = assignment("stale", "gen-1", "F", Phase::Blocked);
        returned.commit = Some("a".repeat(40));
        returned.handoff = Some(HandoffEvidence::Code {
            commit: "a".repeat(40),
        });
        returned.recovery_note = Some("note-r".into());
        assert!(returned.returned_for_review());
        let open = assignment("active", "gen-1", "G", Phase::Handoff);
        // An unregistered holder whose only record is a findings handoff on a closed ticket.
        let mut ghost = assignment("ghost", "gen-0", "H", Phase::Handoff);
        ghost.handoff = Some(HandoffEvidence::Findings {
            note_id: "note-f".into(),
        });
        for a in [returned, open, ghost] {
            snapshot.state.assignments.insert(a.id.clone(), a);
        }
        snapshot.finished_assignments = vec!["operation-F".into(), "operation-H".into()];
        let view = WorkerView::from_snapshot(&snapshot, 250, &BTreeMap::new(), false);
        let ids: Vec<_> = view.workers.iter().map(|row| row.id.as_str()).collect();
        assert_eq!(ids, ["integrator", "active", "live"]);
        assert_eq!(view.past_workers, 2, "stale and idle are past workers");
        assert_eq!(view.workers[1].availability, Availability::Disconnected);
        assert_eq!(view.workers[1].tickets.len(), 1);
        assert!(
            !view
                .workers
                .iter()
                .any(|row| row.availability == Availability::UnresolvedClaimHolder),
            "a holder whose only records are finished is not an unresolved row"
        );

        let all = WorkerView::from_snapshot_including_past(&snapshot, 250, &BTreeMap::new(), false);
        let ids: Vec<_> = all.workers.iter().map(|row| row.id.as_str()).collect();
        assert_eq!(ids, ["integrator", "active", "idle", "live", "stale"]);
        assert_eq!(all.past_workers, 2);
        assert!(
            all.workers
                .iter()
                .all(|row| row.id != "stale" || row.tickets.is_empty())
        );
        assert_eq!(
            all.cursor, view.cursor,
            "the option never changes the cursor"
        );

        // Reopening the ticket (finished set empties) restores the held record and worker.
        snapshot.finished_assignments.clear();
        let reopened = WorkerView::from_snapshot(&snapshot, 250, &BTreeMap::new(), false);
        let ids: Vec<_> = reopened.workers.iter().map(|row| row.id.as_str()).collect();
        assert_eq!(ids, ["integrator", "active", "live", "stale", "ghost"]);
        assert_eq!(reopened.past_workers, 1);
        assert_eq!(
            reopened.workers[4].availability,
            Availability::UnresolvedClaimHolder
        );

        // A renewed lease or an observed live connection is never past.
        snapshot.finished_assignments = vec!["operation-F".into(), "operation-H".into()];
        snapshot.state.workers.get_mut("idle").unwrap().return_route =
            ReturnRoute::Terminal("connection-idle".into());
        let observed = BTreeMap::from([(
            "connection-idle".into(),
            ConnectionObservation {
                connection_id: "connection-idle".into(),
                tool: "claude".into(),
                kind: "pty".into(),
                role: Some("worker".into()),
                busy: Some(false),
                observed_alive: Some(true),
            },
        )]);
        let connected = WorkerView::from_snapshot(&snapshot, 250, &observed, true);
        assert!(connected.workers.iter().any(|row| row.id == "idle"));
        assert_eq!(connected.past_workers, 1);
        snapshot.state.workers.get_mut("stale").unwrap().expires_at = 400;
        let renewed = WorkerView::from_snapshot(&snapshot, 250, &observed, true);
        assert!(renewed.workers.iter().any(|row| row.id == "stale"
            && row.availability == Availability::Live
            && row.tickets.is_empty()));
        assert_eq!(renewed.past_workers, 0);
    }
}
