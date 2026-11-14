//! HS2-W27N1D: Project Type and adding or removing projects through the real
//! authenticated service, the owned CLI process and the stdio MCP tool, which all call
//! the one shared workflow; and the change event a client follows instead of polling.
use super::orchestration::{api, configured_owner, shared_runtime};
use super::*;

async fn changes_after(router: axum::Router, cursor: u64, owner: &str) -> (u64, usize) {
    let (status, polled) = api(
        router,
        "GET",
        &format!("/ws/poll?since={cursor}&timeout_ms=0"),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{polled}");
    let count = polled["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event["kind"] == "orchestration_changed" && event["id"] == owner)
        .count();
    (polled["cursor"].as_u64().unwrap(), count)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hs2_w27n1d_cli_http_and_mcp_share_project_type_and_projects() {
    assert!(hotsheet_aitools::test_permission_vault_available());
    hotsheet_aitools::enable_test_permission_vault();
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let source = root.path().join("host-store");
    let memory = Arc::new(Memory::default());
    let host = state(&source, &home);
    let owner = configured_owner(&source, &home, &root.path().join("owner"));
    let api_project = configured_owner(&source, &home, &root.path().join("api"));
    let docs_project = configured_owner(&source, &home, &root.path().join("docs"));
    let host = host.with_orchestration_runtime(shared_runtime(&source, &home, &memory));
    let router = app(host.clone());
    let base = format!("/checkouts/{}/orchestration", owner.id);
    let manifest = std::path::Path::new(&owner.root).join(".hotsheet2/orchestration.json");
    let (mut cursor, _) = changes_after(router.clone(), 0, &owner.id).await;

    // HTTP, as the web client calls it: a read changes and announces nothing.
    let (status, shown) = api(
        router.clone(),
        "GET",
        &format!("{base}/project-type"),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(shown["project_type"], "software");
    assert!(!manifest.exists());
    let (next, announced) = changes_after(router.clone(), cursor, &owner.id).await;
    assert_eq!(announced, 0);
    cursor = next;
    let (status, set) = api(
        router.clone(),
        "PUT",
        &format!("{base}/project-type"),
        json!({"project_type":"orchestration","actor":{"role":"human"}}),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{set}");
    assert_eq!(
        (&set["declared"], &set["changed"]),
        (&json!(true), &json!(true))
    );
    assert!(manifest.exists());
    let (next, announced) = changes_after(router.clone(), cursor, &owner.id).await;
    assert_eq!(
        announced, 1,
        "a client following the owner learns of the switch"
    );
    cursor = next;
    // Adding is the consent; the web client acts as a person.
    let (status, added) = api(
        router.clone(),
        "POST",
        &format!("{base}/projects"),
        json!({"destination":{"kind":"local","checkout_id":api_project.id},"label":"API","actor":{"role":"human"}}),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{added}");
    assert_eq!(added["resolution"]["usable"], true);
    let api_target = added["target_id"].as_str().unwrap().to_owned();
    // The existing reads show it: the declaration and this machine's binding.
    let (_, declared) = api(
        router.clone(),
        "GET",
        &format!("{base}/targets"),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(declared["targets"][0]["label"], "API");
    let (_, binding) = api(
        router.clone(),
        "GET",
        &format!("{base}/targets/{api_target}/binding"),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(
        binding["destination"]["checkout_id"],
        api_project.id.as_str()
    );
    let (next, announced) = changes_after(router.clone(), cursor, &owner.id).await;
    assert_eq!(announced, 1);
    cursor = next;
    // A refused request announces nothing.
    let (status, _) = api(
        router.clone(),
        "PUT",
        &format!("{base}/project-type"),
        json!({"project_type":"software"}),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (next, announced) = changes_after(router.clone(), cursor, &owner.id).await;
    assert_eq!(announced, 0);
    cursor = next;
    // The off-box reach of add and remove is refused like any binding write.
    assert_eq!(
        api(
            router.clone(),
            "POST",
            &format!("{base}/projects"),
            json!({"destination":{"kind":"local","checkout_id":docs_project.id}}),
            false,
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );

    // The CLI runs the same workflows headlessly.
    let human = [
        "--actor-role",
        "human",
        "--actor-id",
        "maintainer",
        "-C",
        source.to_str().unwrap(),
        "orchestration",
    ];
    let shown = cli_named(&home, &human, &["project-type", "--project", &owner.id]);
    assert_eq!(
        (&shown["project_type"], &shown["active_projects"]),
        (&json!("orchestration"), &json!(1))
    );
    let docs = cli_named(
        &home,
        &human,
        &[
            "project",
            "add",
            &docs_project.id,
            "--project",
            &owner.id,
            "--label",
            "Docs",
        ],
    );
    assert_eq!(docs["added"], true);
    let again = cli_named(
        &home,
        &human,
        &["project", "add", &docs_project.id, "--project", &owner.id],
    );
    assert_eq!(
        (&again["added"], &again["target_id"]),
        (&json!(false), &docs["target_id"])
    );
    let docs_target = docs["target_id"].as_str().unwrap();
    let removed = cli_named(
        &home,
        &human,
        &["project", "remove", docs_target, "--project", &owner.id],
    );
    assert_eq!(
        (&removed["unbound"], &removed["removed"]),
        (&json!(true), &json!(true))
    );

    // The stdio MCP tool reads and switches the type, and cannot add a project.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let served = host.clone();
    let task = tokio::spawn(async move {
        axum::serve(listener, app(served))
            .with_graceful_shutdown(async move {
                let _ = stopped.await;
            })
            .await
            .unwrap();
    });
    let backend = ["--server", origin.as_str(), "--secret", SECRET];
    let call = |arguments: Value| mcp_named(&home, &backend, "hotsheet_targets", arguments);
    assert_eq!(
        call(json!({"action":"project_type","checkout":owner.id}))["active_projects"],
        1
    );
    mcp_rejected(
        &home,
        &backend,
        "hotsheet_targets",
        json!({"action":"project_type_set","checkout":owner.id,"project_type":"software"}),
    );
    mcp_rejected(
        &home,
        &backend,
        "hotsheet_targets",
        json!({"action":"project_add","checkout":owner.id}),
    );
    let back = call(json!({"action":"project_type_set","checkout":owner.id,
        "project_type":"software","keep_projects":true}));
    assert_eq!(
        (&back["project_type"], &back["active_projects"]),
        (&json!("software"), &json!(1))
    );
    // HS2-J3ZF27: a Software project keeps its projects but acts on none of them.
    // Every client reads the same typed refusal; reading stays allowed.
    let refused =
        json!({"allowed":false,"reason":"policy_denied","detail":"policy_project_software"});
    assert_eq!(
        call(
            json!({"action":"access_check","checkout":owner.id,"target_id":api_target,
            "for_actor":"worker-1","target_action":"work_start","operation_id":"op-1"})
        ),
        refused
    );
    let (status, checked) = api(
        router.clone(),
        "POST",
        &format!("{base}/targets/{api_target}/access"),
        json!({"actor_id":"worker-1","action":"work_start","operation_id":"op-1"}),
        true,
    )
    .await;
    assert_eq!((status, checked), (StatusCode::OK, refused.clone()));
    let checked = cli_named(
        &home,
        &human,
        &[
            "permission",
            "check",
            &api_target,
            "--project",
            &owner.id,
            "--actor",
            "worker-1",
            "--action",
            "work_start",
        ],
    );
    assert_eq!(checked, refused);
    let (_, read) = api(
        router.clone(),
        "POST",
        &format!("{base}/targets/{api_target}/access"),
        json!({"actor_id":"worker-1","action":"tickets_read"}),
        true,
    )
    .await;
    assert_eq!(read["basis"], "observation");
    let _ = stop.send(());
    tokio::time::timeout(std::time::Duration::from_secs(10), task)
        .await
        .unwrap()
        .unwrap();
    let (next, announced) = changes_after(router.clone(), cursor, &owner.id).await;
    assert!(announced >= 1, "the MCP switch went through the server");
    cursor = next;

    // HTTP removal, and the audit journal of every change by every client.
    let (status, removed) = api(
        router.clone(),
        "DELETE",
        &format!("{base}/projects/{api_target}"),
        json!({"actor":{"role":"human"}}),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{removed}");
    assert_eq!(changes_after(router.clone(), cursor, &owner.id).await.1, 1);
    let journal = cli_named(
        &home,
        &human,
        &["audit", "--project", &owner.id, "--limit", "50"],
    );
    let actions: Vec<_> = journal["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["action"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        actions,
        [
            "project_type_set",
            "project_add",
            "project_add",
            "project_remove",
            "project_type_set",
            "project_remove",
        ]
    );
}

/// Wait for the owner's `orchestration_changed` events after `cursor` with the long
/// poll, until one arrives or `wait_ms` passes; then count what arrived.
async fn announced_within(
    router: axum::Router,
    mut cursor: u64,
    owner: &str,
    wait_ms: u64,
) -> (u64, usize) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(wait_ms);
    let mut seen = 0;
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        let (status, polled) = api(
            router.clone(),
            "GET",
            &format!("/ws/poll?since={cursor}&timeout_ms={}", left.as_millis()),
            Value::Null,
            true,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{polled}");
        seen += polled["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| event["kind"] == "orchestration_changed" && event["id"] == owner)
            .count();
        cursor = polled["cursor"].as_u64().unwrap();
        if seen > 0 || left.is_zero() {
            return (cursor, seen);
        }
    }
}

/// HS2-SHHGC6: once a client reads an owner's orchestration settings, a change a CLI
/// process writes without the server (type, policy, a project's binding) is announced
/// as `orchestration_changed`, from file events and not a timer; a write that changes
/// nothing the settings show is not, and a change the server made is announced once.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hs2_shhgc6_a_cli_change_without_the_server_is_announced() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let source = root.path().join("host-store");
    let memory = Arc::new(Memory::default());
    let host = state(&source, &home);
    let owner = configured_owner(&source, &home, &root.path().join("owner"));
    let api_project = configured_owner(&source, &home, &root.path().join("api"));
    let host = host.with_orchestration_runtime(shared_runtime(&source, &home, &memory));
    let router = app(host.clone());
    let base = format!("/checkouts/{}/orchestration", owner.id);
    let (cursor, _) = changes_after(router.clone(), 0, &owner.id).await;
    // A client shows the settings: the read starts the watcher and announces nothing.
    let (status, _) = api(
        router.clone(),
        "GET",
        &format!("{base}/project-type"),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (cursor, quiet) = announced_within(router.clone(), cursor, &owner.id, 600).await;
    assert_eq!(quiet, 0);

    let human = [
        "--actor-role",
        "human",
        "--actor-id",
        "maintainer",
        "-C",
        source.to_str().unwrap(),
        "orchestration",
    ];
    // The type, switched by a CLI process without the server.
    let set = cli_named(
        &home,
        &human,
        &[
            "project-type",
            "--project",
            &owner.id,
            "--set",
            "orchestration",
        ],
    );
    assert_eq!(set["changed"], true);
    let (cursor, announced) = announced_within(router.clone(), cursor, &owner.id, 15_000).await;
    assert!(announced >= 1, "the CLI switch is announced");
    // The same switch again changes nothing the settings show.
    cli_named(
        &home,
        &human,
        &[
            "project-type",
            "--project",
            &owner.id,
            "--set",
            "orchestration",
        ],
    );
    let (cursor, repeated) = announced_within(router.clone(), cursor, &owner.id, 1_500).await;
    assert_eq!(repeated, 0, "an unchanged write is not announced");
    // A project added (declared and bound in this machine's home) by the CLI.
    cli_named(
        &home,
        &human,
        &[
            "project",
            "add",
            &api_project.id,
            "--project",
            &owner.id,
            "--label",
            "API",
        ],
    );
    let (cursor, announced) = announced_within(router.clone(), cursor, &owner.id, 15_000).await;
    assert!(announced >= 1, "the CLI add is announced");
    // The policy, set by the CLI.
    let policy = root.path().join("policy.json");
    std::fs::write(&policy, br#"{"machine":{"max_sessions":3}}"#).unwrap();
    cli_named(
        &home,
        &human,
        &[
            "policy",
            "set",
            "--project",
            &owner.id,
            "--file",
            policy.to_str().unwrap(),
        ],
    );
    let (cursor, announced) = announced_within(router.clone(), cursor, &owner.id, 15_000).await;
    assert!(announced >= 1, "the CLI policy is announced");
    // A change the server makes itself is announced by the server, and the watcher
    // does not add another for the same change.
    let (status, _) = api(
        router.clone(),
        "PUT",
        &format!("{base}/project-type"),
        json!({"project_type":"software","keep_projects":true,"actor":{"role":"human"}}),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (cursor, first) = announced_within(router.clone(), cursor, &owner.id, 5_000).await;
    assert_eq!(first, 1);
    let (_, more) = announced_within(router.clone(), cursor, &owner.id, 1_500).await;
    assert_eq!(more, 0, "announced once");
    host.begin_stopping();
}
