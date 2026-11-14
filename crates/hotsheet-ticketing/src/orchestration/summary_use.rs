//! HS2-XK7TJS: throughput and utilization of an owning checkout's orchestration.
//!
//! A pure fold, like the rest of the summary (HS2-S13719): it reads the orchestrator's
//! session records, the scheduler's decision journal and the policy's caps, and writes
//! nothing. Every measure is a [`Metric`], so an unknown is never a zero.
//!
//! - **Running time and peaks** per tool and per machine come from each session's own
//!   state history: from its transition to `running` to its transition out, or the
//!   summary's time while it still runs. A session known through several records (the
//!   same host terminal) counts once.
//! - **Idle time** comes from the decision journal. A session's idle episode starts at a
//!   decision that keeps it idle (`idle_grace`, `keep_warm`) and ends at the next
//!   decision about it that is anything else, at its wind-down, at its end, or at the
//!   summary's time.
//! - **Time limited by capacity** comes from the decision journal too. A plan step is
//!   limited while its latest decision is unplaced for want of a tool account
//!   (`no_tool_available`) or a resource (`resource_held`, `resource_unknown`), until
//!   its next other decision or the summary's time.
//!
//! The journal logs a pass only when its decisions change, so episodes start and end
//! exactly at the logged change points. A journal whose retention removed entries
//! reports these measures as `decisions_trimmed`.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::{
    DecisionKind, DecisionRecord, Metric, OrchestratedSession, SessionState, UnknownReason,
};

/// Everything the utilization measures are computed from.
pub struct UtilizationInput<'a> {
    pub owner_checkout_id: &'a str,
    pub now_ms: u64,
    pub sessions: &'a [OrchestratedSession],
    /// The owner's scheduler decisions, any order.
    pub decisions: &'a [DecisionRecord],
    /// Retention removed decisions older than the ones given.
    pub decisions_trimmed: bool,
    /// The policy's session cap per tool.
    pub tool_caps: &'a BTreeMap<String, u32>,
    /// The session cap of each machine (server id) the sessions ran on.
    pub machine_caps: &'a BTreeMap<String, u32>,
}

/// How much of one tool's or machine's allowance was used.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapacityUse {
    /// The tool id, or the machine's server id (empty when a session never reached a
    /// host).
    pub key: String,
    /// Distinct sessions.
    pub sessions: u64,
    /// Running time summed over the sessions.
    pub session_ms: Metric<u64>,
    /// The most sessions running at once.
    pub peak_sessions: u64,
    /// The configured cap, when there is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cap: Option<u32>,
    /// Running time over the cap's allowance for the window, in thousandths.
    pub utilization_permille: Metric<u64>,
}

/// Time work could not start for want of capacity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LimitedTime {
    /// Unplaced because no tool account could take it.
    pub tool_capacity_ms: Metric<u64>,
    pub steps_tool_limited: u64,
    /// Unplaced because a resource it needs was held or could not be read.
    pub resources_ms: Metric<u64>,
    pub steps_resource_limited: u64,
}

/// Throughput and utilization of the owner's orchestration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Utilization {
    /// The first time a session ran; utilization is measured from it to `as_of_ms`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_ms: Option<u64>,
    /// Sorted by tool.
    pub tools: Vec<CapacityUse>,
    /// Sorted by server id.
    pub machines: Vec<CapacityUse>,
    /// Time sessions spent idle, kept by the scheduler for work that may come.
    pub idle_ms: Metric<u64>,
    pub limited: LimitedTime,
}

impl Default for Utilization {
    fn default() -> Self {
        let none = Metric::Known { value: 0 };
        Self {
            from_ms: None,
            tools: Vec::new(),
            machines: Vec::new(),
            idle_ms: none,
            limited: LimitedTime {
                tool_capacity_ms: none,
                steps_tool_limited: 0,
                resources_ms: none,
                steps_resource_limited: 0,
            },
        }
    }
}

/// A session's running span, `None` when its history no longer shows the start.
type Span = Option<(u64, u64)>;

