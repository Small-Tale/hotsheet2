//! HS2-KXKA7X: the pure parts of triggers: keys and operation ids, the delivery
//! decision, coalescing history, state transitions, the review digest, validation,
//! the machine-local file (format compatibility) and the shared schedule engine's
//! wake instants. Synthetic data only.
use std::collections::BTreeMap;

use serde_json::json;

use super::*;
use crate::command_schedules::zoned::Zoned;
use crate::command_schedules::{Recurrence, RecurrenceKind};

fn every_five() -> Recurrence {
    Recurrence {
        kind: RecurrenceKind::Minutely,
        date: None,
        start_date: Some("2026-11-01".into()),
        every: Some(5),
        weekdays: Vec::new(),
        on: None,
        month: None,
        dates: Vec::new(),
        end_date: None,
        count: None,
        include_dates: Vec::new(),
        exclude_dates: Vec::new(),
    }
}

fn daily() -> Recurrence {
    Recurrence {
        kind: RecurrenceKind::Daily,
        every: Some(1),
        start_date: Some("2026-01-01".into()),
        ..every_five()
    }
}

fn schedule_spec() -> TriggerSpec {
    serde_json::from_value(json!({
        "name": "Dependency bump",
        "source": {"kind": "schedule", "schedule": {
            "time_zone": "UTC", "local_time": "02:00",
            "recurrence": {"kind": "hourly", "start_date": "2026-11-01", "every": 4}}},
        "action": {"template": {"template_id": "01M8BG8PA9CXEQY8GRMTB36X92",
            "tool": "claude", "event_inputs": {"when": "occurrence"}}},
    }))
    .unwrap()
}

fn trigger(state: TriggerState, overlap: Overlap) -> Trigger {
    let spec = schedule_spec();
    Trigger {
        trigger_id: "01M8BG8PA9CXEQY8GRMTB36X92".into(),
        name: spec.name,
        source: spec.source,
        action: spec.action,
        overlap,
        state,
        paused_from: None,
        review: None,
        pending_on: None,
        created_at_ms: 1,
        updated_at_ms: 1,
        updated_by: None,
        cursor: TriggerCursor::default(),
        active: None,
        queued: None,
        history: Vec::new(),
        handled: Vec::new(),
        warning: None,
        review_notified: None,
    }
}

fn delivery(key: &str) -> Delivery {
    Delivery {
        key: key.into(),
        kind: DeliveryKind::Schedule,
        at_ms: 10,
        evidence: BTreeMap::new(),
        missed: 0,
    }
}

fn entry(key: &str, outcome: DeliveryOutcome, at_ms: u64) -> TriggerHistoryEntry {
    TriggerHistoryEntry {
        delivery_key: key.into(),
        kind: DeliveryKind::Schedule,
        at_ms,
        outcome,
        count: 1,
        evidence: BTreeMap::new(),
        missed: 0,
        operation_id: None,
        goal_slug: None,
        placement: None,
        reason: None,
        would: None,
    }
}

#[test]
fn hs2_kxka7x_keys_name_the_fact_and_hash_into_a_bounded_operation_id() {
    assert_eq!(
        trigger_keys::schedule("2026-11-26T02:00"),
        "schedule:2026-11-26T02:00"
    );
    assert_eq!(trigger_keys::ticket("conn", "01ABC"), "ticket:conn:01ABC");
    assert_eq!(trigger_keys::gate("run", "gate", 7), "gate:run:gate:7");
    assert_eq!(
        trigger_keys::ci("github-actions", "9182", 2),
        "ci:github-actions:9182:2"
    );
    assert_eq!(
        trigger_keys::commit("main", "3f9c2a1"),
        "commit:main:3f9c2a1"
    );
    let trigger_id = "01M8BG8PA9CXEQY8GRMTB36X92";
    let first = delivery_operation_id(trigger_id, "schedule:2026-11-26T02:00");
    // Deterministic: a redelivery is the same template apply.
    assert_eq!(
        first,
        delivery_operation_id(trigger_id, "schedule:2026-11-26T02:00")
    );
    assert_ne!(
        first,
        delivery_operation_id(trigger_id, "schedule:2026-11-26T06:00")
    );
    assert_ne!(
        first,
        delivery_operation_id("01M8BG8PA9CXEQY8GRMTB36X93", "schedule:2026-11-26T02:00")
    );
    assert!(first.starts_with("trg-01M8BG8PA9CXEQY8GRMTB36X92-"));
    assert!(first.len() <= MAX_APPLY_OPERATION_ID);
    // Leaves room for the apply's derived ids and is an ordinary identifier.
    assert!(super::contract::valid_identifier(&format!("{first}.p9")).is_ok());
}

