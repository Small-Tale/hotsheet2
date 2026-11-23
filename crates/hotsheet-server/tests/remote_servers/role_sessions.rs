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
    // Narrowed to the dispatcher, then turned off (an entry with no roles, HS2-R2N33M),
    // then back to Auto: the entry goes, the rest stays.
    let narrowed = cli_named(&home, &human, &strs(&roles(&["--role", "dispatcher"])));
    assert_eq!(
        narrowed["policy"]["placement"]["role_sessions"][0]["roles"],
        json!(["dispatcher"])
    );
    let none = cli_named(&home, &human, &strs(&roles(&[])));
    assert_eq!(
        none["policy"]["placement"]["role_sessions"],
        json!([{"target_id":TARGET,"roles":[]}]),
        "{none}"
    );
    let off = cli_named(&home, &human, &strs(&roles(&["--auto"])));
    assert!(
        off["policy"]["placement"].get("role_sessions").is_none(),
        "{off}"
    );
    assert!(
        cli_failure(
            &home,
            &human,
            &strs(&roles(&["--auto", "--role", "dispatcher"]))
        )
        .contains("cannot be used with")
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

/// HS2-SYZB33: a role-sessions entry on a site that is not its project's home starts
/// nothing. The owned CLI still stores it, so stored policies keep working, but says
/// so in `warnings` and on stderr, pointing at the home; validate and the dry run
/// report the same warning code and stay valid. The home's own entry warns nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hs2_syzb33_role_sessions_on_a_site_say_they_have_no_effect() {
    const SITE: &str = "01ARZ3NDEKTSV4RRFFQ69G5FC6";
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
    let (status, declared) = api(
        router.clone(),
        "POST",
        &format!("{base}/targets"),
        json!({"target_id":TARGET,"label":"Synthetic","expected_revision":null}),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{declared}");
    let (status, declared) = api(
        router.clone(),
        "POST",
        &format!("{base}/targets"),
        json!({"target_id":SITE,"label":"Synthetic second clone","site_of":TARGET,
            "expected_revision":declared["revision"]}),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{declared}");
    let human = [
        "--actor-role",
        "human",
        "--actor-id",
        "maintainer",
        "-C",
        source.to_str().unwrap(),
        "orchestration",
    ];
    let body = json!({
        "machine": {"max_sessions": 4},
        "tools": [{"tool":"codex","max_sessions":2}],
        "targets": [{"target_id":TARGET,"tools":[{"tool":"codex"}],"max_sessions":3},
            {"target_id":SITE,"tools":[{"tool":"codex"}],"max_sessions":3}],
    });
    let file = root.path().join("policy.json");
    std::fs::write(&file, serde_json::to_vec(&body).unwrap()).unwrap();
    let owner_id = owner.id.as_str();
    cli_named(
        &home,
        &human,
        &[
            "policy",
            "set",
            "--project",
            owner_id,
            "--file",
            file.to_str().unwrap(),
        ],
    );
    let on = |target: &'static str, roles: &[&'static str]| -> Vec<&str> {
        let mut arguments = vec![
            "policy",
            "role-sessions",
            "--project",
            owner_id,
            "--target",
            target,
        ];
        for role in roles {
            arguments.extend(["--role", role]);
        }
        arguments
    };
    let expected = json!([{"code":"role_sessions_not_home",
        "message":"role sessions start only on the project's home site; this entry has no effect",
        "subject":SITE,"home_target_id":TARGET}]);

    // On the site: stored as asked, and reported as having no effect.
    let set = cli_named(&home, &human, &on(SITE, &["dispatcher"]));
    assert_eq!(
        set["policy"]["placement"]["role_sessions"],
        json!([{"target_id":SITE,"roles":["dispatcher"]}])
    );
    assert_eq!(set["warnings"], expected, "{set}");
    let printed = cli_outcome(&home, &human, &on(SITE, &["dispatcher"]));
    assert!(
        printed.contains(&format!(
            "warning: role_sessions_not_home: target {SITE} is a site of the project whose home is {TARGET}"
        )),
        "{printed}"
    );
    // The service still reads the stored policy; validate and the dry run warn too.
    let (status, read) = api(
        router.clone(),
        "GET",
        &format!("{base}/policy"),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{read}");
    for route in ["validate", "dry-run"] {
        let (status, answer) = api(
            router.clone(),
            "POST",
            &format!("{base}/policy/{route}"),
            json!({}),
            true,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{route}: {answer}");
        assert_eq!(answer["valid"], true, "{route}: {answer}");
        assert_eq!(answer["warnings"], expected, "{route}: {answer}");
    }

    // Cleared on the site and set on the home: nothing to warn about.
    let cleared = cli_named(&home, &human, &on(SITE, &[]));
    assert!(cleared.get("warnings").is_none(), "{cleared}");
    let on_home = cli_named(&home, &human, &on(TARGET, &["dispatcher", "integrator"]));
    assert!(on_home.get("warnings").is_none(), "{on_home}");
    let printed = cli_outcome(&home, &human, &on(TARGET, &["dispatcher", "integrator"]));
    assert!(!printed.contains("warning:"), "{printed}");
    let (_, answer) = api(
        router.clone(),
        "POST",
        &format!("{base}/policy/validate"),
        json!({}),
        true,
    )
    .await;
    assert!(answer.get("warnings").is_none(), "{answer}");
}
