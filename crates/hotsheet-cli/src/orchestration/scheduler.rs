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
//! Inside the policy the scheduler acts and logs. A decision the gate would not allow
//! is passed through `authorize`, which files the approval a policy asks for and
//! records the denial; nothing is done about it until a person answers.
//!
//! Nothing here runs on a timer. [`OrchestrationRuntime::scheduler_follow_step`]
//! waits on the run event journal and the scheduler's wake marker, and otherwise only
//! until the time a decision is known to change.
use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};
use std::time::Duration;

use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::{
    Blocker, CapacityState, DecisionJournal, DecisionKind, DecisionMode, DecisionPage,
    DecisionRecord, GateFilter, GateRegistry, GateState, HeldResource, MAX_SCHEDULE_SESSIONS,
    Placement, PlacementAction, PlacementOutcome, PolicyBody, Prerequisite, ReadinessLevel,
    RunEventJournal, RunEventKind, Schedule, ScheduleInput, ScheduledSession, SchedulerStore,
    ScoreParts, SessionActivity, SignalSupport, TargetAction, TargetState, TickInputs,
    ToolCapacityRegistry, Unplaced, WorkItem, WorkLevel, plan, state_at, summarize,
};
use serde::{Deserialize, Serialize};

use super::*;

/// The longest one follow step waits for an event.
pub const MAX_SCHEDULER_WAIT_MS: u64 = 25_000;
const MAX_DECISION_PAGE: usize = 200;
const WAKE_DIRECTORY: &str = "orchestration-scheduler-wake";

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
    /// `started`, `wind_down_requested`, `executor_unavailable`, `refused`,
    /// `not_at_safe_point`, `session_not_found`, or the gate's denial code
    /// (`approval_required`, `policy_denied`, `no_grant` and the others).
    pub outcome: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub operation_id: String,
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
    /// `dry_run` when a placement shown here would not be applied by a tick.
    pub mode: DecisionMode,
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
    work: Vec<WorkItem>,
    targets: Vec<TargetState>,
    sessions: Vec<ScheduledSession>,
    /// `available`, `unavailable` or `failed`.
    pub(super) executor: &'static str,
    entries: Vec<hotsheet_ticketing::orchestration::CapacityEntry>,
    supports: BTreeMap<String, SignalSupport>,
    held: Vec<HeldResource>,
    memory: Vec<hotsheet_ticketing::orchestration::PlacementMemory>,
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
        for step in self.readiness_plan(owner_id, tx)?.steps {
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
        Ok(self
            .ready_work(owner_id, tx)?
            .into_iter()
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
                // Due dates, milestones, a preferred tool and declared resources are
                // not in a plan step yet (HS2-2FG91X declares resources).
                due_at_ms: None,
                unblocks: u32::try_from(item.unblocks.len()).unwrap_or(u32::MAX),
                ready_since_ms: item.ready_since_ms,
                created_at_ms: item.ticket_created_at_ms,
                tool: None,
                model: None,
                resources: Vec::new(),
            })
            .collect())
    }

    /// The running sessions as the executor lists them, with a session whose run
    /// waits on a gate marked as free capacity.
    fn scheduler_sessions(&self, owner_id: &str) -> (Vec<ScheduledSession>, &'static str) {
        let mut sessions = match self.session_executor.list(owner_id) {
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
        let work = self.scheduler_work(owner_id, tx)?;
        let memory = SchedulerStore::new(&self.home).memory(owner_id)?;
        let (sessions, executor) = self.scheduler_sessions(owner_id);
        // Only a target that could take a session is contacted: confirmed work, a
        // policy that lets sessions run there, and no refusal still being waited out.
        let wanted: BTreeSet<&str> = work
            .iter()
            .filter(|item| item.level == WorkLevel::Confirmed)
            .map(|item| item.target_id.as_str())
            .filter(|target_id| {
                policy.target(target_id).is_some_and(|target| {
                    target.enabled && target.actions.contains(&TargetAction::SessionsManage)
                })
            })
            .collect();
        let mut targets = Vec::new();
        let mut held = Vec::new();
        for target_id in wanted {
            let waiting = memory
                .iter()
                .find(|entry| entry.target_id == target_id)
                .and_then(|entry| entry.refused_until_ms())
                .is_some_and(|until| until > now_ms);
            let namespaces: Vec<String> = work
                .iter()
                .filter(|item| item.target_id == target_id)
                .flat_map(|item| item.resources.iter().map(|need| need.namespace.clone()))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .take(hotsheet_ticketing::orchestration::MAX_CLAIM_NAMESPACES)
                .collect();
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
                    held.extend(snapshot.claims.iter().flatten().map(|claim| HeldResource {
                        namespace: claim.namespace.clone(),
                        resource: claim.resource.clone(),
                        expires_at_ms: claim.expires_at_ms,
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
        Ok(Some(Gathered {
            policy,
            revision: view.revision,
            work,
            targets,
            sessions,
            executor,
            entries: ToolCapacityRegistry::new(&self.home).entries()?,
            supports: super::tool_capacity::supports(&self.home),
            held,
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
        let (schedule, inputs) = self.scheduler_compute(owner_id, &gathered, now_ms);
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
            mode: DecisionMode::DryRun,
        };
        let Some(step) = step else {
            return Ok(answer);
        };
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
            }
            ReadinessLevel::Finished => answer.standing = "finished".into(),
            ReadinessLevel::Blocked | ReadinessLevel::Stale | ReadinessLevel::Cyclic => {
                answer.standing = "not_ready".into();
            }
            ReadinessLevel::Confirmed | ReadinessLevel::Provisional => {
                let view = self.scheduler_plan(owner_id, actor, tx)?;
                answer.mode = view.mode;
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
        let now_ms = tx.clock.now_ms();
        let tick_id = hotsheet_ticketing::mint_ulid(tx.clock, tx.rng).to_string();
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
        let (schedule, inputs) = self.scheduler_compute(owner_id, &gathered, now_ms);

        let mut outcomes = Vec::new();
        let mut applied: Vec<(String, PlacementOutcome)> = Vec::new();
        if let (DecisionMode::Applied, Some(actor_id)) = (mode, &actor_id) {
            for placement in &schedule.placements {
                let outcome = match placement.action {
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
            }
            SchedulerStore::new(&self.home).record(owner_id, &applied, now_ms)?;
        }

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
        logging: Logging<'_>,
    ) -> Result<usize, OrchestrationError> {
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
                    outcome_of(placement).map(|outcome| (&outcome.outcome, &outcome.detail)),
                )
                    .hash(&mut hasher);
            }
            for item in &schedule.unplaced {
                (&item.step_id, item.reason.code(), &item.detail).hash(&mut hasher);
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
        Ok(logged)
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
        // Watch before reading, so a change in between is not missed.
        let waiter =
            hotsheet_jobs::ChangeWaiter::watching(&[journal.directory(), &wake_directory]).ok();
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
