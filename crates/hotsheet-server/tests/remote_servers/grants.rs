//! HS2-SBQ3T8: grants, access checks and the audit journal through the real
//! authenticated service, the owned CLI process and the stdio MCP tool.
use super::orchestration::{api, configured_owner, shared_runtime};
use super::*;

const TARGET: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAZ";
const GRANT: &str = "01ARZ3NDEKTSV4RRFFQ69G5FB0";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hs2_sbq3t8_http_cli_and_mcp_share_grants_access_and_audit() {
    assert!(hotsheet_aitools::test_permission_vault_available());
    hotsheet_aitools::enable_test_permission_vault();
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let source = root.path().join("host-store");
    let memory = Arc::new(Memory::default());
    let host = state(&source, &home);
    let owner = configured_owner(&source, &home, &root.path().join("owner"));
    let host = host.with_orchestration_runtime(shared_runtime(&source, &home, &memory));
    let router = app(host.clone());
    let prefix = format!("/checkouts/{}/orchestration/targets", owner.id);
    let target = format!("{prefix}/{TARGET}");
    let audit = format!("/checkouts/{}/orchestration/audit", owner.id);
    // Declare and bind a local target (the owner's own checkout is a valid destination).
    let added = api(
        router.clone(),
        "POST",
        &prefix,
        json!({"target_id":TARGET,"label":"Synthetic","expected_revision":null}),
        true,
    )
    .await
    .1;
    let grant = json!({"grant_id":GRANT,"actor_id":"worker-1","actions":["tickets_read","tickets_write"],
        "expires_at_ms":u64::MAX / 4});
    // Unbound: no reviewed destination exists to pin a grant to.
    let mut human = grant.clone();
    human["actor"] = json!({"role":"human","id":"maintainer"});
    human["expires_at_ms"] = json!(now_ms() + 3_600_000);
    let (status, body) = api(
        router.clone(),
        "POST",
        &format!("{target}/grants"),
        human.clone(),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["code"], "target_unavailable");
    let candidate = api(
        router.clone(),
        "POST",
        &format!("/checkouts/{}/orchestration/inspect", owner.id),
        json!({"destination":{"kind":"local","checkout_id":owner.id}}),
        true,
    )
    .await
    .1;
    let bound = api(
        router.clone(),
        "PUT",
        &format!("{target}/binding"),
        json!({"target":{"owner_checkout_id":owner.id,"target_id":TARGET},
            "expected_manifest_revision":added["revision"],"expected_binding_revision":0,
            "previous_destination":null,"destination":candidate["destination"]}),
        true,
    )
    .await
    .1;
    assert_eq!(bound["usable"], true);
    // Every grant route requires the bearer.
    for (method, path) in [
        ("GET", format!("{target}/grants")),
        ("POST", format!("{target}/grants")),
        ("DELETE", format!("{target}/grants/{GRANT}")),
        ("POST", format!("{target}/access")),
        ("GET", audit.clone()),
    ] {
        assert_eq!(
            api(router.clone(), method, &path, json!({}), false).await.0,
            StatusCode::UNAUTHORIZED,
            "{method} {path}"
        );
    }
    // An AI actor is refused; an over-long lifetime and a missing scope are invalid.
    let mut by_ai = human.clone();
    by_ai["actor"] = json!({"role":"ai","id":"worker-1"});
    let (status, body) = api(
        router.clone(),
        "POST",
        &format!("{target}/grants"),
        by_ai,
        true,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["code"], "forbidden");
    let mut forever = human.clone();
    forever["expires_at_ms"] = grant["expires_at_ms"].clone();
    let mut unscoped = human.clone();
    unscoped["actions"] = json!(["sessions_manage"]);
    let mut unknown_action = human.clone();
    unknown_action["actions"] = json!(["deploy_everything"]);
    for invalid in [forever, unscoped, unknown_action] {
        assert_eq!(
            api(
                router.clone(),
                "POST",
                &format!("{target}/grants"),
                invalid,
                true
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
    }
    assert!(!home.join("orchestration-grants.json").exists());
    let (status, created) = api(
        router.clone(),
        "POST",
        &format!("{target}/grants"),
        human.clone(),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    assert_eq!(created["state"], "active");
    assert_eq!(created["grant_id"], GRANT);
    // The grant file carries no bearer, origin or filesystem path.
    let stored = std::fs::read_to_string(home.join("orchestration-grants.json")).unwrap();
    assert!(!stored.contains(SECRET));
    assert!(!stored.contains(owner.root.as_str()));
    assert!(!stored.contains("http"));
    let allowed = api(
        router.clone(),
        "POST",
        &format!("{target}/access"),
        json!({"actor_id":"worker-1","action":"tickets_write"}),
        true,
    )
    .await
    .1;
    assert_eq!(allowed, json!({"allowed":true,"grant_id":GRANT}));
    // The owned CLI process reads the same machine state without a server.
    let grant_cli = ["-C", source.to_str().unwrap(), "orchestration", "grant"];
    let listed = cli_named(&home, &grant_cli, &["list", TARGET, "--project", &owner.id]);
    assert_eq!(listed[0]["grant_id"], GRANT);
    assert_eq!(listed[0]["state"], "active");
    let checked = cli_named(
        &home,
        &grant_cli,
        &[
            "check",
            TARGET,
            "--project",
            &owner.id,
            "--actor",
            "worker-1",
            "--action",
            "work_start",
        ],
    );
    assert_eq!(
        checked,
        json!({"allowed":false,"reason":"action_not_granted"})
    );
    // The CLI issues a scoped session grant for a person and refuses an AI actor.
    let ai_cli = [
        "--actor-role",
        "ai",
        "--actor-id",
        "worker-1",
        "-C",
        source.to_str().unwrap(),
        "orchestration",
        "grant",
    ];
    let add = [
        "add",
        TARGET,
        "--project",
        &owner.id,
        "--actor",
        "worker-1",
        "--action",
        "sessions_manage",
        "--expires-in",
        "8h",
        "--tool",
        "claude:model-a,model-b",
        "--tool",
        "codex",
        "--max-concurrency",
        "2",
    ];
    assert!(cli_failure(&home, &ai_cli, &add).contains("forbidden"));
    let human_cli = [
        "--actor-role",
        "human",
        "--actor-id",
        "maintainer",
        "-C",
        source.to_str().unwrap(),
        "orchestration",
        "grant",
    ];
    let scoped = cli_named(&home, &human_cli, &add);
    assert_eq!(scoped["state"], "active");
    assert_eq!(scoped["session_scope"]["max_concurrency"], 2);
    assert_eq!(
        scoped["session_scope"]["tools"],
        json!([{"tool":"claude","models":["model-a","model-b"]},{"tool":"codex"}])
    );
    for (extra, expected) in [
        (&["--tool", "codex", "--starting"][..], json!(true)),
        (
            &["--tool", "claude", "--model", "model-c", "--starting"][..],
            json!(false),
        ),
        (
            &["--tool", "codex", "--running", "2", "--starting"][..],
            json!(false),
        ),
    ] {
        let mut arguments = vec![
            "check",
            TARGET,
            "--project",
            &owner.id,
            "--actor",
            "worker-1",
            "--action",
            "sessions_manage",
        ];
        arguments.extend_from_slice(extra);
        assert_eq!(
            cli_named(&home, &grant_cli, &arguments)["allowed"],
            expected
        );
    }
    for bad in [
        &[
            "add",
            TARGET,
            "--project",
            &owner.id,
            "--actor",
            "w",
            "--action",
            "tickets_read",
        ][..],
        &[
            "add",
            TARGET,
            "--project",
            &owner.id,
            "--actor",
            "w",
            "--action",
            "tickets_read",
            "--expires-in",
            "soon",
        ][..],
        &[
            "add",
            TARGET,
            "--project",
            &owner.id,
            "--actor",
            "w",
            "--action",
            "sessions_manage",
            "--expires-in",
            "1h",
            "--tool",
            "codex",
        ][..],
    ] {
        cli_failure(&home, &human_cli, bad);
    }
    // The stdio MCP tool reads grants and the journal through a live loopback service,
    // and offers no way to issue or revoke authority.
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
    let listed = mcp_named(
        &home,
        &backend,
        "hotsheet_targets",
        json!({"action":"grant_list","checkout":owner.id,"target_id":TARGET}),
    );
    assert_eq!(listed.as_array().unwrap().len(), 2);
    let decision = mcp_named(
        &home,
        &backend,
        "hotsheet_targets",
        json!({"action":"access_check","checkout":owner.id,"target_id":TARGET,
            "for_actor":"worker-1","target_action":"sessions_manage",
            "session":{"tool":"codex","running":1,"starting":true}}),
    );
    assert_eq!(decision["allowed"], true);
    for action in ["grant_add", "grant_revoke", "grant_create"] {
        mcp_rejected(
            &home,
            &backend,
            "hotsheet_targets",
            json!({"action":action,"checkout":owner.id,"target_id":TARGET,"grant_id":GRANT}),
        );
    }
    let page = mcp_named(
        &home,
        &backend,
        "hotsheet_targets",
        json!({"action":"audit","checkout":owner.id,"after":0,"limit":1}),
    );
    assert_eq!(page["entries"][0]["action"], "grant_create");
    assert_eq!(page["entries"][0]["actor_id"], "maintainer");
    assert_eq!(page["more"], true);
    // Revocation is immediate, idempotent and visible everywhere.
    let revoked = cli_named(
        &home,
        &human_cli,
        &["revoke", TARGET, GRANT, "--project", &owner.id],
    );
    assert_eq!(revoked["revoked_by"], "maintainer");
    assert!(
        cli_failure(
            &home,
            &ai_cli,
            &[
                "revoke",
                TARGET,
                scoped["grant_id"].as_str().unwrap(),
                "--project",
                &owner.id
            ]
        )
        .contains("forbidden")
    );
    let (status, again) = api(
        router.clone(),
        "DELETE",
        &format!("{target}/grants/{GRANT}"),
        json!({}),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again, revoked);
    assert_eq!(
        api(
            router.clone(),
            "POST",
            &format!("{target}/access"),
            json!({"actor_id":"worker-1","action":"tickets_write"}),
            true
        )
        .await
        .1,
        json!({"allowed":false,"reason":"grant_revoked"})
    );
    assert_eq!(
        api(
            router.clone(),
            "DELETE",
            &format!("{target}/grants/01ARZ3NDEKTSV4RRFFQ69G5FB9"),
            json!({}),
            true
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    // Unbinding leaves the remaining grant inert without deleting it.
    let unbound = api(
        router.clone(),
        "DELETE",
        &format!("{target}/binding"),
        json!({"target":{"owner_checkout_id":owner.id,"target_id":TARGET},
            "expected_binding_revision":1,"previous_destination":candidate["destination"]}),
        true,
    )
    .await
    .1;
    assert_eq!(unbound["revision"], 2);
    let listed = api(
        router.clone(),
        "GET",
        &format!("{target}/grants"),
        Value::Null,
        true,
    )
    .await
    .1;
    let states: Vec<_> = listed
        .as_array()
        .unwrap()
        .iter()
        .map(|grant| grant["state"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(states, ["revoked", "destination_changed"]);
    let journal = cli_named(
        &home,
        &["-C", source.to_str().unwrap(), "orchestration"],
        &["audit", "--project", &owner.id, "--limit", "50"],
    );
    let actions: Vec<_> = journal["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            (
                entry["seq"].as_u64().unwrap(),
                entry["action"].as_str().unwrap().to_owned(),
                entry["outcome"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert_eq!(
        actions,
        [
            (1, "grant_create".to_owned(), "granted".to_owned()),
            (2, "grant_create".to_owned(), "granted".to_owned()),
            (3, "grant_revoke".to_owned(), "revoked".to_owned()),
        ]
    );
    let (status, page) = api(
        router.clone(),
        "GET",
        &format!("{audit}?after=2"),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["entries"].as_array().unwrap().len(), 1);
    assert_eq!(page["cursor"], 3);
    assert_eq!(
        api(
            router,
            "GET",
            &format!("{audit}?after=later"),
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

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis()
        .try_into()
        .unwrap()
}
