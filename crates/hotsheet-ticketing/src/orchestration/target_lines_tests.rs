//! HS2-S3FKJ7 target lines: which items belong to a checkout, one line per ticket
//! with the most pressing step, repeated, out-of-order and interleaved sources, the
//! version, and empty-then-refill. Synthetic ids only.

use serde_json::json;

use super::*;

const SOURCE: &str = "source-a";

fn item(step: &str, target: &str, native: &str, state: &str) -> serde_json::Value {
    json!({
        "step_id": step,
        "goal": {"connection_id": "goals", "native_id": format!("goal-{step}")},
        "goal_slug": "LCH-1",
        "target": {"target_id": target, "connection_id": SOURCE, "native_id": native},
        "project_label": target.to_uppercase(),
        "location": "local",
        "state": state,
        "level": "confirmed",
        "estimated_start_ms": 5,
    })
}

fn timeline(phase: &str, items: &[serde_json::Value]) -> Timeline {
    serde_json::from_value(json!({
        "version": "v",
        "now_ms": 1,
        "declared": true,
        "phase": phase,
        "automatic": phase == "running",
        "kick_off": {"can_start": false, "can_pause": false},
        "sessions": 0,
        "targets": [],
        "waves": [{"kind": "next", "ordinal": 0, "items": items}],
        "counts": {},
        "wave_minutes": 60,
        "estimate": "default",
    }))
    .unwrap()
}

fn source<'a>(
    owner: &'a str,
    targets: &'a [String],
    timeline: &'a Timeline,
) -> TargetLineSource<'a> {
    TargetLineSource {
        owner_checkout_id: owner,
        owner_label: owner,
        target_ids: targets,
        timeline,
    }
}

fn shown(lines: &TargetLines) -> Vec<(String, String, String, u32)> {
    lines
        .lines
        .iter()
        .map(|line| {
            (
                line.item.target.native_id.clone(),
                line.owner_checkout_id.clone(),
                line.item.step_id.clone(),
                line.also_planned,
            )
        })
        .collect()
}

#[test]
fn hs2_s3fkj7_keeps_only_this_checkouts_targets_one_line_per_ticket() {
    let api = vec!["api".to_owned()];
    let plan = timeline(
        "running",
        &[
            item("s1", "api", "t1", "ready"),
            item("s2", "web", "t2", "running"),
            item("s3", "api", "t3", "done"),
        ],
    );
    let lines = target_lines(&[source("launch", &api, &plan)]);
    assert!(lines.orchestrated);
    assert_eq!(
        shown(&lines),
        vec![
            ("t1".into(), "launch".into(), "s1".into(), 0),
            ("t3".into(), "launch".into(), "s3".into(), 0),
        ]
    );
    assert!(lines.lines.iter().all(|line| !line.proposed));
    // A proposed plan's lines say so.
    let proposed = timeline("proposed", &[item("s1", "api", "t1", "ready")]);
    assert!(target_lines(&[source("launch", &api, &proposed)]).lines[0].proposed);
}

#[test]
fn hs2_s3fkj7_several_steps_on_one_ticket_show_the_most_pressing() {
    let api = vec!["api".to_owned()];
    // Each pair: the state shown when both steps work ticket t1.
    let table = [
        (("ready", "running"), "b"),
        (("waits_for_permission", "ready"), "a"),
        (("done", "waits_for_prerequisite"), "b"),
        (("provisional", "waits_for_decision"), "a"),
        // Equal rank: the step id decides, whatever the order.
        (("ready", "waits_for_prerequisite"), "a"),
    ];
    for ((first, second), expected) in table {
        for flipped in [false, true] {
            let mut items = vec![
                item("a", "api", "t1", first),
                item("b", "api", "t1", second),
            ];
            if flipped {
                items.reverse();
            }
            let plan = timeline("running", &items);
            let lines = target_lines(&[source("launch", &api, &plan)]);
            assert_eq!(lines.lines.len(), 1);
            assert_eq!(lines.lines[0].item.step_id, expected, "{first} vs {second}");
            assert_eq!(lines.lines[0].also_planned, 1);
        }
    }
}

#[test]
fn hs2_s3fkj7_two_owners_interleaved_repeated_and_out_of_order() {
    let api = vec!["api".to_owned()];
    let web = vec!["web".to_owned()];
    let launch = timeline("running", &[item("s1", "api", "t1", "ready")]);
    let other = timeline(
        "running",
        &[
            item("s9", "web", "t1", "ready"),
            item("s8", "web", "t2", "running"),
        ],
    );
    let forward = target_lines(&[
        source("launch", &api, &launch),
        source("other", &web, &other),
    ]);
    // Same rank on t1: the owner listed first wins; t2 belongs to the other owner.
    assert_eq!(
        shown(&forward),
        vec![
            ("t1".into(), "launch".into(), "s1".into(), 1),
            ("t2".into(), "other".into(), "s8".into(), 0),
        ]
    );
    // Repeating a source counts nothing twice.
    let repeated = target_lines(&[
        source("launch", &api, &launch),
        source("other", &web, &other),
        source("launch", &api, &launch),
    ]);
    assert_eq!(shown(&repeated), shown(&forward));
    assert_eq!(repeated.version, forward.version);
    // Reversed, the other owner is listed first and takes the tie.
    let backward = target_lines(&[
        source("other", &web, &other),
        source("launch", &api, &launch),
    ]);
    assert_eq!(backward.lines[0].owner_checkout_id, "other");
    assert_eq!(backward.lines[0].also_planned, 1);
}

#[test]
fn hs2_s3fkj7_version_tracks_lines_not_the_clock_and_empty_then_refill() {
    let api = vec!["api".to_owned()];
    let ready = timeline("running", &[item("s1", "api", "t1", "ready")]);
    let first = target_lines(&[source("launch", &api, &ready)]);
    let mut later = ready.clone();
    later.now_ms = 99;
    later.waves[0].items[0].estimated_start_ms = Some(77);
    assert_eq!(
        target_lines(&[source("launch", &api, &later)]).version,
        first.version
    );
    let running = timeline("running", &[item("s1", "api", "t1", "running")]);
    let moved = target_lines(&[source("launch", &api, &running)]);
    assert_ne!(moved.version, first.version);
    // Orchestrated with nothing planned yet differs from not orchestrated at all.
    let empty = timeline("no_plan", &[]);
    let bound = target_lines(&[source("launch", &api, &empty)]);
    assert!(bound.orchestrated && bound.lines.is_empty());
    let none = target_lines(&[]);
    assert!(!none.orchestrated && none.lines.is_empty());
    assert_ne!(bound.version, none.version);
    // Refilled, the same lines and version come back.
    let refilled = target_lines(&[source("launch", &api, &ready)]);
    assert_eq!(refilled, first);
    // A source whose targets are not this checkout's contributes nothing.
    let unrelated = target_lines(&[source("launch", &[], &ready)]);
    assert_eq!(unrelated, none);
}
