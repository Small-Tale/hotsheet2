//! HS2-J89JC7: the orchestration timeline through the real authenticated service, the
//! stdio MCP tool and the owned CLI process, including the version long poll.
//! Synthetic stores and ids.
use super::orchestration::api;
use super::*;
use hotsheet_ticketing::checkouts::CheckoutRegistry;
use hotsheet_ticketing::{NewTicket, ops};

const TARGET: &str = "01ARZ3NDEKTSV4RRFFQ69G5FS1";
const NOW: &str = "2026-10-15T00:00:00Z";

fn ticket(store: &FsStore, title: &str) -> hotsheet_model::Ticket {
    ops::create(
        store,
        hotsheet_model::Ulid::new(),
        "TST",
        NOW.into(),
        NewTicket {
            title: title.into(),
            ..Default::default()
        },
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hs2_j89jc7_http_mcp_and_cli_read_the_same_timeline_and_wait_by_version() {
    assert!(hotsheet_aitools::test_permission_vault_available());
    hotsheet_aitools::enable_test_permission_vault();
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let source = root.path().join("store");
    let host = state(&source, &home);
    let project = root.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let checkout = CheckoutRegistry::new(home.join("checkouts.json"))
        .register(&project, None, None, vec![source.clone()])
        .unwrap();
    let connection = checkout.sources[0].connection_id.clone();
    let store = FsStore::open(&source).unwrap();
    let goal = ticket(&store, "Coordinate the synthetic release");
    let first = ticket(&store, "First synthetic work");
    let second = ticket(&store, "Second synthetic work");
    let router = app(host.clone());
    let base = format!("/checkouts/{}/orchestration", checkout.id);
    let timeline = format!("{base}/timeline");
    assert_eq!(
        api(router.clone(), "GET", &timeline, Value::Null, false)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, empty) = api(router.clone(), "GET", &timeline, Value::Null, true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(empty["phase"], "no_projects");
    assert_eq!(empty["waves"], json!([]));
    assert_eq!(empty["kick_off"]["start_blocked"], "no_projects");

    let added = api(
        router.clone(),
        "POST",
        &format!("{base}/targets"),
        json!({"target_id":TARGET,"label":"Self","expected_revision":null}),
        true,
    )
    .await
    .1;
    let candidate = api(
        router.clone(),
        "POST",
        &format!("{base}/inspect"),
        json!({"destination":{"kind":"local","checkout_id":checkout.id}}),
        true,
    )
    .await
    .1;
    api(
        router.clone(),
        "PUT",
        &format!("{base}/targets/{TARGET}/binding"),
        json!({"target":{"owner_checkout_id":checkout.id,"target_id":TARGET},
            "expected_manifest_revision":added["revision"],"expected_binding_revision":0,
            "previous_destination":null,"destination":candidate["destination"]}),
        true,
    )
    .await;
    // HS2-XHQ1EH: binding is the consent to read; dispatching asks a person unless a
    // remembered rule allows starting work there.
    let (status, _) = api(
        router.clone(),
        "POST",
        &format!("{base}/permission-rules"),
        json!({"action":"work_start","target_id":TARGET,
            "actor":{"role":"human","id":"maintainer"}}),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, declared) = api(router.clone(), "GET", &timeline, Value::Null, true).await;
    assert_eq!(declared["phase"], "no_plan");
    assert_eq!(declared["targets"][0]["label"], "Self");
    assert_eq!(declared["targets"][0]["location"], "local");

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        axum::serve(listener, app(host))
            .with_graceful_shutdown(async move {
                let _ = stopped.await;
            })
            .await
            .unwrap();
    });
    let backend = ["--server", origin.as_str(), "--secret", SECRET];
    let arguments = |mut arguments: Value| {
        arguments["checkout"] = json!(checkout.id);
        arguments["actor_role"] = json!("ai");
        arguments["actor_id"] = json!("scheduler-1");
        arguments
    };
    let call = |body: Value| mcp_named(&home, &backend, "hotsheet_targets", arguments(body));
    let define = |work: &hotsheet_model::Ticket, after: Option<&hotsheet_model::Ticket>| {
        let prerequisites = after.map_or_else(
            || json!([]),
            |after| {
                json!([{"kind":"ticket","target_id":TARGET,"connection_id":connection,
                    "native_id":after.id.to_string(),"gates":"none"}])
            },
        );
        call(json!({"action":"readiness_define","target_id":TARGET,
            "ticket":{"native_id":goal.id.to_string(),"connection_id":connection},
            "connection_id":connection,"native_id":work.id.to_string(),
            "prerequisites":prerequisites}))["step"]["step_id"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let a = define(&first, None);
    let b = define(&second, Some(&first));
    let (status, proposed) = api(router.clone(), "GET", &timeline, Value::Null, true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(proposed["phase"], "proposed");
    assert_eq!(proposed["kick_off"]["start_blocked"], "policy_absent");
    assert_eq!(proposed["waves"][0]["kind"], "planned");
    assert_eq!(proposed["waves"][0]["items"][0]["step_id"], a);
    assert_eq!(proposed["waves"][0]["items"][0]["slug"], first.slug);
    assert_eq!(
        proposed["waves"][0]["items"][0]["title"],
        "First synthetic work"
    );
    assert_eq!(proposed["waves"][0]["items"][0]["goal_slug"], goal.slug);
    assert_eq!(proposed["waves"][0]["items"][0]["state"], "ready");
    assert_eq!(proposed["waves"][1]["ordinal"], 1);
    assert_eq!(proposed["waves"][1]["items"][0]["step_id"], b);
    assert_eq!(
        proposed["waves"][1]["items"][0]["state"],
        "waits_for_prerequisite"
    );
    assert_eq!(
        proposed["waves"][1]["items"][0]["waits_on"][0]["step_id"],
        a
    );

    // MCP and the bare CLI answer the same version and waves.
    let version = proposed["version"].as_str().unwrap().to_owned();
    let over_mcp = call(json!({"action":"timeline"}));
    let plain = ["-C", source.to_str().unwrap(), "orchestration"];
    let over_cli = cli_named(&home, &plain, &["timeline", "--project", &checkout.id]);
    // Estimates move with the clock; everything else is the same answer.
    let shape = |view: &Value| -> Vec<(String, Vec<(String, String)>)> {
        view["waves"]
            .as_array()
            .unwrap()
            .iter()
            .map(|wave| {
                (
                    format!("{}-{}", wave["kind"], wave["ordinal"]),
                    wave["items"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|item| (item["step_id"].to_string(), item["state"].to_string()))
                        .collect(),
                )
            })
            .collect()
    };
    for other in [&over_mcp, &over_cli] {
        assert_eq!(other["version"], proposed["version"]);
        assert_eq!(shape(other), shape(&proposed));
    }
    // The current version waits to the deadline; nothing changed.
    let started = std::time::Instant::now();
    let (_, same) = api(
        router.clone(),
        "GET",
        &format!("{timeline}?after={version}&wait_ms=400"),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(same["version"], proposed["version"]);
    assert!(started.elapsed() >= std::time::Duration::from_millis(350));
    // A dispatch ends a waiting read with the new version: the work is now running
    // and the second step is next.
    let waiting = {
        let router = router.clone();
        let path = format!("{timeline}?after={version}&wait_ms=15000");
        tokio::spawn(async move { api(router, "GET", &path, Value::Null, true).await })
    };
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let run = call(json!({"action":"run_dispatch","target_id":TARGET,
        "ticket":{"native_id":goal.slug},"connection_id":connection,
        "native_id":first.slug,"operation_id":"op-timeline","worker_id":"mcp-worker"}));
    assert_eq!(run["state"], "dispatched");
    let (status, changed) = tokio::time::timeout(std::time::Duration::from_secs(14), waiting)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status, StatusCode::OK);
    assert_ne!(changed["version"], proposed["version"]);
    assert_eq!(changed["phase"], "paused");
    let kinds: Vec<&str> = changed["waves"]
        .as_array()
        .unwrap()
        .iter()
        .map(|wave| wave["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["done", "working_now", "next", "then", "later"]);
    assert_eq!(changed["waves"][1]["items"][0]["state"], "running");
    assert_eq!(
        changed["waves"][1]["items"][0]["run"]["run_id"],
        run["run_id"]
    );
    assert_eq!(changed["waves"][2]["items"][0]["step_id"], b);
    // MCP waits the same way and answers at once for an older version.
    // A waiting read may wake on the first of the dispatch's events; later reads may
    // see more of them (and the target follower's), never the old version again.
    let over_mcp = call(json!({"action":"timeline","version":version,"wait_ms":15000}));
    assert_ne!(over_mcp["version"].as_str(), Some(version.as_str()));
    assert_eq!(over_mcp["phase"], "paused");
    let over_cli = cli_named(
        &home,
        &plain,
        &[
            "timeline",
            "--project",
            &checkout.id,
            "--after",
            &version,
            "--wait-seconds",
            "15",
        ],
    );
    assert_ne!(over_cli["version"].as_str(), Some(version.as_str()));
    assert_eq!(over_cli["waves"][1]["items"][0]["state"], "running");
    // A malformed version is refused before any request, and by the service.
    mcp_rejected(
        &home,
        &backend,
        "hotsheet_targets",
        arguments(json!({"action":"timeline","version":"a/b"})),
    );
    assert_eq!(
        api(
            router.clone(),
            "GET",
            &format!("{timeline}?after=a%2Fb&wait_ms=10"),
            Value::Null,
            true
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let _ = stop.send(());
    tokio::time::timeout(std::time::Duration::from_secs(10), task)
        .await
        .unwrap()
        .unwrap();
}
