//! HS2-1NJ04Q: the scheduler's role sessions (HS2-TK9ADC, docs/26 §26.30) end to end.
//! The controller's own scheduling loop starts a dispatcher on target B, on the peer
//! over mTLS, before its worker; sees it join B's coordination pool; starts an
//! integrator once work is handed off there; and winds all of them down when nothing
//! is left. Where the test plays an agent (joining the pool, claiming, handing off,
//! integrating), it does what the role skills do, on the peer's own checkout.
use super::e2e::{Fleet, Side, TARGET_A, TARGET_B, TOOL_A, TOOL_B};
use super::e2e_loop::{placed, works};
use super::session_work::{launch_report, now_ms};
use super::*;
use hotsheet_cli::coordination::{Assignment, Authority, Coordinator, ReturnRoute, Role, State};
use std::time::Duration;

const MINUTE: Duration = Duration::from_secs(60);

fn pool(fleet: &Fleet) -> Coordinator {
    Coordinator::open_with_registry_home(&fleet.b_project, &fleet.b_store, fleet.peer_home.clone())
        .unwrap()
}

/// Rewrite B's pool as the integrator's own commands would leave it.
fn edit_pool(fleet: &Fleet, change: impl FnOnce(&mut State)) {
    let mut state = pool(fleet).state().unwrap();
    change(&mut state);
    state.revision += 1;
    std::fs::write(
        fleet
            .b_project
            .join(".git")
            .join("hotsheet-coordination")
            .join("state.json"),
        serde_json::to_vec(&state).unwrap(),
    )
    .unwrap();
}

fn now_s() -> i64 {
    i64::try_from(now_ms() / 1000).unwrap()
}