/// One session's running span, or `None` when its history no longer shows the start.
fn running_span(session: &OrchestratedSession, now_ms: u64) -> Option<Option<(u64, u64)>> {
    let started = session
        .history
        .iter()
        .find(|entry| entry.to == SessionState::Running)
        .map(|entry| entry.at_ms);
    let ended = session
        .history
        .iter()
        .find(|entry| entry.from == Some(SessionState::Running))
        .map(|entry| entry.at_ms);
    match (started, session.state) {
        // Never ran: a refused or abandoned start uses no capacity.
        (None, SessionState::Requested | SessionState::Failed) if ended.is_none() => Some(None),
        (None, _) => None,
        (Some(start), SessionState::Running) => Some(Some((start, now_ms.max(start)))),
        (Some(start), _) => ended.map(|end| Some((start, end.max(start)))),
    }
}

fn add(metric: &mut Metric<u64>, value: Option<u64>, reason: UnknownReason) {
    *metric = match (*metric, value) {
        (Metric::Known { value: total }, Some(more)) => Metric::Known {
            value: total.saturating_add(more),
        },
        (Metric::Unknown { reason }, _) => Metric::Unknown { reason },
        (Metric::Known { .. }, None) => Metric::Unknown { reason },
    };
}

/// The most spans open at once.
fn peak(spans: &[(u64, u64)]) -> u64 {
    let mut edges: Vec<(u64, i64)> = Vec::new();
    for (start, end) in spans {
        if end > start {
            edges.push((*start, 1));
            edges.push((*end, -1));
        }
    }
    // An end and a start at the same instant do not overlap.
    edges.sort_unstable();
    let (mut open, mut most) = (0_i64, 0_i64);
    for (_, step) in edges {
        open += step;
        most = most.max(open);
    }
    u64::try_from(most).unwrap_or(0)
}

fn usage(
    key: String,
    spans: &[Option<(u64, u64)>],
    cap: Option<u32>,
    window: Option<(u64, u64)>,
) -> CapacityUse {
    let mut session_ms = Metric::Known { value: 0 };
    let mut known = Vec::new();
    for span in spans {
        add(
            &mut session_ms,
            span.map(|(start, end)| end - start),
            UnknownReason::HistoryTruncated,
        );
        if let Some(span) = span {
            known.push(*span);
        }
    }
    let utilization_permille = match (session_ms, cap, window) {
        (Metric::Unknown { reason }, _, _) => Metric::Unknown { reason },
        (_, None | Some(0), _) => Metric::Unknown {
            reason: UnknownReason::CapNotConfigured,
        },
        (Metric::Known { value }, Some(cap), Some((from, to))) if to > from => Metric::Known {
            value: u64::try_from(
                u128::from(value) * 1000 / (u128::from(cap) * u128::from(to - from)),
            )
            .unwrap_or(u64::MAX),
        },
        (Metric::Known { .. }, Some(_), _) => Metric::Known { value: 0 },
    };
    CapacityUse {
        key,
        sessions: spans.len() as u64,
        session_ms,
        peak_sessions: peak(&known),
        cap,
        utilization_permille,
    }
}

/// Episodes in one subject's decisions, oldest first: each opens at a decision `opens`
/// accepts while none is open and closes at the next one it does not, or at `now_ms`.
pub(super) fn episodes(
    decisions: &[&DecisionRecord],
    opens: &dyn Fn(&DecisionRecord) -> bool,
    now_ms: u64,
) -> u64 {
    let mut total = 0;
    let mut open: Option<u64> = None;
    for decision in decisions {
        match (open, opens(decision)) {
            (None, true) => open = Some(decision.at_ms),
            (Some(since), false) => {
                total += decision.at_ms.saturating_sub(since);
                open = None;
            }
            _ => {}
        }
    }
    if let Some(since) = open {
        total += now_ms.saturating_sub(since);
    }
    total
}

/// The decisions about each subject, by `key`, oldest first; a decision given twice
/// counts once.
pub(super) fn by_subject<'a>(
    decisions: &[&'a DecisionRecord],
    key: &dyn Fn(&DecisionRecord) -> Option<String>,
) -> BTreeMap<String, Vec<&'a DecisionRecord>> {
    let mut subjects: BTreeMap<String, Vec<&DecisionRecord>> = BTreeMap::new();
    for decision in decisions {
        if let Some(subject) = key(decision) {
            subjects.entry(subject).or_default().push(decision);
        }
    }
    for list in subjects.values_mut() {
        list.sort_by(|left, right| (left.at_ms, &left.tick_id).cmp(&(right.at_ms, &right.tick_id)));
        list.dedup_by(|later, earlier| {
            later.tick_id == earlier.tick_id && later.kind == earlier.kind
        });
    }
    subjects
}