#[test]
fn hs2_kxka7x_the_delivery_decision_matrix() {
    use DeliveryDecision as D;
    for state in TriggerState::ALL {
        for overlap in [Overlap::Skip, Overlap::QueueLatest] {
            for running in [false, true] {
                for handled in [false, true] {
                    let mut t = trigger(state, overlap);
                    if handled {
                        t.handled.push("schedule:a".into());
                    }
                    let decided = decide_delivery(&t, &delivery("schedule:a"), running);
                    let expected = match (state, handled, running, overlap) {
                        (TriggerState::Paused | TriggerState::NeedsReview, ..) => {
                            D::Ignore(TriggerError::NotFiring)
                        }
                        (_, true, ..) => D::AlreadyHandled,
                        (TriggerState::DryRun, ..) => D::DryRun,
                        (_, _, true, Overlap::Skip) => D::SkipOverlap,
                        (_, _, true, Overlap::QueueLatest) => D::QueueLatest,
                        _ => D::Apply,
                    };
                    assert_eq!(
                        decided, expected,
                        "{state:?} {overlap:?} {running} {handled}"
                    );
                }
            }
        }
    }
    // A delivery that is the active run, or the queued one, is handled too.
    let mut t = trigger(TriggerState::On, Overlap::Skip);
    t.active = Some(ActiveRun {
        delivery_key: "schedule:a".into(),
        operation_id: "trg-x".into(),
        at_ms: 1,
    });
    t.queued = Some(delivery("schedule:b"));
    assert_eq!(
        decide_delivery(&t, &delivery("schedule:a"), true),
        D::AlreadyHandled
    );
    assert_eq!(
        decide_delivery(&t, &delivery("schedule:b"), true),
        D::AlreadyHandled
    );
    // A placement-only trigger never overlaps: it has no run.
    t.action.template = None;
    t.action.placement = Some(TriggerPlacement::Start);
    assert_eq!(decide_delivery(&t, &delivery("schedule:c"), true), D::Apply);
}

#[test]
fn hs2_kxka7x_history_coalesces_skips_and_redeliveries_and_stays_bounded() {
    let mut t = trigger(TriggerState::On, Overlap::Skip);
    t.record(entry("schedule:1", DeliveryOutcome::Applied, 1));
    t.record(entry("schedule:2", DeliveryOutcome::SkippedOverlap, 2));
    t.record(entry("schedule:3", DeliveryOutcome::SkippedOverlap, 3));
    // "Skipped 2 runs": one entry, the latest key and time.
    assert_eq!(t.history.len(), 2);
    assert_eq!(t.history[1].count, 2);
    assert_eq!(t.history[1].delivery_key, "schedule:3");
    assert_eq!(t.history[1].at_ms, 3);
    // Interleaved: a redelivery between skips breaks the run of skips.
    t.record(entry("schedule:1", DeliveryOutcome::AlreadyHandled, 4));
    t.record(entry("schedule:1", DeliveryOutcome::AlreadyHandled, 5));
    t.record(entry("schedule:2", DeliveryOutcome::AlreadyHandled, 6));
    t.record(entry("schedule:4", DeliveryOutcome::SkippedOverlap, 7));
    let outcomes: Vec<_> = t
        .history
        .iter()
        .map(|entry| (entry.delivery_key.as_str(), entry.outcome, entry.count))
        .collect();
    assert_eq!(
        outcomes,
        [
            ("schedule:1", DeliveryOutcome::Applied, 1),
            ("schedule:3", DeliveryOutcome::SkippedOverlap, 2),
            ("schedule:1", DeliveryOutcome::AlreadyHandled, 2),
            ("schedule:2", DeliveryOutcome::AlreadyHandled, 1),
            ("schedule:4", DeliveryOutcome::SkippedOverlap, 1),
        ]
    );
    // Every key is handled once, in order.
    assert_eq!(
        t.handled,
        ["schedule:1", "schedule:2", "schedule:3", "schedule:4"]
    );
    // Bounded: the oldest entries and keys go first.
    for n in 0..(MAX_HANDLED_KEYS + 10) {
        t.record(entry(
            &format!("k{n}"),
            DeliveryOutcome::Applied,
            100 + n as u64,
        ));
    }
    assert_eq!(t.history.len(), MAX_TRIGGER_HISTORY);
    assert_eq!(t.handled.len(), MAX_HANDLED_KEYS);
    assert_eq!(
        t.handled.last().unwrap(),
        &format!("k{}", MAX_HANDLED_KEYS + 9)
    );
    assert!(!t.handled("schedule:1"));
}

