//! HS2-WBREDM: reconciling a run with the world after an interruption.
//!
//! Pure: every external source arrives as a typed observation that is either verified
//! facts or unverifiable with a reason code. [`align`] says how the run record follows
//! its target ticket, and [`assess`] says, source by source, what agrees, what does
//! not and what could not be checked, and whether the run may be resumed. Nothing
//! here reads a clock, a store or a network.

use serde::{Deserialize, Serialize};

use super::contract::valid_identifier;
use super::{DispatchMode, OrchestrationError, Run, RunState, TargetOpError};

/// The reason of a source nobody reports yet. Such a source never holds a run back,
/// because the run records no work of that kind; any other reason does.
pub const SOURCE_NOT_WIRED: &str = "source_not_wired";
pub const MAX_FINDINGS: usize = 16;

/// One kind of state outside the run record that a run depends on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalSource {
    /// Whether the target answered at all.
    TargetReachability,
    /// The target ticket's existence and status.
    TargetTicket,
    /// Who holds a live claim on the target ticket.
    TargetClaim,
    /// The lease the run recorded against the lease the target shows.
    RunLease,
    /// CI runs gating the run (HS2-5JBYPV).
    CiRun,
    /// Deployments started for the run (HS2-5JBYPV).
    Deployment,
    /// HS2-G5H9FQ: validation attempts started for the run, on the machine that hosts
    /// the step's target (HS2-WN0MXZ).
    Validation,
    /// Orchestrator-owned AI sessions working the run (HS2-GDJXGZ).
    Session,
}

impl ExternalSource {
    pub const ALL: [Self; 8] = [
        Self::TargetReachability,
        Self::TargetTicket,
        Self::TargetClaim,
        Self::RunLease,
        Self::CiRun,
        Self::Deployment,
        Self::Validation,
        Self::Session,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Self::TargetReachability => "target_reachability",
            Self::TargetTicket => "target_ticket",
            Self::TargetClaim => "target_claim",
            Self::RunLease => "run_lease",
            Self::CiRun => "ci_run",
            Self::Deployment => "deployment",
            Self::Validation => "validation",
            Self::Session => "session",
        }
    }

    /// The sources read from the target ticket itself.
    pub fn is_target(self) -> bool {
        matches!(
            self,
            Self::TargetReachability | Self::TargetTicket | Self::TargetClaim | Self::RunLease
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verification {
    /// The source was read and agrees with the run record.
    Verified,
    /// The source was read and disagrees with the run record.
    Mismatched,
    /// The source could not be read; nothing is known about it.
    Unverifiable,
}

/// What one source said, as a caller supplies it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Observed<T> {
    Verified(T),
    /// A stable reason code, never free text.
    Unverifiable(String),
}

impl<T> Observed<T> {
    /// A source that no ticket reports yet.
    pub fn not_wired() -> Self {
        Self::Unverifiable(SOURCE_NOT_WIRED.into())
    }
}

/// The target ticket as one successful read showed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetFacts {
    /// False when the target answered that it has no such ticket.
    pub exists: bool,
    pub status: String,
    pub claimed_by: Option<String>,
    pub lease_expires_at_ms: Option<u64>,
    /// HS2-ZNET66: how many claims the ticket has seen; `None` when unknown.
    pub claim_count: Option<u32>,
}

impl TargetFacts {
    /// The target answered, and the ticket is not there.
    pub fn missing() -> Self {
        Self {
            exists: false,
            status: String::new(),
            claimed_by: None,
            lease_expires_at_ms: None,
            claim_count: None,
        }
    }

    /// Who holds the ticket now. A lapsed lease is not a live claim, whoever's name is
    /// still on it.
    pub fn live_holder(&self, now_ms: u64) -> Option<&str> {
        self.claimed_by
            .as_deref()
            .filter(|_| self.lease_expires_at_ms.is_some_and(|at| at > now_ms))
    }

    fn finished(&self) -> bool {
        ticket_finished(&self.status)
    }
}

/// The ticket statuses that mean its work is over. HS2-42TARA reads the same rule.
pub(super) fn ticket_finished(status: &str) -> bool {
    matches!(status, "completed" | "verified" | "archive")
}

