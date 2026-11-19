//! HS2-2EY9DP: the whole orchestration loop across two targets on two servers, through
//! the shipped server routes, the server's own scheduling loop, the peer's hosted
//! halves over mTLS and a real Job monitor. Where the test plays an agent (joining
//! the pool, claiming, finishing), it does what the worker skill does, through the
//! hosting server's own ticket routes.
use super::e2e::{ACTOR, Fleet, Side, TARGET_A, TARGET_B, TOOL_A, TOOL_B};
use super::session_work::{launch_report, received_prompt};
use super::*;
use std::time::Duration;

const MINUTE: Duration = Duration::from_secs(60);

/// What one walk of the loop leaves behind, for the scenarios that continue from it.
pub(super) struct Walked {
    pub(super) orchestration: hotsheet_model::Ticket,
    pub(super) a: hotsheet_model::Ticket,
    pub(super) b: hotsheet_model::Ticket,
    pub(super) step_a: String,
    pub(super) step_b: String,
}

/// The plan: target B's ticket builds on target A's, which must be finished with its
/// CI green first. Then a person turns automatic placement on.
pub(super) async fn planned(fleet: &mut Fleet) -> Walked {
    let orchestration = fleet.orchestration_ticket("Coordinate the synthetic release");
    let a = fleet.target_ticket(Side::Controller, "Synthetic library change");
    let b = fleet.target_ticket(Side::Peer, "Synthetic consumer change");
    let defined_a = fleet
        .plan(&orchestration, Side::Controller, &a, json!([]))
        .await;
    assert_eq!(defined_a["level"], "confirmed", "{defined_a}");
    let prerequisite = fleet.after(Side::Controller, &a);
    let defined_b = fleet
        .plan(&orchestration, Side::Peer, &b, json!([prerequisite]))
        .await;
    assert_eq!(defined_b["level"], "blocked", "{defined_b}");
    let step_a = defined_a["step"]["step_id"].as_str().unwrap().to_owned();
    let step_b = defined_b["step"]["step_id"].as_str().unwrap().to_owned();
    // Nothing runs anywhere before a person enables placement.
    assert!(fleet.drawer(Side::Controller).await.is_empty());
    assert!(fleet.drawer(Side::Peer).await.is_empty());
    fleet.automatic().await;
    Walked {
        orchestration,
        a,
        b,
        step_a,
        step_b,
    }
}

/// A session started for a step: its run, terminal and worker.
pub(super) struct Placed {
    pub(super) session_id: String,
    pub(super) run_id: String,
    pub(super) terminal: String,
    pub(super) worker: String,
}

/// The loop places a session for `step` on `side`: it dispatches the step's run in
/// dispatcher mode and starts the session of the target's tool on the target's own
/// server, set up in the target checkout, linked to the run.
pub(super) async fn placed(fleet: &Fleet, step: &str, side: Side) -> Placed {
    let start = fleet.started_for(step, 2 * MINUTE).await;
    assert_eq!(start["mode"], "applied", "{start}");
    assert_eq!(start["actor_id"], ACTOR, "{start}");
    let run_id = start["run_id"]
        .as_str()
        .expect("the start names its run")
        .to_owned();
    let (session, terminal, worker) = fleet.started_session(&start).await;
    assert_eq!(session["state"], "running", "{session}");
    assert_eq!(session["run_id"], run_id.as_str(), "{session}");
    assert_eq!(session["target_id"], Fleet::target(side), "{session}");
    let tool = if side == Side::Controller {
        TOOL_A
    } else {
        TOOL_B
    };
    assert_eq!(session["tool"], tool, "{session}");
    let run = fleet
        .ok("GET", &format!("/runs/{run_id}"), Value::Null)
        .await;
    assert_eq!(run["mode"], "dispatcher", "{run}");
    assert_eq!(run["operation_id"], format!("sched-run-{step}").as_str());
    // The session is in the drawer of the server hosting the target, and only there.
    let here = fleet.drawer(side).await;
    assert!(
        here.iter().any(|tab| tab["id"] == terminal.as_str()),
        "{here:?}"
    );
    let other = if side == Side::Controller {
        Side::Peer
    } else {
        Side::Controller
    };
    assert!(
        !fleet
            .drawer(other)
            .await
            .iter()
            .any(|tab| tab["id"] == terminal.as_str())
    );
    // It started in the target checkout, set up for its tool, as the run's worker.
    let report = launch_report(&fleet.reports, &terminal);
    assert_eq!(
        std::path::Path::new(report["cwd"].as_str().unwrap()),
        fleet.project(side).canonicalize().unwrap(),
        "{report}"
    );
    assert_eq!(report["env"]["HOTSHEET_WORKER_ID"], worker.as_str());
    assert_eq!(report["env"]["HOTSHEET_SESSION_ROLE"], "worker");
    Placed {
        session_id: session["session_id"].as_str().unwrap().to_owned(),
        run_id,
        terminal,
        worker,
    }
}

