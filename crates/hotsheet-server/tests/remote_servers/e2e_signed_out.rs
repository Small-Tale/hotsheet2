//! HS2-ZRPZJJ: a tool its machine reports signed out is not placed there. The
//! controller's fake tool answers a sign-in status program, as a real tool answers its
//! declared `auth status` command: signed out at first, so the server's own loop holds
//! the step with the tool named; once it signs in, the machine's next capacity version
//! wakes the loop and the session starts, with no timer or retry time in between.
use super::e2e::{Fleet, Side, TOOL_A};
use super::*;
use std::time::Duration;

const MINUTE: Duration = Duration::from_secs(60);

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_zrpzjj_a_signed_out_tool_is_not_placed_until_it_signs_in() {
    let mut fleet = Fleet::signed_out().await;
    let orchestration = fleet.orchestration_ticket("Coordinate the synthetic sign-in");
    let work = fleet.target_ticket(Side::Controller, "Synthetic work behind a sign-in");
    let defined = fleet
        .plan(&orchestration, Side::Controller, &work, json!([]))
        .await;
    assert_eq!(defined["level"], "confirmed", "{defined}");
    let step = defined["step"]["step_id"].as_str().unwrap().to_owned();
    let target = format!("/targets/{}", Fleet::target(Side::Controller));

    // The machine says the tool is signed out, and its admission refuses it by name.
    let capacity = fleet
        .ok(
            "GET",
            &format!("{target}/capacity?actor_id=scheduler-1&actor_role=ai"),
            Value::Null,
        )
        .await;
    assert_eq!(
        capacity["capabilities"]["ai_tools"][0],
        json!({"tool":TOOL_A,"signed_in":false}),
        "{capacity}"
    );
    let refused = fleet
        .ok(
            "POST",
            &format!("{target}/admission"),
            json!({"tool":TOOL_A,"actor":Fleet::ai()}),
        )
        .await;
    assert_eq!(refused["outcome"], "refused", "{refused}");
    assert_eq!(refused["reason"], "tool_signed_out", "{refused}");
    assert_eq!(refused["refusal"]["tool"], TOOL_A, "{refused}");

    // Placement on: the loop holds the step, naming the signed-out tool, and starts
    // nothing.
    fleet.automatic().await;
    let detail = format!("tool_signed_out:{TOOL_A}");
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
    let explained = fleet
        .ok(
            "POST",
            "/scheduler/explain",
            json!({"step_id":step,"actor":Fleet::ai()}),
        )
        .await;
    assert_eq!(explained["standing"], "not_placed", "{explained}");
    assert_eq!(explained["reason"], "no_tool_available", "{explained}");
    assert_eq!(explained["detail"], detail.as_str(), "{explained}");
    // It waits on the machine's change, not on a time.
    assert!(explained.get("retry_at_ms").is_none(), "{explained}");
    assert!(fleet.drawer(Side::Controller).await.is_empty());
    assert!(fleet.sessions().await.is_empty());

    // The tool signs in on the machine: its next capacity version places the step.
    fleet.sign_in(true);
    let start = fleet.started_for(&step, 2 * MINUTE).await;
    let (session, _, _) = fleet.started_session(&start).await;
    assert_eq!(session["tool"], TOOL_A, "{session}");
    assert_eq!(session["target_id"], Fleet::target(Side::Controller));
    let admitted = fleet
        .ok(
            "POST",
            &format!("{target}/admission"),
            json!({"tool":TOOL_A,"actor":Fleet::ai()}),
        )
        .await;
    assert_ne!(admitted["reason"], "tool_signed_out", "{admitted}");

    // Signed out again, the machine refuses another start of the tool by name.
    fleet.sign_in(false);
    let again = fleet
        .ok(
            "POST",
            &format!("{target}/admission"),
            json!({"tool":TOOL_A,"actor":Fleet::ai()}),
        )
        .await;
    assert_eq!(again["reason"], "tool_signed_out", "{again}");
    fleet.shut_down().await;
}
