//! HS2-1NJ04Q: the scheduler's role sessions (HS2-TK9ADC, docs/26 §26.30) end to end.
//! The controller's own scheduling loop starts a dispatcher on target B, on the peer
//! over mTLS, before its worker; sees it join B's coordination pool; starts an
//! integrator once work is handed off there; and winds all of them down when nothing
//! is left. Where the test plays an agent (joining the pool, claiming, handing off,
//! integrating), it does what the role skills do, on the peer's own checkout.
use super::e2e::{Fleet, Side, TARGET_A, TARGET_B, TOOL_A, TOOL_B, edit_pool_state};
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

/// Rewrite B's pool as the integrator's own commands would leave it (HS2-M2KQPA: locked).
fn edit_pool(fleet: &Fleet, change: impl FnOnce(&mut State)) {
    edit_pool_state(&fleet.b_project, change);
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

/// HS2-ESVYQH: the worker lets its claim go on the peer, as a handoff does.
async fn release(fleet: &Fleet, ticket: &hotsheet_model::Ticket, worker: &str) {
    let (status, body) = super::orchestration::api(
        fleet.side_router(Side::Peer),
        "POST",
        &format!("/tickets/{}/release", ticket.id),
        json!({"worker": worker}),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

/// HS2-ESVYQH: the request the peer recorded on the worker's hosted session: its kind
/// and the return it names. The Codex-like fake takes no push, so the record is what
/// the session would be told.
async fn request_on_peer(fleet: &Fleet, placed: &super::e2e_loop::Placed) -> (String, String) {
    let operation = fleet.session(&placed.session_id).await["operation_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let hosted = hotsheet_ticketing::orchestration::HostedSessionRegistry::new(&fleet.peer_home)
        .get(&fleet.b_checkout.id, &operation)
        .unwrap()
        .expect("the peer hosts the worker");
    let request = hosted.request.expect("a request was recorded");
    (
        request.kind.code().to_owned(),
        request.commit.unwrap_or_default(),
    )
}

/// HS2-ESVYQH: the peer's hosted feed cursor now, and then the first page after it
/// that says the pool changed, as the controller's follower reads it.
async fn feed_cursor(fleet: &Fleet) -> u64 {
    let feed = format!("/checkouts/{}/target-events", fleet.b_checkout.id);
    let (status, page) =
        super::orchestration::api(fleet.peer_api.clone(), "GET", &feed, Value::Null, true).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    page["cursor"].as_u64().unwrap()
}

async fn pool_changed_after(fleet: &Fleet, mut cursor: u64) {
    let deadline = std::time::Instant::now() + MINUTE;
    loop {
        let path = format!(
            "/checkouts/{}/target-events?since={cursor}&wait_ms=8000",
            fleet.b_checkout.id
        );
        let (status, page) =
            super::orchestration::api(fleet.peer_api.clone(), "GET", &path, Value::Null, true)
                .await;
        assert_eq!(status, StatusCode::OK, "{page}");
        cursor = page["cursor"].as_u64().unwrap();
        if page["pool_changed"] == true {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "no pool change: {page}"
        );
    }
}

/// HS2-ESVYQH: the integrator's `## Review return` note on the peer's ticket, naming
/// the assignment and its commit, and the return itself, as `coordination
/// review-return` records it. Returns the note id.
fn review_return(
    fleet: &Fleet,
    ticket: &hotsheet_model::Ticket,
    assignment: &str,
    commit: &str,
) -> String {
    let store = hotsheet_ticketing::FsStore::open(&fleet.b_store).unwrap();
    let note = hotsheet_model::Ulid::new();
    hotsheet_ticketing::ops::add_note(
        &store,
        &ticket.id,
        note,
        hotsheet_model::Timestamp::from_datetime(time::OffsetDateTime::now_utc()),
        hotsheet_model::NoteKind::Regular,
        format!("## Review return\n{assignment} {commit}\nCorrect the synthetic behaviour."),
    )
    .unwrap();
    pool(fleet)
        .review_return(
            "integrator-1",
            "integrate-1",
            time::OffsetDateTime::now_utc(),
            assignment,
            &note.to_string(),
        )
        .unwrap();
    note.to_string()
}

/// HS2-ESVYQH (docs/26 §26.24): a scheduler-placed worker on the peer hands off twice
/// and is returned twice, in a dispatcher-authority pool whose dispatcher has gone
/// and whose integrator alone is live. Each return changes the peer's pool, which
/// wakes the controller's loop through the peer's target feed; the loop reads the
/// return from the peer's hosted session listing over mTLS, claims the ticket for the
/// same session again and tells it which return to correct. The step stays in
/// progress through both rounds, and only completing the ticket finishes it.
#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn hs2_esvyqh_a_returned_handoff_is_claimed_again_for_its_session_each_round() {
    const FIRST: &str = "1111111111111111111111111111111111111111";
    const SECOND: &str = "2222222222222222222222222222222222222222";
    let mut fleet = Fleet::new().await;
    let coordinator = pool(&fleet);
    let now = now_s();
    coordinator
        .register(
            Role::Integrator,
            "integrator-1",
            "integrate-1",
            now,
            3600,
            ReturnRoute::HumanRelay,
        )
        .unwrap();
    coordinator
        .set_authority(
            "integrator-1",
            "integrate-1",
            now,
            Authority::Dispatcher,
            true,
        )
        .unwrap();
    coordinator
        .register(
            Role::Dispatcher,
            "dispatcher-1",
            "dispatch-1",
            now,
            3600,
            ReturnRoute::HumanRelay,
        )
        .unwrap();
    let orchestration = fleet.orchestration_ticket("Coordinate the synthetic consumer");
    let ticket = fleet.target_ticket(Side::Peer, "Synthetic consumer change");
    let defined = fleet
        .plan(&orchestration, Side::Peer, &ticket, json!([]))
        .await;
    let step = defined["step"]["step_id"].as_str().unwrap().to_owned();
    // No role sessions: the pool's own dispatcher and integrator are the people's.
    // A returned correction nobody assigns is claimed at once.
    fleet
        .policy(json!({
            "placement":{"automatic":true,"actor":super::e2e::ACTOR,"cooldown_minutes":0,
                "dispatcher_wait_minutes":0,
                "role_sessions":[{"target_id":TARGET_A,"roles":[]},
                    {"target_id":TARGET_B,"roles":[]}]}
        }))
        .await;
    // The dispatcher assigns the ticket; the worker takes it up.
    let worker = placed(&fleet, &step, Side::Peer).await;
    works(&fleet, Side::Peer, &worker, &ticket).await;
    let hand_off = |id: &str, commit: &str| -> Assignment {
        serde_json::from_value(json!({
            "id":id,"worker":worker.worker,"generation":"synthetic-generation",
            "ticket":{"connection_id":fleet.connection(Side::Peer),"native_id":ticket.id.to_string()},
            "slug":ticket.slug,"base":"0".repeat(40),"phase":"handoff","commit":commit,
            "handoff":{"kind":"code","commit":commit},
            "return_route":{"kind":"human_relay"}
        }))
        .unwrap()
    };

    // Round one: handed off, the dispatcher leaves, the integrator returns it.
    let original = hand_off("original", FIRST);
    edit_pool(&fleet, |state| {
        state
            .assignments
            .insert(original.id.clone(), original.clone());
        state.dispatcher.as_mut().unwrap().expires_at = now_s() - 1;
    });
    release(&fleet, &ticket, &worker.worker).await;
    fleet
        .run_until(&worker.run_id, "released", |status| {
            status["state"] == "intervened"
        })
        .await;
    assert_eq!(fleet.readiness(&step).await["level"], "in_progress");
    let cursor = feed_cursor(&fleet).await;
    let first_note = review_return(&fleet, &ticket, "original", FIRST);
    // The return reaches the peer's hosted feed as a pool change.
    pool_changed_after(&fleet, cursor).await;
    let claimed = fleet
        .decided(
            "the first correction claimed",
            |entry| {
                entry["kind"] == "assignment"
                    && entry["detail"] == "review_returned"
                    && entry["outcome"] == "claimed"
            },
            2 * MINUTE,
        )
        .await;
    assert_eq!(claimed["run_id"], worker.run_id.as_str(), "{claimed}");
    assert_eq!(
        claimed["session_id"],
        worker.session_id.as_str(),
        "{claimed}"
    );
    let run = fleet
        .run_until(&worker.run_id, "claimed again", |status| {
            status["state"] == "dispatched"
        })
        .await;
    assert_eq!(run["worker_id"], worker.worker.as_str(), "{run}");
    assert_eq!(
        request_on_peer(&fleet, &worker).await,
        ("returned".to_owned(), first_note.clone())
    );
    assert_eq!(fleet.readiness(&step).await["level"], "in_progress");

    // Round two: the correction is handed off and returned too.
    let correction = hand_off("correction-1", SECOND);
    edit_pool(&fleet, |state| {
        state
            .assignments
            .insert(correction.id.clone(), correction.clone());
    });
    release(&fleet, &ticket, &worker.worker).await;
    fleet
        .run_until(&worker.run_id, "released again", |status| {
            status["state"] == "intervened"
        })
        .await;
    let second_note = review_return(&fleet, &ticket, "correction-1", SECOND);
    // The decision is logged once the pass claimed and told the session.
    let first_at = claimed["at_ms"].as_u64().unwrap();
    fleet
        .decided(
            "the second correction claimed",
            |entry| {
                entry["kind"] == "assignment"
                    && entry["detail"] == "review_returned"
                    && entry["outcome"] == "claimed"
                    && entry["at_ms"].as_u64().is_some_and(|at| at > first_at)
            },
            2 * MINUTE,
        )
        .await;
    fleet
        .run_until(&worker.run_id, "claimed a second time", |status| {
            status["state"] == "dispatched"
        })
        .await;
    assert_eq!(
        request_on_peer(&fleet, &worker).await,
        ("returned".to_owned(), second_note.clone())
    );
    let claims = fleet
        .decisions()
        .await
        .into_iter()
        .filter(|entry| {
            entry["kind"] == "assignment"
                && entry["detail"] == "review_returned"
                && entry["outcome"] == "claimed"
        })
        .count();
    assert_eq!(claims, 2);
    assert_eq!(fleet.readiness(&step).await["level"], "in_progress");

    // Integrated and completed: the run and the step finish; one session and one run
    // served every round.
    fleet.finish(Side::Peer, &ticket, &worker.worker).await;
    fleet
        .run_until(&worker.run_id, "completed", |status| {
            status["state"] == "completed"
        })
        .await;
    fleet.step_until(&step, "finished").await;
    assert_eq!(fleet.runs().await.len(), 1);
    assert_eq!(fleet.sessions().await.len(), 1);
    fleet.shut_down().await;
}

/// HS2-GK6HC8: a dispatcher that cannot start because every AI tool it may run is
/// signed out on the home's machine (the peer) is reported with the tool named, as a
/// worker step is; the watch of that machine asks it to re-check sign-in while the role
/// is held (HS2-P2TCQS), so signing in there starts the dispatcher.
#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn hs2_gk6hc8_a_dispatcher_held_by_a_signed_out_tool_names_it_and_starts_at_sign_in() {
    let mut fleet = Fleet::signed_out_rechecked(Side::Peer, Duration::from_secs(1)).await;
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
    let orchestration = fleet.orchestration_ticket("Coordinate the synthetic sign-in");
    let ticket = fleet.target_ticket(Side::Peer, "Synthetic change behind a sign-in");
    fleet
        .plan(&orchestration, Side::Peer, &ticket, json!([]))
        .await;
    fleet
        .policy(json!({
            "targets":[
                {"target_id":TARGET_A,"tools":[{"tool":TOOL_A}],"max_sessions":1},
                {"target_id":TARGET_B,"tools":[{"tool":TOOL_B}],"max_sessions":3}],
            "placement":{"automatic":true,"actor":super::e2e::ACTOR,"cooldown_minutes":0,
                "role_sessions":[{"target_id":TARGET_B,"roles":["dispatcher","integrator"]}]}
        }))
        .await;

    let detail = format!("tool_signed_out:{TOOL_B}");
    let held = fleet
        .decided(
            "the dispatcher held by the signed-out tool",
            |entry| {
                entry["kind"] == "role_unplaced"
                    && entry["reason"] == "dispatcher_not_started"
                    && entry["detail"] == detail.as_str()
            },
            2 * MINUTE,
        )
        .await;
    assert_eq!(held["target_id"], TARGET_B, "{held}");
    assert!(held.get("retry_at_ms").is_none_or(Value::is_null), "{held}");
    let planned = fleet
        .ok("POST", "/scheduler/plan", json!({"actor":Fleet::ai()}))
        .await;
    assert!(
        planned["roles_unplaced"]
            .as_array()
            .is_some_and(|roles| roles.iter().any(|role| role["role"] == "dispatcher"
                && role["reason"] == "dispatcher_not_started"
                && role["detail"] == detail.as_str())),
        "{planned}"
    );
    assert!(fleet.sessions().await.is_empty());

    // Signed in on the peer: its next re-check sees it and the dispatcher starts.
    fleet.sign_in(true);
    let (dispatcher, _, _) = role_started(&fleet, "dispatcher").await;
    assert_eq!(dispatcher["tool"], TOOL_B, "{dispatcher}");
    fleet.shut_down().await;
}
