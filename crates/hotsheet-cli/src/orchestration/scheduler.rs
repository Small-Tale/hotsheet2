//! HS2-ECWG50: the throughput scheduler over the shared runtime.
//!
//! One pass gathers this machine's records (ready work, the policy, tool capacity,
//! placement memory), reads each involved target's capacity through the gate, lists
//! the running sessions through [`SessionExecutor`], and hands all of it to the pure
//! core (`hotsheet_ticketing::orchestration::plan`).
//!
//! Applying a decision is the [`SessionExecutor`] seam; starting and stopping sessions
//! is HS2-GDJXGZ. A pass applies decisions only when the policy enables automatic
//! placement. Otherwise, and for `scheduler_plan`, it is a dry run: computed, and for
//! a tick logged, with nothing started or wound down.
//!
//! Inside the policy's limits the scheduler acts and logs. A decision the gate would
//! not allow is passed through `authorize`, which files a permission request shown
//! through the permission bridge and records the denial (HS2-XHQ1EH); nothing is done
//! about it until a person answers, and the answer wakes the scheduler.
//!
//! Nothing here runs on a timer. [`OrchestrationRuntime::scheduler_follow_step`]
//! waits on the run event journal and the scheduler's wake marker, and otherwise only
//! until the time a decision is known to change.
use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};
use std::time::Duration;

use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::{
    AssignmentState, Blocker, CapacityState, DecisionJournal, DecisionKind, DecisionMode,
    DecisionPage, DecisionRecord, GateFilter, GateRegistry, GateState, HeldResource,
    MAX_SCHEDULE_SESSIONS, OfflinePolicy, Placement, PlacementAction, PlacementOutcome, PolicyBody,
    Prerequisite, ProvisionalStanding, ReadinessLevel, ResourceNeed, ResourceStanding,
    RunEventJournal, RunEventKind, Schedule, ScheduleInput, ScheduledSession, SchedulerStore,
    ScoreParts, SessionActivity, SignalSupport, TargetState, TickInputs, ToolCapacityRegistry,
    Unplaced, UnreadableClaims, UnreadableReason, WorkItem, WorkLevel, WorkSpeculation, plan,
    resource_standings, state_at, summarize,
};
use serde::{Deserialize, Serialize};

use super::*;

/// The longest one follow step waits for an event.
pub const MAX_SCHEDULER_WAIT_MS: u64 = 25_000;
const MAX_DECISION_PAGE: usize = 200;
pub(super) const WAKE_DIRECTORY: &str = "orchestration-scheduler-wake";

// ---- Requests and views ----

/// `plan` takes no member of its own; an unknown one is refused.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanRequest {}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TickRequest {
    /// Compute and log, and apply nothing, whatever the policy says.
    #[serde(default)]
    pub dry_run: bool,
}

/// The work to explain: a plan step, or the exact target ticket a step works.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExplainRequest {
    #[serde(default)]
    pub step_id: Option<String>,
    #[serde(default)]
    pub target_id: Option<String>,
    #[serde(default)]
    pub connection_id: Option<String>,
    #[serde(default)]
    pub native_id: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionsRequest {
    #[serde(default)]
    pub after: u64,
    #[serde(default)]
    pub limit: Option<usize>,
}

/// One computed schedule with what it was computed from.
#[derive(Debug, Clone, Serialize)]
pub struct ScheduleView {
    pub now_ms: u64,
    /// False when the project has no policy: nothing is scheduled without one.
    pub enabled: bool,
    /// What a tick would do with this schedule now.
    pub mode: DecisionMode,
    pub inputs: TickInputs,
    #[serde(flatten)]
    pub schedule: Schedule,
    /// HS2-T16SK0: each placed step's run a running session waits on for the target's
    /// dispatcher, with who will assign it and, applied, whether its session claimed
    /// it. Absent when none waits.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub assignments: Vec<super::assignment::Assignment>,
}

/// What applying one placement did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DecisionOutcome {
    pub action: PlacementAction,
    pub target_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step_id: Option<String>,
    /// `started`, `adopted` (HS2-DG0YPA), `assigned`, `wind_down_requested`,
    /// `executor_unavailable`, `refused`,
    /// `not_at_safe_point`, `session_not_found`, or the gate's denial code
    /// (`permission_required` with the request id as `detail`, `permission_denied`,
    /// `policy_denied` with its limit, `target_unavailable`).
    pub outcome: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub operation_id: String,
    /// HS2-YC2P7K: the run dispatched for the step of a start or a refill.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TickView {
    pub tick_id: String,
    pub trigger: String,
    #[serde(flatten)]
    pub view: ScheduleView,
    /// Empty for a dry run.
    pub outcomes: Vec<DecisionOutcome>,
    /// Decision records this pass appended to the log.
    pub logged: usize,
}

/// Why one piece of work is, or is not, running.
#[derive(Debug, Clone, Serialize)]
pub struct Explanation {
    pub now_ms: u64,
    /// An unfinished run stands on the ticket.
    pub running: bool,
    /// `running`, `finished`, `not_planned`, `not_ready`, `start`, `keep` or
    /// `not_placed`.
    pub standing: String,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<ReadinessLevel>,
    /// The reason code of each prerequisite that does not hold.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub readiness_reasons: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rank: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<ScoreParts>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_at_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placement: Option<Placement>,
    /// HS2-2FG91X: every resource ready work requires, and how this pass saw it.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<RequiredResource>,
    /// HS2-JSF985: for a provisional step under a policy that opts in, the gates it
    /// would rest on (`bases`) or why the provisional rule refuses it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speculation: Option<ProvisionalStanding>,
    /// HS2-JSF985: the work already started on this step ahead of a pending gate,
    /// with each gate and its result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provisional: Option<ProvisionalWork>,
    /// HS2-JSF985: the steps whose provisional work stands on a failed gate of this
    /// step's ticket. Its work is that gate's repair.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub provisional_dependents: Vec<String>,
    /// HS2-T16SK0: for a running step whose run waits for the target's dispatcher,
    /// who will assign it and when its session claims it for itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignment: Option<super::assignment::Assignment>,
    /// `dry_run` when a placement shown here would not be applied by a tick.
    pub mode: DecisionMode,
}

/// One resource a step's work requires, with who declared it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RequiredResource {
    #[serde(flatten)]
    pub standing: ResourceStanding,
    /// `step` when the plan step names it, `target` when the policy requires it of
    /// every piece of work on the target.
    pub declared_by: &'static str,
}

