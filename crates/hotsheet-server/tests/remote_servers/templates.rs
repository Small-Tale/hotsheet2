//! HS2-A4RVYA: plan templates on a real server. Applying a template opens a goal
//! ticket carrying the brief and starts the planner, a real PTY running the workspace
//! fake agent in the orchestration project itself, exactly once; a repeat through the
//! route, the CLI process or the MCP tool changes nothing; the goal is a snapshot of
//! the template; and an AI session's apply waits for a person's answer. No AI tool is
//! launched and nothing leaves loopback.
use super::orchestration::api;
use super::session_work::{
    PUSH_AGENT, PUSH_PLUGIN, REPORT_TOOL, TARGET, World, launch_report, received_prompt,
};
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

/// HS2-4BQACQ: a template apply whose planner the machine refuses (its admission file
/// allows no session) waits with the refusal's reason, limit and observed count on the
/// record, read back through the route and the CLI process's `template apply-show`;
/// once the machine admits sessions again the repeat starts the planner and the
/// refusal is gone. No real machine load is involved.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_4bqacq_a_refused_planner_admission_keeps_its_numbers_on_a_real_server() {
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
    let admission_file = world.home.join("orchestration-admission.json");
    std::fs::write(
        &admission_file,
        br#"{"schema_version":1,"admission":{"max_sessions":0}}"#,
    )
    .unwrap();
    let body = json!({"template_id":template_id,"inputs":{"feature":"Search"},
        "tool":REPORT_TOOL,"operation_id":"tpl-refused","actor":person});
    let (status, refused) = world.call("POST", "/templates/apply", body.clone()).await;
    assert_eq!(status, StatusCode::OK, "{refused}");
    assert_eq!(refused["state"], "waiting_planner", "{refused}");
    assert_eq!(refused["reason"], "session_admission_refused", "{refused}");
    let expected = refused["admission"].clone();
    assert_eq!(expected["reason"], "session_cap_reached", "{refused}");
    assert_eq!(expected["setting"], "max_sessions", "{refused}");
    assert_eq!(expected["limit"], 0, "{refused}");
    // The sessions the machine counted (other AI sessions on it count too).
    assert!(expected["observed"].is_u64(), "{refused}");
    assert_eq!(goals(&world).len(), 1, "the goal is open while it waits");
    assert!(drawer(&world.router).await.is_empty(), "no planner started");

    // Read back through the route and headlessly through the CLI process.
    let (status, shown) = world
        .call(
            "POST",
            "/templates/apply_show",
            json!({"operation_id":"tpl-refused"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{shown}");
    assert_eq!(shown["admission"], expected, "{shown}");
    let store_arg = world.project_store.to_str().unwrap().to_owned();
    let from_cli = cli_named(
        &world.home,
        &[
            "-C",
            store_arg.as_str(),
            "orchestration",
            "template",
            "--project",
            world.checkout.id.as_str(),
        ],
        &["apply-show", "tpl-refused"],
    );
    assert_eq!(from_cli["state"], "waiting_planner", "{from_cli}");
    assert_eq!(from_cli["admission"], expected, "{from_cli}");

    // The machine admits sessions again: the repeat starts the planner, once.
    std::fs::remove_file(&admission_file).unwrap();
    let (status, taken) = world.call("POST", "/templates/apply", body).await;
    assert_eq!(status, StatusCode::OK, "{taken}");
    assert_eq!(taken["state"], "planning", "{taken}");
    assert!(taken.get("admission").is_none(), "{taken}");
    assert!(taken.get("reason").is_none(), "{taken}");
    assert_eq!(goals(&world).len(), 1, "no second goal");
    assert_eq!(drawer(&world.router).await.len(), 1);
}

/// HS2-DMA5DR: with the server's automation thread running its resume pass on every
/// apply write, applies the machine refuses each ask their planner once: the pass
/// never takes an apply that is still asking for an interrupted one, so no record
/// shows a second attempt or loses its refusal, and the audit holds one refused
/// `plan_propose` and one `template_apply` per apply. The refusal comes from the admission file (no real load).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_dma5dr_the_resume_pass_leaves_a_refused_apply_at_one_attempt_on_a_real_server() {
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
    std::fs::write(
        world.home.join("orchestration-admission.json"),
        br#"{"schema_version":1,"admission":{"max_sessions":0}}"#,
    )
    .unwrap();
    let operations: Vec<String> = (1..=5).map(|n| format!("tpl-refused-{n}")).collect();
    let shown = |operation: &str| {
        let world = &world;
        let operation = operation.to_owned();
        async move {
            let (status, shown) = world
                .call(
                    "POST",
                    "/templates/apply_show",
                    json!({"operation_id": operation}),
                )
                .await;
            assert_eq!(status, StatusCode::OK, "{shown}");
            shown
        }
    };
    for (n, operation) in operations.iter().enumerate() {
        let (status, refused) = world
            .call(
                "POST",
                "/templates/apply",
                json!({"template_id":template_id,"inputs":{"feature":format!("Feature {n}")},
                    "tool":REPORT_TOOL,"operation_id":operation,"actor":person}),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{refused}");
        assert_eq!(refused["reason"], "session_admission_refused", "{refused}");
        assert_eq!(refused["attempts"], 1, "{refused}");
        // Every earlier apply keeps its one refused attempt while the pass runs.
        for earlier in &operations[..=n] {
            let record = shown(earlier).await;
            assert_eq!(record["state"], "waiting_planner", "{record}");
            assert_eq!(record["attempts"], 1, "{record}");
            assert_eq!(record["proposal_id"], earlier.as_str(), "{record}");
            assert_eq!(record["reason"], "session_admission_refused", "{record}");
        }
    }
    // A headless pass, as the automation thread's, finds nothing to continue.
    let (status, again) = world.call("POST", "/templates/resume", json!({})).await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again["resumed"], json!([]), "{again}");
    for operation in &operations {
        let record = shown(operation).await;
        assert_eq!(record["attempts"], 1, "{record}");
        assert_eq!(record["reason"], "session_admission_refused", "{record}");
    }
    let (_, audit) = world.call("GET", "/audit", Value::Null).await;
    // One planner ask (refused) and one apply per operation, in order.
    let asks: Vec<(String, String, String)> = audit["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["action"] == "template_apply" || entry["action"] == "plan_propose")
        .map(|entry| {
            let text = |key: &str| entry[key].as_str().unwrap_or_default().to_owned();
            (text("action"), text("operation_id"), text("reason"))
        })
        .collect();
    let expected: Vec<(String, String, String)> = operations
        .iter()
        .flat_map(|operation| {
            [
                (
                    "plan_propose".to_owned(),
                    operation.clone(),
                    "session_admission_refused".to_owned(),
                ),
                (
                    "template_apply".to_owned(),
                    operation.clone(),
                    "template_waiting_planner".to_owned(),
                ),
            ]
        })
        .collect();
    assert_eq!(asks, expected, "{audit}");
    assert!(drawer(&world.router).await.is_empty(), "no planner started");
    world.host.begin_stopping();
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

/// HS2-A1M4KN: the planner asks a template's reuse `CHOICE` on the goal without claiming
/// it, as the `hotsheet-planner` skill says, so the goal stays Not Started. A person's
/// answer through the ticket note route still reaches that planner (and not the
/// project's older push-capable session), and the planner then creates the step's ticket
/// under `tpl-<operation id>-<step key>` and moves the step to it. A person's plain note
/// on the Not Started goal before any question tells nobody, as before.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_a1m4kn_a_reuse_answer_on_an_unclaimed_template_goal_reaches_the_planner() {
    let world = World::with_store(
        &["tickets_read", "tickets_write", "sessions_manage"],
        PUSH_AGENT,
        PUSH_PLUGIN,
        true,
    )
    .await;
    world.allow(&["sessions_manage"], json!({})).await;
    // An older push-capable session in the project: the project's primary.
    let (status, other) = world.start("op-other").await;
    assert_eq!(status, StatusCode::OK, "{other}");
    let other = other["hosted"]["session_id"].as_str().unwrap().to_owned();
    received_prompt(&world.reports, &other, "`hotsheet-worker`");
    // The work the find-or-create step describes already has a ticket.
    let existing = world.ticket("Build Search");

    let person = json!({"role":"human","id":"maintainer"});
    let (status, set) = world
        .call(
            "POST",
            "/templates/set",
            json!({"templates":[template("Build {{feature}}")],"actor":person}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{set}");
    let template_id = set["templates"][0]["template_id"].clone();
    let (status, applied) = world
        .call(
            "POST",
            "/templates/apply",
            json!({"template_id":template_id,"inputs":{"feature":"Search"},
                "tool":REPORT_TOOL,"operation_id":"tpl-1","actor":person}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{applied}");
    let goal = goals(&world).pop().expect("the template's goal");
    let planner_terminal = drawer(&world.router)
        .await
        .into_iter()
        .map(|tab| tab["id"].as_str().unwrap().to_owned())
        .find(|id| *id != other)
        .expect("the planner's terminal");
    received_prompt(&world.reports, &planner_terminal, "`hotsheet-planner`");
    let worker = launch_report(&world.reports, &planner_terminal)["env"]["HOTSHEET_WORKER_ID"]
        .as_str()
        .unwrap()
        .to_owned();
    let planner = json!({"role":"ai","id":worker});
    let connection = world.checkout.sources[0].connection_id.clone();
    let goal_route = format!(
        "/checkouts/{}/tickets/{connection}:{}",
        world.checkout.id, goal.id
    );
    let patch = |body: Value| {
        let router = world.router.clone();
        let goal_route = goal_route.clone();
        async move {
            let (status, ticket) = api(router, "PATCH", &goal_route, body, true).await;
            assert!(status.is_success(), "{status} {ticket}");
            ticket
        }
    };

    // Before any question, a person's note on the Not Started goal tells nobody.
    let plain = patch(json!({"note":"Synthetic context for the planner.",
        "note_kind":"regular","actor":person}))
    .await;
    assert!(
        plain.get("feedback_delivery").is_none_or(Value::is_null),
        "{plain}"
    );

    // The planner reuses the ticket search found and asks once, without a claim.
    let define = |native: String| {
        let world = &world;
        let planner = planner.clone();
        let goal = goal.slug.clone();
        let connection = connection.clone();
        async move {
            let (status, defined) = world
                .call(
                    "POST",
                    "/readiness/define",
                    json!({"actor":planner,"ticket":{"native_id":goal},"target_id":TARGET,
                        "connection_id":connection,"native_id":native,"prerequisites":[]}),
                )
                .await;
            assert_eq!(status, StatusCode::OK, "{defined}");
            defined["step"]["step_id"].as_str().unwrap().to_owned()
        }
    };
    let reused_step = define(existing.slug.clone()).await;
    let asked = patch(json!({"note":format!(
            "FEEDBACK NEEDED: I reused tickets the template steps found by search.\n\n\
             CHOICE: Step build reuses {}. Keep it, or create a new ticket?\n\
             - **Keep {}.** The step stays on the existing ticket.\n\
             - **Create a new ticket for step build.** I create one and move the step to it.",
            existing.slug, existing.slug),
        "note_kind":"feedback_needed","actor":planner}))
    .await;
    assert_eq!(asked["status"], "not_started", "{asked}");
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

    // The person answers on the Not Started goal: the answer reaches the planner.
    let answered = patch(json!({"note":"Create a new ticket for step build",
        "note_kind":"regular","response_to":question,"actor":person}))
    .await;
    assert_eq!(answered["status"], "not_started", "{answered}");
    assert_eq!(
        answered["feedback_delivery"]["status"], "sent",
        "{answered}"
    );
    assert_eq!(
        answered["feedback_delivery"]["connection_id"],
        planner_terminal.as_str(),
        "{answered}"
    );
    // The role prompt names the goal too; the answer is its own prompt.
    received_prompt(
        &world.reports,
        &planner_terminal,
        &format!(
            "Hot Sheet notice: a person answered FEEDBACK NEEDED on {}",
            goal.slug
        ),
    );
    let other_prompts =
        std::fs::read_to_string(world.reports.join(format!("{other}.prompts"))).unwrap();
    assert!(
        !other_prompts.contains(&goal.slug),
        "the older session is not told: {other_prompts}"
    );

    // The planner plans by the answer: a new ticket under the template's operation id,
    // the step moved to it. Creating again under that id returns the same ticket.
    let create = || {
        let world = &world;
        let planner = planner.clone();
        let goal = goal.slug.clone();
        let connection = connection.clone();
        async move {
            let (status, created) = world
                .call(
                    "POST",
                    "/tickets/create",
                    json!({"actor":planner,"ticket":{"native_id":goal},"target_id":TARGET,
                        "connection_id":connection,"operation_id":"tpl-tpl-1-build",
                        "title":"Build Search"}),
                )
                .await;
            assert_eq!(status, StatusCode::OK, "{created}");
            created
        }
    };
    let created = create().await;
    let again = create().await;
    let slug_of = |created: &Value| {
        created["ticket"]["slug"]
            .as_str()
            .or_else(|| created["slug"].as_str())
            .unwrap_or_else(|| panic!("{created}"))
            .to_owned()
    };
    let new_slug = slug_of(&created);
    assert_eq!(slug_of(&again), new_slug, "{again}");
    assert_ne!(new_slug, existing.slug);
    let (status, removed) = world
        .call(
            "POST",
            "/readiness/remove",
            json!({"actor":planner,"step_id":reused_step}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{removed}");
    let moved_step = define(new_slug.clone()).await;
    let (status, plan) = world.call("POST", "/readiness/plan", json!({})).await;
    assert_eq!(status, StatusCode::OK, "{plan}");
    let steps: Vec<&str> = plan["steps"]
        .as_array()
        .unwrap_or_else(|| panic!("{plan}"))
        .iter()
        .map(|view| view["step"]["step_id"].as_str().unwrap())
        .collect();
    assert_eq!(steps, [moved_step.as_str()], "{plan}");
    // Answered: the question no longer waits, and the goal was never claimed.
    let (_, shown) = world.call("POST", "/planning/show", json!({})).await;
    assert!(
        shown
            .get("questions")
            .is_none_or(|questions| questions.as_array().is_some_and(Vec::is_empty)),
        "{shown}"
    );
    let goal_now = goals(&world)
        .into_iter()
        .find(|ticket| ticket.id == goal.id)
        .unwrap();
    assert_eq!(goal_now.status, hotsheet_model::Status::NotStarted);
    assert!(goal_now.claimed_by.is_none());
}