#[test]
fn hs2_kxka7x_queue_latest_keeps_one_and_the_replaced_one_is_handled() {
    let mut t = trigger(TriggerState::On, Overlap::QueueLatest);
    t.queue_latest(delivery("schedule:1"));
    t.record(entry("schedule:1", DeliveryOutcome::Queued, 1));
    // Queued is not handled yet: it is taken once the run ends.
    assert!(t.handled.is_empty());
    assert!(
        t.handled("schedule:1"),
        "the queued key is not delivered twice meanwhile"
    );
    t.queue_latest(delivery("schedule:2"));
    t.record(entry("schedule:2", DeliveryOutcome::Queued, 2));
    assert_eq!(t.queued.as_ref().unwrap().key, "schedule:2");
    assert_eq!(t.handled, ["schedule:1"]);
    // The same one again keeps it.
    t.queue_latest(delivery("schedule:2"));
    assert_eq!(t.handled, ["schedule:1"]);
    // Taken: no longer queued, and its delivery is decided anew.
    t.queued = None;
    assert_eq!(
        decide_delivery(&t, &delivery("schedule:2"), false),
        DeliveryDecision::Apply
    );
}

#[test]
fn hs2_kxka7x_state_transitions_resume_and_review() {
    use TriggerState as S;
    let cases = [
        // (state, paused_from, op, reviewed) -> next
        (S::DryRun, None, TriggerOp::On, true, S::On),
        (S::NeedsReview, None, TriggerOp::On, true, S::On),
        (S::On, None, TriggerOp::On, true, S::On),
        (S::On, None, TriggerOp::DryRun, true, S::DryRun),
        (S::On, None, TriggerOp::Pause, true, S::Paused),
        (S::Paused, None, TriggerOp::Pause, true, S::Paused),
        (S::Paused, Some(S::On), TriggerOp::Resume, true, S::On),
        (
            S::Paused,
            Some(S::On),
            TriggerOp::Resume,
            false,
            S::NeedsReview,
        ),
        (
            S::Paused,
            Some(S::NeedsReview),
            TriggerOp::Resume,
            true,
            S::On,
        ),
        (
            S::Paused,
            Some(S::DryRun),
            TriggerOp::Resume,
            false,
            S::DryRun,
        ),
        (S::Paused, None, TriggerOp::Resume, true, S::DryRun),
        // Resuming what is not paused changes nothing.
        (S::On, None, TriggerOp::Resume, true, S::On),
        (S::DryRun, None, TriggerOp::Resume, true, S::DryRun),
        // A template change sends only an On trigger to review.
        (
            S::On,
            None,
            TriggerOp::TemplateChanged,
            false,
            S::NeedsReview,
        ),
        (S::On, None, TriggerOp::TemplateChanged, true, S::On),
        (
            S::DryRun,
            None,
            TriggerOp::TemplateChanged,
            false,
            S::DryRun,
        ),
        (
            S::Paused,
            Some(S::On),
            TriggerOp::TemplateChanged,
            false,
            S::Paused,
        ),
        (S::On, None, TriggerOp::Edited, false, S::NeedsReview),
        (
            S::NeedsReview,
            None,
            TriggerOp::Edited,
            false,
            S::NeedsReview,
        ),
    ];
    for (state, from, op, reviewed, next) in cases {
        assert_eq!(
            trigger_transition(state, from, op, reviewed),
            next,
            "{state:?} {from:?} {op:?} {reviewed}"
        );
    }
}

