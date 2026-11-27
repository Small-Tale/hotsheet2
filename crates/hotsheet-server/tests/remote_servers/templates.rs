//! HS2-A4RVYA: plan templates on a real server. Applying a template opens a goal
//! ticket carrying the brief and starts the planner, a real PTY running the workspace
//! fake agent in the orchestration project itself, exactly once; a repeat through the
//! route, the CLI process or the MCP tool changes nothing; the goal is a snapshot of
//! the template; and an AI session's apply waits for a person's answer. No AI tool is
//! launched and nothing leaves loopback.
use super::session_work::{PUSH_AGENT, PUSH_PLUGIN, REPORT_TOOL, TARGET, World, received_prompt};
use super::sessions::drawer;
use super::support::{cli_named, mcp_named};
use super::*;

/// `plan show` once the planner sits idle in its open tab, woken by the machine's
/// session records changing rather than read on a timer.
async fn replannable(world: &World) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        let waiter = hotsheet_jobs::ChangeWaiter::starting(&[world.home.as_path()]).unwrap();
        let (_, shown) = world.call("POST", "/planning/show", json!({})).await;
        if shown["replannable"] == true {
            return;
        }
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        assert!(
            !remaining.is_zero(),
            "the planner never became replannable: {shown}"
        );
        tokio::task::block_in_place(|| waiter.wait(remaining));
    }
}

fn template(title: &str) -> Value {
    json!({
        "name": "Feature in the launch project",
        "inputs": [
            {"key": "feature", "label": "Feature", "kind": "text", "required": true},
            {"key": "project", "label": "Project", "kind": "target", "default": TARGET}
        ],
        "goal": {"title": "{{feature}} everywhere"},
        "steps": [
            {"key": "build", "target": "{{project}}", "title": title},
            {"key": "docs", "target": "{{project}}", "title": "Document {{feature}}",
             "ticket": "create", "after": [{"step": "build", "gates": "ci"}]}
        ]
    })
}