/// What a host's follower remembers between steps.
#[derive(Debug, Default)]
pub struct SchedulerFollowState {
    started: bool,
    cursor: u64,
    wake: Option<String>,
    digest: Option<u64>,
    pub(super) retry_at_ms: Option<u64>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SchedulerStep {
    /// The owner's policy enables automatic placement; call again.
    pub active: bool,
    /// What caused a pass in this step: `start`, `event`, `signal` or `retry`.
    pub trigger: Option<&'static str>,
    /// The pass could not be completed; the host backs off before the next step.
    pub failed: bool,
    pub retry_at_ms: Option<u64>,
}

/// Everything one pass reads.
pub(super) struct Gathered {
    pub(super) policy: PolicyBody,
    revision: Option<String>,
    pub(super) work: Vec<WorkItem>,
    targets: Vec<TargetState>,
    sessions: Vec<ScheduledSession>,
    /// `available`, `unavailable` or `failed`.
    pub(super) executor: &'static str,
    entries: Vec<hotsheet_ticketing::orchestration::CapacityEntry>,
    supports: BTreeMap<String, SignalSupport>,
    held: Vec<HeldResource>,
    /// HS2-2FG91X: namespaces whose claims a machine could not read.
    unreadable: Vec<UnreadableClaims>,
    memory: Vec<hotsheet_ticketing::orchestration::PlacementMemory>,
    /// HS2-JSF985: the provisional rule's answer per provisional step. Empty while
    /// the policy does not opt in.
    pub(super) standings: BTreeMap<String, ProvisionalStanding>,
}

impl Gathered {
    /// HS2-2FG91X: every resource the named steps' work requires, each once.
    pub(super) fn resources_of(&self, steps: &[String]) -> Vec<ResourceNeed> {
        let mut needs: Vec<ResourceNeed> = Vec::new();
        for item in self
            .work
            .iter()
            .filter(|item| steps.contains(&item.step_id))
        {
            for need in &item.resources {
                if !needs.contains(need) {
                    needs.push(need.clone());
                }
            }
        }
        needs
    }
}

enum Logging<'a> {
    Always,
    /// A follower logs a pass only when its decisions differ from the last logged.
    Changed(&'a mut Option<u64>),
}

impl OrchestrationRuntime {
    /// Replace who starts and stops sessions (the session-hosting server, tests).
    #[must_use]
    pub fn with_session_executor(mut self, sessions: Arc<dyn SessionExecutor>) -> Self {
        self.session_executor = sessions;
        self
    }

    fn decisions(&self, owner_id: &str) -> Result<DecisionJournal, OrchestrationError> {
        DecisionJournal::new(&self.home, owner_id)
    }

    /// Tell every scheduler following on this machine that something outside the
    /// run event stream changed: a tool limit, a policy. Best effort.
    pub fn scheduler_wake_at(home: &Path) {
        let directory = home.join(WAKE_DIRECTORY);
        if std::fs::create_dir_all(&directory).is_ok() {
            let _ = std::fs::write(directory.join("token"), ulid::Ulid::new().to_string());
        }
    }

    /// HS2-2FG91X: wake the owner's scheduling loop, when its policy runs one. An
    /// owner without automatic placement has no loop, and nothing is written for it.
    pub(super) fn scheduler_wake(&self, owner_id: &str) {
        if matches!(self.scheduler_actor(owner_id), Ok(Some(_))) {
            Self::scheduler_wake_at(&self.home);
        }
    }

    fn wake_token(&self) -> Option<String> {
        std::fs::read_to_string(self.home.join(WAKE_DIRECTORY).join("token")).ok()
    }

    /// Owning checkouts whose policy enables automatic placement.
    pub fn scheduler_owners(&self) -> Vec<String> {
        self.checkouts
            .list()
            .unwrap_or_default()
            .into_iter()
            .filter(|checkout| {
                self.policy(&checkout.id)
                    .ok()
                    .and_then(|view| view.policy)
                    .is_some_and(|policy| policy.placement.automatic)
            })
            .map(|checkout| checkout.id)
            .collect()
    }