/// A role session the loop started on B, by the role its decision names.
async fn role_started(fleet: &Fleet, role: &str) -> (Value, String, String) {
    let start = fleet
        .decided(
            &format!("a started {role}"),
            |entry| {
                entry["kind"] == "start"
                    && entry["outcome"] == "started"
                    // HS2-4JQ41V: an integrator's start names the handoff it is for.
                    && if role == "integrator" {
                        entry["reason"] == "integrator_started"
                            && entry["detail"] == "handoff_pending"
                    } else {
                        entry["reason"] == "role_needed" && entry["detail"] == role
                    }
            },
            2 * MINUTE,
        )
        .await;
    assert_eq!(start["mode"], "applied", "{start}");
    assert_eq!(start["target_id"], TARGET_B, "{start}");
    assert_eq!(
        start["operation_id"],
        format!("sched-role-{role}-{TARGET_B}").as_str(),
        "{start}"
    );
    assert!(start.get("run_id").is_none_or(Value::is_null), "{start}");
    fleet.started_session(&start).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn hs2_1nj04q_the_loop_starts_a_dispatcher_and_an_integrator_on_a_remote_target() {
    let mut fleet = Fleet::new().await;
    // A person set target B's pool up for dispatcher authority earlier; the integrator
    // who did it has since left (its lease is long expired).
    let earlier = now_s() - 7200;
    let coordinator = pool(&fleet);
    coordinator
        .register(
            Role::Integrator,
            "person-root",
            "root-1",
            earlier,
            60,
            ReturnRoute::HumanRelay,
        )
        .unwrap();
    coordinator
        .set_authority(
            "person-root",
            "root-1",
            earlier,
            Authority::Dispatcher,
            true,
        )
        .unwrap();
    let orchestration = fleet.orchestration_ticket("Coordinate the synthetic consumer");
    let ticket = fleet.target_ticket(Side::Peer, "Synthetic consumer change");
    let defined = fleet
        .plan(&orchestration, Side::Peer, &ticket, json!([]))
        .await;
    let step = defined["step"]["step_id"].as_str().unwrap().to_owned();
    // B opts in to both role sessions and has room for them and a worker.
    fleet
        .policy(json!({
            "targets":[
                {"target_id":TARGET_A,"tools":[{"tool":TOOL_A}],"max_sessions":1},
                {"target_id":TARGET_B,"tools":[{"tool":TOOL_B}],"max_sessions":3}],
            "placement":{"automatic":true,"actor":super::e2e::ACTOR,"cooldown_minutes":0,
                "role_sessions":[{"target_id":TARGET_B,"roles":["dispatcher","integrator"]}]}
        }))
        .await;

    // 1. Ready work and no live dispatcher: the dispatcher is started on the peer,
    //    in its role, before the worker for the step.
    let (dispatcher, dispatcher_terminal, dispatcher_worker) =
        role_started(&fleet, "dispatcher").await;
    assert_eq!(dispatcher["role"], "dispatcher", "{dispatcher}");
    assert_eq!(dispatcher["state"], "running", "{dispatcher}");
    assert!(dispatcher["run_id"].is_null(), "{dispatcher}");
    let report = launch_report(&fleet.reports, &dispatcher_terminal);
    assert_eq!(
        report["env"]["HOTSHEET_SESSION_ROLE"], "dispatcher",
        "{report}"
    );
    assert!(
        fleet
            .drawer(Side::Peer)
            .await
            .iter()
            .any(|tab| tab["id"] == dispatcher_terminal.as_str())
    );
    let worker = placed(&fleet, &step, Side::Peer).await;
    let log = fleet.decisions().await;
    let order: Vec<_> = log
        .iter()
        .filter(|entry| entry["kind"] == "start" && entry["outcome"] == "started")
        .map(|entry| entry["reason"].as_str().unwrap_or_default().to_owned())
        .collect();
    assert_eq!(order, ["role_needed", "ready_work"], "{log:#?}");

    // 2. The dispatcher joins the pool; the orchestrator sees it joined, and no second
    //    dispatcher is ever started.
    coordinator
        .register(
            Role::Dispatcher,
            &dispatcher_worker,
            "dispatch-1",
            now_s(),
            3600,
            ReturnRoute::HumanRelay,
        )
        .unwrap();
    let dispatcher_id = dispatcher["session_id"].as_str().unwrap().to_owned();
    let synced = fleet
        .ok(
            "POST",
            &format!("/sessions/{dispatcher_id}/sync"),
            json!({"actor":Fleet::ai()}),
        )
        .await;
    assert!(synced["joined_at_ms"].as_u64().is_some(), "{synced}");
    // The worker takes up its ticket as in the loop scenario.
    works(&fleet, Side::Peer, &worker, &ticket).await;

    // 3. The worker hands its work off in the pool and finishes the ticket. The pool
    //    now shows a handoff and no integrator: the loop starts one.
    let assignment: Assignment = serde_json::from_value(json!({
        "id":"synthetic-assignment","worker":worker.worker,"generation":"synthetic-generation",
        "ticket":{"connection_id":fleet.connection(Side::Peer),"native_id":ticket.id.to_string()},
        "slug":ticket.slug,"base":"0".repeat(40),"phase":"handoff","commit":"b".repeat(40),
        "handoff":{"kind":"code","commit":"b".repeat(40)},
        "return_route":{"kind":"human_relay"}
    }))
    .unwrap();
    edit_pool(&fleet, |state| {
        state
            .assignments
            .insert(assignment.id.clone(), assignment.clone());
    });
    fleet.finish(Side::Peer, &ticket, &worker.worker).await;
    let (integrator, integrator_terminal, integrator_worker) =
        role_started(&fleet, "integrator").await;
    assert_eq!(integrator["role"], "integrator", "{integrator}");
    let report = launch_report(&fleet.reports, &integrator_terminal);
    assert_eq!(
        report["env"]["HOTSHEET_SESSION_ROLE"], "integrator",
        "{report}"
    );
    coordinator
        .register(
            Role::Integrator,
            &integrator_worker,
            "integrate-1",
            now_s(),
            3600,
            ReturnRoute::HumanRelay,
        )
        .unwrap();
    let integrator_id = integrator["session_id"].as_str().unwrap().to_owned();
    let synced = fleet
        .ok(
            "POST",
            &format!("/sessions/{integrator_id}/sync"),
            json!({"actor":Fleet::ai()}),
        )
        .await;
    assert!(synced["joined_at_ms"].as_u64().is_some(), "{synced}");

    // 4. Integrated: nothing is pending and no work is left. Every session idles out
    //    and is wound down at its host's safe point; the role sessions last.
    edit_pool(&fleet, |state| {
        state
            .assignments
            .get_mut("synthetic-assignment")
            .unwrap()
            .phase = hotsheet_cli::coordination::Phase::Done;
    });
    for (session, terminal) in [
        (&worker.session_id, &worker.terminal),
        (&dispatcher_id, &dispatcher_terminal),
        (&integrator_id, &integrator_terminal),
    ] {
        let stop = fleet
            .decided(
                "wind-down",
                |entry| {
                    entry["kind"] == "wind_down"
                        && entry["outcome"] == "wind_down_requested"
                        && entry["session_id"] == session.as_str()
                },
                4 * MINUTE,
            )
            .await;
        // HS2-4JQ41V: the integrator's wind-down has a reason of its own.
        let expected = if session == &integrator_id {
            "integrator_wound_down"
        } else {
            "idle"
        };
        assert_eq!(stop["reason"], expected, "{stop}");
        assert_eq!(fleet.session(session).await["state"], "stopped");
        assert!(
            !fleet
                .drawer(Side::Peer)
                .await
                .iter()
                .any(|tab| tab["id"] == terminal.as_str() && tab["alive"] == true)
        );
    }
    // One of each, whatever the loop saw in between, and nothing on the controller.
    let roles: Vec<_> = fleet
        .sessions()
        .await
        .iter()
        .map(|session| session["role"].as_str().unwrap_or_default().to_owned())
        .collect();
    assert_eq!(roles.len(), 3, "{roles:?}");
    for role in ["worker", "dispatcher", "integrator"] {
        assert_eq!(
            roles.iter().filter(|seen| *seen == role).count(),
            1,
            "{roles:?}"
        );
    }
    assert!(fleet.drawer(Side::Controller).await.is_empty());
    fleet.shut_down().await;
}