/// The session takes up its role, asks for its exact ticket, joins the pool and
/// claims the ticket; the run follows the target's change feed to `dispatched`.
pub(super) async fn works(
    fleet: &Fleet,
    side: Side,
    placed: &Placed,
    ticket: &hotsheet_model::Ticket,
) {
    let prompt = received_prompt(&fleet.reports, &placed.terminal, "`hotsheet-worker`");
    assert!(prompt.contains(&placed.worker), "{prompt}");
    assert!(prompt.contains(&format!("`{}`", ticket.slug)), "{prompt}");
    fleet.join_pool(side, &placed.worker);
    let synced = fleet
        .ok(
            "POST",
            &format!("/sessions/{}/sync", placed.session_id),
            json!({"actor":Fleet::ai()}),
        )
        .await;
    assert!(synced["joined_at_ms"].as_u64().is_some(), "{synced}");
    // The setup files the tool wrote are committed, as a person commits project files.
    super::sessions::commit_all(fleet.project(side));
    fleet.claim(side, ticket, &placed.worker).await;
    let dispatched = fleet
        .run_until(&placed.run_id, "dispatched to its session", |status| {
            status["state"] == "dispatched"
        })
        .await;
    assert_eq!(dispatched["worker_id"], placed.worker.as_str());
}

