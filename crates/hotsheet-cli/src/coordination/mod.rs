//! Durable, same-project local mailboxes and event waits (HS2-76VMYG).
//! Assignment is a reservation, not a ticket claim or permission to integrate.

mod authority;
#[cfg(test)]
mod authority_tests;
pub mod contract;
#[cfg(test)]
mod findings_review_return_tests;
mod review_return;
#[cfg(test)]
mod review_return_tests;
mod snapshot;
mod source_support;
#[cfg(test)]
mod source_support_tests;
#[cfg(test)]
mod tests;
mod wait;

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Result, ensure};
use hotsheet_ticketing::{FileLock, FsStore, TicketRef, checkouts, git_connection_id, ops};
use sha2::{Digest, Sha256};

pub use authority::{CAPABILITIES, Capabilities};
pub use contract::{
    Assignment, Authority, HandoffEvidence, IntegrationReport, Phase, ReturnRoute, Role, Session,
    Snapshot, State, WaitRequest,
};

pub struct Coordinator {
    pub(crate) directory: PathBuf,
    pub(crate) source: PathBuf,
    project_id: String,
    source_id: String,
    /// Deterministic interleaving point: runs once after a waiting dispatcher observes
    /// dispatcher authority and before its atomic registration (HS2-TWZW7X).
    #[cfg(test)]
    pub(crate) before_dispatcher_register: std::sync::Mutex<Option<Box<dyn FnOnce() + Send>>>,
    /// Readiness signal: runs once a dispatcher wait is subscribed and waiting unregistered,
    /// so tests never infer that state from elapsed time (HS2-TWZW7X).
    #[cfg(test)]
    pub(crate) waiting_unregistered: std::sync::Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

/// Fresh reservation checks run under the ticket transaction and state lock, after the
/// current assignment authority is verified: the state and any replayed operation.
pub type ReservationCheck<'a> = Box<dyn FnOnce(&State, Option<&Assignment>) -> Result<()> + 'a>;

impl Coordinator {
    /// Idempotent local setup. The directory is under the shipping checkout's private
    /// git directory, so worker worktrees share it and no local path enters a commit.
    pub fn open(project: &Path, source: &Path) -> Result<Self> {
        let project = project.canonicalize()?;
        let output = hotsheet_ticketing::git::launch("git")
            .arg("-C")
            .arg(&project)
            .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
            .output()?;
        ensure!(
            output.status.success(),
            "project must be a code Git checkout"
        );
        let common = PathBuf::from(String::from_utf8(output.stdout)?.trim());
        let shipping = common
            .parent()
            .ok_or_else(|| anyhow::anyhow!("missing shipping checkout"))?;
        let project_id = checkouts::checkout_id(shipping)?;
        let directory = common.join("hotsheet-coordination");
        fs::create_dir_all(&directory)?;
        let this = Self {
            directory,
            source: source.canonicalize()?,
            project_id,
            source_id: format!(
                "{:x}",
                Sha256::digest(source.canonicalize()?.to_string_lossy().as_bytes())
            ),
            #[cfg(test)]
            before_dispatcher_register: std::sync::Mutex::new(None),
            #[cfg(test)]
            waiting_unregistered: std::sync::Mutex::new(None),
        };
        this.change(|_| Ok(()))?;
        Ok(this)
    }

