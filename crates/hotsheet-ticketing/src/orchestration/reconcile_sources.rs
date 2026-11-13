//! Typed observations of the external sources a run depends on, built from this
//! machine's own records (HS2-7QNK7Y and its siblings). Pure: each function takes
//! records and adapter readings that a caller has already read, and answers one
//! [`Observed<ExternalWork>`] for [`super::assess`].
//!
//! The rule for every source is the same three-way answer. Work the record says is in
//! flight is compared with work that is live. The same work, or nothing on both
//! sides, is verified; work the run recorded that is gone is mismatched; anything that
//! could not be read is unverifiable with the code of why. Work the adapter shows live
//! that the record does not name yet is adopted: it is the run's, and it is never
//! started a second time.

use super::reconcile::{ExternalWork, Observed};
use super::{Gate, GateKind, GateReading, GateState, ValidationResult, ValidationState};

/// A verified source's note: no CI gate is linked to the run.
pub const NOTE_NO_GATE: &str = "no_gate";
/// A verified source's note: a live watch the record did not name yet is the run's.
pub const NOTE_WATCH_ADOPTED: &str = "watch_adopted";

/// A standing gate with its adapter's own answer, or the code of why there is none.
pub type GateObservation = (Gate, Result<GateReading, String>);

/// What one piece of recorded work (a gate, a validation attempt, a session) amounts
/// to for reconciliation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceWork {
    /// Nothing is in flight: the work ended in the open, or never started. The note,
    /// when present, says which, as a stable code.
    Settled(Option<String>),
    /// A watch or execution is going on. `adopted` when the record did not name it.
    Live { reference: String, adopted: bool },
    /// The record says something is in flight and its adapter has no trace of it.
    Missing { reference: String },
    /// The adapter could not answer; the code says why.
    Unverifiable(String),
}

/// A CI gate as its adapter's own records show it. `reading` is the adapter's answer,
/// or the code of why there is none (`gate_provider_unsupported` when no adapter
/// serves the gate's provider, `gate_provider_unavailable` when its records could
/// not be read).
fn ci_gate(gate: &Gate, reading: &Result<GateReading, String>) -> SourceWork {
    // A passed gate is final; a superseded one no longer stands for the run.
    if matches!(gate.state, GateState::Passed | GateState::Superseded) {
        return SourceWork::Settled(None);
    }
    let reading = match reading {
        Ok(reading) => reading,
        Err(code) => return SourceWork::Unverifiable(code.clone()),
    };
    let recorded = gate.awaits_provider();
    let reference = || {
        gate.watch_ref
            .clone()
            .unwrap_or_else(|| gate.gate_id.clone())
    };
    match reading.state {
        GateState::Pending => SourceWork::Live {
            reference: reading.watch_ref.clone().unwrap_or_else(reference),
            // The record named no watch, or another one: the live watch is adopted.
            adopted: !recorded
                || (reading.watch_ref.is_some() && reading.watch_ref != gate.watch_ref),
        },
        // A verdict ended the watch in the open; the record follows it on the next sync.
        GateState::Passed | GateState::Failed | GateState::Superseded => SourceWork::Settled(None),
        GateState::Unknown | GateState::AwaitingApproval => match reading.reason.as_deref() {
            // Somebody stopped watching on purpose: nothing is in flight.
            Some("watch_cancelled") => SourceWork::Settled(None),
            // The Job is there and its observer is gone: the CI run may have moved on.
            Some("watch_lost") if recorded => SourceWork::Unverifiable("watch_lost".into()),
            // The record says a watch runs and the adapter has none.
            Some("not_watched") | None if recorded => SourceWork::Missing {
                reference: reference(),
            },
            Some(other) if recorded => SourceWork::Unverifiable(other.into()),
            // A gate already settled or never watched: nothing was in flight.
            _ => SourceWork::Settled(None),
        },
    }
}