/// HS2-2EY9DP priority 1: an orchestration ticket with a readiness plan across two
/// targets on two servers; an automatic policy places a session on each server in
/// turn; target A's CI gate goes green through the Job monitor before target B is
/// started; both target tickets are completed and both sessions are wound down.
#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn hs2_2ey9dp_the_loop_completes_a_plan_across_two_servers_through_a_ci_gate() {
    let mut fleet = Fleet::new().await;
    let walked = planned(&mut fleet).await;

    // 1. Target A's step is confirmed: the controller places a session for it here.
    let a = placed(&fleet, &walked.step_a, Side::Controller).await;
    works(&fleet, Side::Controller, &a, &walked.a).await;
    // Target B's step waits on A: nothing was placed on the peer.
    assert_eq!(fleet.readiness(&walked.step_b).await["level"], "blocked");
    assert!(fleet.drawer(Side::Peer).await.is_empty());

    // 2. The session integrates a commit; its CI gate is linked and watched by the
    //    Job monitor the server starts. The provider reports the run in progress.
    let commit = "a".repeat(40);
    fleet.provider.run(41, &commit, None);
    let linked = fleet
        .ok(
            "POST",
            &format!("/runs/{}/gates", a.run_id),
            json!({"commit":commit,"repository":"acme/widgets","actor":Fleet::ai()}),
        )
        .await;
    assert_eq!(linked["watching"], true, "{linked}");
    let gate = linked["gate"]["gate_id"].as_str().unwrap().to_owned();
    // While the gate waits, target B's work is provisional, which this policy does
    // not dispatch: still nothing on the peer.
    fleet.step_until(&walked.step_b, "provisional").await;
    let explained = fleet
        .ok(
            "POST",
            "/scheduler/explain",
            json!({"actor":Fleet::ai(),"step_id":walked.step_b}),
        )
        .await;
    assert_eq!(
        explained["reason"], "provisional_not_dispatched",
        "{explained}"
    );
    assert!(fleet.drawer(Side::Peer).await.is_empty());

    // 3. The session finishes the ticket; the run follows the feed to completion.
    fleet.finish(Side::Controller, &walked.a, &a.worker).await;
    fleet
        .run_until(&a.run_id, "completed", |status| {
            status["state"] == "completed"
        })
        .await;
    let waiting = fleet.readiness(&walked.step_b).await;
    assert_eq!(waiting["level"], "provisional", "{waiting}");
    assert!(fleet.drawer(Side::Peer).await.is_empty());

    // 4. CI goes green. Nobody calls sync: the monitor observes the provider, the
    //    follower records the verdict, readiness confirms B, and the loop places a
    //    session for it on the peer.
    fleet.provider.run(41, &commit, Some("success"));
    fleet.gate_until(&gate, "passed").await;
    fleet.step_until(&walked.step_b, "in_progress").await;
    let b = placed(&fleet, &walked.step_b, Side::Peer).await;
    works(&fleet, Side::Peer, &b, &walked.b).await;
    fleet.finish(Side::Peer, &walked.b, &b.worker).await;
    fleet
        .run_until(&b.run_id, "completed", |status| {
            status["state"] == "completed"
        })
        .await;
    fleet.step_until(&walked.step_b, "finished").await;
    assert_eq!(fleet.readiness(&walked.step_a).await["level"], "finished");

    // 5. Both sessions idle out and are wound down at their hosts' safe points.
    for (placed, side) in [(&a, Side::Controller), (&b, Side::Peer)] {
        let stop = fleet
            .decided(
                "wind-down",
                |entry| {
                    entry["kind"] == "wind_down"
                        && entry["outcome"] == "wind_down_requested"
                        && entry["session_id"] == placed.session_id.as_str()
                },
                3 * MINUTE,
            )
            .await;
        assert_eq!(stop["reason"], "idle", "{stop}");
        assert_eq!(fleet.session(&placed.session_id).await["state"], "stopped");
        assert!(
            !fleet
                .drawer(side)
                .await
                .iter()
                .any(|tab| tab["id"] == placed.terminal.as_str() && tab["alive"] == true)
        );
    }

    // One run and one session per step, whatever the loop saw in between.
    assert_eq!(fleet.runs().await.len(), 2);
    assert_eq!(fleet.sessions().await.len(), 2);
    let log = fleet.decisions().await;
    let starts = log
        .iter()
        .filter(|entry| entry["kind"] == "start" && entry["outcome"] == "started")
        .count();
    assert_eq!(starts, 2, "{log:#?}");
    // The orchestration ticket links both target tickets.
    let links = fleet
        .ok(
            "POST",
            "/tickets/links",
            json!({"actor":Fleet::ai(),"ticket":{"native_id":walked.orchestration.slug},
                "resolve":true}),
        )
        .await;
    let links = links.as_array().unwrap_or_else(|| panic!("{links}"));
    assert_eq!(links.len(), 2, "{links:?}");
    for (target, ticket) in [(TARGET_A, &walked.a), (TARGET_B, &walked.b)] {
        let link = links
            .iter()
            .find(|link| link.to_string().contains(target))
            .unwrap_or_else(|| panic!("no link to {target}: {links:?}"));
        // Each resolves through its own server and reads finished there.
        assert_eq!(link["state"], "resolved", "{link}");
        assert_eq!(link["summary"]["status"], "completed", "{link}");
        assert_eq!(link["summary"]["slug"], ticket.slug.as_str(), "{link}");
    }
    fleet.shut_down().await;
}
