//! HS2-T16SK0: a placed step's run waits, in dispatcher mode, for the target's
//! dispatcher or legacy integrator to assign its ticket. This finds the runs a
//! running scheduler session waits on, reads the target's coordination pool through
//! the hosted session listing (`roles`), reports the runs nobody will assign, and,
//! after the policy's bounded wait, claims the ticket for the session's own worker id.
//! The decision itself is the pure `decide_assignment`.
use std::collections::BTreeMap;

use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::{
    AssignmentInput, AssignmentState, DispatchMode, Disposition, ExternalSource,
    OrchestratedSession, PlacementAction, PlacementMemory, PlacementOutcome, PolicyBody,
    ProjectSites, RequestKind, RolePrompt, Run, RunReconciliation, RunState, RunUpdate,
    SessionOwner, SessionRole, SessionState, SessionUpdate, SourceFinding, TargetAction,
    TargetOpError, Verification, decide_assignment,
};
use serde::Serialize;

use super::runs::field_ms;
use super::sessions::{HostedRoles, HostedSessionList};
use super::site_pool::{HostedPoolMember, HostedPoolReply, MAX_MEMBER_LEASE_SECONDS};
use super::tickets::encode;
use super::*;

/// The run reason when a session claimed its run's ticket because no dispatcher
/// would assign it.
pub const CLAIMED_FOR_SESSION: &str = "claimed_for_session";

/// HS2-ESVYQH: the decision-log detail of an assignment record for a run whose handoff
/// was returned for corrections.
pub const REVIEW_RETURNED: &str = "review_returned";

/// HS2-5P112Z: what one pass's relay did for a worker session on a site that is not
/// its project's home: what the home's pool answered, and whether the session was
/// told its assignment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SiteRelay {
    pub session_id: String,
    pub run_id: String,
    /// The site the session runs on.
    pub target_id: String,
    /// The project's home site, whose pool it joined.
    pub home_target_id: String,
    /// The home's answer; absent when it could not be asked (offline, refused, an
    /// older host).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool: Option<HostedPoolReply>,
    /// The session was told the home assigned it the run's ticket (`site_assigned`).
    pub told: bool,
    /// HS2-RCR4A6: when to ask the home again. Now when the home just recorded a step,
    /// so the project's role sessions are decided on the changed pool. Otherwise set
    /// while the run has seen the worker release its claim and the home still waits for
    /// that release, its note or its branch to reach the home's clone of the shared
    /// remote, so a handoff never waits for an unrelated event; the interval doubles
    /// from one second to a minute.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_at_ms: Option<u64>,
    /// HS2-RQDH0Q: the home's waiting code when the release has waited past
    /// [`SITE_HANDOFF_GIVE_UP_MS`] and the run now needs a person.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attention: Option<String>,
}

/// HS2-RCR4A6: the waiting codes that mean the home has not yet seen what the site
/// published: its release, its handoff note or its pushed branch.
pub const SITE_EVIDENCE_LAG: [&str; 4] = [
    "claim_not_taken",
    "claim_held",
    "handoff_note_missing",
    "handoff_fetch_failed",
];

/// HS2-RQDH0Q: how long after a site's release the home is asked again on a timer.
/// A handoff that has not reached the home by then is not going to without someone:
/// the run is marked as needing a person, and only an event asks the home again.
pub const SITE_HANDOFF_GIVE_UP_MS: u64 = 30 * 60 * 1000;

/// HS2-26Z8KB: whether the relay records a site session as joined now: it has not
/// joined yet, the home's pool answers that it is a member, and its role reached it
/// (delivered, given at launch, or the tool takes no message). The relay itself made
/// it a member, so with a prompt still pending or failed it proves nothing, and the
/// join check (HS2-NPTKS8) decides as before.
pub fn joins_through_home(
    joined_at_ms: Option<u64>,
    role_prompt: Option<RolePrompt>,
    home_member: bool,
) -> bool {
    joined_at_ms.is_none()
        && home_member
        && matches!(
            role_prompt,
            Some(RolePrompt::Delivered | RolePrompt::AtLaunch | RolePrompt::Unsupported)
        )
}

