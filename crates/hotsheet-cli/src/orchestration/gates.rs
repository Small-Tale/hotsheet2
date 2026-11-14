//! HS2-5JBYPV: CI and deployment gates of dispatch runs over the shared runtime.
//!
//! A gate changes only on a typed reading from a [`GateProvider`] adapter or on a
//! person's answer to a deployment's permission request. Reads answer from this machine's records and contact nothing.
//! The GitHub Actions adapter is the watch Job of HS2-3R9V0R: the Job monitor observes
//! the provider, and this module wakes on the Job store's file changes instead of
//! polling.
use std::collections::BTreeMap;
use std::time::Duration;

use hotsheet_jobs::{
    CiTarget, CiWatchOptions, GITHUB_ACTIONS_KIND, JobStore, MonitorSlot, SlotState, TicketLink,
    ensure_ci_watch, gate_state, rewatch_ci,
};
use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::{
    ArtifactRef, CiScope, Coverage, DenialReason, DeploymentScope, Gate, GateError, GateEvidence,
    GateFilter, GateKind, GateOutcome, GateReading, GateRegistry, GateState, GateSummary, NewGate,
    Run, RunEvent, RunEventKind, RunEventSource, TargetAction, summarize, valid_commit,
    valid_script,
};
use serde::{Deserialize, Serialize};

use super::job_monitor::JobMonitor;
use super::tickets::identifier;
use super::*;

/// The provider name of deployments run by the target's own checked-in script.
pub const TARGET_SCRIPT_PROVIDER: &str = "target-script";
/// The longest a sync or a follow step waits for a provider's records to change.
pub const MAX_GATE_WAIT_MS: u64 = 20_000;
/// A follower re-confirms its authority this often.
const REAUTHORIZE_MS: u64 = 10 * 60 * 1000;
/// HS2-VVHVF0: after a monitor start that failed, a follower waits this long before it
/// tries again, doubling up to the ceiling. A start is a process launch.
const OBSERVER_RETRY_MS: u64 = 30_000;
const MAX_OBSERVER_RETRY_MS: u64 = 15 * 60 * 1000;

/// A provider adapter: the only source of gate evidence. An implementation answers
/// from records it keeps itself (a watch Job, an execution record) and never from text
/// a session wrote.
pub trait GateProvider: Send + Sync {
    /// The stable provider name a gate carries, for example `github-actions`.
    fn provider(&self) -> &'static str;
    fn kind(&self) -> GateKind;
    /// Begin watching (CI) or executing (deployment) for the gate. Idempotent: repeating
    /// it never starts a second watch or execution. `again` asks for a fresh watch
    /// after one that failed or was stopped.
    fn start(&self, gate: &Gate, run: &Run, again: bool) -> Result<(), GateError>;
    /// The current reading from the adapter's own records. Must not block on, or
    /// contact, the provider.
    fn read(&self, gate: &Gate) -> Result<GateReading, GateError>;
    /// A cursor over the adapter's records of one owning checkout. Read it before
    /// reading gates, so a change in between is seen by the next [`Self::wait`].
    fn cursor(&self, owner_checkout_id: &str) -> u64;
    /// Block until the records changed after `cursor` or `timeout` passes, woken by
    /// the change itself. Never a timer loop.
    fn wait(&self, owner_checkout_id: &str, cursor: u64, timeout: Duration);
    /// Whether something currently observes the provider for this adapter. `None`
    /// when the question does not apply.
    fn watching(&self) -> Option<bool> {
        None
    }
    /// HS2-VVHVF0: make sure something observes the provider, starting the observer
    /// when none runs and the adapter was given a way to. Answers like
    /// [`Self::watching`]. Contacts no provider and is safe to repeat.
    fn ensure_watching(&self) -> Option<bool> {
        self.watching()
    }
}

/// The GitHub Actions gate of a commit, read from its watch Job (HS2-3R9V0R).
pub struct GithubActionsGates {
    home: PathBuf,
    store: JobStore,
    /// The Job store waiter, kept between follower steps (HS2-N1QDZW).
    waiter: hotsheet_jobs::KeptWaiter,
    /// HS2-VVHVF0: starts the Job monitor when no monitor observes the watch Jobs.
    monitor: Option<Arc<dyn JobMonitor>>,
}

impl GithubActionsGates {
    pub fn new(home: &Path) -> Self {
        Self {
            home: home.into(),
            store: JobStore::at(home),
            waiter: hotsheet_jobs::KeptWaiter::default(),
            monitor: None,
        }
    }

