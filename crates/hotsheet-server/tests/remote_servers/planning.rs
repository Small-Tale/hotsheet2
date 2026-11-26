//! HS2-HCW7MD: Propose Plan on a real server. The planner is a real PTY running the
//! workspace fake agent in the orchestration project itself; no AI tool is launched
//! and nothing leaves loopback.
use super::orchestration::api;
use super::session_work::{
    PUSH_AGENT, PUSH_PLUGIN, REPORT_TOOL, TARGET, World, launch_report, received_prompt,
};
use super::sessions::drawer;
use super::support::{cli_named, mcp_named, mcp_rejected};
use super::*;
use std::path::Path;

/// `plan show` once the server sees the planner idle in its open tab. The fake records a
/// prompt before the server records its delivery, so this waits, woken by the machine
/// session records changing, rather than reading once (HS2-FDS95D flake under load).
async fn replannable(world: &World) -> Value {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        let waiter = hotsheet_jobs::ChangeWaiter::starting(&[world.home.as_path()]).unwrap();
        let (_, shown) = world.call("POST", "/planning/show", json!({})).await;
        if shown["replannable"] == true {
            return shown;
        }
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        assert!(
            !remaining.is_zero(),
            "the planner never became replannable: {shown}"
        );
        tokio::task::block_in_place(|| waiter.wait(remaining));
    }
}

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
    // No open goal ticket yet: `plan show` names the implicit default goal (HS2-YSRT6Y),
    // and nothing launches until a press.
    let (status, idle) = world.call("POST", "/planning/show", json!({})).await;
    assert_eq!(status, StatusCode::OK, "{idle}");
    assert_eq!(idle["default_goal"], true, "{idle}");
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
        hotsheet_sync::paths::canonicalize(&world.project).unwrap(),
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
    assert_eq!(shown["default_goal"], false, "{shown}");
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
    // Started, then answered with the same planner by the route, the CLI and the MCP
    // tool.
    let reasons: Vec<&str> = pressed
        .iter()
        .map(|entry| entry["reason"].as_str().unwrap())
        .collect();
    assert_eq!(
        reasons,
        [
            "plan_started",
            "plan_existing",
            "plan_existing",
            "plan_existing",
        ],
        "{audit}"
    );
    assert_eq!(pressed[0]["operation_id"], "press-1");
}