    fn change<T>(&self, update: impl FnOnce(&mut State) -> Result<T>) -> Result<T> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.directory.join("state.lock"))?;
        let _lock = FileLock::acquire(file)?;
        let path = self.directory.join("state.json");
        let mut state = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice::<State>(&bytes)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                State::new(self.project_id.clone(), self.source_id.clone())
            }
            Err(error) => return Err(error.into()),
        };
        // HS2-TWZW7X: schema four is read and written only once the live integrator has
        // explicitly enabled dispatcher authority; anything newer is refused.
        ensure!(
            matches!(state.version, 1..=4) && state.project_id == self.project_id,
            "incompatible coordination state"
        );
        let before = state.clone();
        if state.version == 1 {
            for assignment in state.assignments.values_mut() {
                if let Some(commit) = assignment.commit.as_ref() {
                    assignment.handoff = Some(HandoffEvidence::Code {
                        commit: commit.clone(),
                    });
                }
            }
        }
        // HS2-AEBJYN: schema-two mutators do not fence returned findings from Done.
        // Upgrade before exposing any state so those older mutators refuse this pool.
        // A schema-four pool is never lowered (HS2-TWZW7X).
        state.version = state.version.max(3);
        ensure!(
            state.source_id == self.source_id,
            "coordination pool is bound to another source; use the same explicit -C source as the integrator"
        );
        let result = update(&mut state)?;
        Self::schema_fence(&state)?;
        if state != before || !path.exists() {
            state.revision += 1;
            let mut temporary = tempfile::NamedTempFile::new_in(&self.directory)?;
            serde_json::to_writer_pretty(&mut temporary, &state)?;
            temporary.write_all(b"\n")?;
            temporary.as_file().sync_all()?;
            temporary.persist(&path)?;
        }
        Ok(result)
    }

    pub fn state(&self) -> Result<State> {
        self.change(|state| Ok(state.clone()))
    }

    pub fn register(
        &self,
        role: Role,
        id: &str,
        generation: &str,
        now: i64,
        lease: i64,
        return_route: ReturnRoute,
    ) -> Result<()> {
        Self::valid_registration(id, generation, lease)?;
        self.change(|state| {
            if role == Role::Dispatcher {
                Self::require_dispatcher_authority(state)?;
            }
            Self::register_in(state, role, id, generation, now, lease, return_route)
        })
    }

    /// Atomically register unless this is a dispatcher without dispatcher authority, which
    /// returns `false` without changing state. Duplicate and stale-generation conflicts
    /// still fail (HS2-TWZW7X).
    pub fn try_register(
        &self,
        role: Role,
        id: &str,
        generation: &str,
        now: i64,
        lease: i64,
        return_route: ReturnRoute,
    ) -> Result<bool> {
        Self::valid_registration(id, generation, lease)?;
        self.change(|state| {
            if role == Role::Dispatcher && Self::require_dispatcher_authority(state).is_err() {
                return Ok(false);
            }
            Self::register_in(state, role, id, generation, now, lease, return_route)?;
            Ok(true)
        })
    }

    fn valid_registration(id: &str, generation: &str, lease: i64) -> Result<()> {
        ensure!(
            !id.trim().is_empty() && id.len() <= 256,
            "invalid session id"
        );
        ensure!(
            !generation.trim().is_empty() && generation.len() <= 256,
            "invalid session generation"
        );
        ensure!(
            (10..=3600).contains(&lease),
            "availability lease must be 10..3600 seconds"
        );
        Ok(())
    }

    fn register_in(
        state: &mut State,
        role: Role,
        id: &str,
        generation: &str,
        now: i64,
        lease: i64,
        return_route: ReturnRoute,
    ) -> Result<()> {
        if role == Role::Worker
            && let Some(a) = state.active_assignment(id)
        {
            ensure!(
                a.generation == generation,
                "stale assignment requires explicit recovery; do not overwrite its worktree"
            );
        }
        let previous = match role {
            Role::Integrator => state.integrator.as_ref(),
            Role::Dispatcher => state.dispatcher.as_ref(),
            Role::Worker => state.workers.get(id),
        };
        if let Some(previous) = previous {
            ensure!(
                previous.id == id && previous.generation == generation || !previous.live(now),
                "another live session holds this role/worker identity"
            );
            if role == Role::Worker && previous.generation != generation {
                ensure!(
                    state.active_assignment(id).is_none(),
                    "stale assignment requires explicit recovery; do not overwrite its worktree"
                );
            }
        }
        let session = Session {
            id: id.to_string(),
            generation: generation.to_string(),
            expires_at: now + lease,
            return_route,
        };
        match role {
            Role::Integrator => state.integrator = Some(session),
            Role::Dispatcher => state.dispatcher = Some(session),
            Role::Worker => {
                state.workers.insert(id.to_string(), session);
            }
        }
        Ok(())
    }

    fn integrator(state: &State, id: &str, generation: &str, now: i64) -> Result<()> {
        ensure!(
            state
                .integrator
                .as_ref()
                .is_some_and(|s| s.id == id && s.generation == generation && s.live(now)),
            "register or renew the exact integrator session first"
        );
        Ok(())
    }

    /// Reserve an exact assignment as the role holding assignment authority: the
    /// integrator in legacy mode, otherwise the dispatcher (HS2-TWZW7X).
    pub fn assign(
        &self,
        authority: &str,
        generation: &str,
        now: i64,
        assignment: Assignment,
    ) -> Result<Assignment> {
        self.assign_verified(
            authority,
            generation,
            &|| now,
            assignment,
            Box::new(|_, _| Ok(())),
        )
    }

    /// Reserve with `check` evaluated at the serialized reservation boundary: under the
    /// selected source's ticket transaction, then the state lock (the order review returns
    /// use), after the current authority is verified. A concurrent authority switch,
    /// dependency reopen, or claim cannot slip between the checks and the reservation.
    ///
    /// `clock` is sampled inside that boundary and again after ticket validation, before the
    /// final authority and worker availability checks. Neither lock contention nor validation
    /// that outlasts a session can authorize a reservation or replay (HS2-TWZW7X).
    pub fn assign_verified(
        &self,
        authority: &str,
        generation: &str,
        clock: &dyn Fn() -> i64,
        assignment: Assignment,
        check: ReservationCheck<'_>,
    ) -> Result<Assignment> {
        ensure!(
            assignment.base.len() == 40 && assignment.base.bytes().all(|c| c.is_ascii_hexdigit()),
            "base must be a full shipping SHA"
        );
        ensure!(
            !assignment.id.is_empty()
                && assignment.phase == Phase::Offered
                && assignment.commit.is_none()
                && assignment.handoff.is_none(),
            "new assignment must be an offered operation id"
        );
        ensure!(
            !assignment.ticket.connection_id.is_empty() && !assignment.ticket.native_id.is_empty(),
            "qualified ticket reference required"
        );
        let scope = authority::normalized_scope(&assignment.scope)?;
        let assignment = Assignment {
            scope,
            ..assignment
        };
        let reserve = || {
            self.change(|state| {
                let now = clock();
                Self::assignment_authority(state, authority, generation, now)?;
                ensure!(
                    state.stopped.is_none(),
                    "coordination run is stopped; resume explicitly"
                );
                check(state, state.assignments.get(&assignment.id))?;
                // Ticket reads may themselves outlast availability while both locks are held.
                // Replays also require the current authority to remain live at this boundary.
                let now = clock();
                Self::assignment_authority(state, authority, generation, now)?;
                if let Some(existing) = state.assignments.get(&assignment.id) {
                    ensure!(
                        existing.worker == assignment.worker
                            && existing.generation == assignment.generation
                            && existing.ticket == assignment.ticket
                            && existing.slug == assignment.slug
                            && existing.base == assignment.base
                            && existing.return_route == assignment.return_route
                            && existing.scope == assignment.scope,
                        "operation id already refers to a different assignment"
                    );
                    return Ok(existing.clone());
                }
                Self::dispatch_rules(state, &assignment)?;
                ensure!(
                    state
                        .workers
                        .get(&assignment.worker)
                        .is_some_and(|s| s.generation == assignment.generation && s.live(now)),
                    "worker is not currently available"
                );
                ensure!(
                    state.active_assignment(&assignment.worker).is_none(),
                    "worker already has an active assignment"
                );
                ensure!(
                    !state
                        .assignments
                        .values()
                        .any(|a| a.ticket == assignment.ticket
                            && a.phase.pending()
                            && !a.returned_for_review()),
                    "ticket already assigned or awaiting integration/CI"
                );
                state
                    .assignments
                    .insert(assignment.id.clone(), assignment.clone());
                Ok(assignment)
            })
        };
        if self
            .source
            .join(hotsheet_ticketing::STORE_METADATA_FILE)
            .exists()
        {
            FsStore::open_without_maintenance(&self.source)?.with_ticket_transaction(reserve)
        } else {
            reserve()
        }
    }

    pub fn worker_transition(
        &self,
        worker: &str,
        generation: &str,
        id: &str,
        phase: Phase,
        commit: Option<String>,
    ) -> Result<()> {
        self.worker_handoff_transition(
            worker,
            generation,
            id,
            phase,
            commit.map(|commit| HandoffEvidence::Code { commit }),
        )
    }

    pub fn worker_findings_transition(
        &self,
        worker: &str,
        generation: &str,
        id: &str,
        note_id: String,
    ) -> Result<()> {
        self.worker_handoff_transition(
            worker,
            generation,
            id,
            Phase::Handoff,
            Some(HandoffEvidence::Findings { note_id }),
        )
    }

    fn worker_handoff_transition(
        &self,
        worker: &str,
        generation: &str,
        id: &str,
        phase: Phase,
        handoff: Option<HandoffEvidence>,
    ) -> Result<()> {
        self.change(|state| {
            let a = state
                .assignments
                .get_mut(id)
                .ok_or_else(|| anyhow::anyhow!("unknown assignment"))?;
            ensure!(
                a.worker == worker && a.generation == generation,
                "assignment belongs to another session"
            );
            if phase == a.phase && handoff == a.handoff {
                return Ok(());
            }
            ensure!(
                matches!(
                    (a.phase, phase),
                    (Phase::Offered, Phase::Accepted)
                        | (Phase::Accepted, Phase::Handoff)
                        | (Phase::Offered | Phase::Accepted, Phase::Blocked)
                ),
                "invalid worker transition"
            );
            if phase == Phase::Handoff {
                match handoff.as_ref() {
                    Some(HandoffEvidence::Code { commit }) => ensure!(
                        commit.len() == 40 && commit.bytes().all(|c| c.is_ascii_hexdigit()),
                        "code handoff requires full verified commit SHA"
                    ),
                    Some(HandoffEvidence::Findings { note_id }) => {
                        let _ = hotsheet_model::Ulid::from_string(note_id)?;
                    }
                    None => anyhow::bail!("handoff requires a code commit or findings note"),
                }
            } else {
                ensure!(handoff.is_none(), "evidence is only recorded with handoff");
            }
            a.commit = match handoff.as_ref() {
                Some(HandoffEvidence::Code { commit }) => Some(commit.clone()),
                _ => None,
            };
            a.phase = phase;
            a.handoff = handoff;
            Ok(())
        })
    }

    pub fn mark(
        &self,
        integrator: &str,
        generation: &str,
        now: i64,
        id: &str,
        phase: Phase,
    ) -> Result<()> {
        self.change(|state| {
            Self::integrator(state, integrator, generation, now)?;
            if matches!(phase, Phase::Integrating | Phase::Verifying) {
                ensure!(
                    !state
                        .assignments
                        .values()
                        .any(|a| a.id != id
                            && matches!(a.phase, Phase::Integrating | Phase::Verifying)),
                    "another ticket is integrating or verifying; serialize integrator work"
                );
            }
            let a = state
                .assignments
                .get_mut(id)
                .ok_or_else(|| anyhow::anyhow!("unknown assignment"))?;
            if phase == Phase::Integrating {
                ensure!(
                    matches!(a.handoff, Some(HandoffEvidence::Code { .. })),
                    "only code handoffs enter integration"
                );
            }
            if phase == Phase::Verifying {
                ensure!(
                    matches!(a.handoff, Some(HandoffEvidence::Findings { .. })),
                    "only findings handoffs enter verification"
                );
            }
            if phase == Phase::Ci {
                ensure!(
                    matches!(a.handoff, Some(HandoffEvidence::Code { .. })),
                    "only code handoffs enter CI"
                );
            }
            ensure!(
                !a.returned_for_review(),
                "review-returned handoff is preserved; integrate its correction assignment instead"
            );
            ensure!(
                a.phase == phase
                    || matches!(
                        (a.phase, phase),
                        (Phase::Handoff, Phase::Verifying)
                            | (Phase::Verifying, Phase::Done)
                            | (Phase::Handoff, Phase::Integrating)
                            | (Phase::Integrating, Phase::Ci)
                            | (Phase::Ci, Phase::Done)
                            | (Phase::Ci, Phase::Integrating)
                            | (Phase::Blocked, Phase::Done)
                    ),
                "invalid integrator transition"
            );
            a.phase = phase;
            Ok(())
        })
    }

    pub fn finish(&self, integrator: &str, generation: &str, now: i64, reason: &str) -> Result<()> {
        ensure!(
            matches!(reason, "drained" | "blocked"),
            "finish reason must be drained or blocked"
        );
        let snapshot = self.snapshot(now)?;
        ensure!(
            snapshot.live_claims.is_empty(),
            "live ticket claims remain in flight, including tickets outside Up Next"
        );
        if reason == "drained" {
            ensure!(
                snapshot.uncovered_sources.is_empty(),
                "additional enabled ticket sources are not covered by the local queue adapter"
            );
        }
        if let Some(queue) = &snapshot.local_queue {
            if reason == "drained" {
                ensure!(queue.is_empty(), "Up Next is not drained");
            }
            ensure!(
                !queue.iter().any(|t| t.claimed_by.is_some()
                    || t.phase == Some(hotsheet_model::StartedPhase::FinalTesting)),
                "ticket work or CI remains in flight"
            );
        } else {
            ensure!(
                reason != "drained",
                "provider queue cannot be declared drained without its query/event adapter"
            );
        }
        self.change(|state| {
            Self::integrator(state, integrator, generation, now)?;
            ensure!(
                !state.assignments.values().any(|a| matches!(
                    a.phase,
                    Phase::Offered
                        | Phase::Accepted
                        | Phase::Handoff
                        | Phase::Verifying
                        | Phase::Integrating
                        | Phase::Ci
                )),
                "work or CI remains in flight"
            );
            state.stopped = Some(reason.into());
            Ok(())
        })
    }

    /// Resolve an interrupted reservation only after preserving and inspecting artifacts.
    /// Expiry alone never dispatches the same ticket to another worker (HS2-76VMYG).
    pub fn recover(
        &self,
        integrator: &str,
        generation: &str,
        now: i64,
        id: &str,
        evidence_note: &str,
    ) -> Result<()> {
        let _ = hotsheet_model::Ulid::from_string(evidence_note)?;
        self.change(|state| {
            Self::integrator(state, integrator, generation, now)?;
            let a = state
                .assignments
                .get(id)
                .ok_or_else(|| anyhow::anyhow!("unknown assignment"))?;
            ensure!(
                matches!(a.phase, Phase::Offered | Phase::Accepted),
                "only an interrupted worker reservation can be recovered"
            );
            ensure!(
                !state
                    .workers
                    .get(&a.worker)
                    .is_some_and(|s| s.generation == a.generation && s.live(now)),
                "worker availability is still live"
            );
            let a = state
                .assignments
                .get_mut(id)
                .expect("assignment checked above");
            a.phase = Phase::Blocked;
            a.recovery_note = Some(evidence_note.into());
            Ok(())
        })
    }

    pub fn resume(&self) -> Result<()> {
        self.change(|state| {
            state.stopped = None;
            Ok(())
        })
    }

    pub fn leave(&self, role: Role, id: &str, generation: &str) -> Result<()> {
        self.change(|state| {
            match role {
                Role::Integrator => {
                    if state
                        .integrator
                        .as_ref()
                        .is_some_and(|s| s.id == id && s.generation == generation)
                    {
                        state.integrator = None;
                    }
                }
                Role::Dispatcher => {
                    if state
                        .dispatcher
                        .as_ref()
                        .is_some_and(|s| s.id == id && s.generation == generation)
                    {
                        state.dispatcher = None;
                    }
                }
                Role::Worker => {
                    if state
                        .workers
                        .get(id)
                        .is_some_and(|s| s.generation == generation)
                    {
                        state.workers.remove(id);
                    }
                }
            }
            Ok(())
        })
    }
}
