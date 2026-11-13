//! HS2-YCJDEB: the throughput policy, dry-run and approvals through the real
//! authenticated service, the owned CLI process and the stdio MCP tool.
use super::orchestration::{api, configured_owner, shared_runtime};
use super::*;
use hotsheet_ticketing::orchestration::{ApprovalGate, ApprovalRegistry, TargetRef, Transaction};
use std::path::Path;

const TARGET: &str = "01ARZ3NDEKTSV4RRFFQ69G5FC0";

struct Clock;
impl hotsheet_ticketing::Clock for Clock {
    fn now_ms(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
            .try_into()
            .unwrap()
    }
}
struct Rng(u128);
impl hotsheet_ticketing::Rng for Rng {
    fn next_u128(&mut self) -> u128 {
        self.0 += 1;
        self.0
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hs2_ycjdeb_cli_http_and_mcp_share_policy_dry_run_and_approvals() {
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
    let base = format!("/checkouts/{}/orchestration", owner.id);
    let target = format!("{base}/targets/{TARGET}");
    let added = api(
        router.clone(),
        "POST",
        &format!("{base}/targets"),
        json!({"target_id":TARGET,"label":"Synthetic","expected_revision":null}),
        true,
    )
    .await
    .1;
    let candidate = api(
        router.clone(),
        "POST",
        &format!("{base}/inspect"),
        json!({"destination":{"kind":"local","checkout_id":owner.id}}),
        true,
    )
    .await
    .1;
    api(
        router.clone(),
        "PUT",
        &format!("{target}/binding"),
        json!({"target":{"owner_checkout_id":owner.id,"target_id":TARGET},
            "expected_manifest_revision":added["revision"],"expected_binding_revision":0,
            "previous_destination":null,"destination":candidate["destination"]}),
        true,
    )
    .await;
    let human = [
        "--actor-role",
        "human",
        "--actor-id",
        "maintainer",
        "-C",
        source.to_str().unwrap(),
        "orchestration",
    ];
    let ai = [
        "--actor-role",
        "ai",
        "--actor-id",
        "worker-1",
        "-C",
        source.to_str().unwrap(),
        "orchestration",
    ];
    let plain = ["-C", source.to_str().unwrap(), "orchestration"];
    cli_named(
        &home,
        &human,
        &[
            "grant",
            "add",
            TARGET,
            "--project",
            &owner.id,
            "--actor",
            "worker-1",
            "--action",
            "tickets_read",
            "--action",
            "tickets_write",
            "--expires-in",
            "1h",
        ],
    );
    // Headless setup: an absent policy reads as disabled and creates nothing.
    let policy_path = Path::new(&owner.root).join(".hotsheet2/orchestration-policy.json");
    assert_eq!(
        cli_named(&home, &plain, &["policy", "show", "--project", &owner.id]),
        json!({"enabled":false,"revision":null,"policy":null})
    );
    assert!(!policy_path.exists());
    let body = json!({
        "machine": {"max_sessions": 2, "min_free_ram_mb": 4096},
        "tools": [{"tool":"codex","max_sessions":2,"accounts":[{"account":"team","max_sessions":1}]}],
        "targets": [{"target_id":TARGET,"actions":["tickets_read","work_start","sessions_manage"],
            "tools":[{"tool":"codex"}],"max_sessions":2}],
        "approval": {"required_for":["work_start"]},
        "speculation": {"allowed": true, "max_depth": 2, "risk_weight": 30}
    });
    let good = root.path().join("policy.json");
    std::fs::write(&good, serde_json::to_vec(&body).unwrap()).unwrap();
    // A conflicting policy: the account cap exceeds its tool cap and the target names
    // a tool that is not configured.
    let mut conflicting = body.clone();
    conflicting["tools"][0]["accounts"][0]["max_sessions"] = json!(5);
    conflicting["targets"][0]["tools"] = json!([{"tool":"codex"},{"tool":"claude"}]);
    let bad = root.path().join("conflicting.json");
    std::fs::write(&bad, serde_json::to_vec(&conflicting).unwrap()).unwrap();
    let malformed = root.path().join("malformed.json");
    std::fs::write(&malformed, b"{\"machine\":").unwrap();
    let report = cli_named(
        &home,
        &plain,
        &[
            "policy",
            "validate",
            "--project",
            &owner.id,
            "--file",
            bad.to_str().unwrap(),
        ],
    );
    assert_eq!(report["valid"], false);
    let codes: Vec<_> = report["problems"]
        .as_array()
        .unwrap()
        .iter()
        .map(|problem| problem["code"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(codes, ["account_cap_exceeds_tool", "unconfigured_tool"]);
    for (file, expected) in [(&bad, "invalid_request"), (&malformed, "must be JSON")] {
        let stderr = cli_failure(
            &home,
            &human,
            &[
                "policy",
                "set",
                "--project",
                &owner.id,
                "--file",
                file.to_str().unwrap(),
            ],
        );
        assert!(stderr.contains(expected), "{stderr}");
    }
    let set = [
        "policy",
        "set",
        "--project",
        &owner.id,
        "--file",
        good.to_str().unwrap(),
    ];
    assert!(cli_failure(&home, &ai, &set).contains("forbidden"));
    assert!(!policy_path.exists());
    // A dry run of the proposal, before anything is enabled.
    let dry = cli_named(
        &home,
        &plain,
        &[
            "policy",
            "dry-run",
            "--project",
            &owner.id,
            "--file",
            good.to_str().unwrap(),
            "--actor",
            "worker-1",
        ],
    );
    assert_eq!(dry["source"], "proposed");
    let row = |report: &Value, action: &str| -> Value {
        report["targets"][0]["actions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["action"] == action)
            .unwrap()
            .clone()
    };
    assert_eq!(row(&dry, "tickets_read")["automatic"], true);
    assert_eq!(row(&dry, "sessions_manage")["policy"], "allowed");
    assert_eq!(
        row(&dry, "sessions_manage")["grant"]["reason"],
        "action_not_granted"
    );
    assert_eq!(row(&dry, "work_start")["policy"], "approval_required");
    assert_eq!(
        row(&dry, "tickets_write")["policy_detail"],
        "policy_action_not_permitted"
    );
    assert_eq!(row(&dry, "tickets_write")["grant"]["allowed"], true);
    assert!(!policy_path.exists());
    // Setting is idempotent: the second run changes neither revision nor bytes.
    let stored = cli_named(&home, &human, &set);
    assert_eq!(stored["enabled"], true);
    assert_eq!(stored["policy"]["machine"]["min_free_ram_mb"], 4096);
    assert_eq!(stored["policy"]["machine"]["max_cpu_percent"], 90);
    assert_eq!(stored["policy"]["speculation"]["max_depth"], 2);
    assert_eq!(
        stored["policy"]["speculation"]["opt_out_tag"],
        "speculate:never"
    );
    let bytes = std::fs::read(&policy_path).unwrap();
    assert_eq!(cli_named(&home, &human, &set), stored);
    assert_eq!(std::fs::read(&policy_path).unwrap(), bytes);
    // `show` output can be edited and fed straight back to `set`; a stale explicit
    // revision conflicts.
    let mut shown = cli_named(&home, &plain, &["policy", "show", "--project", &owner.id]);
    assert_eq!(shown, stored);
    shown["policy"]["wind_down"]["idle_minutes"] = json!(45);
    let edited = root.path().join("edited.json");
    std::fs::write(&edited, serde_json::to_vec(&shown).unwrap()).unwrap();
    let edit = [
        "policy",
        "set",
        "--project",
        &owner.id,
        "--file",
        edited.to_str().unwrap(),
    ];
    let changed = cli_named(&home, &human, &edit);
    assert_eq!(changed["policy"]["wind_down"]["idle_minutes"], 45);
    assert_ne!(changed["revision"], stored["revision"]);
    let mut stale = edit.to_vec();
    stale.extend(["--expected-revision", stored["revision"].as_str().unwrap()]);
    assert!(cli_failure(&home, &human, &stale).contains("conflict"));
    // The service reads the same checked-in file.
    assert_eq!(
        api(
            router.clone(),
            "GET",
            &format!("{base}/policy"),
            Value::Null,
            false
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, served) = api(
        router.clone(),
        "GET",
        &format!("{base}/policy"),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(served, changed);
    let (status, refused) = api(
        router.clone(),
        "PUT",
        &format!("{base}/policy"),
        json!({"expected_revision":changed["revision"],"policy":body,
            "actor":{"role":"ai","id":"worker-1"}}),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(refused["code"], "forbidden");
    let (status, problems) = api(
        router.clone(),
        "POST",
        &format!("{base}/policy/validate"),
        json!({"policy": conflicting}),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(problems["valid"], false);
    // Both must allow: the grant covers tickets_write, the policy does not list it;
    // the policy lists work_start, no grant covers it.
    let check = |action: &'static str| {
        let router = router.clone();
        let path = format!("{target}/access");
        async move {
            api(
                router,
                "POST",
                &path,
                json!({"actor_id":"worker-1","action":action,"operation_id":"op-1"}),
                true,
            )
            .await
            .1
        }
    };
    assert_eq!(
        check("tickets_write").await,
        json!({"allowed":false,"reason":"policy_denied","detail":"policy_action_not_permitted"})
    );
    assert_eq!(
        check("work_start").await,
        json!({"allowed":false,"reason":"action_not_granted"})
    );
    assert_eq!(check("tickets_read").await["allowed"], true);
    // Approvals: a pending request (filed by the gate) is listed, an AI actor cannot
    // answer it, a person can, and the answer is audited.
    cli_named(
        &home,
        &human,
        &[
            "grant",
            "add",
            TARGET,
            "--project",
            &owner.id,
            "--actor",
            "worker-1",
            "--action",
            "work_start",
            "--expires-in",
            "1h",
        ],
    );
    assert_eq!(
        check("work_start").await,
        json!({"allowed":false,"reason":"approval_required","detail":"work_start"})
    );
    let mut rng = Rng(7);
    ApprovalRegistry::new(&home)
        .consume_or_request(
            &TargetRef {
                owner_checkout_id: owner.id.clone(),
                target_id: TARGET.into(),
            },
            "worker-1",
            ApprovalGate::WorkStart,
            "op-1",
            &mut Transaction {
                clock: &Clock,
                rng: &mut rng,
            },
        )
        .unwrap();
    let listed = cli_named(&home, &plain, &["approval", "list", "--project", &owner.id]);
    assert_eq!(listed[0]["state"], "pending");
    assert_eq!(listed[0]["gate"], "work_start");
    let approval = listed[0]["approval_id"].as_str().unwrap().to_owned();
    assert!(
        cli_failure(
            &home,
            &ai,
            &["approval", "approve", &approval, "--project", &owner.id]
        )
        .contains("forbidden")
    );
    let approved = cli_named(
        &home,
        &human,
        &["approval", "approve", &approval, "--project", &owner.id],
    );
    assert_eq!(approved["state"], "approved");
    assert_eq!(approved["decided_by"], "maintainer");
    assert_eq!(check("work_start").await["allowed"], true);
    assert!(
        cli_failure(
            &home,
            &human,
            &["approval", "deny", &approval, "--project", &owner.id]
        )
        .contains("conflict")
    );
    let (status, served) = api(
        router.clone(),
        "GET",
        &format!("{base}/approvals"),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(served[0]["state"], "approved");
    // The stdio MCP tool reads the same state and offers no way to change it.
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
    let call = |arguments: Value| mcp_named(&home, &backend, "hotsheet_targets", arguments);
    assert_eq!(
        call(json!({"action":"policy_show","checkout":owner.id})),
        changed
    );
    assert_eq!(
        call(json!({"action":"policy_validate","checkout":owner.id}))["valid"],
        true
    );
    assert_eq!(
        call(json!({"action":"policy_validate","checkout":owner.id,"policy":conflicting}))["valid"],
        false
    );
    let dry = call(json!({"action":"policy_dry_run","checkout":owner.id,"for_actor":"worker-1"}));
    assert_eq!(dry["source"], "stored");
    assert_eq!(row(&dry, "work_start")["grant"]["allowed"], true);
    assert_eq!(
        call(json!({"action":"approval_list","checkout":owner.id}))[0]["approval_id"],
        approval
    );
    for action in ["policy_set", "approval_approve", "approval_decide"] {
        mcp_rejected(
            &home,
            &backend,
            "hotsheet_targets",
            json!({"action":action,"checkout":owner.id,"policy":body,"approval_id":approval}),
        );
    }
    let journal = cli_named(
        &home,
        &plain,
        &["audit", "--project", &owner.id, "--limit", "50"],
    );
    let actions: Vec<_> = journal["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            format!(
                "{}:{}",
                entry["action"].as_str().unwrap(),
                entry["outcome"].as_str().unwrap()
            )
        })
        .collect();
    assert_eq!(
        actions,
        [
            "grant_create:granted",
            "policy_set:changed",
            "policy_set:changed",
            "grant_create:granted",
            "work_start:approved",
        ]
    );
    let _ = stop.send(());
    tokio::time::timeout(std::time::Duration::from_secs(10), task)
        .await
        .unwrap()
        .unwrap();
}
