//! HS2-HCW7MD: Propose Plan on a real server. The planner is a real PTY running the
//! workspace fake agent in the orchestration project itself; no AI tool is launched
//! and nothing leaves loopback.
use super::session_work::{REPORT_TOOL, World, launch_report, received_prompt};
use super::sessions::drawer;
use super::support::{cli_failure, cli_named, mcp_named, mcp_rejected};
use super::*;
use std::path::Path;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_hcw7md_propose_plan_starts_the_planner_in_the_orchestration_project() {
    // The project orchestrates itself as its one target; the plugin takes its first
    // prompt on the command line, so the planner's prompt arrives at launch.
    let world = World::new(
        &["tickets_read", "sessions_manage"],
        "",
        r#"prompt_args = ["--prompt", "{prompt}"]"#,
    )
    .await;
    let person = json!({"role":"human","id":"maintainer"});
    let store_arg = world.project_store.to_str().unwrap().to_owned();
    let as_person = [
        "--actor-role",
        "human",
        "--actor-id",
        "maintainer",
        "-C",
        store_arg.as_str(),
        "orchestration",
        "plan",
        "--project",
        world.checkout.id.as_str(),
    ];
    // The headless CLI answers the same refusal before anything launches.
    let refusal = cli_failure(&world.home, &as_person, &["propose", "--tool", REPORT_TOOL]);
    assert!(refusal.contains("plan_no_goals"), "{refusal}");
    // No open goal ticket yet: nothing launches.
    let (status, refused) = world
        .call(
            "POST",
            "/planning/propose",
            json!({"tool":REPORT_TOOL,"actor":person}),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["code"], "plan_no_goals", "{refused}");
    assert!(drawer(&world.router).await.is_empty());

    let goal = world.ticket("Ship the launch");
    let (status, proposed) = world
        .call(
            "POST",
            "/planning/propose",
            json!({"tool":REPORT_TOOL,"operation_id":"press-1","actor":person}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{proposed}");
    assert_eq!(proposed["outcome"], "started", "{proposed}");
    assert_eq!(proposed["proposal_id"], "press-1");
    assert_eq!(proposed["targets"], 1);
    assert_eq!(
        proposed["goals"][0]["slug"],
        goal.slug.as_str(),
        "{proposed}"
    );
    assert_eq!(proposed["role_prompt"], "at_launch", "{proposed}");
    let terminal = proposed["session"]["session"]["session_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let worker = proposed["session"]["worker_id"]
        .as_str()
        .unwrap()
        .to_owned();

    // It runs in the orchestration project, set up for its tool, as the planner.
    let report = launch_report(&world.reports, &terminal);
    assert_eq!(
        Path::new(report["cwd"].as_str().unwrap()),
        world.project.canonicalize().unwrap(),
        "{report}"
    );
    assert_eq!(report["files"]["AGENTS.md"], true, "{report}");
    assert_eq!(report["files"][".mcp.json"], true, "{report}");
    assert_eq!(
        report["env"]["HOTSHEET_SESSION_ROLE"], "planner",
        "{report}"
    );
    assert_eq!(
        report["env"]["HOTSHEET_ROLE_SKILL"], "hotsheet-planner",
        "{report}"
    );
    assert_eq!(
        report["env"]["HOTSHEET_WORKER_ID"],
        worker.as_str(),
        "{report}"
    );
    let prompt = received_prompt(&world.reports, &terminal, "`hotsheet-planner`");
    for part in [world.checkout.id.as_str(), "press-1", worker.as_str()] {
        assert!(prompt.contains(part), "{prompt} lacks {part}");
    }

    // It shows in the drawer, labelled as the project's planner.
    let terminals = drawer(&world.router).await;
    assert_eq!(terminals.len(), 1, "{terminals:?}");
    assert!(
        terminals[0]["name"]
            .as_str()
            .is_some_and(|name| name.ends_with(": planner")),
        "{terminals:?}"
    );

    // Pressing again answers with the same planner; nothing else launches.
    let (status, again) = world
        .call(
            "POST",
            "/planning/propose",
            json!({"tool":REPORT_TOOL,"actor":person}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again["outcome"], "existing", "{again}");
    assert_eq!(again["proposal_id"], "press-1");
    assert_eq!(drawer(&world.router).await.len(), 1);

    let (status, shown) = world.call("POST", "/planning/show", json!({})).await;
    assert_eq!(status, StatusCode::OK, "{shown}");
    assert_eq!(shown["state"], "planning", "{shown}");
    assert_eq!(shown["proposal_id"], "press-1");
    assert_eq!(shown["goal_count"], 1);
    assert_eq!(shown["steps"], 0);
    assert_eq!(shown["placement_automatic"], false);

    // The CLI process reads the same records: the same planner answers, and it does
    // not need the terminal host to say so.
    let from_cli = cli_named(&world.home, &as_person, &["propose", "--tool", REPORT_TOOL]);
    assert_eq!(from_cli["outcome"], "existing", "{from_cli}");
    assert_eq!(from_cli["proposal_id"], "press-1");
    let shown_cli = cli_named(&world.home, &as_person, &["show"]);
    assert_eq!(shown_cli["state"], "planning", "{shown_cli}");
    assert_eq!(shown_cli["goals"][0]["slug"], goal.slug.as_str());

    // The stdio MCP tool reaches the same runtime through this server.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let router = world.router.clone();
    let task = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = stopped.await;
            })
            .await
            .unwrap();
    });
    let backend = ["--server", origin.as_str(), "--secret", SECRET];
    let as_worker = |mut arguments: Value| {
        arguments["checkout"] = json!(world.checkout.id);
        arguments["actor_role"] = json!("ai");
        arguments["actor_id"] = json!("scheduler-1");
        arguments
    };
    let call = |arguments: Value| {
        mcp_named(
            &world.home,
            &backend,
            "hotsheet_targets",
            as_worker(arguments),
        )
    };
    let mcp_shown = call(json!({"action":"plan_show"}));
    let mcp_pressed = call(json!({"action":"plan_propose","tool":REPORT_TOOL}));
    // A tool is required.
    mcp_rejected(
        &world.home,
        &backend,
        "hotsheet_targets",
        as_worker(json!({"action":"plan_propose"})),
    );
    let _ = stop.send(());
    task.await.unwrap();
    assert_eq!(mcp_shown["state"], "planning", "{mcp_shown}");
    assert_eq!(mcp_pressed["outcome"], "existing", "{mcp_pressed}");
    assert_eq!(mcp_pressed["proposal_id"], "press-1");
    assert_eq!(drawer(&world.router).await.len(), 1);

    // The press is in the audit journal as identifiers only.
    let (_, audit) = world.call("GET", "/audit", Value::Null).await;
    let pressed: Vec<&Value> = audit["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["action"] == "plan_propose")
        .collect();
    // Refused, started, then answered with the same planner by the route, the CLI and
    // the MCP tool.
    let reasons: Vec<&str> = pressed
        .iter()
        .map(|entry| entry["reason"].as_str().unwrap())
        .collect();
    assert_eq!(
        reasons,
        [
            "plan_no_goals",
            "plan_no_goals",
            "plan_started",
            "plan_existing",
            "plan_existing",
            "plan_existing",
        ],
        "{audit}"
    );
    assert_eq!(pressed[2]["operation_id"], "press-1");
}