    fn scheduler_work(
        &self,
        owner_id: &str,
        tx: &mut Transaction<'_>,
    ) -> Result<Vec<WorkItem>, OrchestrationError> {
        // Steps blocked by a failed gate, per prerequisite ticket: the work on that
        // ticket is the gate's repair.
        let mut stacked: BTreeMap<(String, String, String), u32> = BTreeMap::new();
        let plan = self.readiness_plan(owner_id, tx)?;
        for step in &plan.steps {
            if step.readiness.level != ReadinessLevel::Blocked {
                continue;
            }
            let failed: BTreeSet<_> = step
                .readiness
                .reasons
                .iter()
                .filter(|reason| reason.code == "gate_failed")
                .filter_map(|reason| match &reason.prerequisite {
                    Some(Prerequisite::Ticket {
                        target_id,
                        connection_id,
                        native_id,
                        ..
                    }) => Some((target_id.clone(), connection_id.clone(), native_id.clone())),
                    _ => None,
                })
                .collect();
            for key in failed {
                *stacked.entry(key).or_default() += 1;
            }
        }
        // HS2-JSF985: provisional work at risk on a failed gate stands on it too. Its
        // run keeps its step in progress, so it was not counted above.
        for (key, steps) in self.provisional_stacked(owner_id)? {
            *stacked.entry(key).or_default() += u32::try_from(steps.len()).unwrap_or(u32::MAX);
        }
        // HS2-HV9ED3: a step whose run a person cancelled is held, not placed again.
        let all_runs = self.run_list(owner_id, false)?;
        let held: BTreeSet<String> = plan
            .steps
            .iter()
            .filter(|view| super::cancel_hold::held_by(&view.readiness.step, &all_runs).is_some())
            .map(|view| view.readiness.step.step_id.clone())
            .collect();
        let mut work: Vec<WorkItem> = self
            .ready_work(owner_id, tx)?
            .into_iter()
            .filter(|item| !held.contains(&item.step_id))
            .map(|item| WorkItem {
                red_gate_stack: stacked
                    .get(&(
                        item.target.target_id.clone(),
                        item.target.connection_id.clone(),
                        item.target.native_id.clone(),
                    ))
                    .copied()
                    .unwrap_or(0),
                step_id: item.step_id,
                target_id: item.target.target_id,
                connection_id: item.target.connection_id,
                native_id: item.target.native_id,
                level: match item.level {
                    ReadinessLevel::Confirmed => WorkLevel::Confirmed,
                    _ => WorkLevel::Provisional,
                },
                priority: item.priority,
                up_next: item.up_next.unwrap_or(false),
                // Due dates, milestones and a preferred tool are not in a plan step
                // yet (HS2-TH53NN).
                due_at_ms: None,
                unblocks: u32::try_from(item.unblocks.len()).unwrap_or(u32::MAX),
                ready_since_ms: item.ready_since_ms,
                created_at_ms: item.ticket_created_at_ms,
                tool: None,
                model: None,
                // HS2-2FG91X: the step's own; the policy's are added per target.
                resources: item.resources,
                // HS2-JSF985: decided once the involved targets were read.
                speculation: None,
            })
            .collect();
        // HS2-YC2P7K: a step whose scheduler run is unfinished and no unfinished
        // session works it (its start was refused, or its session was lost) needs a
        // session again. The run is reused, so nothing is dispatched twice.
        let runs = self.run_list(owner_id, true)?;
        let sessions = self.session_list(owner_id, true)?;
        for view in plan.steps {
            let step = view.readiness.step;
            if view.readiness.level != ReadinessLevel::InProgress {
                continue;
            }
            let Some(run) = runs
                .iter()
                .find(|run| super::session_executor::scheduled_run_of(run, &step.step_id))
            else {
                continue;
            };
            if sessions
                .iter()
                .any(|session| session.run_id.as_deref() == Some(run.run_id.as_str()))
            {
                continue;
            }
            // HS2-3REXQ7: a run that waits on a gate, `final_testing` or a validation
            // attempt needs no live session, as reconciliation judges it; its step is
            // placeable again only when the run needs work.
            if self.run_waits(owner_id, run, tx.clock.now_ms()).is_some() {
                continue;
            }
            work.push(WorkItem {
                step_id: step.step_id,
                target_id: step.target.target_id,
                connection_id: step.target.connection_id,
                native_id: step.target.native_id,
                level: WorkLevel::Confirmed,
                priority: None,
                up_next: false,
                due_at_ms: None,
                unblocks: u32::try_from(view.readiness.unblocks.len()).unwrap_or(u32::MAX),
                red_gate_stack: 0,
                ready_since_ms: Some(run.created_at_ms),
                created_at_ms: None,
                tool: None,
                model: None,
                resources: step.resources,
                // HS2-JSF985: an orphaned run's step is placed as confirmed work.
                speculation: None,
            });
        }
        Ok(work)
    }

    /// The running sessions as the executor lists them, with a session whose run
    /// waits on a gate marked as free capacity.
    fn scheduler_sessions(
        &self,
        owner_id: &str,
        tx: &mut Transaction<'_>,
    ) -> (Vec<ScheduledSession>, &'static str) {
        let mut sessions = match self.session_executor.list(self, owner_id, tx) {
            Ok(sessions) => sessions,
            Err(SessionExecutorError::Unavailable) => return (Vec::new(), "unavailable"),
            Err(SessionExecutorError::Failed(_)) => return (Vec::new(), "failed"),
        };
        if sessions.len() > MAX_SCHEDULE_SESSIONS
            || sessions.iter().any(|session| session.validate().is_err())
        {
            return (Vec::new(), "failed");
        }
        let gates = GateRegistry::new(&self.home);
        for session in &mut sessions {
            let (SessionActivity::Busy, Some(run_id)) = (session.activity, &session.run_id) else {
                continue;
            };
            let filter = GateFilter {
                run_id: Some(run_id.clone()),
                ..GateFilter::default()
            };
            let Ok(linked) = gates.list(owner_id, &filter) else {
                continue;
            };
            let summary = summarize(linked);
            let waits = |state| matches!(state, GateState::Pending | GateState::AwaitingApproval);
            if waits(summary.ci) || summary.deployment.is_some_and(waits) {
                session.activity = SessionActivity::WaitingOnGate;
            }
        }
        (sessions, "available")
    }

