//! HS2-9FXASE: time an owning checkout's work spent waiting on systems outside it.
//!
//! A pure fold, like the rest of the summary (HS2-S13719): it reads the run gates, the
//! scheduler's decision journal and a tally of the provisional ledger, and writes
//! nothing. Every measure is a [`Metric`], so an unknown is never a zero.
//!
//! - **CI and deployments** wait while a gate of that kind is `pending`: from each
//!   transition into it to the transition out, or the summary's time. The gates of one
//!   run and kind are joined, so a superseding gate does not count the same wait twice.
//! - **Reviews** are a person's approval of a deployment: the time a deployment gate is
//!   `awaiting_approval`, joined the same way.
//! - **Provider limits** are the time a plan step stayed unplaced because every tool
//!   account it could use was rate- or quota-limited (`no_tool_available` with detail
//!   `limited`), until the step's next other decision.
//! - **Idle while waiting** is the time the scheduler kept a session that had nothing to
//!   do but wait on its ticket's gate (`waiting_on_gate`).
//! - **Provisional work** started, rebased onto a fix, or reopened after a red gate is
//!   counted from the provisional ledger (§26.22).
//!
//! A gate whose bounded history no longer shows its first transition, a trimmed
//! decision journal and a ledger that may have forgotten settled work make the measures
//! they feed unknown instead of lower.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::summary_use::{by_subject, episodes};
use super::{DecisionKind, DecisionRecord, Gate, GateKind, GateState, Metric, UnknownReason};

/// What the provisional ledger holds for the owner, tallied by the runtime that owns
/// the ledger's type.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProvisionalTally {
    pub started: u64,
    pub rebased: u64,
    pub reopened: u64,
    /// The ledger is at its bound, so it may have forgotten settled work.
    pub trimmed: bool,
}

/// Everything the external-wait measures are computed from.
pub struct ExternalWaitInput<'a> {
    pub owner_checkout_id: &'a str,
    pub now_ms: u64,
    pub gates: &'a [Gate],
    /// The owner's scheduler decisions, any order.
    pub decisions: &'a [DecisionRecord],
    /// Retention removed decisions older than the ones given.
    pub decisions_trimmed: bool,
    pub provisional: ProvisionalTally,
}

/// The time work waited on one external system.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemWait {
    pub wait_ms: Metric<u64>,
    /// Runs (for gates) or plan steps (for provider limits) that waited at all.
    pub waited: u64,
}

impl SystemWait {
    fn none() -> Self {
        Self {
            wait_ms: Metric::Known { value: 0 },
            waited: 0,
        }
    }
}

/// Provisional work, counted from the ledger.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProvisionalWork {
    pub started: Metric<u64>,
    /// Told to rebase onto a fix after a red gate.
    pub rebased: Metric<u64>,
    /// Reopened after a red gate.
    pub reopened: Metric<u64>,
}

/// HS2-9FXASE: what the owner's work waited on outside the orchestrator.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExternalWait {
    pub ci: SystemWait,
    pub deployments: SystemWait,
    pub reviews: SystemWait,
    pub provider_limits: SystemWait,
    /// Session time kept with nothing to do but wait on a gate.
    pub idle_waiting_ms: Metric<u64>,
    pub provisional: ProvisionalWork,
}

impl Default for ExternalWait {
    fn default() -> Self {
        let zero = || Metric::Known { value: 0 };
        Self {
            ci: SystemWait::none(),
            deployments: SystemWait::none(),
            reviews: SystemWait::none(),
            provider_limits: SystemWait::none(),
            idle_waiting_ms: zero(),
            provisional: ProvisionalWork {
                started: zero(),
                rebased: zero(),
                reopened: zero(),
            },
        }
    }
}

/// The intervals one gate spent in `state`, or `None` when its bounded history no
/// longer shows where it began.
fn intervals(gate: &Gate, state: GateState, now_ms: u64) -> Option<Vec<(u64, u64)>> {
    if gate
        .history
        .first()
        .is_some_and(|first| first.from.is_some())
    {
        return None;
    }
    let mut spans = Vec::new();
    let mut open: Option<u64> = None;
    for transition in &gate.history {
        match (open, transition.to == state) {
            (None, true) => open = Some(transition.at_ms),
            (Some(since), false) => {
                spans.push((since, transition.at_ms.max(since)));
                open = None;
            }
            _ => {}
        }
    }
    if let Some(since) = open {
        spans.push((since, now_ms.max(since)));
    }
    Some(spans)
}