/// Work outside the target ticket that a run may have started: a CI run, a
/// deployment, a session. Identifiers only.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExternalWork {
    /// What the run's records say is still going on.
    pub recorded: Option<String>,
    /// What actually exists and is still going.
    pub live: Option<String>,
    /// HS2-7QNK7Y: a stable code that qualifies a verified finding, such as `no_gate`
    /// or `watch_adopted`, carried as the finding's reason. Never free text.
    pub note: Option<String>,
}

/// Everything a reconciliation knows about the world. Each field is one source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observations {
    pub target: Observed<TargetFacts>,
    pub ci: Observed<ExternalWork>,
    pub deployment: Observed<ExternalWork>,
    /// HS2-G5H9FQ.
    pub validation: Observed<ExternalWork>,
    pub session: Observed<ExternalWork>,
}

impl Observations {
    /// The target as observed, with the sources nobody reports yet declared so.
    pub fn target_only(target: Observed<TargetFacts>) -> Self {
        Self {
            target,
            ci: Observed::not_wired(),
            deployment: Observed::not_wired(),
            validation: Observed::not_wired(),
            session: Observed::not_wired(),
        }
    }
}

/// How a run record follows its target ticket. Everything `None` means the record
/// already agrees.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Alignment {
    pub to: Option<RunState>,
    pub reason: Option<&'static str>,
    /// The worker the target's dispatcher assigned.
    pub worker_id: Option<String>,
    /// `Some` replaces the recorded lease.
    pub lease_expires_at_ms: Option<Option<u64>>,
}

impl Alignment {
    fn step(to: RunState, reason: &'static str) -> Self {
        Self {
            to: Some(to),
            reason: Some(reason),
            ..Self::default()
        }
    }

    pub fn is_none(&self) -> bool {
        *self == Self::default()
    }
}

/// The change that brings an unfinished run in line with its target ticket. The
/// target is authoritative: a finished ticket finishes the run, a live claim is whose
/// work it is, and nothing here ever asks the target to change.
pub fn align(run: &Run, facts: &TargetFacts, now_ms: u64) -> Alignment {
    if run.state.is_terminal() {
        return Alignment::default();
    }
    if !facts.exists {
        return Alignment::step(RunState::Failed, TargetOpError::NotFound.code());
    }
    if facts.finished() {
        return Alignment::step(RunState::Completed, "target_completed");
    }
    if facts.status == "deleted" {
        return Alignment::step(RunState::Failed, "target_deleted");
    }
    let live = facts.live_holder(now_ms);
    let ours = live.is_some() && live == run.worker_id.as_deref();
    let lease = Some(facts.lease_expires_at_ms);
    let assigned = |worker: &str| Alignment {
        worker_id: Some(worker.into()),
        lease_expires_at_ms: lease,
        ..Alignment::step(RunState::Dispatched, "assigned_by_dispatcher")
    };
    match (run.state, run.mode) {
        // HS2-ZNET66: its worker claimed, worked and released between two reads, so
        // the run was never seen dispatched; the release still makes it `intervened`.
        (RunState::Queued, _) if live.is_none() && claimed_meanwhile(run, facts) => {
            Alignment::step(RunState::Intervened, "claim_released")
        }
        (RunState::Queued, _) => live.map_or_else(Alignment::default, assigned),
        // The Up Next mark was set and the answer lost: whoever the target's
        // dispatcher assigned holds the work, and it is never queued twice.
        (RunState::Requested, DispatchMode::Dispatcher) => {
            live.map_or_else(Alignment::default, assigned)
        }
        // A claim taken before the answer was recorded is the run's own.
        (RunState::Requested | RunState::Intervened, DispatchMode::Direct) if ours => Alignment {
            lease_expires_at_ms: lease,
            ..Alignment::step(RunState::Dispatched, "claim_confirmed")
        },
        (RunState::Dispatched, _) if ours => Alignment {
            lease_expires_at_ms: lease.filter(|lease| *lease != run.lease_expires_at_ms),
            ..Alignment::default()
        },
        (RunState::Dispatched, _) => Alignment::step(
            RunState::Intervened,
            if live.is_some() {
                "claim_taken"
            } else {
                "claim_released"
            },
        ),
        _ => Alignment::default(),
    }
}