/// Combine the work of one source: any unverifiable piece makes the source
/// unverifiable, then a missing piece outranks a live one, and with nothing in flight
/// the first settled note (or `empty_note` when there is no piece at all) qualifies
/// the verified answer.
pub fn combine_work(works: Vec<SourceWork>, empty_note: Option<&str>) -> Observed<ExternalWork> {
    if let Some(code) = works.iter().find_map(|work| match work {
        SourceWork::Unverifiable(code) => Some(code.clone()),
        _ => None,
    }) {
        return Observed::Unverifiable(code);
    }
    if let Some(reference) = works.iter().find_map(|work| match work {
        SourceWork::Missing { reference } => Some(reference.clone()),
        _ => None,
    }) {
        return Observed::Verified(ExternalWork {
            recorded: Some(reference),
            live: None,
            note: None,
        });
    }
    if let Some((reference, adopted)) = works.iter().find_map(|work| match work {
        SourceWork::Live { reference, adopted } => Some((reference.clone(), *adopted)),
        _ => None,
    }) {
        return Observed::Verified(ExternalWork {
            recorded: Some(reference.clone()),
            live: Some(reference),
            note: adopted.then(|| NOTE_WATCH_ADOPTED.into()),
        });
    }
    Observed::Verified(ExternalWork {
        recorded: None,
        live: None,
        note: if works.is_empty() {
            empty_note.map(Into::into)
        } else {
            works.into_iter().find_map(|work| match work {
                SourceWork::Settled(note) => note,
                _ => None,
            })
        },
    })
}

/// HS2-7QNK7Y: the CI runs gating a run, from its standing CI gates and what the CI
/// adapter's own records (for GitHub Actions, the watch Jobs) say about each. Contacts
/// nothing. A gate still waiting on its provider is in flight; the adapter's live
/// watch is the run's own; a recorded watch the adapter no longer has is missing; a
/// lost watch or an adapter that cannot answer leaves the source unverifiable.
pub fn ci_work(gates: &[GateObservation]) -> Observed<ExternalWork> {
    combine_work(
        gates
            .iter()
            .filter(|(gate, _)| gate.kind == GateKind::Ci && gate.state != GateState::Superseded)
            .map(|(gate, reading)| ci_gate(gate, reading))
            .collect(),
        Some(NOTE_NO_GATE),
    )
}

/// HS2-BYEB60: no deployment was requested for the run.
pub const NOTE_NO_DEPLOYMENT: &str = "no_deployment";
/// HS2-BYEB60: a deployment waits for a person; nothing was executed.
pub const NOTE_AWAITING_APPROVAL: &str = "awaiting_approval";
/// HS2-BYEB60: a deployment was approved and no executor exists to run it, so this
/// machine's records show that nothing was executed (HS2-G3VAK4).
pub const NOTE_DEPLOYMENT_NOT_EXECUTED: &str = "deployment_not_executed";
/// The reason a deployment that is approved and cannot run is recorded with.
const EXECUTOR_UNAVAILABLE: &str = "deployment_executor_unavailable";
/// The reason a deployment whose executor was asked to start it is recorded with.
const DEPLOYMENT_STARTING: &str = "deployment_starting";
/// What a deployment executor answers for an operation it has no execution of.
pub const EXECUTION_NOT_FOUND: &str = "execution_not_found";

/// A deployment gate against its executor's own records, by its retained operation id.
fn deployment_gate(gate: &Gate, reading: &Result<GateReading, String>) -> SourceWork {
    let operation = gate
        .deployment
        .as_ref()
        .map_or_else(|| gate.gate_id.clone(), |scope| scope.operation_id.clone());
    let reason = gate.reason.as_deref();
    match gate.state {
        GateState::Passed | GateState::Failed | GateState::Superseded => {
            return SourceWork::Settled(None);
        }
        // A person has not answered: nothing was asked of an executor.
        GateState::AwaitingApproval => {
            return SourceWork::Settled(Some(NOTE_AWAITING_APPROVAL.into()));
        }
        GateState::Unknown if reason == Some(EXECUTOR_UNAVAILABLE) => {
            return SourceWork::Settled(Some(NOTE_DEPLOYMENT_NOT_EXECUTED.into()));
        }
        // Withdrawn, or otherwise never handed to an executor.
        GateState::Unknown if reason != Some(DEPLOYMENT_STARTING) => {
            return SourceWork::Settled(None);
        }
        // Executing, or asked to start: only the executor can say.
        GateState::Pending | GateState::Unknown => {}
    }
    let reading = match reading {
        Ok(reading) => reading,
        // No executor serves the provider on this machine: nothing can confirm it.
        Err(code) if code == "gate_provider_unsupported" => {
            return SourceWork::Unverifiable(EXECUTOR_UNAVAILABLE.into());
        }
        Err(code) => return SourceWork::Unverifiable(code.clone()),
    };
    match (reading.state, reading.reason.as_deref()) {
        (GateState::Pending, _) => SourceWork::Live {
            reference: operation,
            adopted: false,
        },
        (GateState::Passed | GateState::Failed | GateState::Superseded, _) => {
            SourceWork::Settled(None)
        }
        (_, Some(EXECUTION_NOT_FOUND)) => SourceWork::Missing {
            reference: operation,
        },
        (_, reason) => {
            SourceWork::Unverifiable(reason.unwrap_or("deployment_state_unknown").into())
        }
    }
}

