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
use super::{Gate, GateKind, GateReading, GateState};

/// A verified source's note: no CI gate is linked to the run.
pub const NOTE_NO_GATE: &str = "no_gate";
/// A verified source's note: a live watch the record did not name yet is the run's.
pub const NOTE_WATCH_ADOPTED: &str = "watch_adopted";

/// A standing gate with its adapter's own answer, or the code of why there is none.
pub type GateObservation = (Gate, Result<GateReading, String>);

/// What one gate amounts to for reconciliation.
#[derive(Debug, Clone, PartialEq, Eq)]
enum GateWork {
    /// Nothing is in flight: the gate holds a verdict, was never watched, or its watch
    /// was stopped on purpose.
    Settled,
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
fn ci_gate(gate: &Gate, reading: &Result<GateReading, String>) -> GateWork {
    // A passed gate is final; a superseded one no longer stands for the run.
    if matches!(gate.state, GateState::Passed | GateState::Superseded) {
        return GateWork::Settled;
    }
    let reading = match reading {
        Ok(reading) => reading,
        Err(code) => return GateWork::Unverifiable(code.clone()),
    };
    let recorded = gate.awaits_provider();
    let reference = || {
        gate.watch_ref
            .clone()
            .unwrap_or_else(|| gate.gate_id.clone())
    };
    match reading.state {
        GateState::Pending => GateWork::Live {
            reference: reading.watch_ref.clone().unwrap_or_else(reference),
            // The record named no watch, or another one: the live watch is adopted.
            adopted: !recorded
                || (reading.watch_ref.is_some() && reading.watch_ref != gate.watch_ref),
        },
        // A verdict ended the watch in the open; the record follows it on the next sync.
        GateState::Passed | GateState::Failed | GateState::Superseded => GateWork::Settled,
        GateState::Unknown | GateState::AwaitingApproval => match reading.reason.as_deref() {
            // Somebody stopped watching on purpose: nothing is in flight.
            Some("watch_cancelled") => GateWork::Settled,
            // The Job is there and its observer is gone: the CI run may have moved on.
            Some("watch_lost") if recorded => GateWork::Unverifiable("watch_lost".into()),
            // The record says a watch runs and the adapter has none.
            Some("not_watched") | None if recorded => GateWork::Missing {
                reference: reference(),
            },
            Some(other) if recorded => GateWork::Unverifiable(other.into()),
            // A gate already settled or never watched: nothing was in flight.
            _ => GateWork::Settled,
        },
    }
}

fn combine(works: Vec<GateWork>, empty_note: Option<&str>) -> Observed<ExternalWork> {
    if let Some(code) = works.iter().find_map(|work| match work {
        GateWork::Unverifiable(code) => Some(code.clone()),
        _ => None,
    }) {
        return Observed::Unverifiable(code);
    }
    if let Some(reference) = works.iter().find_map(|work| match work {
        GateWork::Missing { reference } => Some(reference.clone()),
        _ => None,
    }) {
        return Observed::Verified(ExternalWork {
            recorded: Some(reference),
            live: None,
            note: None,
        });
    }
    if let Some((reference, adopted)) = works.iter().find_map(|work| match work {
        GateWork::Live { reference, adopted } => Some((reference.clone(), *adopted)),
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
            None
        },
    })
}

/// HS2-7QNK7Y: the CI runs gating a run, from its standing CI gates and what the CI
/// adapter's own records (for GitHub Actions, the watch Jobs) say about each. Contacts
/// nothing. A gate still waiting on its provider is in flight; the adapter's live
/// watch is the run's own; a recorded watch the adapter no longer has is missing; a
/// lost watch or an adapter that cannot answer leaves the source unverifiable.
pub fn ci_work(gates: &[GateObservation]) -> Observed<ExternalWork> {
    combine(
        gates
            .iter()
            .filter(|(gate, _)| gate.kind == GateKind::Ci && gate.state != GateState::Superseded)
            .map(|(gate, reading)| ci_gate(gate, reading))
            .collect(),
        Some(NOTE_NO_GATE),
    )
}