/// HS2-RCR4A6: the next time after `now_ms` to ask the home again about a release
/// the run saw at `released_at_ms`: one second after it, then doubling up to a minute
/// apart. Pure, so the schedule needs no memory between passes.
///
/// HS2-RQDH0Q: the last ask is at [`SITE_HANDOFF_GIVE_UP_MS`] after the release, and
/// from then on there is none (`None`): the run needs a person.
pub fn site_handoff_retry_at(released_at_ms: u64, now_ms: u64) -> Option<u64> {
    let give_up = released_at_ms.saturating_add(SITE_HANDOFF_GIVE_UP_MS);
    if now_ms >= give_up {
        return None;
    }
    let mut interval = 1_000_u64;
    let mut at = released_at_ms.saturating_add(interval);
    while at <= now_ms {
        interval = (interval * 2).min(60_000);
        at = at.saturating_add(interval);
    }
    Some(at.min(give_up))
}

/// HS2-RQDH0Q: the stored outcome of a release whose handoff the home still had not
/// seen when the timed asks gave up: `needs_attention`, with one finding naming the
/// home's waiting code, at the instant it gave up, so each release asks a person once.
pub fn site_handoff_attention(released_at_ms: u64, code: &str) -> RunReconciliation {
    let source = if code.starts_with("claim_") {
        ExternalSource::TargetClaim
    } else {
        ExternalSource::TargetTicket
    };
    RunReconciliation {
        at_ms: released_at_ms.saturating_add(SITE_HANDOFF_GIVE_UP_MS),
        disposition: Disposition::NeedsAttention,
        findings: vec![SourceFinding {
            source,
            verification: Verification::Mismatched,
            reason: Some(code.to_owned()),
        }],
    }
}

/// HS2-RQDH0Q: the home's waiting code when the run's stored outcome is a site
/// handoff that never arrived ([`site_handoff_attention`]).
pub fn site_handoff_waiting(run: &Run) -> Option<&str> {
    let found = run.reconciliation.as_ref()?;
    match found.findings.as_slice() {
        [finding] if found.disposition == Disposition::NeedsAttention => finding
            .reason
            .as_deref()
            .filter(|code| SITE_EVIDENCE_LAG.contains(code)),
        _ => None,
    }
}

/// One placed step's run that a running scheduler session waits on, with who will
/// assign it and what the pass did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Assignment {
    pub run_id: String,
    pub session_id: String,
    pub target_id: String,
    /// HS2-5P112Z: the project's home site whose pool assigns the run, when the
    /// session runs on another site of the project.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub home_target_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step_id: Option<String>,
    pub state: AssignmentState,
    /// The target's pool shows a live dispatcher; absent when it was not read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dispatcher_live: Option<bool>,
    /// The target's pool shows a live legacy integrator, which also assigns.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub integrator_live: Option<bool>,
    pub queued_at_ms: u64,
    /// HS2-ESVYQH: the run's handoff was returned for corrections at this time, and no
    /// correction is in flight: the wait for an assigner counts from here.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub returned_at_ms: Option<u64>,
    /// While nobody will assign it: when the session claims the ticket for itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claim_at_ms: Option<u64>,
    /// The session registered in the target's pool in its role (HS2-2DPGDY). A
    /// session that has not is not given a claim; its joining is the event that
    /// lets the claim go ahead.
    pub joined: bool,
    /// What an applied pass did: `claimed`, or `refused` with the code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome_detail: Option<String>,
    /// The pass should claim now.
    #[serde(skip)]
    pub(super) claim: bool,
    /// When this changes by time alone.
    #[serde(skip)]
    pub(super) retry_at_ms: Option<u64>,
    /// The worker id the session's claims carry.
    #[serde(skip)]
    pub(super) worker_id: String,
    /// HS2-ESVYQH: the `## Review return` note the claim answers, for a returned run.
    #[serde(skip)]
    pub(super) review_note: Option<String>,
}

/// HS2-ESVYQH: what one read of a target's pool brought in.
#[derive(Debug, Clone, Default)]
struct PoolRead {
    roles: HostedRoles,
    /// Tickets with a returned handoff and no correction in flight.
    returned: Vec<super::review_returns::ReturnedHandoff>,
    /// The pool's assigning role is live; `None` when unknown.
    assigner_live: Option<bool>,
}

/// HS2-ESVYQH: `found` is a return of the run's ticket recorded after the run was last
/// dispatched. HS2-SK9B3T: `found` comes from the pool of the project's `home` site,
/// which names the ticket by the home's own connection id.
pub(super) fn returned_for(
    found: &super::review_returns::ReturnedHandoff,
    run: &Run,
    sites: &ProjectSites,
    home: &str,
) -> bool {
    let connection = if home == run.target.target_id {
        run.target.connection_id.clone()
    } else {
        sites.connection_on(&run.target.target_id, &run.target.connection_id, home)
    };
    found.connection_id == connection
        && found.native_id == run.target.native_id
        && found.returned_at_ms >= dispatched_at(run)
}

