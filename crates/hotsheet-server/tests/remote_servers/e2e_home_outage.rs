//! A multi-site project's home site out of reach, on two real servers (the fleet of
//! [`Fleet::sites_peer_home`]): the peer's target B is the project's home and the
//! controller, which runs the orchestrator, holds its other site A. There is no
//! failover (HS2-EX594V): the outage is shown and announced, and a run on the site
//! claims its own ticket after the bounded dispatcher wait.
//!
//! - HS2-1CC1TQ: an outage whose project goes idle is still followed, so it clears and
//!   its end is announced once the home is back, with no pass anyone asked for.
//! - HS2-5DF941: the home cut off from the site keeps running; its dispatcher and the
//!   site's run both try to claim one ticket, and exactly one claim holds.
//! - HS2-S5RH57: when the home's worker won, the site's session placed for that run
//!   is wound down at its safe point (nothing else is ready) and never replaced.
//! - HS2-JGNCAR: from the moment the home's worker holds the run, the timeline names no
//!   session of the site as the run's, while that session idles and once it is stopped.
//!
//! Synthetic tickets and identifiers only. Nothing is waited for by time: every wait is
//! woken by an event stream or a file change, with a deadline.
use super::e2e::{ACTIONS, ACTOR, Fleet, Side, TARGET_A, TARGET_B, TOOL_A};
use super::e2e_sites::{Placed, claim, converge, home_notifications, placed_from, ticket_in};
use super::orchestration::api;
use super::session_work::received_prompt;
use super::*;
use hotsheet_model::ClaimEventKind;
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

/// HS2-JGNCAR: the controller's timeline item whose run is `run_id`.
async fn run_item(fleet: &Fleet, run_id: &str) -> Value {
    let timeline = fleet.ok("GET", "/timeline", Value::Null).await;
    timeline["waves"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|wave| wave["items"].as_array().into_iter().flatten())
        .find(|item| item["run"]["run_id"] == run_id)
        .cloned()
        .unwrap_or_else(|| panic!("no timeline item has run {run_id}: {timeline:#}"))
}

/// The controller's home-outage notifications once there are `count`, woken by the
/// server's change events (a notification is published just after the ledger that
/// the timeline reads is written).
async fn notifications_until(
    fleet: &Fleet,
    events: &mut tokio::sync::broadcast::Receiver<hotsheet_server::ChangeEvent>,
    count: usize,
) -> Vec<(String, String)> {
    let deadline = tokio::time::Instant::now() + MINUTE;
    loop {
        let told = home_notifications(fleet).await;
        if told.len() >= count {
            return told;
        }
        assert!(
            tokio::time::timeout_at(deadline, events.recv())
                .await
                .is_ok(),
            "never {count} notifications: {told:?}"
        );
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
    let mut events = fleet.controller.subscribe();
    fleet.peer_online().await;
    timeline_until(&fleet, "cleared", |timeline| {
        timeline.get("home_outages").is_none()
    })
    .await;
    let told = notifications_until(&fleet, &mut events, 2).await;
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

/// Who claims the project's ticket first while the home is cut off from the site.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum First {
    /// The home's dispatcher assigns it to one of the home's own workers.
    Home,
    /// The site's run claims it for its own session after the bounded wait.
    Site,
}

/// The worker id the home's dispatcher assigns the ticket to on its own side.
const HOME_WORKER: &str = "home-worker-synthetic";

/// HS2-5DF941: the home's dispatcher wins. While the home is cut off from the site it
/// still runs and assigns the ticket to one of its own workers; the site's run then
/// ends its bounded wait and tries to claim the same ticket for its own session.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn hs2_5df941_a_partitioned_homes_claim_wins_over_its_sites_run() {
    partitioned_claim(First::Home).await;
}

/// HS2-5DF941: the site's run wins. It claims after the bounded wait; the home's
/// dispatcher, still running on the other side of the partition, then assigns the same
/// ticket to one of its own workers, whose claim is refused.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn hs2_5df941_a_sites_claim_wins_over_its_partitioned_home() {
    partitioned_claim(First::Site).await;
}