#[test]
fn hs2_kxka7x_the_review_digest_covers_the_definition_and_the_template_version() {
    let mut t = trigger(TriggerState::On, Overlap::Skip);
    let digest = spec_digest(&t.spec(), Some(1));
    assert_eq!(digest, spec_digest(&t.spec(), Some(1)));
    assert_ne!(digest, spec_digest(&t.spec(), Some(2)));
    t.review = Some(TriggerReview {
        role: "human".into(),
        actor_id: "maintainer".into(),
        at_ms: 1,
        digest,
        template_version: Some(1),
        permission_requests: Vec::new(),
    });
    assert!(t.reviewed(Some(1)));
    assert!(!t.reviewed(Some(2)), "a template edit needs a review");
    t.overlap = Overlap::QueueLatest;
    assert!(!t.reviewed(Some(1)), "an edited definition needs a review");
}

#[test]
fn hs2_kxka7x_validation_names_each_problem() {
    assert!(validate_trigger(&schedule_spec()).is_empty());
    let spec: TriggerSpec = serde_json::from_value(json!({
        "name": " ",
        "source": {"kind": "branch_ci_failed", "target_id": "has space",
            "watch": {"repository": "o/n", "workflow": "ci yml", "branch": "main"}},
        "action": {"template": {"template_id": "t", "tool": "claude",
            "inputs": {"x": 1}, "event_inputs": {"x": "commit", "y": "occurrence"}}},
    }))
    .unwrap();
    let codes: Vec<(String, String)> = validate_trigger(&spec)
        .into_iter()
        .map(|problem| (problem.code, problem.at))
        .collect();
    assert_eq!(
        codes,
        [
            (
                "event_input_unavailable".into(),
                "action.template.event_inputs.y".into()
            ),
            (
                "input_twice".into(),
                "action.template.event_inputs.x".into()
            ),
            ("name_invalid".into(), "name".into()),
            ("target_invalid".into(), "source.target_id".into()),
            ("watch_invalid".into(), "source.watch.workflow".into()),
        ]
    );
    let empty: TriggerSpec = serde_json::from_value(json!({
        "name": "Nothing",
        "source": {"kind": "schedule", "schedule": {"time_zone": "Mars/Base",
            "local_time": "25:00", "recurrence": {"kind": "minutely",
            "start_date": "2026-11-01", "every": 1}}},
        "action": {},
    }))
    .unwrap();
    let codes: Vec<String> = validate_trigger(&empty)
        .into_iter()
        .map(|problem| problem.code)
        .collect();
    // The five-minute floor of the command-schedule engine applies.
    assert_eq!(
        codes,
        [
            "action_empty",
            "local_time_invalid",
            "recurrence_invalid",
            "time_zone_invalid"
        ]
    );
    // Unknown fields are refused rather than ignored.
    assert!(
        serde_json::from_value::<TriggerSpec>(json!({"name":"x","source":{"kind":"schedule"},
            "action":{},"surprise":true}))
        .is_err()
    );
}