/// What may happen to a run next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Disposition {
    /// The run is over; nothing to resume.
    Finished,
    /// A person paused it; nothing is resumed until it is resumed.
    Paused,
    /// Something the run depends on could not be verified; do nothing until it can.
    HoldUnverified,
    /// A cancel was asked for and the target was not told; repeat the cancel.
    RetryCancel,
    /// Nothing is held in the target; repeating the dispatch with the same operation
    /// id is safe.
    RetryDispatch,
    /// Queued for the target's own dispatcher; nothing to do but wait.
    WaitForDispatcher,
    /// The run's worker holds the target ticket; carry on.
    Continue,
    /// The record and the world disagree in a way only a person can settle: hand the
    /// run off, correct it or cancel it.
    NeedsAttention,
}

impl Disposition {
    pub const ALL: [Self; 8] = [
        Self::Finished,
        Self::Paused,
        Self::HoldUnverified,
        Self::RetryCancel,
        Self::RetryDispatch,
        Self::WaitForDispatcher,
        Self::Continue,
        Self::NeedsAttention,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Self::Finished => "finished",
            Self::Paused => "paused",
            Self::HoldUnverified => "hold_unverified",
            Self::RetryCancel => "retry_cancel",
            Self::RetryDispatch => "retry_dispatch",
            Self::WaitForDispatcher => "wait_for_dispatcher",
            Self::Continue => "continue",
            Self::NeedsAttention => "needs_attention",
        }
    }
}

/// HS2-AH8CRS: what the scheduler does about a run's stored reconciliation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispositionAction {
    /// Repeat the dispatch with the run's retained operation id: nothing is held in
    /// the target, and the run is still `requested`.
    Redispatch,
    /// Tell a person on the goal ticket; nothing is decided for them.
    AskPerson,
    /// Leave the run alone, for this reason (`paused`, `hold_unverified`, ...).
    Leave(&'static str),
}

impl DispositionAction {
    /// The code the scheduler's decision log records for this action.
    pub fn code(self) -> &'static str {
        match self {
            Self::Redispatch => "redispatch",
            Self::AskPerson => "needs_attention",
            Self::Leave(reason) => reason,
        }
    }
}

