//! HS2-P2TCQS: work held because a tool is signed out is placed soon after the tool
//! signs in, not at the machine's next ten-minute capability detection. The machine
//! keeps that detection here, as a shipped server does; only the held re-check (its
//! first step shortened for the test) or a person looking can see the sign-in sooner.
//! The fake tool answers a sign-in status program as a real tool answers its declared
//! `auth status` command.
use super::e2e::{Fleet, Side, TOOL_A, TOOL_B};
use super::orchestration::api;
use super::*;
use std::time::Duration;

const MINUTE: Duration = Duration::from_secs(60);
/// Far inside the ten-minute capability detection the fleet keeps.
const WELL_BEFORE_THE_DETECTION: Duration = Duration::from_secs(90);

/// The step held on `side` because `tool` is signed out there, after placement is on.
async fn held(fleet: &mut Fleet, side: Side, tool: &str) -> String {
    let orchestration = fleet.orchestration_ticket("Coordinate the synthetic sign-in");
    let work = fleet.target_ticket(side, "Synthetic work behind a sign-in");
    let defined = fleet.plan(&orchestration, side, &work, json!([])).await;
    assert_eq!(defined["level"], "confirmed", "{defined}");
    let step = defined["step"]["step_id"].as_str().unwrap().to_owned();
    fleet.automatic().await;
    let detail = format!("tool_signed_out:{tool}");
    let held = fleet
        .decided(
            "a deferral on the signed-out tool",
            |entry| {
                entry["kind"] == "unplaced"
                    && entry["step_id"] == step.as_str()
                    && entry["detail"] == detail.as_str()
            },
            2 * MINUTE,
        )
        .await;
    assert_eq!(held["reason"], "no_tool_available", "{held}");
    assert!(fleet.sessions().await.is_empty());
    step
}

/// The hosted capacity read of the controller's target A, as Systems makes it.
async fn controller_capacity(fleet: &Fleet, query: &str) -> Value {
    let (status, body) = api(
        fleet.router.clone(),
        "GET",
        &format!("/checkouts/{}/target-capacity{query}", fleet.a_checkout.id),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

fn signed_in(capacity: &Value) -> &Value {
    &capacity["capabilities"]["ai_tools"][0]["signed_in"]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_p2tcqs_held_work_is_placed_at_the_first_recheck_not_the_detection() {
    let mut fleet = Fleet::signed_out_rechecked(Side::Controller, Duration::from_secs(1)).await;
    let step = held(&mut fleet, Side::Controller, TOOL_A).await;
    // While the step is held, the machine re-checks sign-in: more than the one
    // detection a ten-minute lifetime allows.
    let detections = fleet.controller.capability_detections();
    let deadline = std::time::Instant::now() + MINUTE;
    while fleet.controller.capability_detections() < detections + 2 {
        assert!(
            std::time::Instant::now() < deadline,
            "no sign-in re-check while held"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(fleet.sessions().await.is_empty());

    // The tool signs in: the next re-check sees it, the capacity version changes, and
    // the watch places the step, well before the detection would have run again.
    fleet.sign_in(true);
    let start = fleet.started_for(&step, WELL_BEFORE_THE_DETECTION).await;
    let (session, _, _) = fleet.started_session(&start).await;
    assert_eq!(session["tool"], TOOL_A, "{session}");
    fleet.shut_down().await;
}

/// No step is held, so nothing re-checks; a person looking (Systems' read, the held
/// card's explain) re-checks at once.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_p2tcqs_a_person_looking_rechecks_sign_in_at_once() {
    // A held step would re-check only after ten minutes.
    let mut fleet = Fleet::signed_out_rechecked(Side::Controller, 10 * MINUTE).await;
    assert_eq!(
        signed_in(&controller_capacity(&fleet, "").await),
        &json!(false)
    );
    let step = held(&mut fleet, Side::Controller, TOOL_A).await;

    // Signed in: an ordinary read still answers the detection it has.
    fleet.sign_in(true);
    assert_eq!(
        signed_in(&controller_capacity(&fleet, "").await),
        &json!(false)
    );
    // The held card's inspector asks why: the explain re-checks and finds it signed
    // in, and the changed capacity places the step.
    let explained = fleet
        .ok(
            "POST",
            "/scheduler/explain",
            json!({"step_id":step,"actor":Fleet::ai()}),
        )
        .await;
    assert_ne!(
        explained["detail"],
        format!("tool_signed_out:{TOOL_A}").as_str(),
        "{explained}"
    );
    assert_eq!(
        signed_in(&controller_capacity(&fleet, "").await),
        &json!(true)
    );
    let start = fleet.started_for(&step, WELL_BEFORE_THE_DETECTION).await;
    let (session, _, _) = fleet.started_session(&start).await;
    assert_eq!(session["tool"], TOOL_A, "{session}");

    // Signed in, nothing is re-checked, even on demand: a sign-out is seen at the
    // next detection, as before.
    fleet.sign_in(false);
    assert_eq!(
        signed_in(&controller_capacity(&fleet, "?sign_in_recheck=now").await),
        &json!(true)
    );
    let ordinary = controller_capacity(&fleet, "").await;
    assert_eq!(signed_in(&ordinary), &json!(true), "{ordinary}");
    fleet.shut_down().await;
}

/// The same for a remote site: the controller's watch of work held on the peer asks
/// the peer to re-check, so the peer sees its own sign-in.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_p2tcqs_a_remote_site_rechecks_sign_in_for_work_held_there() {
    let mut fleet = Fleet::signed_out_rechecked(Side::Peer, Duration::from_secs(1)).await;
    let step = held(&mut fleet, Side::Peer, TOOL_B).await;
    let detections = fleet.peer_state.capability_detections();
    let deadline = std::time::Instant::now() + MINUTE;
    while fleet.peer_state.capability_detections() < detections + 2 {
        assert!(
            std::time::Instant::now() < deadline,
            "the peer did not re-check sign-in while held"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    fleet.sign_in(true);
    let start = fleet.started_for(&step, WELL_BEFORE_THE_DETECTION).await;
    let (session, _, _) = fleet.started_session(&start).await;
    assert_eq!(session["tool"], TOOL_B, "{session}");
    assert_eq!(session["target_id"], Fleet::target(Side::Peer), "{session}");
    fleet.shut_down().await;
}