/// HS2-5DF941: a network partition between the home's server and the site's, both
/// running, both trying to claim one ticket of the project. The shared store's claim
/// (the `git_ref` backend both clones push to) lets exactly one hold it; the loser is
/// told why in plain words and starts no second session; the outage is announced once
/// each way.
async fn partitioned_claim(first: First) {
    let mut fleet =
        Fleet::sites_peer_home_rechecked(Duration::ZERO, Duration::from_millis(200)).await;
    let orchestration = fleet.orchestration_ticket("Coordinate the synthetic project");
    let change = fleet.target_ticket(Side::Controller, "Change both sides want");
    converge(&fleet);
    // The partition: the controller (the orchestrator and site A) can no longer reach
    // the home's server, which keeps running and serving its own side.
    fleet.peer_offline().await;
    place_at_once(&mut fleet).await;
    let (step, worker) = placed_on_site(&fleet, &orchestration, &change).await;
    let run_id = worker.run_id.clone().unwrap();

    let holder = match first {
        First::Home => {
            // The home's dispatcher assigns the ticket on its side; its worker claims.
            assert_eq!(claim(&fleet, Side::Peer, &change, HOME_WORKER).await, 200);
            // The site's worker joins: its run's bounded wait is over, so the loop
            // tries to claim for its session. The shared claim refuses it, and the
            // decision log says why with a code the app words in plain language
            // ("The ticket changed in that project meanwhile.").
            join(&fleet, &worker).await;
            let refused = fleet
                .decided(
                    "the site's claim answered",
                    |entry| {
                        entry["kind"] == "assignment"
                            && entry["run_id"] == run_id.as_str()
                            && entry["outcome"].is_string()
                    },
                    2 * MINUTE,
                )
                .await;
            assert_eq!(refused["outcome"], "refused", "{refused:#}");
            assert_eq!(refused["outcome_detail"], "target_conflict", "{refused:#}");
            HOME_WORKER.to_owned()
        }
        First::Site => {
            join(&fleet, &worker).await;
            let claimed = fleet
                .decided(
                    "the site's run claimed for its own session",
                    |entry| {
                        entry["kind"] == "assignment"
                            && entry["outcome"] == "claimed"
                            && entry["run_id"] == run_id.as_str()
                    },
                    2 * MINUTE,
                )
                .await;
            assert_eq!(claimed["session_id"], worker.session_id.as_str());
            // The home's dispatcher, cut off, assigns the same ticket on its side: its
            // worker's claim is refused, naming who holds it.
            let (status, body) = api(
                fleet.side_router(Side::Peer),
                "POST",
                &format!("/tickets/{}/claim", change.id),
                json!({"worker": HOME_WORKER, "lease_minutes": 30}),
                true,
            )
            .await;
            assert_eq!(status, StatusCode::CONFLICT, "{body}");
            let said = body["error"].as_str().unwrap();
            assert!(
                said.contains(&format!("is claimed by '{}'", worker.worker)),
                "{said}"
            );
            worker.worker.clone()
        }
    };
    // The one run of the step names the winner and how it got the ticket, in words the
    // app shows ("assigned by the project's dispatcher", "claimed for its session").
    let run = fleet
        .ok("GET", &format!("/runs/{run_id}"), Value::Null)
        .await;
    assert_eq!(run["state"], "dispatched", "{run:#}");
    assert_eq!(run["worker_id"], holder.as_str(), "{run:#}");
    let how = match first {
        First::Home => "assigned_by_dispatcher",
        First::Site => "claimed_for_session",
    };
    let dispatched: Vec<&Value> = run["history"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["to"] == "dispatched")
        .collect();
    assert_eq!(dispatched.len(), 1, "{run:#}");
    assert_eq!(dispatched[0]["reason"], how, "{run:#}");

    // Exactly one claim, held by the winner, in both clones once they sync.
    converge(&fleet);
    for store in [&fleet.a_store, &fleet.b_store] {
        let read = ticket_in(store, &change);
        assert_eq!(
            read.claimed_by.as_deref(),
            Some(holder.as_str()),
            "{read:?}"
        );
        let claims = read
            .claim_history
            .iter()
            .filter(|event| event.kind == ClaimEventKind::Claim)
            .count();
        assert_eq!(claims, 1, "{read:?}");
    }

    // HS2-JGNCAR: the timeline names the site's session as the run's only while it
    // works the run. Once the home's worker holds the ticket, the item names no session
    // and says the work is another worker's, already while that session idles before
    // its wind-down (it is read as still running after the timeline was read).
    let item = run_item(&fleet, &run_id).await;
    match first {
        First::Home => {
            assert_eq!(item["worked_elsewhere"], true, "{item:#}");
            assert!(item.get("session_id").is_none(), "{item:#}");
            assert!(item.get("session_state").is_none(), "{item:#}");
            assert_eq!(item["run"]["worker_id"], HOME_WORKER, "{item:#}");
            assert_eq!(fleet.session(&worker.session_id).await["state"], "running");
        }
        First::Site => {
            assert!(item.get("worked_elsewhere").is_none(), "{item:#}");
            assert_eq!(item["session_id"], worker.session_id.as_str(), "{item:#}");
            assert_eq!(item["session_state"], "running", "{item:#}");
        }
    }

    // HS2-S5RH57: the site's session placed for the run has no work left in it once
    // the home's worker holds the ticket. Nothing else is ready, so it is wound down
    // by the idle rule at its host's safe point: stopped on purpose, not lost. The
    // winner's session keeps the ticket in the site-first case.
    if first == First::Home {
        let stop = fleet
            .decided(
                "the idle site session wound down",
                |entry| {
                    entry["kind"] == "wind_down"
                        && entry["outcome"] == "wind_down_requested"
                        && entry["session_id"] == worker.session_id.as_str()
                },
                3 * MINUTE,
            )
            .await;
        assert_eq!(stop["reason"], "idle", "{stop:#}");
        let ended = session_until(&fleet, &worker.session_id, "stopped").await;
        let last = ended["history"].as_array().unwrap().last().unwrap().clone();
        assert_eq!(last["reason"], "wound_down", "{ended:#}");
        assert!(
            !fleet
                .drawer(Side::Controller)
                .await
                .iter()
                .any(|tab| tab["id"] == worker.terminal.as_str() && tab["alive"] == true)
        );
        // The run is still the home worker's, and nothing was taken from it.
        let run = fleet
            .ok("GET", &format!("/runs/{run_id}"), Value::Null)
            .await;
        assert_eq!(
            (&run["state"], &run["worker_id"]),
            (&json!("dispatched"), &json!(HOME_WORKER)),
            "{run:#}"
        );
        // HS2-JGNCAR: wound down, the session is still not named as the run's.
        let item = run_item(&fleet, &run_id).await;
        assert_eq!(item["worked_elsewhere"], true, "{item:#}");
        assert!(item.get("session_id").is_none(), "{item:#}");
    }

    // More passes start no second session for the step, and no role session.
    for _ in 0..2 {
        fleet
            .ok("POST", "/scheduler/tick", json!({"actor":Fleet::ai()}))
            .await;
    }
    let log = fleet.decisions().await;
    let starts: Vec<_> = log
        .iter()
        .filter(|entry| entry["kind"] == "start" && entry["outcome"] == "started")
        .collect();
    assert_eq!(starts.len(), 1, "{starts:#?}");
    assert_eq!(starts[0]["step_id"], step.as_str(), "{starts:#?}");
    let sessions = fleet.sessions().await;
    assert_eq!(sessions.len(), 1, "{sessions:#?}");
    assert_eq!(fleet.runs().await.len(), 1);

    // Heal: the outage ends, announced once each way.
    let mut events = fleet.controller.subscribe();
    fleet.peer_online().await;
    timeline_until(&fleet, "cleared", |timeline| {
        timeline.get("home_outages").is_none()
    })
    .await;
    let told = notifications_until(&fleet, &mut events, 2).await;
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
    // After the heal the shared store still names one holder.
    converge(&fleet);
    let read = ticket_in(&fleet.a_store, &change);
    assert_eq!(
        read.claimed_by.as_deref(),
        Some(holder.as_str()),
        "{read:?}"
    );
    fleet.shut_down().await;
}
