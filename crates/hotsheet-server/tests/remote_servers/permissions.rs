//! HS2-XHQ1EH: orchestrator permission requests through the real authenticated
//! service and its permission bridge, the owned CLI process and the stdio MCP tool.
//! One checkout is both the orchestration project and its own local target, which is a
//! valid destination and keeps the fixture to one store. Synthetic data only.
use super::orchestration::api;
use super::*;
use hotsheet_ticketing::checkouts::CheckoutRegistry;
use hotsheet_ticketing::{NewTicket, ops};

const TARGET: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAZ";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hs2_xhq1eh_permission_requests_round_trip_through_the_bridge_cli_and_mcp() {
    assert!(hotsheet_aitools::test_permission_vault_available());
    hotsheet_aitools::enable_test_permission_vault();
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let source = root.path().join("store");
    let host = state(&source, &home);
    let project = root.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let checkout = CheckoutRegistry::new(home.join("checkouts.json"))
        .register(&project, Some("Launch"), None, vec![source.clone()])
        .unwrap();
    let connection = checkout.sources[0].connection_id.clone();
    let store = FsStore::open(&source).unwrap();
    let orchestration = ops::create(
        &store,
        hotsheet_model::Ulid::new(),
        "TST",
        "2026-10-15T00:00:00Z".into(),
        NewTicket {
            title: "Coordinate".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let router = app(host.clone());
    let base = format!("/checkouts/{}/orchestration", checkout.id);
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
    let bound = api(
        router.clone(),
        "PUT",
        &format!("{base}/targets/{TARGET}/binding"),
        json!({"target":{"owner_checkout_id":checkout.id,"target_id":TARGET},
            "expected_manifest_revision":added["revision"],"expected_binding_revision":0,
            "previous_destination":null,"destination":candidate["destination"]}),
        true,
    )
    .await
    .1;
    assert_eq!(bound["usable"], true, "{bound}");
    // Every permission route requires the bearer.
    for (method, path) in [
        ("GET", format!("{base}/permissions")),
        ("POST", format!("{base}/permissions/{TARGET}")),
        ("POST", format!("{base}/permission-rules")),
        ("DELETE", format!("{base}/permission-rules/{TARGET}")),
        ("GET", "/permissions".to_owned()),
    ] {
        assert_eq!(
            api(router.clone(), method, &path, json!({}), false).await.0,
            StatusCode::UNAUTHORIZED,
            "{method} {path}"
        );
    }
    let create = |operation: &str| {
        json!({"ticket":{"native_id":orchestration.slug},"target_id":TARGET,
            "connection_id":connection,"operation_id":operation,"title":"Planned work",
            "actor":{"role":"ai","id":"worker-1"}})
    };
    // An AI's write asks a person: a typed refusal names the request, and nothing is
    // written. Nothing waits on the answer.
    let (status, refused) = api(
        router.clone(),
        "POST",
        &format!("{base}/tickets/create"),
        create("op-1"),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{refused}");
    assert_eq!(refused["code"], "permission_required");
    let request_id = refused["request_id"].as_str().unwrap().to_owned();
    assert_eq!(store.list_tickets().unwrap().len(), 1);
    // The request is the ordinary permission card, with the orchestrator requester.
    let pending = api(router.clone(), "GET", "/permissions", Value::Null, true)
        .await
        .1;
    let cards = pending.as_array().unwrap();
    assert_eq!(cards.len(), 1, "{pending}");
    let card = &cards[0];
    assert_eq!(card["requester"], "orchestrator");
    assert_eq!(card["tool"], "Orchestrator");
    assert_eq!(card["project"], format!("checkout:{}", checkout.id));
    assert_eq!(card["action"], "Create or change tickets in Self");
    assert_eq!(card["always_allow_supported"], true);
    assert_eq!(card["orchestrator"]["request_id"], request_id.as_str());
    assert_eq!(card["orchestrator"]["target_label"], "Self");
    assert_eq!(card["orchestrator"]["owner_label"], "Launch");
    assert_eq!(card["orchestrator"]["action"], "tickets_write");
    let card_id = card["id"].as_u64().unwrap();
    // Asking again shows no second card.
    api(
        router.clone(),
        "POST",
        &format!("{base}/tickets/create"),
        create("op-1"),
        true,
    )
    .await;
    assert_eq!(
        api(router.clone(), "GET", "/permissions", Value::Null, true)
            .await
            .1
            .as_array()
            .unwrap()
            .len(),
        1
    );
    // Answers the orchestrator does not take: this AI session, a session, a review.
    for answer in [
        json!({"decision":"allow","scope":"chat"}),
        json!({"decision":"allow","scope":"session"}),
        json!({"decision":"allow","scope":"once","continue_review":true}),
    ] {
        assert_eq!(
            api(
                router.clone(),
                "POST",
                &format!("/permissions/{card_id}"),
                answer.clone(),
                true
            )
            .await
            .0,
            StatusCode::BAD_REQUEST,
            "{answer}"
        );
    }
    // HS2-42WS4Z: a refused review leaves the card waiting for a person, and no rule
    // review continues from it.
    assert_eq!(
        api(router.clone(), "GET", "/permissions", Value::Null, true)
            .await
            .1[0]["id"],
        card_id
    );
    assert_eq!(
        api(
            router.clone(),
            "POST",
            &format!("/permissions/{card_id}/remember"),
            json!({}),
            true
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    // Allow Once: the card is settled and the request allowed, not yet used.
    let (status, resolved) = api(
        router.clone(),
        "POST",
        &format!("/permissions/{card_id}"),
        json!({"decision":"allow","scope":"once"}),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{resolved}");
    assert_eq!(resolved["persisted"], false);
    assert!(
        api(router.clone(), "GET", "/permissions", Value::Null, true)
            .await
            .1
            .as_array()
            .unwrap()
            .is_empty()
    );
    let listed = api(
        router.clone(),
        "GET",
        &format!("{base}/permissions"),
        Value::Null,
        true,
    )
    .await
    .1;
    assert_eq!(listed["requests"][0]["state"], "allowed");
    assert_eq!(listed["requests"][0]["answered_by"], Value::Null);
    // The action happens and uses the answer.
    let (status, created) = api(
        router.clone(),
        "POST",
        &format!("{base}/tickets/create"),
        create("op-1"),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    assert_eq!(created["created"], true);
    let listed = api(
        router.clone(),
        "GET",
        &format!("{base}/permissions"),
        Value::Null,
        true,
    )
    .await
    .1;
    assert_eq!(listed["requests"][0]["state"], "used");
    // Always Allow: a remembered rule, listed in Settings → Permissions with the
    // other rules, that answers later operations with the same key.
    let second = api(
        router.clone(),
        "POST",
        &format!("{base}/tickets/create"),
        create("op-2"),
        true,
    )
    .await
    .1;
    assert_eq!(second["code"], "permission_required");
    let card_id = api(router.clone(), "GET", "/permissions", Value::Null, true)
        .await
        .1[0]["id"]
        .as_u64()
        .unwrap();
    let (status, resolved) = api(
        router.clone(),
        "POST",
        &format!("/permissions/{card_id}"),
        json!({"decision":"allow","scope":"always"}),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{resolved}");
    assert_eq!(resolved["persisted"], true);
    let (status, third) = api(
        router.clone(),
        "POST",
        &format!("{base}/tickets/create"),
        create("op-3"),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{third}");
    let rules = api(
        router.clone(),
        "GET",
        "/permissions/rules",
        Value::Null,
        true,
    )
    .await
    .1;
    let rule = rules
        .as_array()
        .unwrap()
        .iter()
        .find(|rule| rule["tool"] == "Orchestrator")
        .unwrap()
        .clone();
    assert_eq!(rule["project"], format!("checkout:{}", checkout.id));
    assert_eq!(rule["scope"], "always");
    assert_eq!(rule["decision"], "allow");
    assert_eq!(rule["exact_action"], "Create or change tickets in Self");
    assert_eq!(rule["orchestrator"]["target_label"], "Self");
    assert_eq!(rule["orchestrator"]["owner_label"], "Launch");
    assert_eq!(rule["orchestrator"]["action"], "tickets_write");
    // A remembered orchestrator rule is kept or forgotten, nothing else.
    let slot = json!({"project":rule["project"],"tool":rule["tool"],"action":rule["action"]});
    let mut change = slot.clone();
    change["decision"] = json!("deny");
    assert_eq!(
        api(router.clone(), "PATCH", "/permissions/rules", change, true)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        api(
            router.clone(),
            "DELETE",
            "/permissions/rules",
            slot.clone(),
            true
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert!(
        api(
            router.clone(),
            "GET",
            "/permissions/rules",
            Value::Null,
            true
        )
        .await
        .1
        .as_array()
        .unwrap()
        .iter()
        .all(|rule| rule["tool"] != "Orchestrator")
    );
    assert_eq!(
        api(router.clone(), "DELETE", "/permissions/rules", slot, true)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    // Headless: the owned CLI files a request without a server's bridge, and the
    // service shows it the next time a client reads the bridge.
    let cli_prefix = ["-C", source.to_str().unwrap(), "orchestration"];
    let ai_cli = [
        "--actor-role",
        "ai",
        "--actor-id",
        "worker-1",
        "-C",
        source.to_str().unwrap(),
        "orchestration",
    ];
    let human_cli = [
        "--actor-role",
        "human",
        "--actor-id",
        "maintainer",
        "-C",
        source.to_str().unwrap(),
        "orchestration",
    ];
    let failed = cli_failure(
        &home,
        &ai_cli,
        &[
            "ticket",
            "create",
            "--project",
            &checkout.id,
            "--ticket",
            &orchestration.slug,
            "--target",
            TARGET,
            "--connection",
            &connection,
            "--operation-id",
            "op-cli",
            "--title",
            "Headless work",
        ],
    );
    assert!(failed.contains("permission_required"), "{failed}");
    let shown = api(router.clone(), "GET", "/permissions", Value::Null, true)
        .await
        .1;
    assert_eq!(shown.as_array().unwrap().len(), 1, "{shown}");
    let headless = shown[0]["orchestrator"]["request_id"]
        .as_str()
        .unwrap()
        .to_owned();
    // The CLI answers it as a person, never as the AI it governs; the card settles.
    assert!(
        cli_failure(
            &home,
            &ai_cli,
            &[
                "permission",
                "answer",
                &headless,
                "--project",
                &checkout.id,
                "--answer",
                "deny"
            ]
        )
        .contains("forbidden")
    );
    let denied = cli_named(
        &home,
        &human_cli,
        &[
            "permission",
            "answer",
            &headless,
            "--project",
            &checkout.id,
            "--answer",
            "deny",
        ],
    );
    assert_eq!(denied["request"]["state"], "denied");
    assert!(
        api(router.clone(), "GET", "/permissions", Value::Null, true)
            .await
            .1
            .as_array()
            .unwrap()
            .is_empty()
    );
    // A person remembers a rule ahead of any request, and forgets it again.
    assert!(
        cli_failure(
            &home,
            &ai_cli,
            &[
                "permission",
                "remember",
                TARGET,
                "--project",
                &checkout.id,
                "--action",
                "work_start"
            ]
        )
        .contains("forbidden")
    );
    let remembered = cli_named(
        &home,
        &human_cli,
        &[
            "permission",
            "remember",
            TARGET,
            "--project",
            &checkout.id,
            "--action",
            "work_start",
        ],
    );
    assert_eq!(remembered["created"], true);
    assert_eq!(remembered["summary"], "Start work on a ticket in Self");
    let listed = cli_named(
        &home,
        &cli_prefix,
        &["permission", "list", "--project", &checkout.id],
    );
    assert_eq!(listed["rules"].as_array().unwrap().len(), 1);
    assert_eq!(listed["requests"].as_array().unwrap().len(), 3);
    assert_eq!(listed["retired"]["grants_file"], false);
    let checked = cli_named(
        &home,
        &cli_prefix,
        &[
            "permission",
            "check",
            TARGET,
            "--project",
            &checkout.id,
            "--actor",
            "worker-1",
            "--action",
            "work_start",
        ],
    );
    assert_eq!(checked["allowed"], true);
    assert_eq!(checked["basis"], "remembered_rule");
    // The stdio MCP tool reads requests and rules through a live loopback service and
    // offers no way to answer, remember or forget.
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
    let read = mcp_named(
        &home,
        &backend,
        "hotsheet_targets",
        json!({"action":"permission_list","checkout":checkout.id}),
    );
    assert_eq!(read["rules"][0]["rule_id"], remembered["rule_id"]);
    for action in [
        "permission_answer",
        "permission_remember",
        "permission_forget",
        "grant_list",
        "approval_list",
    ] {
        mcp_rejected(
            &home,
            &backend,
            "hotsheet_targets",
            json!({"action":action,"checkout":checkout.id,"request_id":headless}),
        );
    }
    let forgotten = cli_named(
        &home,
        &human_cli,
        &[
            "permission",
            "forget",
            remembered["rule_id"].as_str().unwrap(),
            "--project",
            &checkout.id,
        ],
    );
    assert_eq!(forgotten["rule_id"], remembered["rule_id"]);
    // Every answer is in the journal, by identifiers only.
    let journal = cli_named(
        &home,
        &cli_prefix,
        &["audit", "--project", &checkout.id, "--limit", "100"],
    );
    let answers: Vec<_> = journal["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["action"].as_str().unwrap().starts_with("permission_"))
        .map(|entry| {
            (
                entry["action"].as_str().unwrap().to_owned(),
                entry["outcome"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert_eq!(
        answers,
        [
            ("permission_answer".to_owned(), "approved".to_owned()),
            ("permission_answer".to_owned(), "approved".to_owned()),
            ("permission_remember".to_owned(), "changed".to_owned()),
            ("permission_forget".to_owned(), "revoked".to_owned()),
            ("permission_answer".to_owned(), "rejected".to_owned()),
            ("permission_remember".to_owned(), "changed".to_owned()),
            ("permission_forget".to_owned(), "revoked".to_owned()),
        ]
    );
    let stored = std::fs::read_to_string(home.join("orchestration-permissions.json")).unwrap();
    assert!(!stored.contains(SECRET));
    assert!(!stored.contains("Planned work"));
    assert!(!stored.contains(project.to_str().unwrap()));
    let _ = stop.send(());
    tokio::time::timeout(std::time::Duration::from_secs(10), task)
        .await
        .unwrap()
        .unwrap();
}

/// One checkout bound as its own target, with a goal ticket and its source connection.
struct SelfTarget {
    _root: tempfile::TempDir,
    root_path: std::path::PathBuf,
    host: hotsheet_server::AppState,
    router: axum::Router,
    base: String,
    checkout_id: String,
    connection: String,
    goal: String,
}

async fn self_target() -> SelfTarget {
    assert!(hotsheet_aitools::test_permission_vault_available());
    hotsheet_aitools::enable_test_permission_vault();
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let source = root.path().join("store");
    let host = state(&source, &home);
    let project = root.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let checkout = CheckoutRegistry::new(home.join("checkouts.json"))
        .register(&project, Some("Launch"), None, vec![source.clone()])
        .unwrap();
    let store = FsStore::open(&source).unwrap();
    let goal = ops::create(
        &store,
        hotsheet_model::Ulid::new(),
        "TST",
        "2026-10-15T00:00:00Z".into(),
        NewTicket {
            title: "Coordinate".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let router = app(host.clone());
    let base = format!("/checkouts/{}/orchestration", checkout.id);
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
    let bound = api(
        router.clone(),
        "PUT",
        &format!("{base}/targets/{TARGET}/binding"),
        json!({"target":{"owner_checkout_id":checkout.id,"target_id":TARGET},
            "expected_manifest_revision":added["revision"],"expected_binding_revision":0,
            "previous_destination":null,"destination":candidate["destination"]}),
        true,
    )
    .await
    .1;
    assert_eq!(bound["usable"], true, "{bound}");
    SelfTarget {
        root_path: root.path().to_path_buf(),
        _root: root,
        host,
        router,
        base,
        connection: checkout.sources[0].connection_id.clone(),
        checkout_id: checkout.id,
        goal: goal.slug,
    }
}

impl SelfTarget {
    /// An AI's ticket write that asks a person; answers the card's bridge id.
    async fn ask(&self, operation: &str) -> u64 {
        let (status, refused) = api(
            self.router.clone(),
            "POST",
            &format!("{}/tickets/create", self.base),
            json!({"ticket":{"native_id":self.goal},"target_id":TARGET,
                "connection_id":self.connection,"operation_id":operation,"title":"Planned work",
                "actor":{"role":"ai","id":"worker-1"}}),
            true,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{refused}");
        assert_eq!(refused["code"], "permission_required");
        let cards = api(
            self.router.clone(),
            "GET",
            "/permissions",
            Value::Null,
            true,
        )
        .await
        .1;
        cards
            .as_array()
            .unwrap()
            .iter()
            .find(|card| card["orchestrator"]["request_id"] == refused["request_id"])
            .unwrap()["id"]
            .as_u64()
            .unwrap()
    }

    async fn orchestrator_rules(&self) -> Vec<Value> {
        let (status, rules) = api(
            self.router.clone(),
            "GET",
            "/permissions/rules?source=orchestrator",
            Value::Null,
            true,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{rules}");
        rules.as_array().unwrap().clone()
    }
}

/// HS2-4K43DK: orchestrator rules live in the orchestration ledger, not the AI
/// permission vault, so a locked or still-loading vault neither hides them nor refuses
/// Always Allow or a revoke on them. The full list keeps answering 503 in both states,
/// which is how a client shows the locked vault; `?source=orchestrator` lists the
/// orchestrator's rules alone.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hs2_4k43dk_orchestrator_rules_stay_listed_while_the_vault_is_locked_or_loading() {
    let fixture = self_target().await;
    let router = fixture.router.clone();
    assert_eq!(
        api(
            router.clone(),
            "GET",
            "/permissions/rules?source=other",
            Value::Null,
            true
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    // Locked: the vault's key is unavailable; then still loading after a restart.
    for (round, label) in ["locked", "loading"].into_iter().enumerate() {
        if label == "locked" {
            fixture.host.permission_bridge().drop_durable_rules();
        } else {
            let _ = fixture
                .host
                .clone()
                .with_permission_rules_deferred(fixture.root_path.join("vault.json"));
        }
        assert!(fixture.host.permission_bridge().durable_rules_locked());
        let (status, _) = api(
            router.clone(),
            "GET",
            "/permissions/rules",
            Value::Null,
            true,
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{label}");
        assert!(fixture.orchestrator_rules().await.is_empty(), "{label}");
        // Always Allow on the card is remembered, not refused with 503.
        let card = fixture.ask(&format!("op-{round}")).await;
        let (status, resolved) = api(
            router.clone(),
            "POST",
            &format!("/permissions/{card}"),
            json!({"decision":"allow","scope":"always"}),
            true,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{label}: {resolved}");
        assert_eq!(resolved["persisted"], true, "{label}");
        let rules = fixture.orchestrator_rules().await;
        assert_eq!(rules.len(), 1, "{label}");
        assert_eq!(
            rules[0]["project"],
            format!("checkout:{}", fixture.checkout_id)
        );
        assert_eq!(rules[0]["orchestrator"]["action"], "tickets_write");
        // The rule answers: the next operation with the same key does not ask.
        let (status, created) = api(
            router.clone(),
            "POST",
            &format!("{}/tickets/create", fixture.base),
            json!({"ticket":{"native_id":fixture.goal},"target_id":TARGET,
                "connection_id":fixture.connection,"operation_id":format!("op-{round}-next"),
                "title":"Planned work","actor":{"role":"ai","id":"worker-1"}}),
            true,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{label}: {created}");
        // Revoking it works the same, and the full list still answers 503.
        let slot = json!({"project":rules[0]["project"],"tool":rules[0]["tool"],
            "action":rules[0]["action"]});
        assert_eq!(
            api(
                router.clone(),
                "DELETE",
                "/permissions/rules",
                slot.clone(),
                true
            )
            .await
            .0,
            StatusCode::NO_CONTENT,
            "{label}"
        );
        assert!(fixture.orchestrator_rules().await.is_empty(), "{label}");
        assert_eq!(
            api(router.clone(), "DELETE", "/permissions/rules", slot, true)
                .await
                .0,
            StatusCode::NOT_FOUND,
            "{label}"
        );
        assert_eq!(
            api(
                router.clone(),
                "GET",
                "/permissions/rules",
                Value::Null,
                true
            )
            .await
            .0,
            StatusCode::SERVICE_UNAVAILABLE,
            "{label}"
        );
    }
}

/// The next permission nudge on the server's event bus, if one arrives within `wait`.
async fn next_permission_event(
    events: &mut tokio::sync::broadcast::Receiver<hotsheet_server::ChangeEvent>,
    wait: std::time::Duration,
) -> Option<String> {
    tokio::time::timeout(wait, async {
        loop {
            match events.recv().await {
                Ok(event) if event.kind.starts_with("permission_") => return event.kind,
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    return String::new();
                }
            }
        }
    })
    .await
    .ok()
}

/// HS2-E16FWJ: a request a headless CLI files reaches the server's bridge, and so its
/// clients, without any client reading `GET /permissions`; a CLI answer settles the
/// card the same way. Idle, the follower publishes nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hs2_e16fwj_a_cli_filed_request_reaches_clients_without_a_read() {
    let fixture = self_target().await;
    let home = fixture.root_path.join("home");
    let source = fixture.root_path.join("store");
    let mut events = fixture.host.subscribe();
    let orchestrator_cards = || {
        fixture
            .host
            .permission_bridge()
            .pending()
            .into_iter()
            .filter(|request| request.orchestrator.is_some())
            .collect::<Vec<_>>()
    };
    let quiet = std::time::Duration::from_millis(1_500);
    let ai_cli = [
        "--actor-role",
        "ai",
        "--actor-id",
        "worker-1",
        "-C",
        source.to_str().unwrap(),
        "orchestration",
    ];
    let human_cli = [
        "--actor-role",
        "human",
        "--actor-id",
        "maintainer",
        "-C",
        source.to_str().unwrap(),
        "orchestration",
    ];
    let file = |operation: &str| {
        cli_failure(
            &home,
            &ai_cli,
            &[
                "ticket",
                "create",
                "--project",
                &fixture.checkout_id,
                "--ticket",
                &fixture.goal,
                "--target",
                TARGET,
                "--connection",
                &fixture.connection,
                "--operation-id",
                operation,
                "--title",
                "Headless work",
            ],
        )
    };
    let wait = std::time::Duration::from_secs(15);
    let deny = |request_id: &str| {
        cli_named(
            &home,
            &human_cli,
            &[
                "permission",
                "answer",
                request_id,
                "--project",
                &fixture.checkout_id,
                "--answer",
                "deny",
            ],
        )
    };
    // Filed before the follower starts: shown once its watch runs.
    assert!(file("op-cli-0").contains("permission_required"));
    assert!(orchestrator_cards().is_empty());
    fixture.host.spawn_permission_follower();
    assert_eq!(
        next_permission_event(&mut events, wait).await.as_deref(),
        Some("permission_asked")
    );
    let early = orchestrator_cards();
    assert_eq!(early.len(), 1);
    deny(&early[0].orchestrator.as_ref().unwrap().request_id);
    assert_eq!(
        next_permission_event(&mut events, wait).await.as_deref(),
        Some("permission_resolved")
    );
    // Idle: nothing is published.
    assert_eq!(next_permission_event(&mut events, quiet).await, None);
    assert!(file("op-cli-1").contains("permission_required"));
    assert_eq!(
        next_permission_event(&mut events, wait).await.as_deref(),
        Some("permission_asked")
    );
    let cards = orchestrator_cards();
    assert_eq!(cards.len(), 1);
    let request_id = cards[0].orchestrator.as_ref().unwrap().request_id.clone();
    // Asking again for the same operation files nothing and shows no second card.
    assert!(file("op-cli-1").contains("permission_required"));
    assert_eq!(next_permission_event(&mut events, quiet).await, None);
    assert_eq!(orchestrator_cards().len(), 1);
    // A CLI answer settles the card on its own.
    deny(&request_id);
    assert_eq!(
        next_permission_event(&mut events, wait).await.as_deref(),
        Some("permission_resolved")
    );
    assert!(orchestrator_cards().is_empty());
    // Empty, then refilled by a new operation; then idle again.
    assert!(file("op-cli-2").contains("permission_required"));
    assert_eq!(
        next_permission_event(&mut events, wait).await.as_deref(),
        Some("permission_asked")
    );
    assert_eq!(orchestrator_cards().len(), 1);
    assert_eq!(next_permission_event(&mut events, quiet).await, None);
    fixture.host.begin_stopping();
}

/// HS2-JWV5S1: the retired grant and approval files are shown with their counts and
/// removed by a person through the real service and the owned CLI; an AI is refused.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hs2_jwv5s1_a_person_removes_the_retired_files_through_http_and_the_cli() {
    let fixture = self_target().await;
    let home = fixture.root_path.join("home");
    let source = fixture.root_path.join("store");
    let router = fixture.router.clone();
    let path = format!("{}/retired-permissions", fixture.base);
    let write_retired = || {
        std::fs::write(
            home.join("orchestration-grants.json"),
            br#"{"schema_version":1,"grants":[{},{},{}]}"#,
        )
        .unwrap();
        std::fs::write(
            home.join("orchestration-approvals.json"),
            br#"{"schema_version":1,"approvals":[{}]}"#,
        )
        .unwrap();
    };
    write_retired();
    // The bearer is required for both methods.
    for method in ["GET", "DELETE"] {
        assert_eq!(
            api(router.clone(), method, &path, json!({}), false).await.0,
            StatusCode::UNAUTHORIZED
        );
    }
    let (status, shown) = api(router.clone(), "GET", &path, Value::Null, true).await;
    assert_eq!(status, StatusCode::OK, "{shown}");
    assert_eq!(shown["grants"], 3);
    assert_eq!(shown["approvals"], 1);
    // An AI cannot remove them.
    let (status, refused) = api(
        router.clone(),
        "DELETE",
        &path,
        json!({"actor":{"role":"ai","id":"worker-1"}}),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{refused}");
    assert!(home.join("orchestration-grants.json").exists());
    let (status, removed) = api(
        router.clone(),
        "DELETE",
        &path,
        json!({"actor":{"role":"human","id":"maintainer"}}),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{removed}");
    assert_eq!(removed["removed"]["grants"], 3);
    assert_eq!(removed["removed"]["approvals"], 1);
    assert_eq!(removed["remaining"]["grants_file"], false);
    assert!(!home.join("orchestration-grants.json").exists());
    // Again: idempotent.
    let (status, again) = api(
        router.clone(),
        "DELETE",
        &path,
        json!({"actor":{"role":"human","id":"maintainer"}}),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again["removed"]["grants_file"], false);
    // Headless: the CLI shows the counts, then a person removes them.
    write_retired();
    let prefix = |role: &'static str| {
        [
            "--actor-role",
            role,
            "--actor-id",
            "someone",
            "-C",
            source.to_str().unwrap(),
            "orchestration",
        ]
        .map(str::to_owned)
    };
    let human = prefix("human");
    let human: Vec<&str> = human.iter().map(String::as_str).collect();
    let ai = prefix("ai");
    let ai: Vec<&str> = ai.iter().map(String::as_str).collect();
    let shown = cli_named(
        &home,
        &human,
        &["permission", "retired", "--project", &fixture.checkout_id],
    );
    assert_eq!(shown["grants"], 3);
    assert!(home.join("orchestration-grants.json").exists());
    assert!(
        cli_failure(
            &home,
            &ai,
            &[
                "permission",
                "retired",
                "--remove",
                "--project",
                &fixture.checkout_id
            ]
        )
        .contains("forbidden")
    );
    let removed = cli_named(
        &home,
        &human,
        &[
            "permission",
            "retired",
            "--remove",
            "--project",
            &fixture.checkout_id,
        ],
    );
    assert_eq!(removed["removed"]["grants"], 3);
    assert!(!home.join("orchestration-approvals.json").exists());
    // The permission list no longer reports them.
    let listed = cli_named(
        &home,
        &human,
        &["permission", "list", "--project", &fixture.checkout_id],
    );
    assert_eq!(listed["retired"]["grants_file"], false);
    assert_eq!(listed["retired"]["approvals_file"], false);
}