    /// HS2-VVHVF0: let the adapter start the Job monitor for the watches it creates.
    #[must_use]
    pub fn with_monitor(mut self, monitor: Arc<dyn JobMonitor>) -> Self {
        self.monitor = Some(monitor);
        self
    }

    fn target(gate: &Gate) -> Result<CiTarget, GateError> {
        let scope = gate.ci.as_ref().ok_or(GateError::ProviderUnsupported)?;
        CiTarget::new(
            &scope.repository,
            &gate.commit,
            &scope.workflow,
            &scope.branch,
        )
        .map_err(|_| GateError::ProviderUnsupported)
    }
}

impl GateProvider for GithubActionsGates {
    fn provider(&self) -> &'static str {
        GITHUB_ACTIONS_KIND
    }

    fn kind(&self) -> GateKind {
        GateKind::Ci
    }

    fn start(&self, gate: &Gate, run: &Run, again: bool) -> Result<(), GateError> {
        let target = Self::target(gate)?;
        let options = CiWatchOptions {
            ticket: Some(TicketLink {
                connection_id: run.ticket.connection_id.clone(),
                native_id: run.ticket.native_id.clone(),
            }),
            can_cancel: false,
        };
        let watch = if again { rewatch_ci } else { ensure_ci_watch };
        watch(&self.store, &gate.owner_checkout_id, &target, &options)
            .map(|_| ())
            .map_err(|_| GateError::ProviderUnavailable)
    }

    fn read(&self, gate: &Gate) -> Result<GateReading, GateError> {
        use hotsheet_jobs::GateState as Ci;
        let target = Self::target(gate)?;
        let reading = gate_state(&self.store, &gate.owner_checkout_id, &target)
            .map_err(|_| GateError::ProviderUnavailable)?;
        let evidence = |run: &hotsheet_jobs::CiRun| {
            let commit = run.head_sha.to_ascii_lowercase();
            GateEvidence {
                provider: GITHUB_ACTIONS_KIND.into(),
                reference: run.id.to_string(),
                order: run.id,
                attempt: run.attempt,
                // The watch source judged only runs whose head is the commit or has it
                // as an ancestor on the shipping branch.
                coverage: if commit == gate.commit {
                    Coverage::Exact
                } else {
                    Coverage::Ancestor
                },
                commit,
                // HS2-3YC577: what a green run built, when the source recorded one.
                artifact: run
                    .artifact
                    .as_ref()
                    .map(|artifact| ArtifactRef {
                        name: artifact.name.clone(),
                        digest: artifact.digest.clone(),
                    })
                    .filter(ArtifactRef::is_valid),
                exit_code: None,
                at_ms: self.store.now_ms(),
            }
        };
        let mut answer = match &reading.state {
            Ci::Pending => GateReading::new(GateState::Pending),
            Ci::Green { run } => GateReading::new(GateState::Passed).evidence(evidence(run)),
            Ci::Red { run } => GateReading::new(GateState::Failed).evidence(evidence(run)),
            Ci::Superseded => GateReading::new(GateState::Superseded).reason("branch_moved"),
            Ci::Cancelled => GateReading::new(GateState::Unknown).reason("watch_cancelled"),
            Ci::Unknown if reading.job_id.is_some() => {
                GateReading::new(GateState::Unknown).reason("watch_lost")
            }
            Ci::Unknown => GateReading::new(GateState::Unknown).reason("not_watched"),
        };
        // A verdict that repeats the recorded evidence keeps the time it was first seen.
        if let (Some(new), Some(old)) = (&mut answer.evidence, &gate.evidence) {
            if (new.order, new.attempt, &new.commit) == (old.order, old.attempt, &old.commit) {
                new.at_ms = old.at_ms;
            }
        }
        answer.watch_ref = reading.job_id;
        Ok(answer)
    }

    fn cursor(&self, owner_checkout_id: &str) -> u64 {
        self.store.revision(owner_checkout_id).unwrap_or(0)
    }

    fn wait(&self, owner_checkout_id: &str, cursor: u64, timeout: Duration) {
        let _ =
            self.store
                .wait_for_change_keeping(&self.waiter, owner_checkout_id, cursor, timeout);
    }

    fn watching(&self) -> Option<bool> {
        Some(matches!(
            MonitorSlot::at(&self.home).state(),
            Ok(SlotState::Held(_))
        ))
    }

    fn ensure_watching(&self) -> Option<bool> {
        if self.watching() == Some(true) {
            return Some(true);
        }
        // Without a monitor to start, the answer is the plain reading.
        Some(
            self.monitor
                .as_ref()
                .is_some_and(|monitor| monitor.ensure(&self.home).is_ok()),
        )
    }
}

