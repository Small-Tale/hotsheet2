//! HS2-XK7TJS: utilization measures as tables, then repeated, out-of-order,
//! interleaved, trimmed and empty-then-refill inputs. Synthetic ids only.

use std::collections::BTreeMap;

use serde_json::json;

use super::*;

const OWNER: &str = "owner-0123456789ab";

/// A session of `tool` on `server`, with `(at, to)` state transitions.
fn session(
    name: &str,
    tool: &str,
    server: &str,
    state: &str,
    steps: &[(u64, &str)],
) -> OrchestratedSession {
    let mut from: Option<&str> = None;
    let history: Vec<_> = steps
        .iter()
        .map(|(at, to)| {
            let entry = json!({"at_ms": at, "from": from, "to": to});
            from = Some(to);
            entry
        })
        .collect();
    serde_json::from_value(json!({
        "session_id": name,
        "owner_checkout_id": OWNER,
        "operation_id": format!("op-{name}"),
        "target_id": "01ARZ3NDEKTSV4RRFFQ69G5FAV",
        "tool": tool,
        "state": state,
        "hosted": {"server_id": server, "session_id": format!("term-{name}"), "worker_id": "w"},
        "created_at_ms": steps.first().map_or(0, |step| step.0),
        "updated_at_ms": steps.last().map_or(0, |step| step.0),
        "history": history,
    }))
    .unwrap()
}

fn decision(at_ms: u64, kind: DecisionKind, subject: &str, reason: &str) -> DecisionRecord {
    let session = matches!(kind, DecisionKind::Keep | DecisionKind::WindDown);
    serde_json::from_value(json!({
        "at_ms": at_ms,
        "tick_id": format!("tick-{at_ms}"),
        "mode": "applied",
        "trigger": "event",
        "kind": kind,
        "session_id": session.then_some(subject),
        "step_id": (!session).then_some(subject),
        "reason": reason,
        "probe": false,
    }))
    .unwrap()
}

/// A free session given step `step`'s work: a keep decision naming both.
fn refill(at_ms: u64, step: &str) -> DecisionRecord {
    DecisionRecord {
        session_id: Some("session-free".into()),
        step_id: Some(step.into()),
        ..decision(at_ms, DecisionKind::Keep, "session-free", "refill")
    }
}

struct World {
    sessions: Vec<OrchestratedSession>,
    decisions: Vec<DecisionRecord>,
    trimmed: bool,
    tool_caps: BTreeMap<String, u32>,
    machine_caps: BTreeMap<String, u32>,
}
impl World {
    fn new() -> Self {
        Self {
            sessions: Vec::new(),
            decisions: Vec::new(),
            trimmed: false,
            tool_caps: [("claude".to_owned(), 2), ("codex".to_owned(), 4)].into(),
            machine_caps: [("server-a".to_owned(), 2)].into(),
        }
    }
    fn at(&self, now_ms: u64) -> Utilization {
        summarize_utilization(&UtilizationInput {
            owner_checkout_id: OWNER,
            now_ms,
            sessions: &self.sessions,
            decisions: &self.decisions,
            decisions_trimmed: self.trimmed,
            tool_caps: &self.tool_caps,
            machine_caps: &self.machine_caps,
        })
    }
}

fn known(value: u64) -> Metric<u64> {
    Metric::Known { value }
}
fn unknown(reason: UnknownReason) -> Metric<u64> {
    Metric::Unknown { reason }
}

#[test]
fn hs2_xk7tjs_nothing_recorded_is_known_zero_and_lists_nothing() {
    let summary = World::new().at(1_000);
    assert_eq!(summary, Utilization::default());
    assert_eq!(summary.idle_ms, known(0));
    assert_eq!(summary.limited.tool_capacity_ms, known(0));
    assert_eq!(summary.from_ms, None);
}