    fn scheduler_gather(
        &self,
        owner_id: &str,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<Option<Gathered>, OrchestrationError> {
        let view = self.policy(owner_id)?;
        let Some(policy) = view.policy else {
            return Ok(None);
        };
        let now_ms = tx.clock.now_ms();
        // HS2-YC2P7K: the sessions first. Listing them brings each record in line with
        // its host, so a run whose session was just found lost is placeable work.
        let (sessions, executor) = self.scheduler_sessions(owner_id, tx);
        let mut work = self.scheduler_work(owner_id, tx)?;
        // HS2-2FG91X: what the policy requires of every piece of work on a target.
        for item in &mut work {
            for need in policy.target_resources(&item.target_id) {
                if !item.resources.contains(need) {
                    item.resources.push(need.clone());
                }
            }
        }
        let memory = SchedulerStore::new(&self.home).memory(owner_id)?;
        // Only a target that could take a session is contacted: confirmed work, a
        // policy that lets sessions run there, and no refusal still being waited out.
        // HS2-JSF985: provisional work counts where the policy opts in for its target.
        let speculates = policy.speculates();
        let places = |item: &WorkItem| {
            item.level == WorkLevel::Confirmed
                || (speculates && policy.speculation_allowed(&item.target_id))
        };
        let wanted: BTreeSet<&str> = work
            .iter()
            .filter(|item| places(item))
            .map(|item| item.target_id.as_str())
            .filter(|target_id| {
                policy
                    .target(target_id)
                    .is_some_and(|target| target.enabled && !target.tools.is_empty())
            })
            .collect();
        let mut targets = Vec::new();
        let mut held = Vec::new();
        let mut unreadable = Vec::new();
        for target_id in wanted {
            let waiting = memory
                .iter()
                .find(|entry| entry.target_id == target_id)
                .and_then(|entry| entry.refused_until_ms())
                .is_some_and(|until| until > now_ms);
            // One read lists a bounded number of namespaces. A namespace left out is
            // unknown, never assumed free (HS2-2FG91X).
            let mut namespaces: Vec<String> = work
                .iter()
                .filter(|item| places(item) && item.target_id == target_id)
                .flat_map(|item| item.resources.iter().map(|need| need.namespace.clone()))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            let left_out = namespaces.split_off(
                namespaces
                    .len()
                    .min(hotsheet_ticketing::orchestration::MAX_CLAIM_NAMESPACES),
            );
            let read = if waiting {
                Err("refusal_backoff".to_owned())
            } else if actor.is_none() {
                Err("actor_required".to_owned())
            } else {
                Self::target_ref(owner_id, target_id)
                    .and_then(|target| self.target_capacity(&target, &namespaces, actor, tx))
                    .map_err(|error| error.code().to_owned())
            };
            match read {
                Ok(snapshot) => {
                    // A resource name means something only on the machine that
                    // answered: its claims are that machine's.
                    let unread = |namespace: &String, offline, reason| UnreadableClaims {
                        server_id: snapshot.server_id.clone(),
                        namespace: namespace.clone(),
                        offline,
                        reason,
                    };
                    match &snapshot.claims {
                        Some(claims) => held.extend(
                            claims
                                .iter()
                                .filter(|claim| namespaces.contains(&claim.namespace))
                                .map(|claim| HeldResource {
                                    namespace: claim.namespace.clone(),
                                    resource: claim.resource.clone(),
                                    expires_at_ms: claim.expires_at_ms,
                                    server_id: Some(snapshot.server_id.clone()),
                                }),
                        ),
                        // An older peer says nothing of a policy: that fails closed.
                        None => unreadable.extend(namespaces.iter().map(|namespace| {
                            unread(
                                namespace,
                                snapshot.claims_offline.unwrap_or_default(),
                                UnreadableReason::BackendUnreachable,
                            )
                        })),
                    }
                    unreadable.extend(left_out.iter().map(|namespace| {
                        unread(
                            namespace,
                            OfflinePolicy::FailClosed,
                            UnreadableReason::NamespaceLimit,
                        )
                    }));
                    targets.push(TargetState {
                        target_id: target_id.into(),
                        capacity: Some(snapshot),
                        unavailable: None,
                    });
                }
                Err(code) => targets.push(TargetState {
                    target_id: target_id.into(),
                    capacity: None,
                    unavailable: Some(code),
                }),
            }
        }
        // HS2-JSF985: what each read target's project says of speculation, then the
        // rule's answer for every provisional step. Nothing is read, stored or
        // decided while the policy does not opt in.
        let mut standings = BTreeMap::new();
        if speculates {
            let seen: Vec<_> = targets
                .iter()
                .filter_map(|state| {
                    let snapshot = state.capacity.as_ref()?;
                    Some((
                        state.target_id.clone(),
                        Some(snapshot.speculation.unwrap_or(
                            hotsheet_ticketing::orchestration::TargetSpeculation::NOT_OPTED_IN,
                        )),
                    ))
                })
                .collect();
            if actor.is_some() {
                let steps = self.readiness_plan(owner_id, tx)?.steps;
                let others: BTreeSet<String> = steps
                    .iter()
                    .filter(|step| {
                        step.readiness.level == ReadinessLevel::Provisional
                            && policy.speculation_allowed(&step.readiness.step.target.target_id)
                    })
                    .flat_map(|step| Self::provisional_targets_of(&step.readiness.step))
                    .filter(|id| !seen.iter().any(|(read, _)| read == id))
                    .collect();
                self.provisional_targets_seen(owner_id, &seen, now_ms)?;
                self.provisional_targets_read(owner_id, &others, actor, tx)?;
            }
            let read = |runtime: &Self, tx: &mut Transaction<'_>| {
                runtime.readiness_plan(owner_id, tx).map(|plan| {
                    plan.steps
                        .into_iter()
                        .filter_map(|step| Some((step.readiness.step.step_id, step.speculation?)))
                        .collect::<BTreeMap<_, _>>()
                })
            };
            standings = read(self, tx)?;
            // A plan defined before the opt-in never read its tickets' tags. They are
            // read once, through the gate, and then known.
            let unread: Vec<String> = standings
                .iter()
                .filter(|(_, standing)| standing.refusal.as_deref() == Some("opt_out_unknown"))
                .map(|(step_id, _)| step_id.clone())
                .collect();
            if actor.is_some() && !unread.is_empty() {
                for step_id in unread {
                    let _ = self.readiness_refresh(
                        owner_id,
                        &RefreshReadiness {
                            step_id: Some(step_id),
                        },
                        actor,
                        tx,
                    );
                }
                standings = read(self, tx)?;
            }
            for item in &mut work {
                if item.level == WorkLevel::Provisional {
                    item.speculation =
                        standings
                            .get(&item.step_id)
                            .map(|standing| WorkSpeculation {
                                allowed: standing.allowed,
                                refusal: standing.refusal.clone(),
                                depth: standing.depth,
                            });
                }
            }
        }
        Ok(Some(Gathered {
            policy,
            revision: view.revision,
            standings,
            work,
            targets,
            sessions,
            executor,
            entries: ToolCapacityRegistry::new(&self.home).entries()?,
            supports: super::tool_capacity::supports(&self.home),
            held,
            unreadable,
            memory,
        }))
    }

    fn scheduler_compute(
        &self,
        owner_id: &str,
        gathered: &Gathered,
        now_ms: u64,
    ) -> (Schedule, TickInputs) {
        let schedule = plan(
            &ScheduleInput {
                now_ms,
                owner_id,
                policy: &gathered.policy,
                work: &gathered.work,
                targets: &gathered.targets,
                sessions: &gathered.sessions,
                tool_entries: &gathered.entries,
                held: &gathered.held,
                unreadable: &gathered.unreadable,
                memory: &gathered.memory,
            },
            &|tool| gathered.supports.get(tool).copied().unwrap_or_default(),
        );
        let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        let of = |action| {
            count(
                schedule
                    .placements
                    .iter()
                    .filter(|placement| placement.action == action)
                    .count(),
            )
        };
        let inputs = TickInputs {
            policy_revision: gathered.revision.clone(),
            automatic: gathered.policy.placement.automatic,
            work: count(gathered.work.len()),
            confirmed: count(
                gathered
                    .work
                    .iter()
                    .filter(|item| item.level == WorkLevel::Confirmed)
                    .count(),
            ),
            sessions: count(gathered.sessions.len()),
            targets: count(gathered.targets.len()),
            targets_unavailable: count(
                gathered
                    .targets
                    .iter()
                    .filter(|target| target.capacity.is_none())
                    .count(),
            ),
            tools_limited: count(
                gathered
                    .entries
                    .iter()
                    .filter(|entry| {
                        let support = gathered
                            .supports
                            .get(&entry.tool)
                            .copied()
                            .unwrap_or_default();
                        matches!(
                            state_at(Some(entry), support, None, now_ms).state,
                            CapacityState::Limited { .. }
                        )
                    })
                    .count(),
            ),
            held_resources: count(gathered.held.len()),
            unreadable_namespaces: count(
                gathered
                    .unreadable
                    .iter()
                    .map(|unread| (&unread.server_id, &unread.namespace))
                    .collect::<BTreeSet<_>>()
                    .len(),
            ),
            executor: gathered.executor.into(),
            starts: of(PlacementAction::Start),
            keeps: of(PlacementAction::Keep),
            wind_downs: of(PlacementAction::WindDown),
            unplaced: count(schedule.unplaced.len()),
        };
        (schedule, inputs)
    }