/// HS2-XK7TJS: throughput and utilization measures.
pub fn summarize_utilization(input: &UtilizationInput<'_>) -> Utilization {
    // One entry per host terminal: a session known through several records counts once.
    let mut distinct: BTreeMap<(String, String), &OrchestratedSession> = BTreeMap::new();
    for session in input
        .sessions
        .iter()
        .filter(|session| session.owner_checkout_id == input.owner_checkout_id)
    {
        let key = match &session.hosted {
            Some(hosted) => (hosted.server_id.clone(), hosted.session_id.clone()),
            None => (String::new(), session.session_id.clone()),
        };
        distinct
            .entry(key)
            .and_modify(|known| {
                if (session.updated_at_ms, &session.session_id)
                    > (known.updated_at_ms, &known.session_id)
                {
                    *known = session;
                }
            })
            .or_insert(session);
    }
    let spans: Vec<(&str, String, Span)> = distinct
        .iter()
        .filter_map(|((server, _), session)| {
            running_span(session, input.now_ms).map_or(
                Some((session.tool.as_str(), server.clone(), None)),
                |span| span.map(|span| (session.tool.as_str(), server.clone(), Some(span))),
            )
        })
        .collect();
    let from_ms = spans
        .iter()
        .filter_map(|(_, _, span)| *span)
        .map(|span| span.0)
        .min();
    let window = from_ms.map(|from| (from, input.now_ms));
    let mut tools: BTreeMap<String, Vec<Span>> = BTreeMap::new();
    let mut machines: BTreeMap<String, Vec<Span>> = BTreeMap::new();
    for (tool, server, span) in &spans {
        tools.entry((*tool).to_owned()).or_default().push(*span);
        machines.entry(server.clone()).or_default().push(*span);
    }

    let decisions: Vec<&DecisionRecord> = input.decisions.iter().collect();
    let trimmed = || Metric::Unknown {
        reason: UnknownReason::DecisionsTrimmed,
    };
    let idle_ms = if input.decisions_trimmed {
        trimmed()
    } else {
        let sessions = by_subject(&decisions, &|decision| {
            matches!(decision.kind, DecisionKind::Keep | DecisionKind::WindDown)
                .then(|| decision.session_id.clone())
                .flatten()
        });
        Metric::Known {
            value: sessions
                .values()
                .map(|list| {
                    episodes(
                        list,
                        &|decision| {
                            decision.kind == DecisionKind::Keep
                                && matches!(
                                    decision.reason.as_deref(),
                                    Some("idle_grace" | "keep_warm")
                                )
                        },
                        input.now_ms,
                    )
                })
                .sum(),
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
    let limited_by = |codes: &[&str]| {
        let mut total = 0;
        let mut steps_limited = BTreeSet::new();
        for (step, list) in &steps {
            let opens = |decision: &DecisionRecord| {
                decision.kind == DecisionKind::Unplaced
                    && decision
                        .reason
                        .as_deref()
                        .is_some_and(|reason| codes.contains(&reason))
            };
            let time = episodes(list, &opens, input.now_ms);
            if list.iter().any(|decision| opens(decision)) {
                steps_limited.insert(step.clone());
            }
            total += time;
        }
        (total, steps_limited.len() as u64)
    };
    let (tool_ms, tool_steps) = limited_by(&["no_tool_available"]);
    let (resource_ms, resource_steps) = limited_by(&["resource_held", "resource_unknown"]);
    let known_or_trimmed = |value| {
        if input.decisions_trimmed {
            trimmed()
        } else {
            Metric::Known { value }
        }
    };
    Utilization {
        from_ms,
        tools: tools
            .into_iter()
            .map(|(tool, spans)| {
                let cap = input.tool_caps.get(&tool).copied();
                usage(tool, &spans, cap, window)
            })
            .collect(),
        machines: machines
            .into_iter()
            .map(|(server, spans)| {
                let cap = input.machine_caps.get(&server).copied();
                usage(server, &spans, cap, window)
            })
            .collect(),
        idle_ms,
        limited: LimitedTime {
            tool_capacity_ms: known_or_trimmed(tool_ms),
            steps_tool_limited: tool_steps,
            resources_ms: known_or_trimmed(resource_ms),
            steps_resource_limited: resource_steps,
        },
    }
}
