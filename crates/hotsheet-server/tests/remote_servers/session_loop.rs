//! HS2-DG0YPA, HS2-T16SK0, HS2-NPTKS8, HS2-R7QH35: the automatic orchestration loop
//! on a real server never doubles a run's session, notices work no target dispatcher
//! will assign, replaces a session that never joins its role, and tells a refilled
//! session its ticket. Sessions are real PTYs running the workspace fake agent; no AI
//! tool is launched and nothing leaves loopback.
use super::orchestration::api;
use super::session_work::{TARGET, World};
use super::*;

/// The project's own connection, where its orchestration and target tickets live.
fn connection(world: &World) -> String {
    world.checkout.sources[0].connection_id.clone()
}

/// Dispatch a direct-mode run of `work` for `orchestration` as the granted actor.
async fn dispatch(
    world: &World,
    orchestration: &hotsheet_model::Ticket,
    work: &hotsheet_model::Ticket,
    operation: &str,
) -> String {
    let (status, run) = world
        .call(
            "POST",
            "/runs",
            json!({"ticket":{"native_id":orchestration.slug},"target_id":TARGET,
                "connection_id":connection(world),"native_id":work.slug,
                "operation_id":operation,"worker_id":"worker-a",
                "actor":{"role":"ai","id":"scheduler-1"}}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{run}");
    run["run_id"].as_str().unwrap().to_owned()
}

async fn start_for_run(
    world: &World,
    operation: &str,
    run_id: &str,
    replaces: Option<&Value>,
) -> (StatusCode, Value) {
    let mut body = json!({"target_id":TARGET,"operation_id":operation,
        "tool":super::session_work::REPORT_TOOL,"run_id":run_id,
        "actor":{"role":"ai","id":"scheduler-1"}});
    if let Some(replaced) = replaces {
        body["replaces"] = replaced.clone();
    }
    world.call("POST", "/sessions", body).await
}

/// HS2-DG0YPA: a second session for a run whose session still runs is refused with a
/// typed code, repeatedly, and launches nothing; once the first session's terminal is
/// killed the replacement is admitted, and the run has one live session throughout.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_dg0ypa_a_held_run_refuses_a_second_session_until_its_holder_is_gone() {
    let actions = ["tickets_read", "sessions_manage", "work_start"];
    let world = World::new(&actions, "", "").await;
    world.allow(&actions, json!({})).await;
    let orchestration = world.ticket("Coordinate the synthetic work");
    let work = world.ticket("Synthetic work");
    let run_id = dispatch(&world, &orchestration, &work, "run-held").await;

    let (status, first) = start_for_run(&world, "op-first", &run_id, None).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(first["state"], "running", "{first}");
    for _ in 0..2 {
        let (status, refused) = start_for_run(&world, "op-second", &run_id, None).await;
        assert_eq!(status, StatusCode::CONFLICT, "{refused}");
        assert_eq!(refused["code"], "session_run_held", "{refused}");
    }
    assert_eq!(super::sessions::drawer(&world.router).await.len(), 1);
    let (_, records) = world.call("GET", "/sessions", Value::Null).await;
    assert_eq!(records.as_array().unwrap().len(), 1, "{records}");

    // The holder's terminal is killed from outside: the next start learns the loss
    // from the host before it decides, and takes the run.
    let killed = api(
        world.router.clone(),
        "DELETE",
        &format!(
            "/terminals/{}",
            first["hosted"]["session_id"].as_str().unwrap()
        ),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(killed.0, StatusCode::NO_CONTENT);
    let (status, second) =
        start_for_run(&world, "op-second", &run_id, Some(&first["session_id"])).await;
    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(second["state"], "running", "{second}");
    assert_eq!(second["replaces"], first["session_id"]);
    let (_, records) = world.call("GET", "/sessions", Value::Null).await;
    let records = records.as_array().unwrap();
    assert_eq!(
        records
            .iter()
            .map(|record| (record["operation_id"].clone(), record["state"].clone()))
            .collect::<Vec<_>>(),
        [
            (json!("op-first"), json!("lost")),
            (json!("op-second"), json!("running"))
        ]
    );
    assert_eq!(super::sessions::drawer(&world.router).await.len(), 1);
}

async fn decisions(world: &World) -> Vec<Value> {
    let mut entries = Vec::new();
    let mut after = 0;
    loop {
        let page = world
            .call(
                "POST",
                "/scheduler/decisions",
                json!({"after": after, "limit": 200}),
            )
            .await
            .1;
        entries.extend(page["entries"].as_array().unwrap().iter().cloned());
        after = page["cursor"].as_u64().unwrap();
        if page["more"] != true {
            return entries;
        }
    }
}

/// Waits, woken by the decision log changing, for the first decision `wanted`
/// accepts, and returns it with the whole log.
async fn decided(
    world: &World,
    what: &str,
    wanted: impl Fn(&Value) -> bool,
    within: std::time::Duration,
) -> (Value, Vec<Value>) {
    let directory = world.home.join("orchestration-scheduler-decisions");
    std::fs::create_dir_all(&directory).unwrap();
    let waiter = hotsheet_jobs::ChangeWaiter::new(&directory).unwrap();
    let deadline = std::time::Instant::now() + within;
    loop {
        let log = decisions(world).await;
        if let Some(found) = log.iter().find(|entry| wanted(entry)) {
            return (found.clone(), log);
        }
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        assert!(!remaining.is_zero(), "no {what}: {log:?}");
        tokio::task::block_in_place(|| waiter.wait(remaining));
    }
}

const LOOP_ACTIONS: [&str; 4] = [
    "tickets_read",
    "gates_observe",
    "sessions_manage",
    "work_start",
];

/// A confirmed step of `work`, planned for `orchestration`.
async fn plan_step(
    world: &World,
    orchestration: &hotsheet_model::Ticket,
    work: &hotsheet_model::Ticket,
) -> String {
    let (status, defined) = world
        .call(
            "POST",
            "/readiness/define",
            json!({"actor":{"role":"ai","id":"scheduler-1"},
                "ticket":{"native_id":orchestration.slug},"target_id":TARGET,
                "connection_id":connection(world),"native_id":work.slug}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{defined}");
    assert_eq!(defined["level"], "confirmed", "{defined}");
    defined["step"]["step_id"].as_str().unwrap().to_owned()
}

/// HS2-T16SK0: on a real server whose target has no coordination pool, the loop
/// places a session for a step and reports that no dispatcher will assign its run;
/// `explain` says the same with the time; at that time, woken by its own named retry,
/// the loop claims the ticket for the session's worker id.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_t16sk0_the_loop_reports_a_run_nobody_assigns_and_claims_it_for_its_session() {
    let world = World::new(&LOOP_ACTIONS, "", "").await;
    let orchestration = world.ticket("Coordinate the synthetic work");
    let work = world.ticket("Synthetic work nobody dispatches");
    let step_id = plan_step(&world, &orchestration, &work).await;
    world
        .allow(
            &LOOP_ACTIONS,
            json!({"wind_down":{"idle_minutes":60,"keep_warm":0},
                "placement":{"automatic":true,"actor":"scheduler-1","cooldown_minutes":0,
                    "dispatcher_wait_minutes":1}}),
        )
        .await;
    let minute = std::time::Duration::from_secs(60);
    let (start, _) = decided(
        &world,
        "started session",
        |entry| entry["kind"] == "start" && entry["outcome"] == "started",
        minute,
    )
    .await;
    let run_id = start["run_id"].as_str().unwrap().to_owned();
    // The session takes up its role, as the worker skill does: it registers in the
    // pool, where there is no dispatcher and no integrator.
    let (_, sessions) = world.call("GET", "/sessions", Value::Null).await;
    let worker = sessions[0]["hosted"]["worker_id"]
        .as_str()
        .unwrap()
        .to_owned();
    super::session_work::join_pool(&world, &worker);
    let (reported, _) = decided(
        &world,
        "unassigned run",
        |entry| entry["kind"] == "assignment" && entry["reason"] == "no_live_dispatcher",
        minute,
    )
    .await;
    assert_eq!(reported["run_id"], run_id.as_str());
    assert_eq!(reported["session_id"], start["session_id"]);
    assert_eq!(reported["step_id"], step_id.as_str());
    let claim_at = reported["retry_at_ms"].as_u64().unwrap();
    let (status, explained) = world
        .call(
            "POST",
            "/scheduler/explain",
            json!({"step_id":step_id,"actor":{"role":"ai","id":"scheduler-1"}}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{explained}");
    assert_eq!(explained["standing"], "running", "{explained}");
    assert_eq!(explained["reason"], "no_live_dispatcher", "{explained}");
    assert_eq!(
        explained["assignment"]["dispatcher_live"], false,
        "{explained}"
    );
    assert_eq!(
        explained["assignment"]["integrator_live"], false,
        "{explained}"
    );
    assert_eq!(explained["retry_at_ms"], claim_at, "{explained}");

    let (claimed, log) = decided(
        &world,
        "claim for the session",
        |entry| entry["kind"] == "assignment" && entry["outcome"] == "claimed",
        2 * minute,
    )
    .await;
    assert_eq!(claimed["trigger"], "retry", "{claimed}");
    assert!(claimed["at_ms"].as_u64().unwrap() >= claim_at, "{claimed}");
    let (_, run) = world
        .call("GET", &format!("/runs/{run_id}"), Value::Null)
        .await;
    let (_, sessions) = world.call("GET", "/sessions", Value::Null).await;
    assert_eq!(run["state"], "dispatched", "{run}");
    assert_eq!(run["worker_id"], worker.as_str(), "{run}");
    let ticket = ops_ticket(&world, &work);
    assert_eq!(ticket.claimed_by.as_deref(), Some(worker.as_str()));
    assert!(sessions[0]["joined_at_ms"].is_u64(), "{sessions}");
    // One session, one run, and no pass on a timer: a handful of logged passes.
    assert_eq!(sessions.as_array().unwrap().len(), 1, "{sessions}");
    let ticks = log.iter().filter(|entry| entry["kind"] == "tick").count();
    assert!(ticks <= 12, "{ticks} logged passes: {log:?}");
}

fn ops_ticket(world: &World, ticket: &hotsheet_model::Ticket) -> hotsheet_model::Ticket {
    FsStore::open(&world.project_store)
        .unwrap()
        .read_ticket(&ticket.id)
        .unwrap()
}