#[test]
fn hs2_kxka7x_the_trigger_file_round_trips_and_refuses_newer_or_damaged_content() {
    let home = tempfile::tempdir().unwrap();
    let store = TriggerStore::new(home.path(), "owner-1").unwrap();
    // A read creates nothing.
    assert!(store.triggers().unwrap().is_empty());
    assert!(!TriggerStore::directory(home.path()).exists());
    let mut t = trigger(TriggerState::DryRun, Overlap::QueueLatest);
    t.record(entry("schedule:1", DeliveryOutcome::DryRun, 1));
    {
        let _lock = store.lock().unwrap();
        store.write(std::slice::from_ref(&t)).unwrap();
    }
    assert_eq!(store.triggers().unwrap(), [t.clone()]);
    assert_eq!(TriggerStore::owners(home.path()), ["owner-1"]);
    // A version 1 file as an earlier build wrote it, with only the fields it knew.
    let path = TriggerStore::directory(home.path()).join("owner-1.json");
    std::fs::write(
        &path,
        serde_json::to_vec(&json!({"schema_version": 1, "triggers": [{
            "trigger_id": "01M8BG8PA9CXEQY8GRMTB36X92", "name": "Repair API main CI",
            "source": {"kind": "branch_ci_failed", "target_id": "api",
                "watch": {"repository": "small/api", "workflow": "ci.yml", "branch": "main"}},
            "action": {"template": {"template_id": "01M8BG8PA9CXEQY8GRMTB36X92", "tool": "claude"}},
            "state": "dry_run", "created_at_ms": 1, "updated_at_ms": 2,
            "history": [{"delivery_key": "ci:github-actions:9182:1", "kind": "ci_failed",
                "at_ms": 3, "outcome": "dry_run"}]
        }]}))
        .unwrap(),
    )
    .unwrap();
    let read = store.triggers().unwrap();
    assert_eq!(read[0].overlap, Overlap::Skip);
    assert_eq!(read[0].history[0].count, 1);
    assert_eq!(read[0].cursor, TriggerCursor::default());
    // A newer schema is refused, never rewritten.
    std::fs::write(&path, br#"{"schema_version": 2, "triggers": []}"#).unwrap();
    assert!(matches!(
        store.triggers(),
        Err(OrchestrationError::UnsupportedVersion { found: 2 })
    ));
    std::fs::write(&path, b"{not json").unwrap();
    assert!(matches!(store.triggers(), Err(OrchestrationError::Corrupt)));
    // Bounded.
    let many: Vec<Trigger> = (0..=MAX_TRIGGERS).map(|_| t.clone()).collect();
    assert!(matches!(
        store.write(&many),
        Err(OrchestrationError::Capacity)
    ));
    // An owner id that is not an identifier is refused before any path is built.
    assert!(TriggerStore::new(home.path(), "../escape").is_err());
}

#[test]
fn hs2_kxka7x_update_writes_only_a_change() {
    let home = tempfile::tempdir().unwrap();
    let store = TriggerStore::new(home.path(), "owner-1").unwrap();
    let t = trigger(TriggerState::DryRun, Overlap::Skip);
    store.write(std::slice::from_ref(&t)).unwrap();
    let path = TriggerStore::directory(home.path()).join("owner-1.json");
    let before = std::fs::metadata(&path).unwrap().modified().unwrap();
    let (same, ()) = store.update(&t.trigger_id, |_| Ok(())).unwrap();
    assert_eq!(same, t);
    assert_eq!(
        std::fs::metadata(&path).unwrap().modified().unwrap(),
        before
    );
    let (changed, ()) = store
        .update(&t.trigger_id, |trigger| {
            trigger.state = TriggerState::Paused;
            Ok(())
        })
        .unwrap();
    assert_eq!(store.find(&t.trigger_id).unwrap().unwrap(), changed);
    assert!(matches!(
        store.update("01M8BG8PA9CXEQY8GRMTB36X99", |_| Ok(())),
        Err(OrchestrationError::Trigger(TriggerError::NotFound))
    ));
}

#[test]
fn hs2_kxka7x_arming_restarts_every_cursor_so_nothing_is_backfilled() {
    let mut t = trigger(TriggerState::Paused, Overlap::QueueLatest);
    t.cursor = TriggerCursor {
        armed_at_ms: 1,
        schedule_after_ms: Some(1),
        run_events: Some(4),
        branch_commit: Some("abc".into()),
    };
    t.queued = Some(delivery("schedule:x"));
    t.warning = Some(TriggerWarning {
        code: "target_unreachable".into(),
        since_ms: 1,
        notified: true,
    });
    t.arm(500, Some(9));
    assert_eq!(
        t.cursor,
        TriggerCursor {
            armed_at_ms: 500,
            schedule_after_ms: Some(500),
            run_events: Some(9),
            branch_commit: None,
        }
    );
    assert!(t.queued.is_none() && t.warning.is_none());
}

fn ts(text: &str) -> jiff::Timestamp {
    text.parse().unwrap()
}

#[test]
fn hs2_kxka7x_schedule_wake_is_the_next_slot_and_missed_slots_are_listed_once() {
    let recurrence = every_five();
    let zoned = Zoned {
        time_zone: "UTC",
        local_time: "00:00",
        recurrence: &recurrence,
    };
    // The next wake is the next slot, never an interval from now.
    let next = zoned.next(ts("2026-11-26T10:02:30Z"), 3).unwrap();
    let instants: Vec<String> = next.iter().map(|o| o.instant.to_string()).collect();
    assert_eq!(
        instants,
        [
            "2026-11-26T10:05:00Z",
            "2026-11-26T10:10:00Z",
            "2026-11-26T10:15:00Z"
        ]
    );
    assert_eq!(next[0].key, "2026-11-26T10:05");
    // A slot exactly at `through` is due; at `after` it is not.
    let due = zoned
        .between(ts("2026-11-26T10:00:00Z"), ts("2026-11-26T10:05:00Z"), 10)
        .unwrap();
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].key, "2026-11-26T10:05");
    // A machine that was off for an hour: every missed slot, oldest first, bounded.
    let missed = zoned
        .between(ts("2026-11-26T09:00:00Z"), ts("2026-11-26T10:00:00Z"), 100)
        .unwrap();
    assert_eq!(missed.len(), 12);
    assert_eq!(missed.last().unwrap().key, "2026-11-26T10:00");
    let latest = zoned
        .between(ts("2026-11-26T09:00:00Z"), ts("2026-11-26T10:00:00Z"), 1)
        .unwrap();
    assert_eq!(latest[0].key, "2026-11-26T10:00");
    // Nothing between two instants inside one slot.
    assert!(
        zoned
            .between(ts("2026-11-26T10:01:00Z"), ts("2026-11-26T10:04:59Z"), 5)
            .unwrap()
            .is_empty()
    );
    // Off for a week: clipped to the last two days.
    let week = zoned
        .between(
            ts("2026-11-19T10:00:00Z"),
            ts("2026-11-26T10:00:00Z"),
            10_000,
        )
        .unwrap();
    assert!(week.len() <= 2 * 24 * 12 + 1, "{}", week.len());
}

#[test]
fn hs2_kxka7x_daily_schedules_follow_the_dst_rules_of_command_schedules() {
    let recurrence = daily();
    // 02:30 does not exist on 2026-03-08 in New York: it runs at 03:00 that day.
    let zoned = Zoned {
        time_zone: "America/New_York",
        local_time: "02:30",
        recurrence: &recurrence,
    };
    let next = zoned.next(ts("2026-03-08T05:00:00Z"), 2).unwrap();
    assert_eq!(next[0].key, "2026-03-08");
    assert_eq!(next[0].instant.to_string(), "2026-03-08T07:00:00Z");
    assert_eq!(next[1].instant.to_string(), "2026-03-09T06:30:00Z");
    // 01:30 occurs twice on 2026-11-01: it runs once, at the first.
    let fold = Zoned {
        local_time: "01:30",
        ..zoned
    };
    let due = fold
        .between(ts("2026-11-01T04:00:00Z"), ts("2026-11-01T08:00:00Z"), 10)
        .unwrap();
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].instant.to_string(), "2026-11-01T05:30:00Z");
}
