//! HS2-ESVYQH: what a pool's state says about handoffs its integrator returned, as the
//! hosted session listing reports it. Synthetic assignments only.
use serde_json::json;

use crate::coordination::{Assignment, Authority, Session, State};
use crate::orchestration::review_returns::{ReturnedHandoff, assigner_live, returned_handoffs};

fn assignment(id: &str, ticket: &str, phase: &str, returned: Option<(&str, &str)>) -> Assignment {
    let mut value = json!({
        "id": id, "worker": "worker-1", "generation": "g1",
        "ticket": {"connection_id": "connection-1", "native_id": ticket},
        "slug": "TGT-1", "base": "0".repeat(40), "phase": phase, "commit": "b".repeat(40),
        "handoff": {"kind": "code", "commit": "b".repeat(40)},
        "return_route": {"kind": "human_relay"}
    });
    if let Some((at, note)) = returned {
        value["phase"] = json!("blocked");
        value["recovery_note"] = json!(note);
        if !at.is_empty() {
            value["transitions"] = json!([
                {"kind": "handoff", "at": "2026-01-01T00:00:00Z"},
                {"kind": "review_return", "at": at}
            ]);
        }
    }
    serde_json::from_value(value).unwrap()
}

fn state(assignments: Vec<Assignment>) -> State {
    let mut state = State::new("project".into(), "source".into());
    for assignment in assignments {
        state.assignments.insert(assignment.id.clone(), assignment);
    }
    state
}

fn session(id: &str, expires_at: i64) -> Session {
    serde_json::from_value(
        json!({"id": id, "generation": "g1", "expires_at": expires_at,
        "return_route": {"kind": "human_relay"}}),
    )
    .unwrap()
}

#[test]
fn hs2_esvyqh_the_latest_return_of_a_ticket_with_no_correction_in_flight_is_reported() {
    // Nothing returned: nothing reported, whatever else is in flight.
    assert!(returned_handoffs(&state(vec![assignment("a", "T1", "handoff", None)])).is_empty());
    // Two rounds on one ticket, recorded out of order: the later return is reported.
    let rounds = state(vec![
        assignment(
            "b",
            "T1",
            "handoff",
            Some(("2026-01-01T00:02:00Z", "NOTE2")),
        ),
        assignment(
            "a",
            "T1",
            "handoff",
            Some(("2026-01-01T00:01:00Z", "NOTE1")),
        ),
    ]);
    assert_eq!(
        returned_handoffs(&rounds),
        [ReturnedHandoff {
            connection_id: "connection-1".into(),
            native_id: "T1".into(),
            returned_at_ms: 1_767_225_720_000,
            review_note: "NOTE2".into(),
        }]
    );
    // A correction offered, accepted, handed off or blocked for another reason is in
    // flight: the ticket is left out. A Done one is not.
    for phase in [
        "offered",
        "accepted",
        "handoff",
        "integrating",
        "ci",
        "blocked",
    ] {
        let mut busy = rounds.clone();
        let correction = assignment("c", "T1", phase, None);
        busy.assignments.insert(correction.id.clone(), correction);
        assert!(returned_handoffs(&busy).is_empty(), "{phase}");
    }
    let mut done = rounds.clone();
    let finished = assignment("c", "T1", "done", None);
    done.assignments.insert(finished.id.clone(), finished);
    assert_eq!(returned_handoffs(&done).len(), 1);
    // Another ticket's work does not hide this one's return; an old record without a
    // timeline reads as returned long ago.
    let mixed = state(vec![
        assignment("a", "T1", "handoff", Some(("", "NOTE1"))),
        assignment("x", "T2", "accepted", None),
    ]);
    let found = returned_handoffs(&mixed);
    assert_eq!(found.len(), 1);
    assert_eq!(
        (found[0].native_id.as_str(), found[0].returned_at_ms),
        ("T1", 0)
    );
}

#[test]
fn hs2_esvyqh_only_the_role_that_assigns_under_the_pools_authority_counts() {
    let mut pool = state(Vec::new());
    assert!(!assigner_live(&pool, 100));
    // A legacy pool: its integrator assigns.
    pool.integrator = Some(session("integrator-1", 200));
    pool.dispatcher = Some(session("dispatcher-1", 200));
    assert!(assigner_live(&pool, 100));
    assert!(!assigner_live(&pool, 200), "a lease ending now is over");
    // Dispatcher authority: only the dispatcher does.
    pool.authority = Authority::Dispatcher;
    assert!(assigner_live(&pool, 100));
    pool.dispatcher = Some(session("dispatcher-1", 50));
    assert!(
        !assigner_live(&pool, 100),
        "the integrator alone assigns nothing"
    );
}