    fn scheduler_empty(now_ms: u64) -> ScheduleView {
        ScheduleView {
            now_ms,
            enabled: false,
            mode: DecisionMode::DryRun,
            inputs: TickInputs {
                executor: "unavailable".into(),
                ..TickInputs::default()
            },
            schedule: Schedule::default(),
            assignments: Vec::new(),
        }
    }

    /// The schedule a tick would compute now. A dry run: it changes and logs
    /// nothing. With an actor it reads each involved target's capacity through the
    /// gate (`gates_observe`); without one those targets read as `actor_required`.
    pub fn scheduler_plan(
        &self,
        owner_id: &str,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<ScheduleView, OrchestrationError> {
        let now_ms = tx.clock.now_ms();
        let Some(gathered) = self.scheduler_gather(owner_id, actor, tx)? else {
            return Ok(Self::scheduler_empty(now_ms));
        };
        let (mut schedule, inputs) = self.scheduler_compute(owner_id, &gathered, now_ms);
        // HS2-NPTKS8: sessions that did not join their role.
        self.scheduler_joining(owner_id, &gathered.policy, &mut schedule, now_ms);
        // HS2-HV9ED3: sessions whose run a person cancelled are wound down.
        self.scheduler_cancelled(owner_id, &mut schedule);
        let assignments =
            self.scheduler_assignments(owner_id, &gathered.policy, &gathered.memory, actor, tx);
        Self::assignment_retry(&mut schedule, &assignments);
        Ok(ScheduleView {
            now_ms,
            enabled: true,
            mode: if gathered.policy.placement.automatic && actor.is_some() {
                DecisionMode::Applied
            } else {
                DecisionMode::DryRun
            },
            inputs,
            schedule,
            assignments,
        })
    }

    /// Why one piece of work is not running, from the plan and a fresh schedule.
    pub fn scheduler_explain(
        &self,
        owner_id: &str,
        request: &ExplainRequest,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<Explanation, OrchestrationError> {
        let now_ms = tx.clock.now_ms();
        let steps = self.readiness_plan(owner_id, tx)?.steps;
        let step = match (&request.step_id, &request.target_id) {
            (Some(step_id), None) => steps
                .into_iter()
                .find(|step| &step.readiness.step.step_id == step_id),
            (None, Some(target_id)) => {
                let (Some(connection_id), Some(native_id)) =
                    (&request.connection_id, &request.native_id)
                else {
                    return Err(OrchestrationError::Invalid(
                        "a ticket is named by target_id, connection_id and native_id",
                    ));
                };
                steps.into_iter().find(|step| {
                    let target = &step.readiness.step.target;
                    &target.target_id == target_id
                        && &target.connection_id == connection_id
                        && &target.native_id == native_id
                })
            }
            _ => {
                return Err(OrchestrationError::Invalid(
                    "explain takes a step_id or one exact target ticket",
                ));
            }
        };
        let mut answer = Explanation {
            now_ms,
            running: false,
            standing: "not_planned".into(),
            reason: "no_plan_step".into(),
            detail: None,
            step_id: None,
            level: None,
            readiness_reasons: Vec::new(),
            rank: None,
            score: None,
            retry_at_ms: None,
            placement: None,
            resources: Vec::new(),
            speculation: None,
            provisional: None,
            provisional_dependents: Vec::new(),
            assignment: None,
            mode: DecisionMode::DryRun,
        };
        let Some(step) = step else {
            return Ok(answer);
        };
        // HS2-JSF985: which work is provisional on which gate.
        answer.speculation = step.speculation.clone();
        answer.provisional = step.provisional.clone();
        answer.provisional_dependents = self
            .provisional_stacked(owner_id)?
            .remove(&hotsheet_ticketing::orchestration::ticket_key(
                &step.readiness.step.target,
            ))
            .map(|steps| steps.into_iter().collect())
            .unwrap_or_default();
        let level = step.readiness.level;
        answer.step_id = Some(step.readiness.step.step_id.clone());
        answer.level = Some(level);
        answer.readiness_reasons = step
            .readiness
            .reasons
            .iter()
            .map(|reason| reason.code.clone())
            .collect();
        answer.reason = level.code().into();
        match level {
            ReadinessLevel::InProgress => {
                answer.running = true;
                answer.standing = "running".into();
                // HS2-T16SK0: a run its target's pool will never assign says so.
                let policy = self.policy(owner_id)?.policy;
                let memory = SchedulerStore::new(&self.home).memory(owner_id)?;
                if let Some(policy) = policy {
                    let step_id = &step.readiness.step.step_id;
                    answer.assignment = self
                        .scheduler_assignments(owner_id, &policy, &memory, actor, tx)
                        .into_iter()
                        .find(|found| found.step_id.as_deref() == Some(step_id.as_str()));
                }
                if let Some(found) = answer
                    .assignment
                    .as_ref()
                    .filter(|found| found.state != AssignmentState::AwaitingDispatcher)
                {
                    answer.reason = found.state.code().into();
                    answer.retry_at_ms = found.claim_at_ms;
                }
            }
            ReadinessLevel::Finished => answer.standing = "finished".into(),
            ReadinessLevel::Blocked | ReadinessLevel::Stale | ReadinessLevel::Cyclic => {
                answer.standing = "not_ready".into();
            }
            // HS2-HV9ED3: a person cancelled the step's run; it waits for them.
            ReadinessLevel::Confirmed | ReadinessLevel::Provisional
                if super::cancel_hold::held_by(
                    &step.readiness.step,
                    &self.run_list(owner_id, false)?,
                )
                .is_some() =>
            {
                answer.standing = "not_placed".into();
                answer.reason = super::cancel_hold::CANCELLED_BY_PERSON.into();
            }
            ReadinessLevel::Confirmed | ReadinessLevel::Provisional => {
                let view = self.scheduler_plan(owner_id, actor, tx)?;
                answer.mode = view.mode;
                // HS2-JSF985: the pass may just have read a target's own setting.
                if let Ok(fresh) = self.readiness(owner_id, &step.readiness.step.step_id, tx) {
                    answer.speculation = fresh.speculation;
                }
                // HS2-2FG91X: what the work must hold, and how this pass saw each.
                let planned = &step.readiness.step;
                let of_target: Vec<ResourceNeed> = self
                    .policy(owner_id)?
                    .policy
                    .map(|policy| policy.target_resources(&planned.target.target_id).to_vec())
                    .unwrap_or_default();
                let needs: Vec<ResourceNeed> = planned
                    .resources
                    .iter()
                    .chain(
                        of_target
                            .iter()
                            .filter(|need| !planned.resources.contains(need)),
                    )
                    .cloned()
                    .collect();
                answer.resources = resource_standings(
                    &view.schedule.contention,
                    &planned.step_id,
                    &needs,
                    // HS2-JSF985: provisional work the rule allows is considered too.
                    view.enabled
                        && (level == ReadinessLevel::Confirmed
                            || step
                                .speculation
                                .as_ref()
                                .is_some_and(|standing| standing.allowed)),
                )
                .into_iter()
                .map(|standing| RequiredResource {
                    declared_by: if planned.resources.iter().any(|need| {
                        need.namespace == standing.namespace && need.resource == standing.resource
                    }) {
                        "step"
                    } else {
                        "target"
                    },
                    standing,
                })
                .collect();
                match view.schedule.standing(&step.readiness.step.step_id) {
                    Some(standing) => {
                        answer.standing = standing.standing.into();
                        answer.reason = standing.reason.into();
                        answer.detail = standing.detail;
                        answer.rank = Some(standing.rank);
                        answer.score = Some(standing.score);
                        answer.retry_at_ms = standing.retry_at_ms;
                        answer.placement = standing.placement;
                    }
                    None => {
                        answer.standing = "not_placed".into();
                        answer.reason = if view.enabled {
                            "not_considered"
                        } else {
                            "policy_absent"
                        }
                        .into();
                    }
                }
            }
        }
        Ok(answer)
    }

    /// The decision log after a cursor, oldest first.
    pub fn scheduler_decisions(
        &self,
        owner_id: &str,
        request: &DecisionsRequest,
    ) -> Result<DecisionPage, OrchestrationError> {
        let limit = request.limit.unwrap_or(100);
        if limit == 0 || limit > MAX_DECISION_PAGE {
            return Err(OrchestrationError::Invalid("limit must be 1 to 200"));
        }
        self.decisions(owner_id)?.read(request.after, limit)
    }

    /// One scheduling pass. It applies its decisions only when the policy enables
    /// automatic placement, the caller did not ask for a dry run, and an identified
    /// actor is given; otherwise the decisions are computed and logged and nothing
    /// is started or wound down.
    pub fn scheduler_tick(
        &self,
        owner_id: &str,
        request: &TickRequest,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<TickView, OrchestrationError> {
        self.scheduler_pass(
            owner_id,
            request.dry_run,
            "manual",
            actor,
            Logging::Always,
            tx,
        )
    }

    fn scheduler_pass(
        &self,
        owner_id: &str,
        dry_run: bool,
        trigger: &'static str,
        actor: Option<&MutationActor>,
        logging: Logging<'_>,
        tx: &mut Transaction<'_>,
    ) -> Result<TickView, OrchestrationError> {
        // HS2-7Q428Z: one pass of this owner at a time, so concurrent passes (the loop,
        // manual ticks, other processes) never decide and log the same start twice.
        let _pass = hotsheet_ticketing::orchestration::scheduler_pass_lock(&self.home, owner_id)?;
        let now_ms = tx.clock.now_ms();
        let tick_id = hotsheet_ticketing::mint_ulid(tx.clock, tx.rng).to_string();
        // HS2-JSF985: a gate result that arrived while nothing was running is applied
        // before anything is decided on it. Without provisional work this reads one
        // absent file.
        self.provisional_follow(owner_id, tx);
        let Some(gathered) = self.scheduler_gather(owner_id, actor, tx)? else {
            return Ok(TickView {
                tick_id,
                trigger: trigger.into(),
                view: Self::scheduler_empty(now_ms),
                outcomes: Vec::new(),
                logged: 0,
            });
        };
        let actor_id = actor.and_then(|actor| actor.id.clone());
        let mode = match (gathered.policy.placement.automatic && !dry_run, &actor_id) {
            (true, Some(_)) => DecisionMode::Applied,
            (true, None) => {
                return Err(OrchestrationError::Invalid(
                    "an identified actor is required to apply placements",
                ));
            }
            (false, _) => DecisionMode::DryRun,
        };
        let (mut schedule, inputs) = self.scheduler_compute(owner_id, &gathered, now_ms);
        // HS2-NPTKS8: sessions that did not join their role are wound down and placed
        // again, before anything is applied.
        self.scheduler_joining(owner_id, &gathered.policy, &mut schedule, now_ms);
        // HS2-HV9ED3: sessions whose run a person cancelled are wound down.
        self.scheduler_cancelled(owner_id, &mut schedule);

        let mut outcomes = Vec::new();
        let mut applied: Vec<(String, PlacementOutcome)> = Vec::new();
        let mut assignments;
        if let (DecisionMode::Applied, Some(actor_id)) = (mode, &actor_id) {
            // HS2-HV9ED3: told why before it is asked to stop.
            self.tell_cancelled(owner_id, &schedule, actor, tx);
            for placement in &schedule.placements {
                let outcome = match placement.action {
                    // HS2-YC2P7K: a free session given ready work gets that work's run.
                    PlacementAction::Keep
                        if placement.reason
                            == hotsheet_ticketing::orchestration::PlacementReason::Refill
                            && gathered.executor == "available" =>
                    {
                        match self.scheduler_assign(owner_id, placement, actor, actor_id, tx) {
                            Some(outcome) => outcome,
                            None => continue,
                        }
                    }
                    PlacementAction::Keep => continue,
                    PlacementAction::Start => self.scheduler_start(
                        owner_id,
                        &gathered,
                        placement,
                        actor,
                        actor_id,
                        &mut applied,
                        tx,
                    ),
                    PlacementAction::WindDown => self.scheduler_wind_down(
                        owner_id,
                        &gathered,
                        placement,
                        actor,
                        actor_id,
                        &tick_id,
                        &mut applied,
                        tx,
                    ),
                };
                outcomes.push(outcome);
            }
            // HS2-T16SK0: after the starts, so a session started in this pass is seen.
            assignments =
                self.scheduler_assignments(owner_id, &gathered.policy, &gathered.memory, actor, tx);
            self.assignment_apply(owner_id, &mut assignments, actor, &mut applied, tx);
            // A machine that had no room, or could not be read, is left alone for a
            // bounded, growing time instead of being asked on every event.
            for item in &schedule.unplaced {
                if matches!(
                    item.reason,
                    Blocker::MachineRefused | Blocker::TargetUnavailable
                ) {
                    applied.push((
                        item.target_id.clone(),
                        PlacementOutcome::Refused(
                            item.detail.clone().unwrap_or_else(|| "refused".into()),
                        ),
                    ));
                }
                // HS2-2FG91X: a machine whose claims cannot be read is asked again
                // after the same growing time, not on every event.
                if item.reason == Blocker::ResourceUnknown {
                    applied.push((
                        item.target_id.clone(),
                        PlacementOutcome::Refused(item.reason.code().into()),
                    ));
                }
            }
            SchedulerStore::new(&self.home).record(owner_id, &applied, now_ms)?;
        } else {
            assignments =
                self.scheduler_assignments(owner_id, &gathered.policy, &gathered.memory, actor, tx);
        }
        Self::assignment_retry(&mut schedule, &assignments);

        let logged = self.scheduler_log(
            owner_id,
            &tick_id,
            mode,
            trigger,
            actor_id.as_deref(),
            now_ms,
            &schedule,
            &inputs,
            &outcomes,
            &assignments,
            logging,
        )?;
        Ok(TickView {
            tick_id,
            trigger: trigger.into(),
            view: ScheduleView {
                now_ms,
                enabled: true,
                mode,
                inputs,
                schedule,
                assignments,
            },
            outcomes,
            logged,
        })
    }

    #[allow(clippy::too_many_arguments)] // HS2-ECWG50: one pass's whole result
    fn scheduler_log(
        &self,
        owner_id: &str,
        tick_id: &str,
        mode: DecisionMode,
        trigger: &str,
        actor_id: Option<&str>,
        now_ms: u64,
        schedule: &Schedule,
        inputs: &TickInputs,
        outcomes: &[DecisionOutcome],
        assignments: &[super::assignment::Assignment],
        logging: Logging<'_>,
    ) -> Result<usize, OrchestrationError> {
        // HS2-T16SK0: only a run nobody will assign, or whose pool is unknown, is news.
        let reported: Vec<_> = assignments
            .iter()
            .filter(|found| found.state != AssignmentState::AwaitingDispatcher)
            .collect();
        let outcome_of = |placement: &Placement| {
            outcomes.iter().find(|outcome| {
                outcome.action == placement.action
                    && outcome.target_id == placement.target_id
                    && match placement.action {
                        PlacementAction::Start => {
                            outcome.step_id.as_ref() == placement.steps.first()
                        }
                        _ => outcome.session_id == placement.session_id,
                    }
            })
        };
        if let Logging::Changed(last) = logging {
            // Scores and retry times move with the clock; what was decided does not.
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            (mode == DecisionMode::Applied).hash(&mut hasher);
            for placement in &schedule.placements {
                (
                    placement.action.code(),
                    &placement.target_id,
                    &placement.tool,
                    &placement.account,
                    &placement.model,
                    &placement.session_id,
                    placement.reason.code(),
                    &placement.detail,
                    &placement.steps,
                    // HS2-95SWQ4: two starts that each began a session are two
                    // decisions, so the session an applied start began is part of it.
                    outcome_of(placement).map(|outcome| {
                        (
                            &outcome.outcome,
                            &outcome.detail,
                            &outcome.session_id,
                            &outcome.run_id,
                        )
                    }),
                )
                    .hash(&mut hasher);
            }
            for item in &schedule.unplaced {
                (&item.step_id, item.reason.code(), &item.detail).hash(&mut hasher);
            }
            for found in &reported {
                (
                    &found.run_id,
                    &found.session_id,
                    found.state.code(),
                    found.claim_at_ms,
                    &found.outcome,
                    &found.outcome_detail,
                )
                    .hash(&mut hasher);
            }
            let digest = hasher.finish();
            if *last == Some(digest) {
                return Ok(0);
            }
            *last = Some(digest);
        }
        let journal = self.decisions(owner_id)?;
        let base = |kind| DecisionRecord {
            at_ms: now_ms,
            tick_id: tick_id.into(),
            mode,
            trigger: trigger.into(),
            kind,
            actor_id: actor_id.map(Into::into),
            target_id: None,
            tool: None,
            account: None,
            model: None,
            session_id: None,
            step_id: None,
            reason: None,
            detail: None,
            outcome: None,
            outcome_detail: None,
            operation_id: None,
            run_id: None,
            probe: false,
            retry_at_ms: None,
            score: None,
            inputs: None,
            tick: None,
        };
        let mut logged = 0;
        journal.append(DecisionRecord {
            retry_at_ms: schedule.retry_at_ms,
            tick: Some(inputs.clone()),
            ..base(DecisionKind::Tick)
        })?;
        logged += 1;
        for placement in &schedule.placements {
            let outcome = outcome_of(placement);
            journal.append(DecisionRecord {
                target_id: Some(placement.target_id.clone()),
                tool: Some(placement.tool.clone()),
                account: placement.account.clone(),
                model: placement.model.clone(),
                session_id: placement
                    .session_id
                    .clone()
                    .or_else(|| outcome.and_then(|outcome| outcome.session_id.clone())),
                step_id: placement.steps.first().cloned(),
                reason: Some(placement.reason.code().into()),
                detail: placement.detail.clone(),
                outcome: outcome.map(|outcome| outcome.outcome.clone()),
                outcome_detail: outcome.and_then(|outcome| outcome.detail.clone()),
                operation_id: outcome.map(|outcome| outcome.operation_id.clone()),
                run_id: outcome.and_then(|outcome| outcome.run_id.clone()),
                probe: placement.probe,
                retry_at_ms: placement.retry_at_ms,
                score: placement.score,
                inputs: Some(placement.inputs.clone()),
                ..base(match placement.action {
                    PlacementAction::Start => DecisionKind::Start,
                    PlacementAction::Keep => DecisionKind::Keep,
                    PlacementAction::WindDown => DecisionKind::WindDown,
                })
            })?;
            logged += 1;
        }
        for Unplaced {
            step_id,
            target_id,
            reason,
            detail,
            retry_at_ms,
            score,
        } in &schedule.unplaced
        {
            journal.append(DecisionRecord {
                target_id: Some(target_id.clone()),
                step_id: Some(step_id.clone()),
                reason: Some(reason.code().into()),
                detail: detail.clone(),
                retry_at_ms: *retry_at_ms,
                score: Some(*score),
                ..base(DecisionKind::Unplaced)
            })?;
            logged += 1;
        }
        for found in reported {
            journal.append(DecisionRecord {
                target_id: Some(found.target_id.clone()),
                session_id: Some(found.session_id.clone()),
                step_id: found.step_id.clone(),
                run_id: Some(found.run_id.clone()),
                reason: Some(found.state.code().into()),
                outcome: found.outcome.clone(),
                outcome_detail: found.outcome_detail.clone(),
                retry_at_ms: found.claim_at_ms,
                ..base(DecisionKind::Assignment)
            })?;
            logged += 1;
        }
        Ok(logged)
    }

    /// HS2-T16SK0: the loop wakes when a waiting run's session is due to claim it.
    fn assignment_retry(schedule: &mut Schedule, assignments: &[super::assignment::Assignment]) {
        for at in assignments.iter().filter_map(|found| found.retry_at_ms) {
            schedule.retry_at_ms = Some(schedule.retry_at_ms.map_or(at, |known| known.min(at)));
        }
    }

    fn scheduler_actor(&self, owner_id: &str) -> Result<Option<MutationActor>, OrchestrationError> {
        Ok(self
            .policy(owner_id)?
            .policy
            .filter(|policy| policy.placement.automatic)
            .and_then(|policy| policy.placement.actor)
            .and_then(|actor| MutationActor::parse("system", Some(actor)).ok()))
    }

    /// One step of an owner's scheduling loop, for a host's follower. It runs a pass
    /// when it starts, when the owner's run event stream has something new (run,
    /// readiness, gate, validation and session events), when the wake marker changed
    /// (a tool limit, a policy change), or when the last pass named a time at which
    /// a decision changes. Otherwise it waits for one of those, at most `wait_ms`,
    /// woken by the journal or the marker changing. Call again while `active`; after
    /// `failed`, back off first.
    pub fn scheduler_follow_step(
        &self,
        owner_id: &str,
        state: &mut SchedulerFollowState,
        wait_ms: u64,
        tx: &mut Transaction<'_>,
    ) -> SchedulerStep {
        let mut step = SchedulerStep::default();
        match self.scheduler_actor(owner_id) {
            Ok(Some(_)) => {}
            Ok(None) => return step,
            Err(_) => {
                step.active = true;
                step.failed = true;
                return step;
            }
        }
        step.active = true;
        step.retry_at_ms = state.retry_at_ms;
        let Ok(journal) = RunEventJournal::new(&self.home, owner_id) else {
            step.failed = true;
            return step;
        };
        let wake_directory = self.home.join(WAKE_DIRECTORY);
        // Watch before reading, so a change in between is not missed. A step that
        // may not wait watches nothing: starting a platform watcher costs far more
        // than the reads below (HS2-2FG91X).
        let waiter = (wait_ms > 0)
            .then(|| {
                hotsheet_jobs::ChangeWaiter::watching(&[journal.directory(), &wake_directory]).ok()
            })
            .flatten();
        let deadline =
            std::time::Instant::now() + Duration::from_millis(wait_ms.min(MAX_SCHEDULER_WAIT_MS));
        loop {
            let mut trigger = (!state.started).then_some("start");
            loop {
                let Ok(page) = journal.read(state.cursor, 500, None) else {
                    step.failed = true;
                    return step;
                };
                // Lease renewals change nothing a placement depends on.
                if page.gap
                    || page
                        .entries
                        .iter()
                        .any(|entry| entry.record.kind != RunEventKind::Lease)
                {
                    trigger = trigger.or(Some("event"));
                }
                let more = page.more && page.cursor != state.cursor;
                state.cursor = page.cursor;
                if !more {
                    break;
                }
            }
            let wake = self.wake_token();
            if wake != state.wake {
                state.wake = wake;
                trigger = trigger.or(Some("signal"));
            }
            let now_ms = tx.clock.now_ms();
            if state.retry_at_ms.is_some_and(|at| at <= now_ms) {
                trigger = trigger.or(Some("retry"));
            }
            if let Some(trigger) = trigger {
                state.started = true;
                // The policy may have changed since the step began: read it again.
                let actor = match self.scheduler_actor(owner_id) {
                    Ok(Some(actor)) => actor,
                    Ok(None) => {
                        step.active = false;
                        return step;
                    }
                    Err(_) => {
                        step.failed = true;
                        return step;
                    }
                };
                step.trigger = Some(trigger);
                match self.scheduler_pass(
                    owner_id,
                    false,
                    trigger,
                    Some(&actor),
                    Logging::Changed(&mut state.digest),
                    tx,
                ) {
                    Ok(view) => {
                        // HS2-2FG91X: a release wakes this loop; an expiry is the
                        // retry time below.
                        self.watch_releases(&view.view.schedule);
                        state.retry_at_ms = view.view.schedule.retry_at_ms;
                        step.failed = view.view.inputs.executor == "failed";
                    }
                    Err(_) => step.failed = true,
                }
                step.retry_at_ms = state.retry_at_ms;
                return step;
            }
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            let Some(waiter) = waiter.as_ref().filter(|_| !remaining.is_zero()) else {
                return step;
            };
            let until_retry = state
                .retry_at_ms
                .map_or(u64::MAX, |at| at.saturating_sub(now_ms));
            waiter.wait(remaining.min(Duration::from_millis(until_retry)));
        }
    }
}