/// When the run was last dispatched to a worker (0 if never).
pub(super) fn dispatched_at(run: &Run) -> u64 {
    run.history
        .iter()
        .rfind(|entry| entry.to == RunState::Dispatched)
        .map_or(0, |entry| entry.at_ms)
}

/// The plan step a scheduler run (`sched-run-<step>` or `sched-run-<step>.<n>`)
/// was dispatched for.
pub(super) fn step_of(run: &Run) -> Option<String> {
    let rest = run.operation_id.strip_prefix("sched-run-")?;
    let step = match rest.rsplit_once('.') {
        Some((step, n)) if n.parse::<u32>().is_ok_and(|n| n >= 2) => step,
        _ => rest,
    };
    (!step.is_empty()).then(|| step.to_owned())
}

/// When a run was last queued for the target's dispatcher.
fn queued_at(run: &Run) -> u64 {
    run.history
        .iter()
        .rfind(|entry| entry.to == RunState::Queued)
        .map_or(run.created_at_ms, |entry| entry.at_ms)
}

impl OrchestrationRuntime {
    /// The target's coordination pool as the hosted listing of `session` reports it:
    /// whether a dispatcher and an integrator are live. `None` when it could not be
    /// asked. One read, through the gate as `sessions_manage` continuing.
    fn pool_roles(
        &self,
        session: &OrchestratedSession,
        actor: &MutationActor,
        tx: &mut Transaction<'_>,
    ) -> Option<PoolRead> {
        let (target, authorized) = self
            .session_gate(session, Some(actor), true, None, tx)
            .ok()?;
        let outcome = (|| {
            let reply = self.hosted_call_at(
                Self::destination(&authorized)?,
                "/target-sessions",
                "GET",
                &format!("?operation_id={}", encode(&session.operation_id)),
                None,
                tx,
            )?;
            serde_json::from_value::<HostedSessionList>(reply)
                .map(|list| PoolRead {
                    roles: list.roles,
                    returned: list.returned,
                    assigner_live: list.assigner_live,
                })
                .map_err(|_| OrchestrationError::Observation(TargetFailure::InvalidResponse))
        })();
        self.finish(&target, &authorized, &outcome, tx).ok()?;
        outcome.ok()
    }

