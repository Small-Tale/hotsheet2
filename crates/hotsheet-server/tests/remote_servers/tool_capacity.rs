//! HS2-AHG7HW: tool capacity through the real authenticated service, the owned CLI
//! process and the stdio MCP tool. Synthetic accounts only; no session is started.
use super::orchestration::{api, configured_owner, shared_runtime};
use super::*;
use std::path::Path;

const TARGET: &str = "01ARZ3NDEKTSV4RRFFQ69G5FC1";

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis()
        .try_into()
        .unwrap()
}
/// `tool[/account]` to `state[:reason]`, in policy order.
fn states(view: &Value) -> Vec<String> {
    view["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            let mut name = row["tool"].as_str().unwrap().to_owned();
            if let Some(account) = row["account"].as_str() {
                name = format!("{name}/{account}");
            }
            match row["reason"].as_str() {
                Some(reason) => format!("{name}={}:{reason}", row["state"].as_str().unwrap()),
                None => format!("{name}={}", row["state"].as_str().unwrap()),
            }
        })
        .collect()
}
fn order(selection: &Value) -> Vec<String> {
    selection["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|candidate| {
            format!(
                "{}/{}={}",
                candidate["tool"].as_str().unwrap(),
                candidate["account"].as_str().unwrap_or("-"),
                candidate["state"].as_str().unwrap()
            )
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hs2_ahg7hw_cli_http_and_mcp_share_tool_capacity_signals_and_selection() {
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
    let plain = ["-C", source.to_str().unwrap(), "orchestration"];
    let registry = home.join("orchestration-tool-capacity.json");
    let show = ["tool-capacity", "show", "--project", &owner.id];
    // Headless, before any policy: nothing configured, nothing created.
    assert_eq!(cli_named(&home, &plain, &show)["enabled"], false);
    assert!(
        cli_failure(
            &home,
            &plain,
            &[
                "tool-capacity",
                "select",
                "--project",
                &owner.id,
                "--target",
                TARGET
            ]
        )
        .contains("invalid_request")
    );
    assert!(!registry.exists());
    let body = json!({
        "machine": {"max_sessions": 4},
        "tools": [
            {"tool":"claude","max_sessions":2,"weight":80,"accounts":[
                {"account":"team-a","max_sessions":1,"budget_cents_per_day":500},
                {"account":"team-b","max_sessions":1}]},
            {"tool":"codex","max_sessions":1,"weight":60},
            {"tool":"opencode","max_sessions":1,"weight":90}],
        "targets": [{"target_id":TARGET,"actions":["sessions_manage"],
            "tools":[{"tool":"claude"},{"tool":"codex"},{"tool":"opencode"}],"max_sessions":2}]
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
    // Configured but unobserved: nothing is available, and reads still write nothing.
    let shown = cli_named(&home, &plain, &show);
    assert_eq!(
        states(&shown),
        [
            "claude/team-a=unknown:not_observed",
            "claude/team-b=unknown:not_observed",
            "codex=unknown:not_observed",
            "opencode=unknown:not_reported",
        ]
    );
    assert_eq!(
        shown["tools"][3]["signals"],
        json!({"limits":false,"reset_times":false,"usage":true,"cost":false})
    );
    assert!(!registry.exists());

    // The CLI reports signals; the same report again changes nothing.
    let started = now_ms();
    let at = started.to_string();
    let resets = (started + 3 * 60 * 60 * 1000).to_string();
    let signal = |extra: &[&str]| {
        let mut arguments = vec!["tool-capacity", "signal", "--project", &owner.id];
        arguments.extend(extra);
        arguments.extend(["--at-ms", &at]);
        arguments
            .iter()
            .map(|value| (*value).to_owned())
            .collect::<Vec<_>>()
    };
    let run = |prefix: &[&str], arguments: Vec<String>| {
        let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
        cli_named(&home, prefix, &arguments)
    };
    for account in ["team-a", "team-b"] {
        let used = run(
            &plain,
            signal(&[
                "--tool",
                "claude",
                "--account",
                account,
                "--kind",
                "usage",
                "--cost-cents",
                "40",
            ]),
        );
        assert_eq!(used["state"], "available");
    }
    let limited = signal(&[
        "--tool",
        "claude",
        "--account",
        "team-a",
        "--kind",
        "rate_limit",
        "--resets-at-ms",
        &resets,
    ]);
    // An AI session may report the limit it ran into.
    let first = run(&ai, limited.clone());
    assert_eq!(
        first,
        json!({"applied":"applied","tool":"claude","account":"team-a","state":"limited",
            "until_ms":started + 3 * 60 * 60 * 1000,"reason":"rate_limit"})
    );
    let bytes = std::fs::read(&registry).unwrap();
    assert_eq!(run(&plain, limited)["applied"], "duplicate");
    assert_eq!(std::fs::read(&registry).unwrap(), bytes);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&registry).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    // Refused reports leave the registry untouched.
    let refuse = |prefix: &[&str], arguments: Vec<String>| {
        let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
        cli_failure(&home, prefix, &arguments)
    };
    let recovered = signal(&[
        "--tool",
        "claude",
        "--account",
        "team-a",
        "--kind",
        "recovered",
    ]);
    assert!(refuse(&ai, recovered.clone()).contains("forbidden"));
    for arguments in [
        signal(&["--tool", "antigravity", "--kind", "usage"]),
        signal(&["--tool", "claude", "--account", "team-z", "--kind", "usage"]),
        signal(&["--tool", "codex", "--kind", "paused"]),
        signal(&[
            "--tool",
            "codex",
            "--kind",
            "usage",
            "--resets-at-ms",
            &resets,
        ]),
    ] {
        assert!(refuse(&plain, arguments).contains("invalid_request"));
    }
    assert_eq!(std::fs::read(&registry).unwrap(), bytes);

    // The service reads the same machine-local state, authenticated only.
    let capacity = format!("{base}/tool-capacity");
    assert_eq!(
        api(router.clone(), "GET", &capacity, Value::Null, false)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, served) = api(router.clone(), "GET", &capacity, Value::Null, true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        states(&served),
        [
            "claude/team-a=limited:rate_limit",
            "claude/team-b=available",
            "codex=unknown:not_observed",
            "opencode=unknown:not_reported",
        ]
    );
    assert_eq!(served["tools"][0]["spent_cents_today"], 40);
    // A service report for Codex, then a selection that prefers the limited account's
    // tool: the sibling account leads, and the limited one is named with its end.
    let (status, reported) = api(
        router.clone(),
        "POST",
        &format!("{capacity}/signals"),
        json!({"tool":"codex","kind":"usage","at_ms":started,"cost_cents":3,
            "session":"session-1"}),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reported["state"], "available");
    let select = format!("{capacity}/select");
    let (status, selected) = api(
        router.clone(),
        "POST",
        &select,
        json!({"target_id":TARGET,"tool":"claude","load":[]}),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        order(&selected),
        [
            "claude/team-b=available",
            "codex/-=available",
            "opencode/-=unknown"
        ]
    );
    assert_eq!(
        selected["excluded"],
        json!([{"tool":"claude","account":"team-a","excluded":"limited","state":"limited",
            "until_ms":started + 3 * 60 * 60 * 1000,"reason":"rate_limit"}])
    );
    for (body, status, code) in [
        (
            json!({"target_id":TARGET}),
            StatusCode::BAD_REQUEST,
            "invalid_request",
        ),
        (
            json!({"tool":"codex","kind":"recovered","at_ms":started,
                "actor":{"role":"ai","id":"worker-1"}}),
            StatusCode::FORBIDDEN,
            "forbidden",
        ),
    ] {
        let leaf = if body.get("kind").is_some() {
            "signals"
        } else {
            "select"
        };
        let (found, refused) = api(
            router.clone(),
            "POST",
            &format!("{capacity}/{leaf}"),
            body,
            true,
        )
        .await;
        assert_eq!((found, refused["code"].as_str()), (status, Some(code)));
    }

    // With the sibling account at its cap the CLI selection fails over to Codex.
    let chosen = cli_named(
        &home,
        &plain,
        &[
            "tool-capacity",
            "select",
            "--project",
            &owner.id,
            "--target",
            TARGET,
            "--tool",
            "claude",
            "--load",
            "claude/team-b=1",
        ],
    );
    assert_eq!(order(&chosen), ["codex/-=available", "opencode/-=unknown"]);
    assert_eq!(chosen["excluded"][1]["excluded"], "account_cap");
    assert!(
        cli_failure(
            &home,
            &plain,
            &[
                "tool-capacity",
                "select",
                "--project",
                &owner.id,
                "--target",
                TARGET,
                "--load",
                "claude",
            ]
        )
        .contains("--load must be")
    );

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
    let before_mcp = std::fs::read(&registry).unwrap();
    let call = |arguments: Value| mcp_named(&home, &backend, "hotsheet_targets", arguments);
    assert_eq!(
        states(&call(json!({"action":"tool_capacity","checkout":owner.id}))),
        [
            "claude/team-a=limited:rate_limit",
            "claude/team-b=available",
            "codex=available",
            "opencode=unknown:not_reported",
        ]
    );
    let chosen = call(
        json!({"action":"tool_select","checkout":owner.id,"target_id":TARGET,
        "tool":"codex","tool_load":[{"tool":"claude","account":"team-b","sessions":0}]}),
    );
    assert_eq!(order(&chosen)[0], "codex/-=available");
    assert_eq!(chosen["candidates"][0]["requested"], true);
    for arguments in [
        json!({"action":"tool_select","checkout":owner.id,"target_id":TARGET}),
        json!({"action":"tool_signal","checkout":owner.id,"tool":"codex","kind":"recovered"}),
        json!({"action":"tool_capacity_signal","checkout":owner.id,"tool":"codex"}),
    ] {
        mcp_rejected(&home, &backend, "hotsheet_targets", arguments);
    }
    assert_eq!(std::fs::read(&registry).unwrap(), before_mcp);

    // A person clears the limit; the change is audited once, with codes only.
    assert_eq!(run(&human, recovered)["state"], "available");
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
                "{}:{}:{}",
                entry["action"].as_str().unwrap(),
                entry["outcome"].as_str().unwrap(),
                entry["reason"].as_str().unwrap_or("-")
            )
        })
        .collect();
    assert_eq!(
        actions,
        [
            "policy_set:changed:-",
            "tool_capacity_usage:changed:available",
            "tool_capacity_usage:changed:available",
            "tool_capacity_rate_limit:changed:limited",
            "tool_capacity_usage:changed:available",
            "tool_capacity_recovered:changed:available",
        ]
    );
    // Corrupt data fails closed for the CLI and the service, and is kept.
    std::fs::write(&registry, b"corrupt").unwrap();
    assert!(cli_failure(&home, &plain, &show).contains("corrupt_orchestration_data"));
    let (status, failed) = api(router.clone(), "GET", &capacity, Value::Null, true).await;
    assert_ne!(status, StatusCode::OK);
    assert_eq!(failed["code"], "corrupt_orchestration_data");
    assert_eq!(std::fs::read(&registry).unwrap(), b"corrupt");
    assert!(
        Path::new(&owner.root)
            .join(".hotsheet2/orchestration-policy.json")
            .exists()
    );
    let _ = stop.send(());
    tokio::time::timeout(std::time::Duration::from_secs(10), task)
        .await
        .unwrap()
        .unwrap();
}