#[test]
fn hs2_xk7tjs_running_time_peaks_and_caps_per_tool_and_machine() {
    let mut world = World::new();
    world.sessions = vec![
        // Ran 100..300 and stopped.
        session(
            "a",
            "claude",
            "server-a",
            "stopped",
            &[(90, "requested"), (100, "running"), (300, "stopped")],
        ),
        // Runs 200..now.
        session(
            "b",
            "claude",
            "server-a",
            "running",
            &[(150, "requested"), (200, "running")],
        ),
        // Codex on another machine, 250..350, lost.
        session(
            "c",
            "codex",
            "server-b",
            "lost",
            &[(240, "requested"), (250, "running"), (350, "lost")],
        ),
        // Refused before it ran: no capacity used.
        session(
            "d",
            "codex",
            "server-b",
            "failed",
            &[(260, "requested"), (261, "failed")],
        ),
    ];
    let summary = world.at(500);
    assert_eq!(summary.from_ms, Some(100));
    let tool = |name: &str| {
        summary
            .tools
            .iter()
            .find(|use_| use_.key == name)
            .unwrap()
            .clone()
    };
    let claude = tool("claude");
    assert_eq!(
        (
            claude.sessions,
            claude.session_ms,
            claude.peak_sessions,
            claude.cap
        ),
        (2, known(200 + 300), 2, Some(2))
    );
    // 500 of 2 sessions x 400 ms.
    assert_eq!(claude.utilization_permille, known(625));
    let codex = tool("codex");
    assert_eq!(
        (codex.sessions, codex.session_ms, codex.peak_sessions),
        // The refused start used no capacity and is not counted.
        (1, known(100), 1)
    );
    assert_eq!(codex.utilization_permille, known(62));
    // Machines: server-b has no cap configured.
    let machine = |name: &str| {
        summary
            .machines
            .iter()
            .find(|use_| use_.key == name)
            .unwrap()
            .clone()
    };
    assert_eq!(machine("server-a").peak_sessions, 2);
    assert_eq!(machine("server-a").utilization_permille, known(625));
    assert_eq!(
        machine("server-b").utilization_permille,
        unknown(UnknownReason::CapNotConfigured)
    );
    // An end and a start at the same instant are not concurrent.
    world.sessions = vec![
        session(
            "a",
            "claude",
            "server-a",
            "stopped",
            &[(0, "requested"), (100, "running"), (200, "stopped")],
        ),
        session(
            "b",
            "claude",
            "server-a",
            "stopped",
            &[(150, "requested"), (200, "running"), (300, "stopped")],
        ),
    ];
    assert_eq!(world.at(300).tools[0].peak_sessions, 1);
}

#[test]
fn hs2_xk7tjs_a_session_counts_once_and_a_lost_start_is_unknown() {
    let mut world = World::new();
    let first = session(
        "a",
        "claude",
        "server-a",
        "running",
        &[(0, "requested"), (100, "running")],
    );
    // The same host terminal known through a second, later record.
    let mut again = first.clone();
    again.session_id = "a-again".into();
    again.updated_at_ms = 150;
    world.sessions = vec![first, again];
    let once = world.at(300);
    assert_eq!(
        (once.tools[0].sessions, once.tools[0].session_ms),
        (1, known(200))
    );
    // A running session whose bounded history no longer shows when it started.
    let mut truncated = session("t", "claude", "server-a", "running", &[(0, "requested")]);
    truncated.history.clear();
    world.sessions.push(truncated);
    let summary = world.at(300);
    assert_eq!(summary.tools[0].sessions, 2);
    assert_eq!(
        summary.tools[0].session_ms,
        unknown(UnknownReason::HistoryTruncated)
    );
    assert_eq!(
        summary.tools[0].utilization_permille,
        unknown(UnknownReason::HistoryTruncated)
    );
    // Other owners' sessions are not counted.
    let mut other = session("x", "claude", "server-a", "running", &[(0, "running")]);
    other.owner_checkout_id = "owner-ba9876543210".into();
    world.sessions = vec![other];
    assert!(world.at(300).tools.is_empty());
}