/// HS2-5RYXNN: on a real server, a planner whose tool reports its session start through
/// its hooks shows `tool_ready: false` in `plan show` until that report arrives, as it
/// does while the tool waits on a first-run question in its terminal, then `true`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_5ryxnn_plan_show_says_the_planner_waits_until_its_tool_reports() {
    let world = World::new(
        &["tickets_read", "sessions_manage"],
        "",
        r#"prompt_args = ["--prompt", "{prompt}"]
[hooks]
target = ".synthetic/hooks.json"
machine_local = true
event = "PermissionRequest"
additional_events = ["SessionStart"]
command = "hotsheet-cli permission-hook --agent synthetic""#,
    )
    .await;
    world.ticket("Ship the launch");
    let person = json!({"role":"human","id":"maintainer"});
    let (status, proposed) = world
        .call(
            "POST",
            "/planning/propose",
            json!({"tool":REPORT_TOOL,"operation_id":"press-1","actor":person}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{proposed}");
    assert_eq!(proposed["session"]["tool_ready"], false, "{proposed}");
    let terminal = proposed["session"]["session"]["session_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let (_, shown) = world.call("POST", "/planning/show", json!({})).await;
    assert_eq!(shown["state"], "planning", "{shown}");
    assert_eq!(shown["session"]["tool_ready"], false, "{shown}");
    // HS2-G2X7RT: the drawer tab's listing carries the same answer.
    let tab_ready = || async {
        let (status, terminals) =
            api(world.router.clone(), "GET", "/terminals", Value::Null, true).await;
        assert_eq!(status, StatusCode::OK, "{terminals}");
        terminals
            .as_array()
            .unwrap()
            .iter()
            .find(|tab| tab["id"] == terminal.as_str())
            .unwrap()["tool_ready"]
            .clone()
    };
    assert_eq!(tab_ready().await, false);

    // The tool got past its questions: its SessionStart hook reports in.
    let (status, _) = api(
        world.router.clone(),
        "POST",
        &format!("/terminals/{terminal}/ai-connection"),
        json!({"agent":"synthetic"}),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, shown) = world.call("POST", "/planning/show", json!({})).await;
    assert_eq!(shown["session"]["tool_ready"], true, "{shown}");
    assert_eq!(tab_ready().await, true);
    // The hosted listing of the checkout says the same.
    let (status, listed) = api(
        world.router.clone(),
        "GET",
        &format!("/checkouts/{}/target-sessions", world.checkout.id),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    assert_eq!(listed["sessions"][0]["tool_ready"], true, "{listed}");
}

/// HS2-FDS95D: on a real server with the push-capable fake agent, a press while the
/// planner that proposed the plan sits idle in its open tab pushes the new proposal
/// into it: no second planner, the prompt names the new proposal, a repeat pushes
/// nothing, and `plan show` names the latest proposal.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_fds95d_a_press_replans_the_idle_planner_in_its_open_tab() {
    let world = World::new(
        &["tickets_read", "sessions_manage"],
        PUSH_AGENT,
        PUSH_PLUGIN,
    )
    .await;
    world.ticket("Ship the launch");
    let person = json!({"role":"human","id":"maintainer"});
    let press = |operation: &'static str| {
        let world = &world;
        let person = person.clone();
        async move {
            let (status, answer) = world
                .call(
                    "POST",
                    "/planning/propose",
                    json!({"tool":REPORT_TOOL,"operation_id":operation,"actor":person}),
                )
                .await;
            assert_eq!(status, StatusCode::OK, "{answer}");
            answer
        }
    };
    let first = press("press-1").await;
    assert_eq!(first["outcome"], "started", "{first}");
    let terminal = first["session"]["session"]["session_id"]
        .as_str()
        .unwrap()
        .to_owned();
    // The tool comes up and ends a turn; its role prompt reaches it then.
    let prompt = received_prompt(&world.reports, &terminal, "`hotsheet-planner`");
    assert!(prompt.contains("press-1"), "{prompt}");
    // It planned and sits idle in its open tab.
    let shown = replannable(&world).await;
    assert_eq!(shown["state"], "planning", "{shown}");

    let again = press("press-2").await;
    assert_eq!(again["outcome"], "replanned", "{again}");
    assert_eq!(again["proposal_id"], "press-2", "{again}");
    assert_eq!(again["session"]["session"]["session_id"], terminal.as_str());
    let prompt = received_prompt(&world.reports, &terminal, "press-2");
    assert!(prompt.contains("`hotsheet-planner`"), "{prompt}");
    assert_eq!(drawer(&world.router).await.len(), 1, "no second planner");

    // The same press again pushes nothing more.
    let repeated = press("press-2").await;
    assert_eq!(repeated["outcome"], "existing", "{repeated}");
    assert_eq!(repeated["proposal_id"], "press-2", "{repeated}");
    let prompts =
        std::fs::read_to_string(world.reports.join(format!("{terminal}.prompts"))).unwrap();
    assert_eq!(prompts.lines().count(), 2, "{prompts}");
    let (_, shown) = world.call("POST", "/planning/show", json!({})).await;
    assert_eq!(shown["proposal_id"], "press-2", "{shown}");

    let (_, audit) = world.call("GET", "/audit", Value::Null).await;
    let reasons: Vec<&str> = audit["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["action"] == "plan_propose")
        .map(|entry| entry["reason"].as_str().unwrap())
        .collect();
    assert_eq!(
        reasons,
        ["plan_started", "plan_replanned", "plan_existing"],
        "{audit}"
    );
}

/// The step ids of the live plan, or with `update` of that plan update's draft.
async fn plan_steps(world: &World, update: Option<&str>) -> Vec<String> {
    let body = match update {
        Some(update) => json!({"plan_update": update}),
        None => json!({}),
    };
    let (status, plan) = world.call("POST", "/readiness/plan", body).await;
    assert_eq!(status, StatusCode::OK, "{plan}");
    let mut steps: Vec<String> = plan["steps"]
        .as_array()
        .unwrap_or_else(|| panic!("{plan}"))
        .iter()
        .map(|view| view["step"]["step_id"].as_str().unwrap().to_owned())
        .collect();
    steps.sort();
    steps
}

/// HS2-PW43N9 (HS2-190RDX): on a real server with the push-capable fake agent, Update
/// Plan while the planner sits idle in its open tab re-prompts that planner with a
/// prompt naming the plan update and its draft. No second planner launches, the draft
/// changes while the live plan keeps its steps, and `update_finish` through the HTTP
/// route swaps the live plan to the draft in one write.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_pw43n9_update_plan_reprompts_the_planner_and_finish_swaps_the_plan() {
    let world = World::new(
        &["tickets_read", "sessions_manage"],
        PUSH_AGENT,
        PUSH_PLUGIN,
    )
    .await;
    let goal = world.ticket("Ship the launch");
    let kept = world.ticket("Add the synthetic token endpoint");
    let dropped = world.ticket("Add the synthetic legacy form");
    let added = world.ticket("Add the synthetic audit log");
    let person = json!({"role":"human","id":"maintainer"});
    let (status, proposed) = world
        .call(
            "POST",
            "/planning/propose",
            json!({"tool":REPORT_TOOL,"operation_id":"press-1","actor":person}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{proposed}");
    assert_eq!(proposed["outcome"], "started", "{proposed}");
    let terminal = proposed["session"]["session"]["session_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let worker = proposed["session"]["worker_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let planner = json!({"role":"ai","id":worker});
    received_prompt(&world.reports, &terminal, "press-1");

    // The planner proposes the live plan: two steps of the goal.
    let define = |native: String, update: Option<String>| {
        let world = &world;
        let mut body = json!({"actor":planner,"ticket":{"native_id":goal.slug},
            "target_id":TARGET,"connection_id":world.checkout.sources[0].connection_id,
            "native_id":native,"prerequisites":[]});
        if let Some(update) = update {
            body["plan_update"] = json!(update);
        }
        async move {
            let (status, defined) = world.call("POST", "/readiness/define", body).await;
            assert_eq!(status, StatusCode::OK, "{defined}");
            defined["step"]["step_id"].as_str().unwrap().to_owned()
        }
    };
    let kept_step = define(kept.slug.clone(), None).await;
    let dropped_step = define(dropped.slug.clone(), None).await;
    let mut live = vec![kept_step.clone(), dropped_step.clone()];
    live.sort();
    assert_eq!(plan_steps(&world, None).await, live);
    replannable(&world).await;

    // Update Plan: the idle planner takes the update in its open tab.
    let (status, update) = world
        .call(
            "POST",
            "/planning/update",
            json!({"tool":REPORT_TOOL,"operation_id":"update-1","actor":person}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{update}");
    assert_eq!(update["outcome"], "started", "{update}");
    assert_eq!(update["state"], "drafting", "{update}");
    assert_eq!(update["draft_steps"], 2, "{update}");
    assert_eq!(
        update["session"]["session"]["session_id"],
        terminal.as_str(),
        "{update}"
    );
    let update_id = update["update_id"].as_str().unwrap().to_owned();
    let prompt = received_prompt(
        &world.reports,
        &terminal,
        &format!("plan update `{update_id}`"),
    );
    for part in [
        format!("--plan-update {update_id}"),
        format!("orchestration plan update-finish {update_id}"),
        "`hotsheet-planner`".to_owned(),
        worker.clone(),
    ] {
        assert!(prompt.contains(&part), "{prompt} lacks {part}");
    }
    assert_eq!(drawer(&world.router).await.len(), 1, "no second planner");
    let (_, shown) = world.call("POST", "/planning/show", json!({})).await;
    assert_eq!(shown["update"]["update_id"], update_id.as_str(), "{shown}");
    assert_eq!(shown["update"]["state"], "drafting", "{shown}");

    // The planner drafts: one step removed, one added. Only the draft changes.
    let (status, removed) = world
        .call(
            "POST",
            "/readiness/remove",
            json!({"actor":planner,"step_id":dropped_step,"plan_update":update_id}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{removed}");
    let added_step = define(added.slug.clone(), Some(update_id.clone())).await;
    let mut draft = vec![kept_step.clone(), added_step.clone()];
    draft.sort();
    assert_eq!(plan_steps(&world, Some(&update_id)).await, draft);
    assert_eq!(
        plan_steps(&world, None).await,
        live,
        "the live plan runs on"
    );

    // The planner finishes the update: the live plan swaps to the draft.
    let (status, applied) = world
        .call(
            "POST",
            "/planning/update_finish",
            json!({"actor":planner,"update_id":update_id}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{applied}");
    assert_eq!(applied["state"], "applied", "{applied}");
    assert_eq!(applied["swap"]["kept"], json!([kept_step]), "{applied}");
    assert_eq!(
        applied["swap"]["removed"],
        json!([dropped_step]),
        "{applied}"
    );
    assert_eq!(applied["swap"]["added"], json!([added_step]), "{applied}");
    assert_eq!(plan_steps(&world, None).await, draft);
    let (_, shown) = world.call("POST", "/planning/show", json!({})).await;
    assert_eq!(shown["update"]["state"], "applied", "{shown}");
    // Finishing again finds no drafting update.
    let (status, again) = world
        .call(
            "POST",
            "/planning/update_finish",
            json!({"actor":planner,"update_id":update_id}),
        )
        .await;
    assert!(status.is_client_error(), "{status} {again}");

    let (_, audit) = world.call("GET", "/audit", Value::Null).await;
    let reasons = |action: &str| -> Vec<String> {
        audit["entries"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|entry| entry["action"] == action)
            .map(|entry| entry["reason"].as_str().unwrap_or_default().to_owned())
            .collect()
    };
    assert_eq!(reasons("plan_update"), ["plan_update_started"], "{audit}");
    assert!(
        reasons("plan_update_apply").contains(&"plan_update_applied".to_owned()),
        "{audit}"
    );
}

/// HS2-PW43N9 (HS2-N5BDE4): on a real server, a planner that asks a FEEDBACK NEEDED
/// question on a Started goal ticket shows in `plan show`, and a person's reply through
/// the ticket note route is pushed into that planner's terminal. The ticket has no live
/// claim and an older push-capable session runs in the same project, which would be the
/// project's primary: the reply reaches the planner because it asked.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_pw43n9_a_reply_to_the_planners_question_is_pushed_into_the_planner() {
    let world = World::with_store(&["sessions_manage"], PUSH_AGENT, PUSH_PLUGIN, true).await;
    world.allow(&["sessions_manage"], json!({})).await;
    // An older push-capable session in the project, up before the planner.
    let (status, other) = world.start("op-other").await;
    assert_eq!(status, StatusCode::OK, "{other}");
    let other = other["hosted"]["session_id"].as_str().unwrap().to_owned();
    received_prompt(&world.reports, &other, "`hotsheet-worker`");

    let tickets = format!("/checkouts/{}/tickets", world.checkout.id);
    let (status, created) = api(
        world.router.clone(),
        "POST",
        &tickets,
        json!({"title":"Ship the launch"}),
        true,
    )
    .await;
    assert!(status.is_success(), "{created}");
    let slug = created["slug"].as_str().unwrap().to_owned();
    let goal = format!("{tickets}/{}", created["qualified_id"].as_str().unwrap());
    let person = json!({"role":"human","id":"maintainer"});
    let (status, proposed) = world
        .call(
            "POST",
            "/planning/propose",
            json!({"tool":REPORT_TOOL,"operation_id":"press-1","actor":person}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{proposed}");
    let terminal = proposed["session"]["session"]["session_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let worker = proposed["session"]["worker_id"]
        .as_str()
        .unwrap()
        .to_owned();
    received_prompt(&world.reports, &terminal, "press-1");

    // The planner starts the goal and asks the person instead of stopping.
    let patch = |body: Value| {
        let router = world.router.clone();
        let goal = goal.clone();
        async move {
            let (status, ticket) = api(router, "PATCH", &goal, body, true).await;
            assert!(status.is_success(), "{status} {ticket}");
            ticket
        }
    };
    patch(json!({"status":"started","actor":{"role":"ai","id":worker}})).await;
    let asked = patch(json!({"note":"Which synthetic region comes first?",
        "note_kind":"feedback_needed","actor":{"role":"ai","id":worker}}))
    .await;
    assert!(
        asked.get("feedback_delivery").is_none_or(Value::is_null),
        "an AI's question notifies no session: {asked}"
    );
    let question = asked["notes"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|note| note["kind"] == "feedback_needed")
        .unwrap_or_else(|| panic!("{asked}"))["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let (_, shown) = world.call("POST", "/planning/show", json!({})).await;
    assert_eq!(shown["state"], "planning", "{shown}");
    assert_eq!(shown["questions"][0]["slug"], slug.as_str(), "{shown}");
    assert_eq!(
        shown["questions"][0]["question"],
        question.as_str(),
        "{shown}"
    );

    // A person answers the question through the ticket note route, as the answer form
    // does: it goes to the planner that asked.
    let answered = patch(json!({"note":"The synthetic north region first.",
        "note_kind":"regular","response_to":question,
        "actor":{"role":"human","id":"maintainer"}}))
    .await;
    assert_eq!(
        answered["feedback_delivery"]["status"], "sent",
        "{answered}"
    );
    assert_eq!(
        answered["feedback_delivery"]["connection_id"],
        terminal.as_str(),
        "{answered}"
    );
    let pushed = received_prompt(&world.reports, &terminal, &slug);
    assert!(
        pushed.starts_with("A human answered FEEDBACK NEEDED on"),
        "{pushed}"
    );
    let other_prompts =
        std::fs::read_to_string(world.reports.join(format!("{other}.prompts"))).unwrap();
    assert!(
        !other_prompts.contains(&slug),
        "the older session is not told: {other_prompts}"
    );
    // Answered: the question no longer waits.
    let (_, shown) = world.call("POST", "/planning/show", json!({})).await;
    assert!(
        shown
            .get("questions")
            .is_none_or(|questions| questions.as_array().is_some_and(Vec::is_empty)),
        "{shown}"
    );
}
