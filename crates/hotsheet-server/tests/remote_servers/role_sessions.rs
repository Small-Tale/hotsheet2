//! HS2-TK9ADC: a target's role sessions in the policy through the owned CLI process
//! and the real service, and the coordination pool a host reports with its capacity.
//! Synthetic identities and an empty git project only.
use super::orchestration::{api, configured_owner, shared_runtime};
use super::*;
use std::path::Path;

const TARGET: &str = "01ARZ3NDEKTSV4RRFFQ69G5FC4";

fn strs(arguments: &[String]) -> Vec<&str> {
    arguments.iter().map(String::as_str).collect()
}

fn write_pool(root: &Path, pool: &Value) {
    let directory = root.join(".git").join("hotsheet-coordination");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join("state.json"),
        serde_json::to_vec(pool).unwrap(),
    )
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hs2_tk9adc_role_sessions_set_headlessly_and_the_pool_reported_with_capacity() {
    assert!(hotsheet_aitools::test_permission_vault_available());
    hotsheet_aitools::enable_test_permission_vault();
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let source = root.path().join("host-store");
    let memory = Arc::new(Memory::default());
    let host = state(&source, &home);
    let owner = configured_owner(&source, &home, &root.path().join("owner"));
    let status = hotsheet_ticketing::git::launch("git")
        .arg("-C")
        .arg(&owner.root)
        .args(["init", "-q"])
        .status()
        .unwrap();
    assert!(status.success());
    let host = host.with_orchestration_runtime(shared_runtime(&source, &home, &memory));
    let router = app(host.clone());
    let base = format!("/checkouts/{}/orchestration", owner.id);
    api(
        router.clone(),
        "POST",
        &format!("{base}/targets"),
        json!({"target_id":TARGET,"label":"Synthetic","expected_revision":null}),
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
    let body = json!({
        "machine": {"max_sessions": 4},
        "tools": [{"tool":"codex","max_sessions":2}],
        "targets": [{"target_id":TARGET,"tools":[{"tool":"codex"}],"max_sessions":3}],
        "placement": {"resources": [{"target_id":TARGET,
            "resources":[{"namespace":"vm","resource":"macos-vm"}]}]}
    });
    let file = root.path().join("policy.json");
    std::fs::write(&file, serde_json::to_vec(&body).unwrap()).unwrap();
    cli_named(
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
    let owner_id = owner.id.clone();
    let roles = |extra: &[&'static str]| -> Vec<String> {
        [
            "policy",
            "role-sessions",
            "--project",
            &owner_id,
            "--target",
            TARGET,
        ]
        .iter()
        .chain(extra)
        .map(|argument| (*argument).to_owned())
        .collect()
    };
    let both = roles(&["--role", "dispatcher", "--role", "integrator"]);
    let both: Vec<&str> = both.iter().map(String::as_str).collect();
    // An AI actor may not change the policy; nothing is written.
    assert!(cli_failure(&home, &ai, &both).contains("forbidden"));
    // A role the scheduler does not start is refused before anything is read.
    assert!(
        cli_failure(&home, &human, &strs(&roles(&["--role", "worker"]))).contains("invalid value")
    );
    let set = cli_named(&home, &human, &both);
    assert_eq!(
        set["policy"]["placement"]["role_sessions"],
        json!([{"target_id":TARGET,"roles":["dispatcher","integrator"]}])
    );
    // Every other member round-trips.
    assert_eq!(
        set["policy"]["placement"]["resources"],
        body["placement"]["resources"]
    );
    // Repeated: the same revision and the same bytes.
    let policy_path = Path::new(&owner.root).join(".hotsheet2/orchestration-policy.json");
    let bytes = std::fs::read(&policy_path).unwrap();
    assert_eq!(cli_named(&home, &human, &both)["revision"], set["revision"]);
    assert_eq!(std::fs::read(&policy_path).unwrap(), bytes);
    // The service reads what the CLI wrote, as the Limits form would.
    let (status, read) = api(
        router.clone(),
        "GET",
        &format!("{base}/policy"),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{read}");
    assert_eq!(read["revision"], set["revision"]);
    assert_eq!(
        read["policy"]["placement"]["role_sessions"][0]["roles"][1],
        "integrator"
    );
    // Narrowed to the dispatcher, then turned off: the entry goes, the rest stays.
    let narrowed = cli_named(&home, &human, &strs(&roles(&["--role", "dispatcher"])));
    assert_eq!(
        narrowed["policy"]["placement"]["role_sessions"][0]["roles"],
        json!(["dispatcher"])
    );
    let off = cli_named(&home, &human, &strs(&roles(&[])));
    assert!(
        off["policy"]["placement"].get("role_sessions").is_none(),
        "{off}"
    );
    assert_eq!(
        off["policy"]["placement"]["resources"],
        body["placement"]["resources"]
    );

    // The host reports the checkout's pool with its capacity: empty without a pool,
    // then a live dispatcher and two handoffs not yet done, and the version changes.
    let capacity = format!("/checkouts/{}/target-capacity", owner.id);
    let (status, empty) = api(router.clone(), "GET", &capacity, Value::Null, true).await;
    assert_eq!(status, StatusCode::OK, "{empty}");
    assert_eq!(
        empty["pool"],
        json!({"dispatcher_live":false,"integrator_live":false,"handoffs_pending":0})
    );
    let assignment = |id: &str, phase: &str| {
        json!({"id":id,"worker":"someone","generation":"g1",
            "ticket":{"connection_id":"target-source","native_id":"01ARZ3NDEKTSV4RRFFQ69G5FC5"},
            "slug":"TGT-1","base":"0".repeat(40),"phase":phase,
            "return_route":{"kind":"human_relay"}})
    };
    let far = 4_000_000_000_i64;
    let mut pool: Value = serde_json::to_value(hotsheet_cli::coordination::State::new(
        "project".into(),
        "source".into(),
    ))
    .unwrap();
    pool["dispatcher"] = json!({"id":"someone","generation":"g1","expires_at":far,
        "return_route":{"kind":"human_relay"}});
    for (id, phase) in [
        ("a1", "handoff"),
        ("a2", "integrating"),
        ("a3", "accepted"),
        ("a4", "done"),
    ] {
        pool["assignments"][id] = assignment(id, phase);
    }
    write_pool(Path::new(&owner.root), &pool);
    let (_, reported) = api(router.clone(), "GET", &capacity, Value::Null, true).await;
    assert_eq!(
        reported["pool"],
        json!({"dispatcher_live":true,"integrator_live":false,"handoffs_pending":2}),
        "{reported}"
    );
    assert_ne!(reported["version"], empty["version"]);
}
