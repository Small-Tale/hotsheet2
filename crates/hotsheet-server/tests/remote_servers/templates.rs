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

/// HS2-K3ESZR: the preview checks the test profiles a template's steps name against
/// the profiles the project offers, read from its checked-in file through the gate
/// as an observation: an unknown profile is a coded problem, a damaged file is one
/// for each step naming profiles there, the problems clear once the file offers them,
/// and the CLI process previews the same. Nothing is asked and nothing is written.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_k3eszr_preview_checks_validation_profiles_on_a_real_server() {
    let world = World::new(&["tickets_read"], PUSH_AGENT, PUSH_PLUGIN).await;
    let person = json!({"role":"human","id":"maintainer"});
    let mut checked = template("Build {{feature}}");
    checked["steps"][0]["validation"] = json!(["smoke", "lint"]);
    let (status, set) = world
        .call(
            "POST",
            "/templates/set",
            json!({"templates":[checked],"actor":person}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{set}");
    let template_id = set["templates"][0]["template_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let preview = || {
        let world = &world;
        let body = json!({"template_id":template_id,"inputs":{"feature":"Search"},"actor":person});
        async move {
            let (status, preview) = world.call("POST", "/templates/preview", body).await;
            assert_eq!(status, StatusCode::OK, "{preview}");
            preview
        }
    };
    let problems = |preview: &Value| {
        preview["problems"]
            .as_array()
            .unwrap()
            .iter()
            .map(|problem| {
                format!(
                    "{}@{}",
                    problem["code"].as_str().unwrap(),
                    problem["at"].as_str().unwrap()
                )
            })
            .collect::<Vec<_>>()
    };
    let profiles = |entries: Value| {
        let directory = world.project.join(".hotsheet2");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("validation-profiles.json"),
            json!({"schema_version": 1, "profiles": entries}).to_string(),
        )
        .unwrap();
    };

    // No profile file yet: the project offers none.
    let none = preview().await;
    assert_eq!(
        problems(&none),
        [
            "validation_profile_unknown@steps[0].validation[0]",
            "validation_profile_unknown@steps[0].validation[1]"
        ],
        "{none}"
    );
    assert_eq!(none["targets"][0]["profiles"], "checked", "{none}");
    // One offered, then a damaged file, then both offered.
    profiles(json!([{"name": "smoke", "program": "true"}]));
    assert_eq!(
        problems(&preview().await),
        ["validation_profile_unknown@steps[0].validation[1]"]
    );
    profiles(json!([{"name": "smoke", "program": "a"}, {"name": "smoke", "program": "b"}]));
    assert_eq!(
        problems(&preview().await),
        ["validation_profiles_invalid@steps[0].validation"]
    );
    profiles(json!([{"name": "smoke", "program": "true"}, {"name": "lint", "program": "true"}]));
    let ok = preview().await;
    assert_eq!(ok["problems"], json!([]), "{ok}");
    assert_eq!(ok["targets"][0]["profiles"], "checked");

    // The CLI process previews through the same route.
    let store_arg = world.project_store.to_str().unwrap().to_owned();
    let from_cli = cli_named(
        &world.home,
        &[
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
        ],
        &["preview", &template_id, "--input", "feature=Search"],
    );
    assert_eq!(from_cli["problems"], json!([]), "{from_cli}");
    assert_eq!(from_cli["targets"][0]["profiles"], "checked", "{from_cli}");

    // Read as observations: audited, and nobody was asked; no goal was opened.
    let (_, audit) = world.call("GET", "/audit", Value::Null).await;
    let observed = audit["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["action"] == "gates_observe")
        .count();
    // Repeated identical observation reads coalesce into the newest entry
    // (HS2-SV4SMY), so previewing after every typing pause does not flood the journal.
    assert!(
        (1..=4).contains(&observed),
        "five route previews and one CLI preview: {audit}"
    );
    let (_, requests) = world.call("GET", "/permissions", Value::Null).await;
    assert!(
        requests["requests"]
            .as_array()
            .is_none_or(|requests| requests.is_empty()),
        "{requests}"
    );
    assert!(goals(&world).is_empty() && drawer(&world.router).await.is_empty());
}
