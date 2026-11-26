//! A multi-site project's home site out of reach, on two real servers (the fleet of
//! [`Fleet::sites_peer_home`]): the peer's target B is the project's home and the
//! controller, which runs the orchestrator, holds its other site A. There is no
//! failover (HS2-EX594V): the outage is shown and announced, and a run on the site
//! claims its own ticket after the bounded dispatcher wait.
//!
//! - HS2-1CC1TQ: an outage whose project goes idle is still followed, so it clears and
//!   its end is announced once the home is back, with no pass anyone asked for.
//!
//! Synthetic tickets and identifiers only. Nothing is waited for by time: every wait is
//! woken by an event stream or a file change, with a deadline.
use super::e2e::{ACTIONS, ACTOR, Fleet, Side, TARGET_A, TARGET_B, TOOL_A};
use super::e2e_sites::{Placed, converge, home_notifications, placed_from};
use super::session_work::received_prompt;
use super::*;
use std::time::Duration;

const MINUTE: Duration = Duration::from_secs(60);

/// The policy of these tests: both sites allowed one session, automatic placement,
/// and a run nobody assigns claims for its own session at once.
async fn place_at_once(fleet: &mut Fleet) {
    fleet
        .policy(json!({
            "targets":[
                {"target_id":TARGET_A,"actions":ACTIONS,"tools":[{"tool":TOOL_A}],"max_sessions":1},
                {"target_id":TARGET_B,"actions":ACTIONS,"tools":[{"tool":TOOL_A}],"max_sessions":1}],
            "placement":{"automatic":true,"actor":ACTOR,"cooldown_minutes":0,
                "dispatcher_wait_minutes":0}
        }))
        .await;
}

/// Plan `change` on site A and wait for the loop to place its worker there, which
/// takes up its role in its own site's pool. Returns the step and the worker.
async fn placed_on_site(
    fleet: &Fleet,
    orchestration: &hotsheet_model::Ticket,
    change: &hotsheet_model::Ticket,
) -> (String, Placed) {
    let defined = fleet
        .ok(
            "POST",
            "/readiness/define",
            json!({"actor":Fleet::ai(),"ticket":{"native_id":orchestration.slug},
                "target_id":TARGET_A,"connection_id":fleet.connection(Side::Controller),
                "native_id":change.slug,"prerequisites":[]}),
        )
        .await;
    let step = defined["step"]["step_id"].as_str().unwrap().to_owned();
    let start = fleet.started_for(&step, 2 * MINUTE).await;
    let worker = placed_from(fleet, &start, TARGET_A).await;
    received_prompt(&fleet.reports, &worker.terminal, "`hotsheet-worker`");
    (step, worker)
}

/// The worker joins its site's pool, which wakes the loop.
async fn join(fleet: &Fleet, worker: &Placed) {
    fleet.join_pool(Side::Controller, &worker.worker);
    let synced = fleet
        .ok(
            "POST",
            &format!("/sessions/{}/sync", worker.session_id),
            json!({"actor":Fleet::ai()}),
        )
        .await;
    assert!(synced["joined_at_ms"].as_u64().is_some(), "{synced}");
}

/// The session's record once its state is `state`, woken by the owner's event stream.
async fn session_until(fleet: &Fleet, session_id: &str, state: &str) -> Value {
    let deadline = std::time::Instant::now() + 2 * MINUTE;
    let mut cursor = 0;
    loop {
        let session = fleet.session(session_id).await;
        if session["state"] == state {
            return session;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "session never {state}: {session:#}"
        );
        cursor = fleet.next_event(None, cursor).await;
    }
}

/// The timeline once `wanted` holds of it, woken by this machine's home-outage ledger
/// changing (the loop's own passes write it).
async fn timeline_until(fleet: &Fleet, what: &str, wanted: impl Fn(&Value) -> bool) -> Value {
    let directory = fleet.home.join("orchestration-home-outages");
    std::fs::create_dir_all(&directory).unwrap();
    let waiter = hotsheet_jobs::ChangeWaiter::starting(&[directory.as_path()]).unwrap();
    let deadline = std::time::Instant::now() + 2 * MINUTE;
    loop {
        let timeline = fleet.ok("GET", "/timeline", Value::Null).await;
        if wanted(&timeline) {
            return timeline;
        }
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        assert!(!remaining.is_zero(), "timeline never {what}: {timeline:#}");
        tokio::task::block_in_place(|| waiter.wait(remaining.min(Duration::from_secs(5))));
    }
}

