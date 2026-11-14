//! HS2-S13719: the progress and cost summary through the real authenticated service,
//! the stdio MCP tool and the owned CLI process. Synthetic stores, ids and amounts.
use super::orchestration::api;
use super::*;
use hotsheet_ticketing::checkouts::CheckoutRegistry;
use hotsheet_ticketing::metrics::{self, UsageEvent};
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
async fn hs2_s13719_http_mcp_and_cli_read_the_same_progress_and_cost_summary() {
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
    let orchestration = ticket(&store, "Coordinate the synthetic summary");
    let unrelated = ticket(&store, "Another synthetic coordination");
    let work = ticket(&store, "Synthetic work to measure");
    let router = app(host.clone());
    let base = format!("/checkouts/{}/orchestration", checkout.id);
    let summary = format!("{base}/summary");
    // The summary needs the bearer like every other route, and starts empty.
    assert_eq!(
        api(router.clone(), "GET", &summary, Value::Null, false)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, empty) = api(router.clone(), "GET", &summary, Value::Null, true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(empty["owner_checkout_id"], checkout.id);
    assert_eq!(empty["attribution"], "work_item_earliest_run");
    assert_eq!(empty["runs"], json!([]));
    assert_eq!(empty["totals"]["runs"], 0);
    assert_eq!(empty["totals"]["cost"]["items_unknown"], 0);

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
    let expires = u64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap()
        + 3_600_000;
    super::orchestration::remember_rules(
        router.clone(),
        &format!("{base}/targets/{TARGET}"),
        json!({"actor_id":"scheduler-1","actions":["tickets_read","work_start"],
            "expires_at_ms":expires,"actor":{"role":"human","id":"maintainer"}}),
    )
    .await;
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
    let run = call(json!({"action":"run_dispatch","target_id":TARGET,
        "ticket":{"native_id":orchestration.slug},"connection_id":connection,
        "native_id":work.slug,"operation_id":"op-summary","worker_id":"mcp-worker"}));
    assert_eq!(run["state"], "dispatched");

    // Dispatched, nothing recorded as used yet: the cost is unknown, not zero.
    let before = call(json!({"action":"summary"}));
    assert_eq!(before["totals"]["runs"], 1);
    assert_eq!(before["runs"][0]["run_id"], run["run_id"]);
    assert_eq!(
        before["runs"][0]["lead_time_ms"],
        json!({"status":"unknown","reason":"run_unfinished"})
    );
    assert_eq!(before["runs"][0]["queue_wait_ms"]["status"], "known");
    assert_eq!(
        before["runs"][0]["ci"],
        json!({"status":"unknown","reason":"no_gate_linked"})
    );
    assert_eq!(
        before["work_items"][0]["cost"],
        json!({"status":"unknown","reason":"no_usage_recorded"})
    );
    assert_eq!(before["totals"]["cost"]["complete"], false);

    // The usage records the analytics dashboards read (docs/14), for this ticket and
    // for unrelated work.
    for (hour, ticket, cost_usd) in [
        (1, Some(&work), 1.5),
        (2, Some(&work), 0.25),
        (3, None, 9.0),
    ] {
        metrics::record(
            &store,
            &UsageEvent {
                ts: format!("2026-10-15T0{hour}:00:00Z"),
                tool: "claude".into(),
                model: Some("model-a".into()),
                tokens_in: 10,
                tokens_out: 2,
                cost_usd: Some(cost_usd),
                ticket: ticket.map(|ticket| ticket.id.to_string()),
                session: Some("session-a".into()),
            },
        )
        .unwrap();
    }
    let cost = json!({"status":"known","value":{"cost_usd":1.75,"events":2,"unpriced_events":0}});
    let (status, over_http) = api(router.clone(), "GET", &summary, Value::Null, true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(over_http["work_items"][0]["cost"], cost);
    assert_eq!(over_http["work_items"][0]["native_id"], work.id.to_string());
    assert_eq!(over_http["work_items"][0]["attributed_target_id"], TARGET);
    assert_eq!(over_http["totals"]["cost"]["known_usd"], 1.75);
    assert_eq!(over_http["totals"]["cost"]["complete"], true);
    assert_eq!(over_http["targets"][0]["target_id"], TARGET);
    assert_eq!(over_http["targets"][0]["cost"]["known_usd"], 1.75);
    // Identifiers and metrics only.
    assert!(!over_http.to_string().contains("Synthetic"));

    // MCP and the bare CLI answer the same, whole and narrowed to one ticket.
    let over_mcp = call(json!({"action":"summary"}));
    let plain = ["-C", source.to_str().unwrap(), "orchestration"];
    let over_cli = cli_named(&home, &plain, &["summary", "--project", &checkout.id]);
    for other in [&over_mcp, &over_cli] {
        for part in ["totals", "targets", "work_items"] {
            assert_eq!(other[part], over_http[part], "{part}");
        }
        assert_eq!(other["runs"][0]["run_id"], run["run_id"]);
    }
    let own = orchestration.id.to_string();
    let other = unrelated.id.to_string();
    assert_eq!(
        call(json!({"action":"summary","ticket":{"native_id":own,"connection_id":connection}}))["work_items"],
        over_http["work_items"]
    );
    assert_eq!(
        call(json!({"action":"summary","ticket":{"native_id":other}}))["totals"]["runs"],
        0
    );
    assert_eq!(
        cli_named(
            &home,
            &plain,
            &[
                "summary",
                "--project",
                &checkout.id,
                "--ticket",
                &own,
                "--connection-id",
                &connection
            ]
        )["totals"]["cost"]["known_usd"],
        1.75
    );
    assert_eq!(
        cli_named(
            &home,
            &plain,
            &["summary", "--project", &checkout.id, "--ticket", &other]
        )["runs"],
        json!([])
    );
    let (status, narrowed) = api(
        router.clone(),
        "GET",
        &format!("{summary}?ticket={other}"),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(narrowed["totals"]["work_items"], 0);
    // A malformed narrowing is refused before any request, and by the service.
    for bad in [
        json!({"native_id":"a/b"}),
        json!({"connection_id":connection}),
        json!("x"),
    ] {
        mcp_rejected(
            &home,
            &backend,
            "hotsheet_targets",
            arguments(json!({"action":"summary","ticket":bad})),
        );
    }
    assert_eq!(
        api(
            router.clone(),
            "GET",
            &format!("{summary}?ticket=a%2Fb"),
            Value::Null,
            true
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert!(
        cli_failure(
            &home,
            &plain,
            &["summary", "--project", &checkout.id, "--ticket", "a%2Fb"]
        )
        .contains("invalid_request")
    );
    let _ = stop.send(());
    tokio::time::timeout(std::time::Duration::from_secs(10), task)
        .await
        .unwrap()
        .unwrap();
}