fn goals(world: &World) -> Vec<hotsheet_model::Ticket> {
    hotsheet_ticketing::ops::query(
        &FsStore::open(&world.project_store).unwrap(),
        &hotsheet_ticketing::ops::TicketQuery::default(),
    )
    .unwrap()
    .into_iter()
    .filter(|ticket| {
        ticket
            .tags
            .iter()
            .any(|tag| tag == "orchestration-template")
    })
    .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_a4rvya_apply_starts_the_planner_once_and_keeps_a_snapshot() {
    let world = World::new(
        &["tickets_read", "sessions_manage"],
        PUSH_AGENT,
        PUSH_PLUGIN,
    )
    .await;
    let person = json!({"role":"human","id":"maintainer"});
    let (status, set) = world
        .call(
            "POST",
            "/templates/set",
            json!({"templates":[template("Build {{feature}}")],"actor":person}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{set}");
    let template_id = set["templates"][0]["template_id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(set["templates"][0]["version"], 1);
    assert!(
        world
            .project
            .join(".hotsheet2/orchestration-templates.json")
            .is_file()
    );

    // The pre-run summary writes nothing.
    let (status, preview) = world
        .call(
            "POST",
            "/templates/preview",
            json!({"template_id":template_id,"inputs":{"feature":"Search"},"actor":person}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["problems"], json!([]), "{preview}");
    assert_eq!(preview["planning"], "propose");
    assert_eq!(preview["targets"][0]["apply"], "person");
    assert!(goals(&world).is_empty() && drawer(&world.router).await.is_empty());

    let apply = |operation: &'static str, actor: Value, feature: &'static str| {
        let world = &world;
        let template_id = template_id.clone();
        async move {
            world
                .call(
                    "POST",
                    "/templates/apply",
                    json!({"template_id":template_id,"inputs":{"feature":feature},
                        "tool":REPORT_TOOL,"operation_id":operation,"actor":actor}),
                )
                .await
        }
    };
    let (status, applied) = apply("tpl-1", person.clone(), "Search").await;
    assert_eq!(status, StatusCode::OK, "{applied}");
    assert_eq!(applied["outcome"], "applied", "{applied}");
    assert_eq!(applied["state"], "planning", "{applied}");
    assert_eq!(applied["planning"], "propose");
    assert_eq!(applied["proposal_id"], "tpl-1");
    let first_goal = goals(&world);
    assert_eq!(first_goal.len(), 1);
    assert_eq!(first_goal[0].title, "Search everywhere");
    assert!(
        first_goal[0]
            .details
            .contains("find or create \"Build Search\"")
    );
    assert!(first_goal[0].details.contains("version 1"));
    let terminals = drawer(&world.router).await;
    assert_eq!(terminals.len(), 1, "{terminals:?}");
    let terminal = terminals[0]["id"].as_str().unwrap().to_owned();
    let prompt = received_prompt(&world.reports, &terminal, "`hotsheet-planner`");
    for part in [
        "tpl-1",
        format!("the goal ticket `{}`", first_goal[0].slug).as_str(),
    ] {
        assert!(prompt.contains(part), "{prompt} lacks {part}");
    }

    // Repeated through the route, the CLI process and the MCP tool: the same apply
    // answers, and no second goal or planner appears.
    let (status, again) = apply("tpl-1", person.clone(), "Search").await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again["outcome"], "existing", "{again}");
    let store_arg = world.project_store.to_str().unwrap().to_owned();
    let as_person = [
        "--actor-role",
        "human",
        "--actor-id",
        "maintainer",
        "-C",
        store_arg.as_str(),
        "orchestration",
        "template",
        "--project",
        world.checkout.id.as_str(),
    ];
    let from_cli = cli_named(
        &world.home,
        &as_person,
        &[
            "apply",
            &template_id,
            "--input",
            "feature=Search",
            "--tool",
            REPORT_TOOL,
            "--operation-id",
            "tpl-1",
        ],
    );
    assert_eq!(from_cli["outcome"], "existing", "{from_cli}");
    let listed = cli_named(&world.home, &as_person, &["list"]);
    assert_eq!(listed["applies"][0]["operation_id"], "tpl-1", "{listed}");
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
    let from_mcp = mcp_named(
        &world.home,
        &backend,
        "hotsheet_targets",
        json!({"action":"template_apply","checkout":world.checkout.id,
            "actor_role":"human","actor_id":"maintainer","template_id":template_id,
            "inputs":{"feature":"Search"},"tool":REPORT_TOOL,"operation_id":"tpl-1"}),
    );
    let _ = stop.send(());
    task.await.unwrap();
    assert_eq!(from_mcp["outcome"], "existing", "{from_mcp}");
    assert_eq!(goals(&world).len(), 1);
    assert_eq!(drawer(&world.router).await.len(), 1);

    // Edit the template: version 2. The applied goal and a repeat keep version 1.
    let mut changed = template("Index {{feature}} first");
    changed["template_id"] = json!(template_id);
    let (status, edited) = world
        .call(
            "POST",
            "/templates/set",
            json!({"templates":[changed],"actor":person}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{edited}");
    assert_eq!(edited["templates"][0]["version"], 2, "{edited}");
    let (_, repeat) = apply("tpl-1", person.clone(), "Search").await;
    assert_eq!(repeat["outcome"], "existing", "{repeat}");
    assert_eq!(repeat["snapshot"]["version"], 1, "{repeat}");
    assert_eq!(goals(&world)[0].details, first_goal[0].details);

    // A new apply uses version 2: a new goal, given to the same planner as a new
    // proposal once it sits idle in its tab.
    replannable(&world).await;
    let (status, second) = apply("tpl-2", person.clone(), "Search").await;
    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(second["state"], "planning", "{second}");
    assert_eq!(second["snapshot"]["version"], 2, "{second}");
    let all = goals(&world);
    assert_eq!(all.len(), 2, "a new goal ticket per apply");
    let newer = all
        .iter()
        .find(|goal| goal.details.contains("operation `tpl-2`"))
        .unwrap();
    assert!(
        newer.details.contains("\"Index Search first\""),
        "{}",
        newer.details
    );
    let pushed = received_prompt(&world.reports, &terminal, "tpl-2");
    assert!(
        pushed.contains(&format!("the goal ticket `{}`", newer.slug)),
        "{pushed}"
    );
    assert_eq!(drawer(&world.router).await.len(), 1, "no second planner");

    // An AI session's apply asks a person first and opens nothing until answered.
    let ai = json!({"role":"ai","id":"worker-1"});
    let (status, asked) = apply("tpl-ai", ai.clone(), "Quotas").await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{asked}");
    assert_eq!(asked["code"], "permission_required", "{asked}");
    let request = asked["request_id"].as_str().unwrap().to_owned();
    assert_eq!(goals(&world).len(), 2);
    let (status, shown) = world
        .call(
            "POST",
            "/templates/apply_show",
            json!({"operation_id":"tpl-ai"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{shown}");
    assert_eq!(shown["state"], "waiting_permission", "{shown}");
    let (status, answered) = world
        .call(
            "POST",
            &format!("/permissions/{request}"),
            json!({"answer":"allow_once","actor":person}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{answered}");
    replannable(&world).await;
    let (status, applied) = apply("tpl-ai", ai, "Quotas").await;
    assert_eq!(status, StatusCode::OK, "{applied}");
    assert_eq!(applied["state"], "planning", "{applied}");
    assert_eq!(goals(&world).len(), 3);
    assert_eq!(drawer(&world.router).await.len(), 1);

    let (_, audit) = world.call("GET", "/audit", Value::Null).await;
    let reasons: Vec<&str> = audit["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["action"] == "template_apply")
        .map(|entry| entry["reason"].as_str().unwrap())
        .collect();
    assert_eq!(
        reasons,
        [
            "template_planning",
            "template_planning",
            "permission_required",
            "template_planning"
        ],
        "{audit}"
    );
}

/// HS2-AMJNER: an AI session's apply that waits for a person continues on its own once
/// the person answers on the permission card: the server's automation thread repeats
/// it under its operation id, so the planner starts with no second call from the AI.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_amjner_answering_the_card_resumes_the_apply_without_a_second_call() {
    let world = World::new(
        &["tickets_read", "sessions_manage"],
        PUSH_AGENT,
        PUSH_PLUGIN,
    )
    .await;
    world.host.spawn_orchestration_automation();
    let person = json!({"role":"human","id":"maintainer"});
    let (status, set) = world
        .call(
            "POST",
            "/templates/set",
            json!({"templates":[template("Build {{feature}}")],"actor":person}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{set}");
    let template_id = set["templates"][0]["template_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let (status, asked) = world
        .call(
            "POST",
            "/templates/apply",
            json!({"template_id":template_id,"inputs":{"feature":"Quotas"},
                "tool":REPORT_TOOL,"operation_id":"tpl-auto",
                "actor":{"role":"ai","id":"worker-1"}}),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{asked}");
    let request = asked["request_id"].as_str().unwrap().to_owned();
    assert!(goals(&world).is_empty());
    let (status, answered) = world
        .call(
            "POST",
            &format!("/permissions/{request}"),
            json!({"answer":"allow_once","actor":person}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{answered}");
    // Woken by the machine's records changing, never read on a timer.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        let waiter = hotsheet_jobs::ChangeWaiter::starting(&[world.home.as_path()]).unwrap();
        let (_, shown) = world
            .call(
                "POST",
                "/templates/apply_show",
                json!({"operation_id":"tpl-auto"}),
            )
            .await;
        // The apply is recorded before its audit entry: wait for both.
        let (_, audit) = world.call("GET", "/audit", Value::Null).await;
        let audited = audit["entries"].as_array().is_some_and(|entries| {
            entries.iter().any(|entry| {
                entry["action"] == "template_apply" && entry["reason"] == "template_planning"
            })
        });
        if shown["state"] == "planning" && audited {
            break;
        }
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        assert!(!remaining.is_zero(), "the apply never resumed: {shown}");
        tokio::task::block_in_place(|| {
            waiter.wait(remaining.min(std::time::Duration::from_secs(1)))
        });
    }
    assert_eq!(goals(&world).len(), 1);
    assert_eq!(drawer(&world.router).await.len(), 1, "one planner");
    // Resumed once: a headless pass afterwards finds nothing to continue.
    let (status, again) = world.call("POST", "/templates/resume", json!({})).await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again["resumed"], json!([]), "{again}");
    let (_, audit) = world.call("GET", "/audit", Value::Null).await;
    let applies = audit["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["action"] == "template_apply")
        .map(|entry| {
            (
                entry["actor_id"].as_str().unwrap_or_default().to_owned(),
                entry["reason"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        applies,
        [
            ("worker-1".to_owned(), "permission_required".to_owned()),
            ("worker-1".to_owned(), "template_planning".to_owned())
        ],
        "{audit}"
    );
    world.host.begin_stopping();
}