/// HS2-1CC1TQ: the home goes out while its project has work, the outage is announced,
/// then the project goes idle (its step withdrawn, its ticket let go, its session wound
/// down) with the home still out. The outage stays on every surface while that is true.
/// When the home comes back nobody asks for a pass: the loop reads the home again on
/// its bounded backoff by itself, clears the outage and announces the return once.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn hs2_1cc1tq_an_idle_projects_outage_clears_by_itself_when_the_home_is_back() {
    // Announced at once; read again 200 ms in, doubling up to six seconds apart.
    let mut fleet =
        Fleet::sites_peer_home_rechecked(Duration::ZERO, Duration::from_millis(200)).await;
    let orchestration = fleet.orchestration_ticket("Coordinate the synthetic project");
    let change = fleet.target_ticket(Side::Controller, "Change the site can make");
    converge(&fleet);
    fleet.peer_offline().await;
    place_at_once(&mut fleet).await;
    let (step, worker) = placed_on_site(&fleet, &orchestration, &change).await;
    let run_id = worker.run_id.clone().unwrap();
    join(&fleet, &worker).await;
    fleet
        .run_until(&run_id, "claimed for its session", |status| {
            status["state"] == "dispatched"
        })
        .await;
    let told = home_notifications(&fleet).await;
    assert_eq!(told.len(), 1, "{told:?}");
    assert_eq!(told[0].0, "warning");

    // The project goes idle with its home still out.
    fleet
        .ok(
            "POST",
            "/readiness/remove",
            json!({"actor":Fleet::person(),"step_id":step}),
        )
        .await;
    fleet
        .finish(Side::Controller, &change, &worker.worker)
        .await;
    session_until(&fleet, &worker.session_id, "stopped").await;
    let plan = fleet
        .ok("POST", "/scheduler/plan", json!({"actor":Fleet::ai()}))
        .await;
    assert_eq!(plan["inputs"]["work"], 0, "{plan:#}");
    assert_eq!(plan["inputs"]["sessions"], 0, "{plan:#}");
    // Still out, so still shown, from when it began, and not announced again.
    let outage = &plan["home_outages"][0];
    assert_eq!(outage["home_target_id"], TARGET_B, "{plan:#}");
    assert_eq!(outage["announced"], true, "{plan:#}");
    let since = outage["since_ms"].as_u64().unwrap();
    let timeline = fleet.ok("GET", "/timeline", Value::Null).await;
    assert_eq!(
        timeline["home_outages"][0]["since_ms"].as_u64(),
        Some(since),
        "{timeline:#}"
    );
    assert_eq!(home_notifications(&fleet).await.len(), 1);

    // Back. No tick is asked for: the loop's own re-read finds it.
    fleet.peer_online().await;
    timeline_until(&fleet, "cleared", |timeline| {
        timeline.get("home_outages").is_none()
    })
    .await;
    let told = home_notifications(&fleet).await;
    assert_eq!(
        told,
        vec![
            (
                "warning".to_owned(),
                format!("Home site Target {TARGET_B} is unavailable")
            ),
            (
                "success".to_owned(),
                format!("Home site Target {TARGET_B} is back")
            ),
        ]
    );
    // Nothing more: an idle project's home is not read again once it is back, and a
    // pass asked for now finds nothing to say.
    let tick = fleet
        .ok("POST", "/scheduler/tick", json!({"actor":Fleet::ai()}))
        .await;
    assert!(tick.get("home_outages").is_none(), "{tick:#}");
    assert_eq!(tick["inputs"]["targets"], 0, "{tick:#}");
    assert_eq!(home_notifications(&fleet).await.len(), 2);
    fleet.shut_down().await;
}