/// The length of the union of intervals.
fn union_ms(mut spans: Vec<(u64, u64)>) -> u64 {
    spans.sort_unstable();
    let mut total = 0;
    let mut current: Option<(u64, u64)> = None;
    for (start, end) in spans {
        current = match current {
            Some((from, to)) if start <= to => Some((from, to.max(end))),
            Some((from, to)) => {
                total += to - from;
                Some((start, end))
            }
            None => Some((start, end)),
        };
    }
    total + current.map_or(0, |(from, to)| to - from)
}

/// Wait in `state` over the gates of `kinds`, joined per run. `none` is the reason
/// when no gate of those kinds stands.
fn gate_wait(
    input: &ExternalWaitInput<'_>,
    kinds: &[GateKind],
    state: GateState,
    none: UnknownReason,
) -> SystemWait {
    let gates: Vec<&Gate> = input
        .gates
        .iter()
        .filter(|gate| {
            gate.owner_checkout_id == input.owner_checkout_id && kinds.contains(&gate.kind)
        })
        .collect();
    if gates.is_empty() {
        let reason = none;
        return SystemWait {
            wait_ms: Metric::Unknown { reason },
            waited: 0,
        };
    }
    let mut per_run: BTreeMap<&str, Vec<(u64, u64)>> = BTreeMap::new();
    let mut truncated = false;
    for gate in gates {
        match intervals(gate, state, input.now_ms) {
            Some(spans) => per_run.entry(&gate.run_id).or_default().extend(spans),
            None => truncated = true,
        }
    }
    let waits: Vec<u64> = per_run.into_values().map(union_ms).collect();
    SystemWait {
        wait_ms: if truncated {
            Metric::Unknown {
                reason: UnknownReason::HistoryTruncated,
            }
        } else {
            Metric::Known {
                value: waits.iter().sum(),
            }
        },
        waited: waits.iter().filter(|wait| **wait > 0).count() as u64,
    }
}

/// HS2-9FXASE: external-wait measures.
pub fn summarize_external_wait(input: &ExternalWaitInput<'_>) -> ExternalWait {
    let decisions: Vec<&DecisionRecord> = input.decisions.iter().collect();
    let journal = |value| {
        if input.decisions_trimmed {
            Metric::Unknown {
                reason: UnknownReason::DecisionsTrimmed,
            }
        } else {
            Metric::Known { value }
        }
    };
    let steps = by_subject(&decisions, &|decision| {
        matches!(
            decision.kind,
            DecisionKind::Start | DecisionKind::Keep | DecisionKind::Unplaced
        )
        .then(|| decision.step_id.clone())
        .flatten()
    });
    let provider_limited = |decision: &DecisionRecord| {
        decision.kind == DecisionKind::Unplaced
            && decision.reason.as_deref() == Some("no_tool_available")
            && decision.detail.as_deref() == Some("limited")
    };
    let mut limited_ms = 0;
    let mut limited_steps = BTreeSet::new();
    for (step, list) in &steps {
        if list.iter().any(|decision| provider_limited(decision)) {
            limited_steps.insert(step);
            limited_ms += episodes(list, &provider_limited, input.now_ms);
        }
    }
    let sessions = by_subject(&decisions, &|decision| {
        matches!(decision.kind, DecisionKind::Keep | DecisionKind::WindDown)
            .then(|| decision.session_id.clone())
            .flatten()
    });
    let idle_waiting: u64 = sessions
        .values()
        .map(|list| {
            episodes(
                list,
                &|decision| {
                    decision.kind == DecisionKind::Keep
                        && decision.reason.as_deref() == Some("waiting_on_gate")
                },
                input.now_ms,
            )
        })
        .sum();
    let tally = input.provisional;
    let ledger = |value| {
        if tally.trimmed {
            Metric::Unknown {
                reason: UnknownReason::HistoryTruncated,
            }
        } else {
            Metric::Known { value }
        }
    };
    ExternalWait {
        ci: gate_wait(
            input,
            &[GateKind::Ci],
            GateState::Pending,
            UnknownReason::NoGateLinked,
        ),
        deployments: gate_wait(
            input,
            &[GateKind::Deployment],
            GateState::Pending,
            UnknownReason::NoDeploymentRequested,
        ),
        // HS2-GD3NNK: requested code reviews, alongside deployments that wait for a
        // person's approval.
        reviews: gate_wait(
            input,
            &[GateKind::Deployment, GateKind::Review],
            GateState::AwaitingApproval,
            UnknownReason::NoReviewRequested,
        ),
        provider_limits: SystemWait {
            wait_ms: journal(limited_ms),
            waited: limited_steps.len() as u64,
        },
        idle_waiting_ms: journal(idle_waiting),
        provisional: ProvisionalWork {
            started: ledger(tally.started),
            rebased: ledger(tally.rebased),
            reopened: ledger(tally.reopened),
        },
    }
}