/// HS2-AH8CRS: the scheduler's action for one run, from its stored reconciliation.
/// `None` when there is nothing to act on: no stored outcome, a finished run, or a
/// disposition that is already being handled (a queued run waits for its dispatcher,
/// a worker holds the ticket, reconciliation repeats a cancel itself). A paused run is
/// left alone whatever its outcome says; a dispatch is repeated only while the run is
/// still `requested`.
pub fn disposition_action(run: &Run) -> Option<DispositionAction> {
    if run.state.is_terminal() {
        return None;
    }
    let found = run.reconciliation.as_ref()?;
    if run.paused.is_some() {
        return Some(DispositionAction::Leave(Disposition::Paused.code()));
    }
    match found.disposition {
        Disposition::RetryDispatch if run.state == RunState::Requested && !run.cancel_requested => {
            Some(DispositionAction::Redispatch)
        }
        Disposition::NeedsAttention => Some(DispositionAction::AskPerson),
        Disposition::HoldUnverified | Disposition::Paused => {
            Some(DispositionAction::Leave(found.disposition.code()))
        }
        Disposition::RetryDispatch
        | Disposition::RetryCancel
        | Disposition::WaitForDispatcher
        | Disposition::Continue
        | Disposition::Finished => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceFinding {
    pub source: ExternalSource,
    pub verification: Verification,
    /// Why it is mismatched or unverifiable. A stable code, never free text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// The result of comparing one run with the observations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assessment {
    /// One finding per source, in [`ExternalSource::ALL`] order. Empty for a finished
    /// run, which depends on nothing any more.
    pub findings: Vec<SourceFinding>,
    pub disposition: Disposition,
    /// Sources where work is going on right now: a live claim, a running session.
    pub live: Vec<ExternalSource>,
}

impl Assessment {
    pub fn finding(&self, source: ExternalSource) -> Option<&SourceFinding> {
        self.findings.iter().find(|found| found.source == source)
    }

    /// Exactly the sources that could not be verified.
    pub fn unverified(&self) -> Vec<ExternalSource> {
        self.findings
            .iter()
            .filter(|found| found.verification == Verification::Unverifiable)
            .map(|found| found.source)
            .collect()
    }

    /// Whether new work of this kind may be started for the run without risking a
    /// duplicate: only when the source was read and shows none going on. A source
    /// that could not be verified never allows a start.
    pub fn may_start(&self, source: ExternalSource) -> bool {
        self.finding(source)
            .is_some_and(|found| found.verification != Verification::Unverifiable)
            && !self.live.contains(&source)
    }
}

fn finding(source: ExternalSource, verdict: Result<(), &str>) -> SourceFinding {
    SourceFinding {
        source,
        verification: if verdict.is_ok() {
            Verification::Verified
        } else {
            Verification::Mismatched
        },
        reason: verdict.err().map(Into::into),
    }
}

fn unverifiable(source: ExternalSource, reason: &str) -> SourceFinding {
    SourceFinding {
        source,
        verification: Verification::Unverifiable,
        reason: Some(reason.into()),
    }
}

fn ticket_verdict(facts: &TargetFacts) -> Result<(), &'static str> {
    if !facts.exists {
        Err(TargetOpError::NotFound.code())
    } else if facts.finished() {
        Err("target_completed")
    } else if facts.status == "deleted" {
        Err("target_deleted")
    } else {
        Ok(())
    }
}

/// HS2-ZNET66: a claim was taken and let go between two reads of the ticket. No
/// holder is left to see, but its claim count moved past what the run last observed.
/// Unknown on either side (an older record, a provider without counts) is never.
fn claimed_meanwhile(run: &Run, facts: &TargetFacts) -> bool {
    facts
        .claim_count
        .zip(run.observed.as_ref().and_then(|seen| seen.claim_count))
        .is_some_and(|(now, seen)| now > seen)
}

fn claim_verdict(run: &Run, facts: &TargetFacts, now_ms: u64) -> Result<(), &'static str> {
    let live = facts.live_holder(now_ms);
    let ours = live.is_some() && live == run.worker_id.as_deref();
    match (run.state, run.mode) {
        (RunState::Dispatched, _) if ours => Ok(()),
        (RunState::Dispatched, _) if live.is_some() => Err("claim_taken"),
        (RunState::Dispatched, _) => Err("claim_released"),
        (RunState::Queued, _) if live.is_some() => Err("assigned_by_dispatcher"),
        (RunState::Queued, _) if claimed_meanwhile(run, facts) => Err("claim_released"),
        (RunState::Requested, DispatchMode::Direct) if live.is_some() && !ours => {
            Err("claim_held_by_other")
        }
        (RunState::Requested, _) if live.is_some() => Err("claim_unrecorded"),
        (RunState::Intervened, DispatchMode::Direct) if ours => Err("claim_returned"),
        _ => Ok(()),
    }
}

fn lease_verdict(run: &Run, facts: &TargetFacts, now_ms: u64) -> Result<(), &'static str> {
    let named = facts.claimed_by.is_some() && facts.claimed_by == run.worker_id;
    match run.state {
        RunState::Dispatched if named && facts.live_holder(now_ms).is_none() => Err("lease_lapsed"),
        RunState::Dispatched if named && facts.lease_expires_at_ms != run.lease_expires_at_ms => {
            Err("lease_differs")
        }
        RunState::Dispatched if named => Ok(()),
        // A lease the run still records for a claim that is not its worker's.
        _ if run.lease_expires_at_ms.is_some() => Err("lease_void"),
        _ => Ok(()),
    }
}

fn work_finding(source: ExternalSource, observed: &Observed<ExternalWork>) -> SourceFinding {
    match observed {
        Observed::Unverifiable(reason) => unverifiable(source, reason),
        // HS2-7QNK7Y: what the verified answer rests on, when the source says.
        Observed::Verified(work) if work.recorded == work.live => SourceFinding {
            source,
            verification: Verification::Verified,
            reason: work.note.clone(),
        },
        Observed::Verified(work) => finding(
            source,
            match (&work.recorded, &work.live) {
                (recorded, live) if recorded == live => Ok(()),
                // Something is going on that the run never recorded: adopt it, never
                // start a second one.
                (None, Some(_)) => Err("unrecorded_live_work"),
                (Some(_), None) => Err("recorded_work_missing"),
                _ => Err("different_live_work"),
            },
        ),
    }
}

