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
