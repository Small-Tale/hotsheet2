//! HS2-ESVYQH: what a target's coordination pool says about handoffs its integrator
//! returned for corrections (docs/05 §5.9.1, HS2-65ETRJ/HS2-AEBJYN), read by the
//! hosted session listing so the orchestrator can see a returned run's correction
//! waiting with nobody to assign it (docs/26 §26.24).
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::coordination::{Authority, Phase, State, TransitionKind};

/// A ticket whose latest handoff the integrator returned for corrections and that no
/// correction assignment has taken up yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReturnedHandoff {
    pub connection_id: String,
    pub native_id: String,
    /// When the latest return was recorded; 0 for a record from before the
    /// assignment timeline (HS2-4273QB), which reads as long ago.
    pub returned_at_ms: u64,
    /// The latest return's `## Review return` note id.
    pub review_note: String,
}

fn at_ms(at: &str) -> Option<u64> {
    let parsed =
        time::OffsetDateTime::parse(at, &time::format_description::well_known::Rfc3339).ok()?;
    u64::try_from(parsed.unix_timestamp_nanos() / 1_000_000).ok()
}

/// Every ticket of the pool with a returned handoff and no correction in flight. An
/// assignment that is not Done and not itself returned (offered, accepted, handed off,
/// being integrated, or blocked for another reason) is the correction or other live
/// work on the ticket, so the ticket is left out. Sorted by ticket.
pub fn returned_handoffs(state: &State) -> Vec<ReturnedHandoff> {
    let mut tickets: BTreeMap<(&str, &str), Option<ReturnedHandoff>> = BTreeMap::new();
    let mut busy: Vec<(&str, &str)> = Vec::new();
    for assignment in state.assignments.values() {
        let key = (
            assignment.ticket.connection_id.as_str(),
            assignment.ticket.native_id.as_str(),
        );
        if !assignment.returned_for_review() {
            if assignment.phase != Phase::Done {
                busy.push(key);
            }
            continue;
        }
        let returned_at_ms = assignment
            .transitions
            .iter()
            .rev()
            .find(|transition| transition.kind == TransitionKind::ReviewReturn)
            .and_then(|transition| at_ms(&transition.at))
            .unwrap_or(0);
        let found = ReturnedHandoff {
            connection_id: key.0.to_owned(),
            native_id: key.1.to_owned(),
            returned_at_ms,
            review_note: assignment.recovery_note.clone().unwrap_or_default(),
        };
        let latest = tickets.entry(key).or_default();
        if latest
            .as_ref()
            .is_none_or(|seen| seen.returned_at_ms <= found.returned_at_ms)
        {
            *latest = Some(found);
        }
    }
    tickets
        .into_iter()
        .filter(|(key, _)| !busy.contains(key))
        .filter_map(|(_, found)| found)
        .collect()
}

/// Whether the role that assigns work in this pool holds a live lease: the dispatcher
/// in a dispatcher-authority pool, the integrator in a legacy one. `now_s` in seconds.
pub fn assigner_live(state: &State, now_s: i64) -> bool {
    let holder = match state.authority {
        Authority::Dispatcher => state.dispatcher.as_ref(),
        Authority::Integrator => state.integrator.as_ref(),
    };
    holder.is_some_and(|lease| lease.expires_at > now_s)
}