/// Link the CI gate of one commit to a run.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkGate {
    /// The integrated target commit: all 40 hex characters.
    pub commit: String,
    /// Defaults to `github-actions`.
    #[serde(default)]
    pub provider: Option<String>,
    /// The provider's repository, `owner/name`. Defaults to, and must agree with, the
    /// GitHub repository the target declares.
    #[serde(default)]
    pub repository: Option<String>,
    /// Defaults to `CI`.
    #[serde(default)]
    pub workflow: Option<String>,
    /// The shipping branch. Defaults to `main`.
    #[serde(default)]
    pub branch: Option<String>,
}

/// Ask for a deployment of the commit the run's CI gate passed.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestDeployment {
    pub environment: String,
    /// The target's checked-in deploy script, relative to its repository root.
    pub script: String,
    /// An irreversible production change. A person is asked first, whatever the policy.
    #[serde(default)]
    pub production: bool,
    pub operation_id: String,
    /// Defaults to `target-script`.
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub artifact: Option<ArtifactRef>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncGates {
    /// With nothing new, wait this long (at most 20 seconds) for the provider's
    /// records to change.
    #[serde(default)]
    pub wait_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GateView {
    pub gate: Gate,
    /// False when an equal link or request already stood.
    pub created: bool,
    /// Ids of the gates this link superseded.
    pub superseded: Vec<String>,
    /// Whether something observes the provider now; absent when that does not apply.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub watching: Option<bool>,
    /// HS2-XHQ1EH: the permission request a person must answer, for a deployment
    /// awaiting approval.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

/// The gates of one run after a sync.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunGates {
    pub run_id: String,
    #[serde(flatten)]
    pub summary: GateSummary,
    /// Gates this call changed.
    pub changed: Vec<String>,
    /// HS2-VVHVF0: whether something observes the provider a gate of the run still
    /// waits on, after this call made sure of it; absent when no gate waits.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub watching: Option<bool>,
}

/// What a follower remembers between steps: when each target's authority was last
/// confirmed, and whether it held.
#[derive(Default)]
pub struct GateFollowState {
    authorized: BTreeMap<String, (bool, u64)>,
    /// HS2-VVHVF0: the delay after the last failed observer start, and when the next
    /// start may be tried.
    observer_retry: Option<(u64, u64)>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct GateFollowStep {
    /// False when no gate of the owner still waits on a provider: the follower ends.
    pub active: bool,
    pub changed: Vec<String>,
    /// HS2-VVHVF0: whether something observes the provider the followed gates wait on.
    /// `None` when the step did not ask: no gate it may follow, or a failed start is
    /// still backing off.
    pub watching: Option<bool>,
}

/// A gate a follower keeps reading: watched, or lost and possibly reattached.
fn followed(gate: &Gate) -> bool {
    gate.awaits_provider()
}

/// `owner/name` of a declared GitHub repository identity, or `None` for another host.
pub(super) fn github_repository(identity: &str) -> Option<String> {
    let (_, rest) = identity.split_once("://")?;
    let (host, path) = rest.split_once('/')?;
    let host = host.rsplit('@').next()?.split(':').next()?;
    if !host.eq_ignore_ascii_case("github.com") {
        return None;
    }
    let path = path.trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let (owner, name) = path.split_once('/')?;
    (!owner.is_empty() && !name.is_empty() && !name.contains('/'))
        .then(|| format!("{owner}/{name}"))
}

impl OrchestrationRuntime {
    /// Add a provider adapter. It replaces a registered one of the same provider name
    /// and kind.
    #[must_use]
    pub fn with_gate_provider(mut self, provider: Arc<dyn GateProvider>) -> Self {
        self.gate_providers.retain(|other| {
            (other.provider(), other.kind()) != (provider.provider(), provider.kind())
        });
        self.gate_providers.push(provider);
        self
    }

    /// HS2-VVHVF0: let the GitHub Actions adapter start the machine's Job monitor for
    /// the watches it creates. Without this the adapter only reports whether one runs.
    #[must_use]
    pub fn with_job_monitor(self, monitor: Arc<dyn JobMonitor>) -> Self {
        let adapter = GithubActionsGates::new(&self.home).with_monitor(monitor);
        self.with_gate_provider(Arc::new(adapter))
    }

    /// HS2-VVHVF0: owning checkouts with a gate that still waits on a provider: what a
    /// restarted host follows, whether or not the gate's run is finished.
    pub fn gate_owners(&self) -> Vec<String> {
        self.gate_registry().waiting_owners().unwrap_or_default()
    }

    pub(super) fn gate_registry(&self) -> GateRegistry {
        GateRegistry::new(&self.home)
    }

    pub(super) fn gate_provider(
        &self,
        provider: &str,
        kind: GateKind,
    ) -> Option<&Arc<dyn GateProvider>> {
        self.gate_providers
            .iter()
            .find(|other| other.provider() == provider && other.kind() == kind)
    }

    /// Publish a gate's current state to the owner's run event stream. Keyed by the
    /// gate's own history, so two readers of one change record it once.
    fn gate_event(&self, gate: &Gate, source: RunEventSource) {
        let event = RunEvent {
            at_ms: gate.updated_at_ms,
            run_id: Some(gate.run_id.clone()),
            step_id: None,
            kind: RunEventKind::Gate,
            source,
            state: None,
            reason: gate.reason.clone(),
            actor_id: None,
            // The reason is part of the fact: an unknown gate can change why it is unknown.
            dedupe: Some(format!(
                "gate-{}-{}-{}-{}-{}",
                gate.gate_id,
                gate.history.len(),
                gate.state.code(),
                gate.updated_at_ms,
                gate.reason
                    .as_deref()
                    .map_or("none", |reason| &reason[..reason.len().min(48)])
            )),
            gate_id: Some(gate.gate_id.clone()),
            gate_state: Some(gate.state),
        };
        if let Ok(journal) = hotsheet_ticketing::orchestration::RunEventJournal::new(
            &self.home,
            &gate.owner_checkout_id,
        ) {
            let _ = journal.record(event);
        }
        // HS2-42TARA: a gate verdict may change what is ready.
        self.readiness_reevaluate(&gate.owner_checkout_id, gate.updated_at_ms);
        // HS2-JSF985: and what becomes of work started ahead of it.
        self.provisional_gate_changed(&gate.owner_checkout_id, gate.updated_at_ms);
    }

    /// Read the gate's adapter and record what it says. Returns the gate as it now
    /// stands and whether it changed. A gate without a registered adapter, or one that
    /// waits for a person, is returned as it is.
    fn gate_refresh(
        &self,
        gate: Gate,
        tx: &mut Transaction<'_>,
    ) -> Result<(Gate, bool), OrchestrationError> {
        if matches!(
            gate.state,
            GateState::Superseded | GateState::AwaitingApproval | GateState::Passed
        ) {
            return Ok((gate, false));
        }
        let Some(provider) = self.gate_provider(&gate.provider, gate.kind) else {
            return Ok((gate, false));
        };
        let reading = match provider.read(&gate) {
            Ok(reading) => reading,
            // An adapter that cannot answer says nothing about the commit.
            Err(error) => GateReading::new(GateState::Unknown).reason(error.code()),
        };
        let (after, outcome) =
            self.gate_registry()
                .apply(&gate.owner_checkout_id, &gate.gate_id, reading, tx)?;
        let changed = outcome == GateOutcome::Changed;
        if changed {
            self.gate_event(&after, RunEventSource::Target);
            // HS2-JSF985: the note a red gate owes the work stacked on it.
            self.provisional_follow(&after.owner_checkout_id, tx);
        }
        Ok((after, changed))
    }

    // ---- Reads: this machine's records only ----

    pub fn gate(&self, owner_id: &str, gate_id: &str) -> Result<Gate, OrchestrationError> {
        self.gate_registry().get(owner_id, gate_id)
    }

    /// The standing gates matching the filter, rolled up.
    pub fn gate_summary(
        &self,
        owner_id: &str,
        filter: &GateFilter,
    ) -> Result<GateSummary, OrchestrationError> {
        Ok(summarize(self.gate_registry().list(owner_id, filter)?))
    }

    /// The gate state of one run: its CI verdict, its deployment state when one was
    /// requested, and the standing gates. `unknown` when nothing is linked.
    pub fn run_gate_state(
        &self,
        owner_id: &str,
        run_id: &str,
    ) -> Result<GateSummary, OrchestrationError> {
        self.run(owner_id, run_id)?;
        self.gate_summary(
            owner_id,
            &GateFilter {
                run_id: Some(run_id.into()),
                ..GateFilter::default()
            },
        )
    }

    /// The gate state of one commit of a target, across every run that links it.
    pub fn commit_gate_state(
        &self,
        owner_id: &str,
        target_id: &str,
        commit: &str,
    ) -> Result<GateSummary, OrchestrationError> {
        self.gate_summary(
            owner_id,
            &GateFilter {
                target_id: Some(target_id.into()),
                commit: Some(commit.to_ascii_lowercase()),
                ..GateFilter::default()
            },
        )
    }

    // ---- CI gates ----

    /// The repository a CI gate of this target looks in. A target that declares a
    /// GitHub repository is only ever gated by that repository.
    pub(super) fn gate_repository(
        &self,
        owner_id: &str,
        target_id: &str,
        requested: Option<String>,
    ) -> Result<String, OrchestrationError> {
        let declared = self
            .declarations(owner_id)?
            .targets
            .into_iter()
            .find(|target| target.target_id == target_id)
            .and_then(|target| target.expected_repository_identity)
            .and_then(|identity| github_repository(&identity));
        match (requested, declared) {
            (Some(requested), Some(declared)) if !requested.eq_ignore_ascii_case(&declared) => {
                Err(OrchestrationError::Gate(GateError::RepositoryMismatch))
            }
            (Some(requested), _) => Ok(requested),
            (None, Some(declared)) => Ok(declared),
            (None, None) => Err(OrchestrationError::Invalid(
                "repository is required: the target declares no GitHub repository",
            )),
        }
    }

    /// Link the CI gate of the integrated target commit to a run and start watching
    /// it. Repeating the link returns the same gate, and watches again when the gate
    /// failed or its watch was stopped. Linking another commit supersedes the older
    /// link. Passes the gate as `gates_observe`.
    pub fn gate_link(
        &self,
        owner_id: &str,
        run_id: &str,
        request: LinkGate,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<GateView, OrchestrationError> {
        let commit = request.commit.to_ascii_lowercase();
        valid_commit(&commit)?;
        let run = self.run(owner_id, run_id)?;
        let target = Self::target_ref(owner_id, &run.target.target_id)?;
        let authorized = self.authorize(
            &target,
            actor,
            ActionRequest::new(TargetAction::GatesObserve),
            tx,
        )?;
        let outcome = (|| {
            let name = request
                .provider
                .unwrap_or_else(|| GITHUB_ACTIONS_KIND.into());
            let provider = self
                .gate_provider(&name, GateKind::Ci)
                .ok_or(OrchestrationError::Gate(GateError::ProviderUnsupported))?;
            let repository =
                self.gate_repository(owner_id, &run.target.target_id, request.repository)?;
            let linked = self.gate_registry().link(
                owner_id,
                NewGate {
                    run_id: run.run_id.clone(),
                    target_id: run.target.target_id.clone(),
                    kind: GateKind::Ci,
                    provider: name,
                    commit,
                    ci: Some(CiScope {
                        repository,
                        workflow: request.workflow.unwrap_or_else(|| "CI".into()),
                        branch: request.branch.unwrap_or_else(|| "main".into()),
                    }),
                    deployment: None,
                    state: GateState::Unknown,
                    reason: Some("not_watched".into()),
                    linked_by: Some(authorized.actor_id.clone()),
                },
                tx,
            )?;
            for gate in &linked.superseded {
                self.gate_event(gate, RunEventSource::Orchestrator);
            }
            if linked.created {
                self.gate_event(&linked.gate, RunEventSource::Orchestrator);
            }
            let again = !linked.created
                && matches!(linked.gate.state, GateState::Failed | GateState::Unknown);
            let started = provider.start(&linked.gate, &run, again);
            let (gate, _) = self.gate_refresh(linked.gate, tx)?;
            started.map_err(OrchestrationError::Gate)?;
            // HS2-VVHVF0: a gate that still waits needs an observer; a settled one
            // starts nothing.
            let watching = if followed(&gate) {
                provider.ensure_watching()
            } else {
                provider.watching()
            };
            Ok(GateView {
                gate,
                created: linked.created,
                superseded: linked
                    .superseded
                    .into_iter()
                    .map(|gate| gate.gate_id)
                    .collect(),
                watching,
                request_id: None,
            })
        })();
        self.finish(&target, &authorized, &outcome, tx)?;
        outcome
    }

    /// Bring a run's gates in line with what their adapters recorded. Contacts no
    /// provider. With `wait_ms` and nothing new, waits for the adapter's records to
    /// change, so a headless caller needs no polling loop.
    pub fn gate_sync(
        &self,
        owner_id: &str,
        run_id: &str,
        request: &SyncGates,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<RunGates, OrchestrationError> {
        let run = self.run(owner_id, run_id)?;
        let target = Self::target_ref(owner_id, &run.target.target_id)?;
        let authorized = self.authorize(
            &target,
            actor,
            ActionRequest::new(TargetAction::GatesObserve),
            tx,
        )?;
        let filter = GateFilter {
            run_id: Some(run.run_id.clone()),
            current_only: true,
            ..GateFilter::default()
        };
        let outcome = (|| {
            let mut changed = Vec::new();
            let mut refresh = |tx: &mut Transaction<'_>| -> Result<_, OrchestrationError> {
                let gates = self.gate_registry().list(owner_id, &filter)?;
                // Before reading: a change in between is then seen by the wait.
                let waits_on = gates
                    .iter()
                    .filter(|gate| followed(gate))
                    .find_map(|gate| self.gate_provider(&gate.provider, gate.kind))
                    .map(|provider| (provider.clone(), provider.cursor(owner_id)));
                let before = changed.len();
                for gate in gates {
                    let (gate, moved) = self.gate_refresh(gate, tx)?;
                    if moved {
                        changed.push(gate.gate_id);
                    }
                }
                Ok((waits_on, changed.len() != before))
            };
            let (waits_on, moved) = refresh(tx)?;
            // HS2-VVHVF0: a gate that waits is observed before anyone waits on it.
            let watching = waits_on
                .as_ref()
                .and_then(|(provider, _)| provider.ensure_watching());
            if let (Some((provider, cursor)), false, true) = (waits_on, moved, request.wait_ms > 0)
            {
                provider.wait(
                    owner_id,
                    cursor,
                    Duration::from_millis(request.wait_ms.min(MAX_GATE_WAIT_MS)),
                );
                refresh(tx)?;
            }
            Ok(RunGates {
                run_id: run.run_id.clone(),
                summary: self.run_gate_state(owner_id, &run.run_id)?,
                changed,
                watching,
            })
        })();
        self.finish(&target, &authorized, &outcome, tx)?;
        outcome
    }

    /// One step of following an owner's gates: read every gate that still waits on a
    /// provider and, when none moved, wait for the provider's records to change. Call
    /// again while `active`. Acts as whoever linked each gate, and confirms that
    /// authority when it starts and every ten minutes; a target the gate no longer
    /// allows `gates_observe` on (unbound, removed or unreachable) is left alone.
    pub fn gate_follow_step(
        &self,
        owner_id: &str,
        state: &mut GateFollowState,
        wait_ms: u64,
        tx: &mut Transaction<'_>,
    ) -> GateFollowStep {
        let mut step = GateFollowStep::default();
        for round in 0..2 {
            let gates: Vec<Gate> = self
                .gate_registry()
                .list(owner_id, &GateFilter::default())
                .unwrap_or_default()
                .into_iter()
                .filter(followed)
                .collect();
            step.active = !gates.is_empty();
            if gates.is_empty() {
                state.authorized.clear();
                state.observer_retry = None;
                return step;
            }
            let waits_on = gates
                .iter()
                .find_map(|gate| self.gate_provider(&gate.provider, gate.kind))
                .map(|provider| (provider.clone(), provider.cursor(owner_id)));
            let now_ms = tx.clock.now_ms();
            let mut observed: Vec<Arc<dyn GateProvider>> = Vec::new();
            for gate in gates {
                let fresh = state
                    .authorized
                    .get(&gate.target_id)
                    .filter(|(_, at)| now_ms.saturating_sub(*at) < REAUTHORIZE_MS)
                    .map(|(allowed, _)| *allowed);
                let allowed = fresh.unwrap_or_else(|| {
                    let allowed = gate
                        .linked_by
                        .clone()
                        .and_then(|id| MutationActor::parse("system", Some(id)).ok())
                        .zip(Self::target_ref(owner_id, &gate.target_id).ok())
                        .is_some_and(|(actor, target)| {
                            self.authorize(
                                &target,
                                Some(&actor),
                                ActionRequest::new(TargetAction::GatesObserve),
                                tx,
                            )
                            .is_ok()
                        });
                    state
                        .authorized
                        .insert(gate.target_id.clone(), (allowed, now_ms));
                    allowed
                });
                if !allowed {
                    continue;
                }
                if let Some(provider) = self.gate_provider(&gate.provider, gate.kind) {
                    if !observed.iter().any(|other| Arc::ptr_eq(other, provider)) {
                        observed.push(provider.clone());
                    }
                }
                if let Ok((gate, true)) = self.gate_refresh(gate, tx) {
                    step.changed.push(gate.gate_id);
                }
            }
            // HS2-VVHVF0: a gate this follower may read is observed. The check is a
            // local lock probe at a moment the step is awake anyway; only a missing
            // observer launches anything, and a failed launch backs off.
            if round == 0 && state.observer_retry.is_none_or(|(_, at)| now_ms >= at) {
                for provider in &observed {
                    if let Some(watching) = provider.ensure_watching() {
                        step.watching = Some(step.watching.unwrap_or(true) && watching);
                    }
                }
                let delay = state
                    .observer_retry
                    .map_or(OBSERVER_RETRY_MS, |(delay, _)| {
                        (delay * 2).min(MAX_OBSERVER_RETRY_MS)
                    });
                state.observer_retry =
                    (step.watching == Some(false)).then_some((delay, now_ms.saturating_add(delay)));
            }
            if round == 1 || !step.changed.is_empty() || wait_ms == 0 {
                break;
            }
            let Some((provider, cursor)) = &waits_on else {
                break;
            };
            provider.wait(
                owner_id,
                *cursor,
                Duration::from_millis(wait_ms.min(MAX_GATE_WAIT_MS)),
            );
        }
        step
    }

    // ---- Deployments ----

    /// Ask for a deployment of the commit the run's CI gate passed, through the
    /// target's checked-in script.
    ///
    /// The request is refused unless every standing CI gate of the run passed for one
    /// commit. It passes the gate as `deployments_run` under its operation id, which
    /// asks a person through a permission request unless a remembered rule allows it
    /// (HS2-XHQ1EH). A production deployment cannot be undone, so it asks for each
    /// operation whatever is remembered. While waiting the gate is
    /// `awaiting_approval`; repeating the request after the answer moves it on. A registered deployment
    /// adapter then runs it and reports typed evidence. Without one the gate is
    /// `unknown` with `deployment_executor_unavailable`: authorized, and not run.
    pub fn gate_deploy(
        &self,
        owner_id: &str,
        run_id: &str,
        request: RequestDeployment,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<GateView, OrchestrationError> {
        identifier(&request.operation_id, "bounded operation id required")?;
        identifier(&request.environment, "bounded environment name required")?;
        valid_script(&request.script)?;
        let run = self.run(owner_id, run_id)?;
        let target = Self::target_ref(owner_id, &run.target.target_id)?;
        let name = request
            .provider
            .clone()
            .unwrap_or_else(|| TARGET_SCRIPT_PROVIDER.into());
        let registry = self.gate_registry();
        let view = |gate: Gate, created, request_id| GateView {
            gate,
            created,
            superseded: Vec::new(),
            watching: None,
            request_id,
        };
        let existing = registry
            .list(owner_id, &GateFilter::default())?
            .into_iter()
            .find(|gate| {
                gate.deployment
                    .as_ref()
                    .is_some_and(|scope| scope.operation_id == request.operation_id)
            });
        // The CI gate the deployment rests on: the one recorded, or the run's verdict.
        let (commit, requires_gate_id) = match &existing {
            Some(gate) => (
                gate.commit.clone(),
                gate.deployment
                    .as_ref()
                    .map(|scope| scope.requires_gate_id.clone())
                    .unwrap_or_default(),
            ),
            None => {
                let standing = self.run_gate_state(owner_id, run_id)?;
                let passed = standing
                    .gates
                    .iter()
                    .find(|gate| gate.kind == GateKind::Ci && gate.state == GateState::Passed);
                let (commit, gate) = match (standing.ci, standing.commits.as_slice(), passed) {
                    (GateState::Passed, [commit], Some(gate)) => (commit.clone(), gate),
                    _ => return Err(OrchestrationError::Gate(GateError::NotPassed)),
                };
                // HS2-3YC577: when both the request and the CI run name what was built,
                // they must be the same artifact. Refused before anything is asked.
                let built = gate
                    .evidence
                    .as_ref()
                    .and_then(|evidence| evidence.artifact.as_ref());
                if let (Some(wanted), Some(built)) = (&request.artifact, built) {
                    if wanted != built {
                        return Err(OrchestrationError::Gate(GateError::ArtifactMismatch));
                    }
                }
                (commit, gate.gate_id.clone())
            }
        };
        let new = |state, reason: Option<&str>, actor_id: &str| NewGate {
            run_id: run.run_id.clone(),
            target_id: run.target.target_id.clone(),
            kind: GateKind::Deployment,
            provider: name.clone(),
            commit: commit.clone(),
            ci: None,
            deployment: Some(DeploymentScope {
                environment: request.environment.clone(),
                script: request.script.clone(),
                production: request.production,
                operation_id: request.operation_id.clone(),
                requires_gate_id: requires_gate_id.clone(),
                artifact: request.artifact.clone(),
            }),
            state,
            reason: reason.map(Into::into),
            linked_by: Some(actor_id.into()),
        };
        if let Some(gate) = &existing {
            // The same operation for other content is a conflict, decided before
            // anything is asked of a person.
            let same = gate.run_id == run.run_id
                && gate.provider == name
                && gate.deployment.as_ref().is_some_and(|scope| {
                    scope.environment == request.environment
                        && scope.script == request.script
                        && scope.production == request.production
                        && scope.artifact == request.artifact
                });
            if !same {
                return Err(OrchestrationError::Conflict);
            }
            if gate.state != GateState::AwaitingApproval {
                // Already answered: a repeat asks nobody again and starts nothing.
                return Ok(view(gate.clone(), false, None));
            }
        }
        let authorized = self.authorize(
            &target,
            actor,
            ActionRequest {
                irreversible: request.production,
                ..ActionRequest::new(TargetAction::DeploymentsRun)
                    .operation(request.operation_id.clone())
            },
            tx,
        );
        let authorized = match authorized {
            Ok(authorized) => authorized,
            Err(
                error @ (OrchestrationError::PermissionRequired { .. }
                | OrchestrationError::Denied(DenialReason::PermissionDenied)),
            ) => {
                let actor_id = actor.and_then(|actor| actor.id.clone()).unwrap_or_default();
                let linked = registry.link(
                    owner_id,
                    new(GateState::AwaitingApproval, None, &actor_id),
                    tx,
                )?;
                for gate in &linked.superseded {
                    self.gate_event(gate, RunEventSource::Orchestrator);
                }
                if linked.created {
                    self.gate_event(&linked.gate, RunEventSource::Orchestrator);
                }
                let mut gate = linked.gate;
                let pending = match error {
                    OrchestrationError::PermissionRequired { request_id } => Some(request_id),
                    _ => {
                        let (denied, outcome) = registry.resolve_approval(
                            owner_id,
                            &gate.gate_id,
                            GateState::Failed,
                            Some("permission_denied"),
                            tx,
                        )?;
                        if outcome == GateOutcome::Changed {
                            self.gate_event(&denied, RunEventSource::Orchestrator);
                        }
                        gate = denied;
                        None
                    }
                };
                return Ok(view(gate, linked.created, pending));
            }
            Err(error) => return Err(error),
        };
        let outcome = (|| {
            let executor = self.gate_provider(&name, GateKind::Deployment);
            // Authorized. A gate that waited moves on; a new one starts where it stands.
            let (mut gate, created) = match existing {
                Some(gate) => (gate, false),
                None => {
                    let linked = registry.link(
                        owner_id,
                        new(
                            GateState::Unknown,
                            Some(if executor.is_some() {
                                "deployment_starting"
                            } else {
                                "deployment_executor_unavailable"
                            }),
                            &authorized.actor_id,
                        ),
                        tx,
                    )?;
                    for gate in &linked.superseded {
                        self.gate_event(gate, RunEventSource::Orchestrator);
                    }
                    self.gate_event(&linked.gate, RunEventSource::Orchestrator);
                    (linked.gate, linked.created)
                }
            };
            if gate.state == GateState::AwaitingApproval {
                let (to, reason) = match executor {
                    Some(_) => (GateState::Pending, None),
                    None => (GateState::Unknown, Some("deployment_executor_unavailable")),
                };
                gate = registry
                    .resolve_approval(owner_id, &gate.gate_id, to, reason, tx)?
                    .0;
                self.gate_event(&gate, RunEventSource::Orchestrator);
            }
            if let Some(executor) = executor {
                let started = executor.start(&gate, &run, false);
                gate = self.gate_refresh(gate, tx)?.0;
                started.map_err(OrchestrationError::Gate)?;
            }
            Ok(view(gate, created, None))
        })();
        self.finish(&target, &authorized, &outcome, tx)?;
        outcome
    }
}