#[test]
fn hs2_xk7tjs_idle_episodes_follow_the_decision_journal() {
    use DecisionKind::{Keep, WindDown};
    let mut world = World::new();
    let cases: Vec<(&str, Vec<DecisionRecord>, u64)> = vec![
        ("never idle", vec![decision(10, Keep, "s", "refill")], 0),
        (
            "idle then refilled",
            vec![
                decision(10, Keep, "s", "idle_grace"),
                decision(40, Keep, "s", "refill"),
            ],
            30,
        ),
        (
            "idle then wound down",
            vec![
                decision(10, Keep, "s", "keep_warm"),
                decision(70, WindDown, "s", "idle"),
            ],
            60,
        ),
        (
            "still idle",
            vec![decision(10, Keep, "s", "idle_grace")],
            90,
        ),
        (
            "idle twice, a repeat in between counts once",
            vec![
                decision(10, Keep, "s", "idle_grace"),
                decision(20, Keep, "s", "keep_warm"),
                decision(30, Keep, "s", "busy"),
                decision(50, Keep, "s", "idle_grace"),
                decision(60, Keep, "s", "refill"),
            ],
            30,
        ),
        (
            "two sessions interleaved",
            vec![
                decision(10, Keep, "s", "idle_grace"),
                decision(15, Keep, "t", "idle_grace"),
                decision(20, Keep, "s", "refill"),
                decision(45, Keep, "t", "refill"),
            ],
            40,
        ),
    ];
    for (name, decisions, idle) in cases {
        world.decisions = decisions.clone();
        assert_eq!(world.at(100).idle_ms, known(idle), "{name}");
        // Out of order: the same records in reverse give the same answer.
        world.decisions = decisions.into_iter().rev().collect();
        assert_eq!(world.at(100).idle_ms, known(idle), "{name} reversed");
    }
    // A journal that lost entries cannot say.
    world.trimmed = true;
    assert_eq!(
        world.at(100).idle_ms,
        unknown(UnknownReason::DecisionsTrimmed)
    );
    // Empty, then refilled.
    world.trimmed = false;
    world.decisions.clear();
    assert_eq!(world.at(100).idle_ms, known(0));
    world.decisions = vec![decision(90, Keep, "s", "idle_grace")];
    assert_eq!(world.at(100).idle_ms, known(10));
}

#[test]
fn hs2_xk7tjs_time_limited_by_tool_capacity_and_resources() {
    use DecisionKind::{Start, Unplaced};
    let mut world = World::new();
    world.decisions = vec![
        // Step a: no tool from 10 to 40, then started.
        decision(10, Unplaced, "a", "no_tool_available"),
        decision(25, Unplaced, "a", "no_tool_available"),
        decision(40, Start, "a", "ready_work"),
        // Step b: a held resource from 20, still waiting at 100.
        decision(20, Unplaced, "b", "resource_held"),
        // Step c: a resource that could not be read 30..50, then a cap.
        decision(30, Unplaced, "c", "resource_unknown"),
        decision(50, Unplaced, "c", "target_cap"),
        // Step d: refilled after no tool 60..70.
        decision(60, Unplaced, "d", "no_tool_available"),
        refill(70, "d"),
    ];
    let limited = world.at(100).limited;
    assert_eq!(limited.tool_capacity_ms, known(30 + 10));
    assert_eq!(limited.steps_tool_limited, 2);
    assert_eq!(limited.resources_ms, known(80 + 20));
    assert_eq!(limited.steps_resource_limited, 2);
    // Repeated input changes nothing.
    let doubled: Vec<_> = world
        .decisions
        .iter()
        .chain(world.decisions.iter())
        .cloned()
        .collect();
    world.decisions = doubled;
    assert_eq!(world.at(100).limited, limited);
    world.trimmed = true;
    let trimmed = world.at(100).limited;
    assert_eq!(
        trimmed.tool_capacity_ms,
        unknown(UnknownReason::DecisionsTrimmed)
    );
    assert_eq!(
        trimmed.resources_ms,
        unknown(UnknownReason::DecisionsTrimmed)
    );
}

#[test]
fn hs2_xk7tjs_reasons_have_stable_codes_and_the_wire_carries_numbers_only() {
    assert_eq!(UnknownReason::DecisionsTrimmed.code(), "decisions_trimmed");
    assert_eq!(UnknownReason::CapNotConfigured.code(), "cap_not_configured");
    let mut world = World::new();
    world.sessions = vec![session(
        "a",
        "claude",
        "server-a",
        "running",
        &[(0, "requested"), (10, "running")],
    )];
    let wire = serde_json::to_value(world.at(20)).unwrap();
    assert_eq!(
        wire["tools"][0]["session_ms"],
        json!({"status":"known","value":10})
    );
    assert_eq!(wire["machines"][0]["key"], "server-a");
    assert!(wire.get("from_ms").is_some());
}