    /// Every dispatcher-mode run still `queued` that a running worker session the
    /// owner's orchestrator owns was given, decided against the target's pool. Each
    /// target's pool is read once, as `actor`; without one nothing is read and every
    /// run reads `pool_unknown`.
    ///
    /// HS2-ESVYQH: so is such a run that reads `intervened` because its handoff was
    /// returned for corrections after it was last dispatched, while the pool shows no
    /// correction in flight. Its wait counts from the return, and only the pool's
    /// assigning role (by its authority) counts as one who will assign it.
    pub(super) fn scheduler_assignments(
        &self,
        owner_id: &str,
        policy: &PolicyBody,
        memory: &[PlacementMemory],
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Vec<Assignment> {
        // HS2-ESVYQH: every unfinished run, `intervened` included (HS2-RCR4A6).
        let (Ok(sessions), Ok(runs)) = (
            self.session_list(owner_id, true),
            self.run_list(owner_id, false),
        ) else {
            return Vec::new();
        };
        let now_ms = tx.clock.now_ms();
        let wait_ms = u64::from(policy.placement.dispatcher_wait_minutes) * 60 * 1000;
        let mut roles: BTreeMap<String, Option<PoolRead>> = BTreeMap::new();
        // HS2-5P112Z: a run on a site that is not its project's home waits on the home's
        // pool, the one that assigns the whole project's work.
        let sites = self.active_project_sites(owner_id);
        // HS2-EX594V: homes this machine found unable to coordinate their project.
        let outages = self.home_outages(owner_id).unwrap_or_default();
        let mut found = Vec::new();
        for session in sessions.iter().filter(|session| {
            session.state == SessionState::Running
                && session.owner == SessionOwner::Orchestrator
                && session.role == SessionRole::Worker
        }) {
            let (Some(hosted), Some(run)) = (
                &session.hosted,
                session
                    .run_id
                    .as_ref()
                    .and_then(|run_id| runs.iter().find(|run| &run.run_id == run_id)),
            ) else {
                continue;
            };
            if run.mode != DispatchMode::Dispatcher
                || !matches!(run.state, RunState::Queued | RunState::Intervened)
                || run.paused.is_some()
                || run.cancel_requested
            {
                continue;
            }
            let home = sites.home_of(&session.target_id).to_owned();
            let pool = match roles.get(&home) {
                Some(known) => known.clone(),
                None => {
                    let read = actor.and_then(|actor| {
                        if home == session.target_id {
                            self.pool_roles(session, actor, tx)
                        } else {
                            // HS2-SK9B3T: the home's pool route reports its returns and
                            // whether its assigning role is live, as the hosted session
                            // listing does for a worker on the home itself.
                            self.home_pool(session, &home, None, &sites, actor, tx)
                                .map(|reply| PoolRead {
                                    roles: reply.roles,
                                    returned: reply.returned,
                                    assigner_live: reply.assigner_live,
                                })
                        }
                    });
                    roles.insert(home.clone(), read.clone());
                    read
                }
            };
            // HS2-ESVYQH: an intervened run is only this pass's business when its
            // handoff came back for corrections since it was last dispatched.
            // HS2-SK9B3T: the pool names the ticket by its home's connection id.
            let returned = (run.state == RunState::Intervened)
                .then(|| {
                    pool.as_ref()?
                        .returned
                        .iter()
                        .find(|found| returned_for(found, run, &sites, &home))
                })
                .flatten()
                .cloned();
            if run.state == RunState::Intervened && returned.is_none() {
                continue;
            }
            let queued_at_ms = queued_at(run);
            let backoff_until_ms = memory
                .iter()
                .find(|entry| entry.target_id == session.target_id)
                .and_then(PlacementMemory::refused_until_ms);
            // HS2-EX594V: a site's run whose home is recorded unavailable, and whose
            // pool this pass could not read, has nobody to assign it: no role session
            // starts while the home is out (§26.30). It keeps the bounded wait and then
            // claims its ticket for its own session, instead of waiting on the home.
            let home_out = pool.is_none()
                && home != session.target_id
                && outages.iter().any(|outage| outage.home_target_id == home);
            let dispatcher_live = pool
                .as_ref()
                .and_then(|pool| pool.roles.dispatcher_live)
                .or(home_out.then_some(false));
            let integrator_live = pool
                .as_ref()
                .and_then(|pool| pool.roles.integrator_live)
                .or(home_out.then_some(false));
            let input = match &returned {
                // Only the pool's assigning role gives out a correction.
                Some(found) => {
                    let assigner = pool.as_ref().and_then(|pool| pool.assigner_live);
                    AssignmentInput {
                        dispatcher_live: assigner,
                        integrator_live: assigner.map(|_| false),
                        queued_at_ms: found.returned_at_ms,
                        wait_ms,
                        backoff_until_ms,
                        now_ms,
                    }
                }
                None => AssignmentInput {
                    dispatcher_live,
                    integrator_live,
                    queued_at_ms,
                    wait_ms,
                    backoff_until_ms,
                    now_ms,
                },
            };
            let decided = decide_assignment(&input);
            let joined = session.joined_at_ms.is_some();
            found.push(Assignment {
                run_id: run.run_id.clone(),
                session_id: session.session_id.clone(),
                target_id: session.target_id.clone(),
                home_target_id: (home != session.target_id).then_some(home),
                step_id: step_of(run),
                state: decided.state,
                dispatcher_live,
                integrator_live,
                queued_at_ms,
                returned_at_ms: returned.as_ref().map(|found| found.returned_at_ms),
                claim_at_ms: (decided.state == AssignmentState::NoLiveDispatcher).then(|| {
                    input
                        .queued_at_ms
                        .saturating_add(wait_ms)
                        .max(backoff_until_ms.unwrap_or(0))
                }),
                joined,
                outcome: None,
                outcome_detail: None,
                claim: decided.claim && joined,
                retry_at_ms: decided.retry_at_ms,
                worker_id: hosted.worker_id.clone(),
                review_note: returned.map(|found| found.review_note),
            });
        }
        found
    }

    /// HS2-ESVYQH: the handoffs a target's pool returned for corrections with no
    /// correction in flight, from its hosted session listing, read as `actor` like a
    /// capacity read (`gates_observe`). `None` when it could not be asked.
    pub(super) fn target_returns(
        &self,
        owner_id: &str,
        target_id: &str,
        filter_operation: &str,
        actor: &MutationActor,
        tx: &mut Transaction<'_>,
    ) -> Option<Vec<super::review_returns::ReturnedHandoff>> {
        let target = Self::target_ref(owner_id, target_id).ok()?;
        let authorized = self
            .authorize(
                &target,
                Some(actor),
                ActionRequest::new(TargetAction::GatesObserve),
                tx,
            )
            .ok()?;
        let outcome = (|| {
            // One operation id narrows the listing to (at most) that session: only the
            // pool's facts are wanted.
            let reply = self.hosted_call_at(
                Self::destination(&authorized)?,
                "/target-sessions",
                "GET",
                &format!("?operation_id={}", encode(filter_operation)),
                None,
                tx,
            )?;
            serde_json::from_value::<HostedSessionList>(reply)
                .map(|list| list.returned)
                .map_err(|_| OrchestrationError::Observation(TargetFailure::InvalidResponse))
        })();
        self.finish(&target, &authorized, &outcome, tx).ok()?;
        outcome.ok()
    }

    /// HS2-SK9B3T: the returns of the pool that assigns `target_id`'s work: its own
    /// listing's when it is its project's home (or in no project), else the home's
    /// `target-pool` answer, read without a member as `gates_observe`. Each return names
    /// its ticket by the home's own connection id ([`returned_for`] matches it).
    pub(super) fn project_returns(
        &self,
        owner_id: &str,
        target_id: &str,
        filter_operation: &str,
        sites: &ProjectSites,
        actor: &MutationActor,
        tx: &mut Transaction<'_>,
    ) -> Option<Vec<super::review_returns::ReturnedHandoff>> {
        let home = sites.home_of(target_id);
        if home == target_id {
            return self.target_returns(owner_id, target_id, filter_operation, actor, tx);
        }
        let target = Self::target_ref(owner_id, home).ok()?;
        let authorized = self
            .authorize(
                &target,
                Some(actor),
                ActionRequest::new(TargetAction::GatesObserve),
                tx,
            )
            .ok()?;
        let outcome = (|| {
            let reply = self.hosted_call_at(
                Self::destination(&authorized)?,
                "/target-pool",
                "GET",
                "",
                None,
                tx,
            )?;
            serde_json::from_value::<HostedPoolReply>(reply)
                .map(|reply| reply.returned)
                .map_err(|_| OrchestrationError::Observation(TargetFailure::InvalidResponse))
        })();
        self.finish(&target, &authorized, &outcome, tx).ok()?;
        outcome.ok()
    }

    /// HS2-5P112Z: the active grouping of the owner's targets into projects. An
    /// unreadable manifest groups nothing, so every target is its own home.
    pub(super) fn active_project_sites(&self, owner_id: &str) -> ProjectSites {
        let sites = self
            .declarations(owner_id)
            .map(|view| ProjectSites::active(&view.targets))
            .unwrap_or_default();
        self.with_bound_sources(owner_id, sites)
    }

    /// HS2-5P112Z: the home site's pool as it answers for `session`, a worker on another
    /// site of the project. With `run` the session joins (or renews in) that pool and
    /// its assignment is synced from the ticket's evidence, through the gate as
    /// `work_start` continuing the run's dispatch; without one it is only read, as
    /// `gates_observe`, like a capacity read. `None` when the home could not be asked.
    fn home_pool(
        &self,
        session: &OrchestratedSession,
        home: &str,
        run: Option<&Run>,
        sites: &ProjectSites,
        actor: &MutationActor,
        tx: &mut Transaction<'_>,
    ) -> Option<HostedPoolReply> {
        let worker = session.hosted.as_ref()?.worker_id.clone();
        let target = Self::target_ref(&session.owner_checkout_id, home).ok()?;
        // Joining continues the run's own dispatch, as a claim for its session does.
        let request = match run {
            Some(run) => ActionRequest::new(TargetAction::WorkStart)
                .operation(run.operation_id.clone())
                .continuing(),
            None => ActionRequest::new(TargetAction::GatesObserve),
        };
        let authorized = self.authorize(&target, Some(actor), request, tx).ok()?;
        let outcome = (|| {
            let reply = match run {
                Some(run) => {
                    let body = serde_json::to_value(HostedPoolMember {
                        worker_id: worker.clone(),
                        generation: session.session_id.clone(),
                        lease_seconds: MAX_MEMBER_LEASE_SECONDS,
                        site_target_id: session.target_id.clone(),
                        // HS2-3JGPDR: the ticket as the home names it.
                        connection_id: Some(sites.connection_on(
                            &run.target.target_id,
                            &run.target.connection_id,
                            home,
                        )),
                        native_id: Some(run.target.native_id.clone()),
                        actor: Some(actor.clone()),
                    })
                    .map_err(|_| OrchestrationError::Corrupt)?;
                    self.hosted_call_at(
                        Self::destination(&authorized)?,
                        "/target-pool",
                        "POST",
                        "",
                        Some(&body),
                        tx,
                    )?
                }
                None => self.hosted_call_at(
                    Self::destination(&authorized)?,
                    "/target-pool",
                    "GET",
                    &format!(
                        "?worker_id={}&generation={}",
                        encode(&worker),
                        encode(&session.session_id)
                    ),
                    None,
                    tx,
                )?,
            };
            serde_json::from_value::<HostedPoolReply>(reply)
                .map_err(|_| OrchestrationError::Observation(TargetFailure::InvalidResponse))
        })();
        self.finish(&target, &authorized, &outcome, tx).ok()?;
        outcome.ok()
    }

    /// HS2-5P112Z: an applied pass relays every running worker session the owner's
    /// orchestrator owns on a site that is not its project's home: it joins the
    /// home's pool (renewing its availability), the home records what the shared
    /// ticket says the worker did (accepted, or handed off over a pushed branch), and
    /// a session whose run's ticket the home's dispatcher offered it is told so
    /// (`site_assigned`; the host pushes the same request only once).
    pub(super) fn site_pool_relay(
        &self,
        owner_id: &str,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Vec<SiteRelay> {
        let Some(actor) = actor else {
            return Vec::new();
        };
        let sites = self.active_project_sites(owner_id);
        if sites.is_empty() {
            return Vec::new();
        }
        // HS2-RCR4A6: every unfinished run, `intervened` included. A site worker's
        // release turns its run `intervened`, and that is the run the relay must carry to
        // the home so the handoff is recorded; the active-only list left it out.
        let (Ok(sessions), Ok(runs)) = (
            self.session_list(owner_id, true),
            self.run_list(owner_id, false),
        ) else {
            return Vec::new();
        };
        let mut relays = Vec::new();
        for session in sessions.iter().filter(|session| {
            session.state == SessionState::Running
                && session.owner == SessionOwner::Orchestrator
                && session.role == SessionRole::Worker
                && session.hosted.is_some()
        }) {
            let home = sites.home_of(&session.target_id);
            if home == session.target_id {
                continue;
            }
            let Some(run) = session
                .run_id
                .as_ref()
                .and_then(|run_id| runs.iter().find(|run| &run.run_id == run_id))
                .filter(|run| !run.state.is_terminal())
            else {
                continue;
            };
            let pool = self.home_pool(session, home, Some(run), &sites, actor, tx);
            // HS2-26Z8KB: a site session is joined when it is a member of its project's
            // home pool, too, not only of its own site clone's pool: a site worker that
            // registers only through this relay is joined once the home says so, so
            // its claim does not wait on a local registration that never comes and the
            // join check does not wind it down. The relay made it a member itself, so
            // that counts only once its role reached it: a prompt still pending or
            // failed is left to the join check. The join is kept (only the first time
            // counts) and published as `session_joined`, which wakes the loop.
            if joins_through_home(
                session.joined_at_ms,
                session.role_prompt,
                pool.as_ref().is_some_and(|reply| reply.joined),
            ) {
                let _ = self.session_apply(
                    owner_id,
                    &session.session_id,
                    SessionUpdate {
                        joined_at_ms: Some(tx.clock.now_ms()),
                        actor_id: actor.id.clone(),
                        ..SessionUpdate::default()
                    },
                    false,
                    tx,
                );
            }
            // HS2-3JGPDR: the home's pool names the ticket by the home's connection id.
            let offered = pool
                .as_ref()
                .and_then(|reply| reply.assignment.as_ref())
                .is_some_and(|assigned| {
                    assigned.phase == crate::coordination::Phase::Offered
                        && assigned.connection_id
                            == sites
                                .home_connection(&run.target.target_id, &run.target.connection_id)
                        && assigned.native_id == run.target.native_id
                });
            let told = offered
                && self
                    .session_tell(
                        owner_id,
                        &session.session_id,
                        Some((RequestKind::SiteAssigned, None)),
                        Some(actor),
                        tx,
                    )
                    .is_ok();
            let assigned = pool.as_ref().and_then(|reply| reply.assignment.as_ref());
            let lagging = assigned
                .and_then(|assigned| assigned.waiting.as_deref())
                .filter(|code| SITE_EVIDENCE_LAG.contains(code));
            // A step the home just recorded changes its pool after this pass decided
            // the project's role sessions: the next pass decides them again at once, so
            // a handoff starts the integrator without waiting for another event.
            let recorded = assigned.is_some_and(|assigned| assigned.recorded.is_some());
            let released_at_ms = (run.state == RunState::Intervened)
                .then(|| {
                    run.history
                        .iter()
                        .rev()
                        .find(|transition| transition.to == RunState::Intervened)
                        .map(|transition| transition.at_ms)
                })
                .flatten();
            let now_ms = tx.clock.now_ms();
            let lag = released_at_ms.zip(lagging);
            let retry_at_ms = lag.and_then(|(at, _)| site_handoff_retry_at(at, now_ms));
            // HS2-RQDH0Q: past the cap a person is asked instead of the home on a timer.
            let attention = lag
                .filter(|_| retry_at_ms.is_none())
                .map(|(at, code)| site_handoff_attention(at, code));
            self.site_handoff_settle(owner_id, run, attention.as_ref(), pool.is_some(), actor, tx);
            relays.push(SiteRelay {
                session_id: session.session_id.clone(),
                run_id: run.run_id.clone(),
                target_id: session.target_id.clone(),
                home_target_id: home.to_owned(),
                pool,
                told,
                retry_at_ms: if recorded { Some(now_ms) } else { retry_at_ms },
                attention: attention
                    .as_ref()
                    .and_then(|found| found.findings.first())
                    .and_then(|finding| finding.reason.clone()),
            });
        }
        relays
    }

    /// HS2-RQDH0Q: keep the run's stored outcome in line with its site handoff. A
    /// handoff that gave up is stored as `needs_attention` (once per release: the
    /// same outcome again writes nothing). A stored site-handoff outcome that no longer
    /// holds, because the home answered without waiting or a newer release is being
    /// asked about, is replaced by reconciling the run against the world.
    fn site_handoff_settle(
        &self,
        owner_id: &str,
        run: &Run,
        attention: Option<&RunReconciliation>,
        answered: bool,
        actor: &MutationActor,
        tx: &mut Transaction<'_>,
    ) {
        let stored = site_handoff_waiting(run).and(run.reconciliation.as_ref());
        match (attention, stored) {
            (Some(found), stored) if stored.is_none_or(|stored| !stored.same_facts(found)) => {
                let _ = self.run_apply(
                    owner_id,
                    &run.run_id,
                    RunUpdate {
                        reconciliation: Some(found.clone()),
                        actor_id: actor.id.clone(),
                        ..RunUpdate::default()
                    },
                    tx,
                );
            }
            (None, Some(_)) if answered => {
                let _ = self.run_reconcile(owner_id, &run.run_id, Some(actor), tx);
            }
            _ => {}
        }
    }

    /// An applied pass claims each run that is due for its session, records what
    /// happened on the assignment and audits it. A claim someone else took is
    /// learned by the run's own sync; any other failure backs the target off like a
    /// refused start, so it is not tried again on every event.
    pub(super) fn assignment_apply(
        &self,
        owner_id: &str,
        assignments: &mut [Assignment],
        actor: Option<&MutationActor>,
        applied: &mut Vec<(String, PlacementOutcome)>,
        tx: &mut Transaction<'_>,
    ) {
        let Some(actor) = actor else {
            return;
        };
        for assignment in assignments.iter_mut().filter(|found| found.claim) {
            match self.assignment_claim(owner_id, assignment, actor, tx) {
                Ok(_) => {
                    assignment.outcome = Some("claimed".into());
                    // HS2-R7QH35: the session is told the ticket is now its own.
                    // HS2-ESVYQH: after a return, told to make the corrections, once
                    // per return.
                    let _ = match &assignment.review_note {
                        Some(note) => self.session_tell(
                            owner_id,
                            &assignment.session_id,
                            Some((RequestKind::Returned, Some(note.clone()))),
                            Some(actor),
                            tx,
                        ),
                        None => self.session_tell_ticket(
                            owner_id,
                            &assignment.session_id,
                            Some(actor),
                            tx,
                        ),
                    };
                }
                Err(error) => {
                    assignment.outcome = Some("refused".into());
                    assignment.outcome_detail = Some(error.code().into());
                    // HS2-ESVYQH: a returned run stays intervened whoever holds its
                    // ticket, so a conflict there backs off like any refusal rather
                    // than being tried on every pass.
                    if assignment.returned_at_ms.is_none()
                        && matches!(
                            error,
                            OrchestrationError::TargetOperation(TargetOpError::Conflict)
                        )
                    {
                        // Someone took the ticket meanwhile: the run learns who now,
                        // so the next pass does not try again.
                        let _ = self.run_sync(owner_id, &assignment.run_id, Some(actor), tx);
                    } else {
                        applied.push((
                            assignment.target_id.clone(),
                            PlacementOutcome::Refused(error.code().into()),
                        ));
                    }
                }
            }
            self.scheduler_audit(
                owner_id,
                "scheduler_assignment",
                &DecisionOutcome {
                    action: PlacementAction::Keep,
                    target_id: assignment.target_id.clone(),
                    session_id: Some(assignment.session_id.clone()),
                    step_id: assignment.step_id.clone(),
                    outcome: assignment.outcome.clone().unwrap_or_default(),
                    detail: assignment.outcome_detail.clone(),
                    operation_id: assignment.run_id.clone(),
                    run_id: Some(assignment.run_id.clone()),
                },
                assignment.outcome.as_deref() == Some("claimed"),
                Some(actor),
                tx.clock.now_ms(),
            );
        }
    }

    /// Claim a queued run's ticket for the session's worker id, because nobody in the
    /// target's pool will assign it. Passes the gate as `work_start` continuing the
    /// run's own dispatch, so a person is not asked again. A ticket
    /// someone else claimed meanwhile is a conflict; nothing is forced.
    pub(super) fn assignment_claim(
        &self,
        owner_id: &str,
        assignment: &Assignment,
        actor: &MutationActor,
        tx: &mut Transaction<'_>,
    ) -> Result<Run, OrchestrationError> {
        // HS2-G9A9DF: the target's claim and the run's `claimed_for_session` record change
        // together, as a dispatch does (HS2-EZV83C). Without the run's operation lock, a
        // sync between the two found the new claim on a still-queued run and recorded it
        // as `assigned_by_dispatcher`.
        let _operation = self.run_lock(owner_id, &assignment.run_id)?;
        let run = self.run(owner_id, &assignment.run_id)?;
        // HS2-ESVYQH: a returned run is intervened when its correction is claimed.
        let claimable = match assignment.returned_at_ms {
            Some(_) => run.state == RunState::Intervened,
            None => run.state == RunState::Queued,
        };
        if !claimable || run.mode != DispatchMode::Dispatcher {
            return Err(OrchestrationError::Run(
                hotsheet_ticketing::orchestration::RunError::NotActive,
            ));
        }
        let target = Self::target_ref(owner_id, &run.target.target_id)?;
        let authorized = self.authorize(
            &target,
            Some(actor),
            ActionRequest::new(TargetAction::WorkStart)
                .operation(run.operation_id.clone())
                .continuing(),
            tx,
        )?;
        let outcome = (|| {
            let body = serde_json::to_value(HostedClaim {
                connection_id: run.target.connection_id.clone(),
                native_id: run.target.native_id.clone(),
                worker: assignment.worker_id.clone(),
                label: None,
                lease_minutes: None,
            })
            .map_err(|_| OrchestrationError::Corrupt)?;
            let hosted: HostedTicket = serde_json::from_value(self.hosted_call_at(
                Self::destination(&authorized)?,
                "/target-claims",
                "POST",
                "",
                Some(&body),
                tx,
            )?)
            .map_err(|_| OrchestrationError::Observation(TargetFailure::InvalidResponse))?;
            let claimed = self.run_apply(
                owner_id,
                &run.run_id,
                RunUpdate {
                    to: Some(RunState::Dispatched),
                    reason: Some(CLAIMED_FOR_SESSION.into()),
                    worker_id: Some(assignment.worker_id.clone()),
                    lease_expires_at_ms: Some(field_ms(&hosted.ticket, "claim_lease_expires_at")),
                    actor_id: actor.id.clone(),
                    ..RunUpdate::default()
                },
                tx,
            )?;
            self.run_note(owner_id, &claimed, "claimed for its session", tx);
            Ok(claimed)
        })();
        self.finish(&target, &authorized, &outcome, tx)?;
        outcome
    }
}