/// Compare one run with the observations. Call it on the run as it stands after
/// [`align`] was applied: what is still mismatched then is what the target cannot
/// settle by itself.
pub fn assess(run: &Run, observed: &Observations, now_ms: u64) -> Assessment {
    if run.state.is_terminal() {
        return Assessment {
            findings: Vec::new(),
            disposition: Disposition::Finished,
            live: Vec::new(),
        };
    }
    let mut live = Vec::new();
    let mut findings = match &observed.target {
        Observed::Unverifiable(reason) => [
            ExternalSource::TargetReachability,
            ExternalSource::TargetTicket,
            ExternalSource::TargetClaim,
            ExternalSource::RunLease,
        ]
        .into_iter()
        .map(|source| unverifiable(source, reason))
        .collect::<Vec<_>>(),
        Observed::Verified(facts) => {
            if facts.exists && facts.live_holder(now_ms).is_some() {
                live.push(ExternalSource::TargetClaim);
            }
            vec![
                finding(ExternalSource::TargetReachability, Ok(())),
                finding(ExternalSource::TargetTicket, ticket_verdict(facts)),
                finding(
                    ExternalSource::TargetClaim,
                    claim_verdict(run, facts, now_ms),
                ),
                finding(ExternalSource::RunLease, lease_verdict(run, facts, now_ms)),
            ]
        }
    };
    for (source, work) in [
        (ExternalSource::CiRun, &observed.ci),
        (ExternalSource::Deployment, &observed.deployment),
        (ExternalSource::Validation, &observed.validation),
        (ExternalSource::Session, &observed.session),
    ] {
        if matches!(work, Observed::Verified(work) if work.live.is_some()) {
            live.push(source);
        }
        findings.push(work_finding(source, work));
    }
    let holds = findings.iter().any(|found| {
        found.verification == Verification::Unverifiable
            && found.reason.as_deref() != Some(SOURCE_NOT_WIRED)
    });
    let mismatched = findings
        .iter()
        .any(|found| found.verification == Verification::Mismatched);
    let disposition = if run.paused.is_some() {
        Disposition::Paused
    } else if holds {
        Disposition::HoldUnverified
    } else if run.cancel_requested {
        Disposition::RetryCancel
    } else if mismatched {
        Disposition::NeedsAttention
    } else {
        match run.state {
            RunState::Requested => Disposition::RetryDispatch,
            RunState::Queued => Disposition::WaitForDispatcher,
            RunState::Dispatched => Disposition::Continue,
            _ => Disposition::NeedsAttention,
        }
    };
    Assessment {
        findings,
        disposition,
        live,
    }
}

/// HS2-WBREDM: the outcome a run keeps from its last reconciliation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunReconciliation {
    /// When this outcome was first reached; re-checking an unchanged world keeps it.
    pub at_ms: u64,
    pub disposition: Disposition,
    pub findings: Vec<SourceFinding>,
}

impl RunReconciliation {
    pub fn new(at_ms: u64, assessment: Assessment) -> Self {
        Self {
            at_ms,
            disposition: assessment.disposition,
            findings: assessment.findings,
        }
    }

    /// The same outcome, whenever it was reached.
    pub fn same_facts(&self, other: &Self) -> bool {
        self.disposition == other.disposition && self.findings == other.findings
    }

    /// Exactly the sources that could not be verified, by code.
    pub fn unverified(&self) -> Vec<&'static str> {
        self.findings
            .iter()
            .filter(|found| found.verification == Verification::Unverifiable)
            .map(|found| found.source.code())
            .collect()
    }

    pub(super) fn validate(&self) -> Result<(), OrchestrationError> {
        if self.findings.len() > MAX_FINDINGS {
            return Err(OrchestrationError::Invalid("bounded findings required"));
        }
        for reason in self
            .findings
            .iter()
            .filter_map(|found| found.reason.as_ref())
        {
            valid_identifier(reason)?;
        }
        Ok(())
    }
}