/// HS2-BYEB60: the deployments started for a run, observed by their retained
/// operation ids. Verified when nothing was executed (no request, a request waiting
/// for a person, or one approved with no executor to run it) or the executor shows the
/// recorded execution; mismatched when the executor has no such execution;
/// unverifiable with the executor's code, or `deployment_executor_unavailable`, when
/// an execution is recorded and nothing can confirm it.
pub fn deployment_work(gates: &[GateObservation]) -> Observed<ExternalWork> {
    combine_work(
        gates
            .iter()
            .filter(|(gate, _)| gate.kind == GateKind::Deployment)
            .filter(|(gate, _)| gate.state != GateState::Superseded)
            .map(|(gate, reading)| deployment_gate(gate, reading))
            .collect(),
        Some(NOTE_NO_DEPLOYMENT),
    )
}

/// HS2-G5H9FQ: no validation attempt names the run.
pub const NOTE_NO_VALIDATION: &str = "no_validation";
/// HS2-G5H9FQ: the host accepted an attempt and no longer knows it.
pub const EXECUTION_LOST: &str = "execution_lost";
/// HS2-G5H9FQ: the host never confirmed an attempt's start and does not know it.
pub const EXECUTION_UNCONFIRMED: &str = "execution_unconfirmed";
/// HS2-G5H9FQ: a running attempt outlived its deadline (see `effective`).
pub const DEADLINE_PASSED: &str = "deadline_passed";

/// What the machine hosting a step answered about one running attempt, with no wait.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttemptAnswer {
    /// The execution is still going on there.
    Running,
    /// It ended there; the caller records the outcome the host observed.
    Finished,
    /// The host has no such execution.
    Unknown,
    /// The host could not be asked, or refused the read; the code says why.
    Unreachable(String),
}

/// What reconciliation records for one running attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttemptRecord {
    /// Leave the attempt `running`.
    Keep,
    /// Record the outcome the host observed, as a continuing request would.
    Outcome,
    /// End the attempt `unverified` with this reason. It is never run a second time.
    Unverified(&'static str),
}

/// HS2-G5H9FQ: one validation attempt that names a run, against what its host said.
/// A finished attempt needs no answer: it is settled, and an `unverified` one keeps
/// its reason as the note, so the answer is the same on every reconciliation.
pub fn validation_attempt(
    result: &ValidationResult,
    answer: Option<&AttemptAnswer>,
    now_ms: u64,
) -> (SourceWork, AttemptRecord) {
    if result.state != ValidationState::Running {
        let note = (result.state == ValidationState::Unverified)
            .then(|| result.reason.clone())
            .flatten();
        return (SourceWork::Settled(note), AttemptRecord::Keep);
    }
    let lapsed = result
        .deadline_ms
        .is_some_and(|deadline| now_ms >= deadline);
    match answer {
        None => (
            SourceWork::Unverifiable(EXECUTION_UNCONFIRMED.into()),
            AttemptRecord::Keep,
        ),
        Some(AttemptAnswer::Running) => (
            SourceWork::Live {
                reference: result.result_id.clone(),
                adopted: false,
            },
            AttemptRecord::Keep,
        ),
        Some(AttemptAnswer::Finished) => (SourceWork::Settled(None), AttemptRecord::Outcome),
        // The host accepted it and lost it: whatever happened cannot be confirmed.
        Some(AttemptAnswer::Unknown) if result.host_started => (
            SourceWork::Settled(Some(EXECUTION_LOST.into())),
            AttemptRecord::Unverified(EXECUTION_LOST),
        ),
        // Never confirmed, and its time is up: nothing more can be learned.
        Some(AttemptAnswer::Unknown) if lapsed => (
            SourceWork::Settled(Some(DEADLINE_PASSED.into())),
            AttemptRecord::Unverified(DEADLINE_PASSED),
        ),
        // Never confirmed: the same request may still start it under the same id.
        Some(AttemptAnswer::Unknown) => (
            SourceWork::Unverifiable(EXECUTION_UNCONFIRMED.into()),
            AttemptRecord::Keep,
        ),
        Some(AttemptAnswer::Unreachable(code)) => {
            (SourceWork::Unverifiable(code.clone()), AttemptRecord::Keep)
        }
    }
}

/// HS2-G5H9FQ: the validation attempts of a run, combined.
pub fn validation_work(attempts: Vec<SourceWork>) -> Observed<ExternalWork> {
    combine_work(attempts, Some(NOTE_NO_VALIDATION))
}
