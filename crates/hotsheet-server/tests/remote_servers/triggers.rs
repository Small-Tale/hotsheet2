//! HS2-KXKA7X: triggers on a real server. A schedule's slot fires once (the pass is
//! asked at the slot's instant, which only a test build honours) and its placement
//! action starts placement; Dry Run writes nothing; the CLI process and the MCP tool
//! reach the same triggers; a ticket opened in the project reaches the server's target
//! follower and fires once, an edit of it nothing more; a branch's red CI, read from
//! the watch Job the server's branch watch started (judged here as the Job monitor
//! would), fires once and a redelivery is already handled. Synthetic data; no AI tool
//! is launched and nothing leaves loopback.
use super::session_work::{PUSH_AGENT, PUSH_PLUGIN, TARGET, World};
use super::support::{cli_named, mcp_named};
use super::*;
use hotsheet_jobs::{Acquire, ActiveStatus, FinalStatus, JobStore, Outcome};

/// 2030-01-01T00:00:00Z, past any real instant this test runs at.
const FUTURE: u64 = 1_893_456_000_000;
const MINUTE: u64 = 60_000;

fn person() -> Value {
    json!({"role":"human","id":"maintainer"})
}

/// Wait, woken by the machine home changing (never on a timer), until `done` reads
/// true from the server.
async fn until<F, Fut>(world: &World, what: &str, done: F) -> Value
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Option<Value>>,
{
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        let waiter = hotsheet_jobs::ChangeWaiter::starting(&[world.home.as_path()]).unwrap();
        if let Some(value) = done().await {
            return value;
        }
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        assert!(!remaining.is_zero(), "{what} never happened");
        tokio::task::block_in_place(|| {
            waiter.wait(remaining.min(std::time::Duration::from_secs(1)))
        });
    }
}

async fn trigger(world: &World, id: &str) -> Value {
    let (status, shown) = world
        .call("POST", "/triggers/show", json!({"trigger_id":id}))
        .await;
    assert_eq!(status, StatusCode::OK, "{shown}");
    shown
}

fn history(shown: &Value) -> Vec<(String, String, u64)> {
    shown["history"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|entry| {
            (
                entry["delivery_key"].as_str().unwrap().to_owned(),
                entry["outcome"].as_str().unwrap().to_owned(),
                entry["count"].as_u64().unwrap_or(1),
            )
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_kxka7x_a_schedule_fires_once_per_slot_and_dry_run_writes_nothing() {
    let world = World::new(&["tickets_read"], PUSH_AGENT, PUSH_PLUGIN).await;
    world.host.spawn_orchestration_automation();
    // The schedule starts in 2030, so the server's own passes (real time) never reach a
    // slot while this test runs; only the passes asked at a slot's instant do.
    let (status, set) = world
        .call(
            "POST",
            "/triggers/set",
            json!({"name":"Start placement every five minutes",
                "source":{"kind":"schedule","schedule":{"time_zone":"UTC","local_time":"00:00",
                    "recurrence":{"kind":"minutely","start_date":"2030-01-01","every":5}}},
                "action":{"placement":"start"},"actor":person()}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{set}");
    assert_eq!(set["state"], "dry_run");
    let id = set["trigger_id"].as_str().unwrap().to_owned();
    assert_eq!(set["next_runs_ms"].as_array().unwrap().len(), 3);
    let automatic = || async {
        let (_, policy) = world.call("GET", "/policy", Value::Null).await;
        policy["policy"]["placement"]["automatic"].clone()
    };
    let before = automatic().await;
    let pass = |at_ms: u64| {
        let world = &world;
        async move {
            let (status, pass) = world
                .call("POST", "/triggers/pass", json!({"at_ms":at_ms}))
                .await;
            assert_eq!(status, StatusCode::OK, "{pass}");
            pass
        }
    };
    // Dry Run at a slot: recorded, nothing changed.
    let dry = pass(FUTURE).await;
    assert_eq!(dry["deliveries"][0]["outcome"], "dry_run", "{dry}");
    assert_eq!(
        dry["deliveries"][0]["delivery_key"],
        "schedule:2030-01-01T00:00"
    );
    assert_eq!(dry["next_wake_ms"], FUTURE + 5 * MINUTE);
    assert_eq!(automatic().await, before, "dry run changed the policy");
    let shown = trigger(&world, &id).await;
    assert_eq!(shown["history"][0]["would"]["placement"], "start");

    // The headless CLI and the MCP tool reach the same trigger; the CLI turns it On.
    let store_arg = world.project_store.to_str().unwrap().to_owned();
    let as_person = [
        "--actor-role",
        "human",
        "--actor-id",
        "maintainer",
        "-C",
        store_arg.as_str(),
        "orchestration",
        "trigger",
        "--project",
        world.checkout.id.as_str(),
    ];
    let listed = cli_named(&world.home, &as_person, &["list"]);
    assert_eq!(listed["triggers"][0]["trigger_id"], id.as_str(), "{listed}");
    let on = cli_named(&world.home, &as_person, &["on", &id]);
    assert_eq!(on["state"], "on", "{on}");
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
    let review = mcp_named(
        &world.home,
        &backend,
        "hotsheet_targets",
        json!({"action":"trigger_review","checkout":world.checkout.id,
            "actor_role":"ai","actor_id":"worker-1","trigger_id":id}),
    );
    let _ = stop.send(());
    task.await.unwrap();
    assert_eq!(
        review["next"]["runs_ms"].as_array().unwrap().len(),
        3,
        "{review}"
    );
    assert!(
        review["permissions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["action"] == "placement_start"),
        "{review}"
    );

    // On: the next slot fires once and starts placement; again at that instant and a
    // minute later, nothing.
    let slot = FUTURE + 10 * MINUTE;
    let fired = pass(slot).await;
    assert_eq!(fired["deliveries"].as_array().unwrap().len(), 1, "{fired}");
    assert_eq!(fired["deliveries"][0]["outcome"], "applied", "{fired}");
    assert_eq!(
        fired["deliveries"][0]["delivery_key"],
        "schedule:2030-01-01T00:10"
    );
    assert_eq!(automatic().await, json!(true));
    for at in [slot, slot + MINUTE] {
        let again = pass(at).await;
        assert_eq!(again["deliveries"], json!([]), "{again}");
    }
    // The server's own passes (real time, before the slot) never fired it again.
    let shown = trigger(&world, &id).await;
    assert_eq!(
        history(&shown),
        [
            ("schedule:2030-01-01T00:00".into(), "dry_run".into(), 1),
            ("schedule:2030-01-01T00:10".into(), "applied".into(), 1)
        ]
    );
    let (_, audit) = world.call("GET", "/audit", Value::Null).await;
    let deliveries = audit["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["action"] == "trigger_delivery")
        .count();
    assert_eq!(deliveries, 2, "{audit}");
    // A pass at an instant only a test build honours is otherwise refused nothing;
    // removing the trigger forgets it.
    let (status, removed) = world
        .call(
            "POST",
            "/triggers/remove",
            json!({"trigger_id":id,"actor":person()}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{removed}");
    let (_, listed) = world.call("POST", "/triggers/list", json!({})).await;
    assert_eq!(listed["triggers"], json!([]));
    world.host.begin_stopping();
}

fn git(root: &std::path::Path, args: &[&str]) -> String {
    let output = hotsheet_ticketing::git::command_in(root)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}");
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

/// Finish a watch Job the way the Job monitor would.
fn judge(home: &std::path::Path, checkout: &str, job_id: &str, head: &str, run: u64) {
    let store = JobStore::at(home);
    let (_, token) = store
        .acquire(
            checkout,
            job_id,
            Acquire {
                owner: "monitor:test".into(),
                status: ActiveStatus::Running,
                launch_id: None,
                external: None,
            },
        )
        .unwrap();
    let refs = [
        ("gate", "red".to_owned()),
        ("run_id", run.to_string()),
        ("run_attempt", "1".to_owned()),
        ("run_head_sha", head.to_owned()),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value))
    .collect();
    store
        .finish(
            checkout,
            job_id,
            &token,
            FinalStatus::Failed,
            Outcome {
                summary: None,
                refs,
            },
        )
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_kxka7x_a_ticket_opened_and_a_red_branch_fire_once_through_the_followers() {
    let world = World::new(&["tickets_read"], PUSH_AGENT, PUSH_PLUGIN).await;
    world.host.spawn_orchestration_automation();
    // The project's checkout names its repository and has a main branch.
    git(
        &world.project,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/example/widgets.git",
        ],
    );
    git(
        &world.project,
        &[
            "-c",
            "user.email=t@example.invalid",
            "-c",
            "user.name=t",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "base",
        ],
    );
    let base = git(&world.project, &["rev-parse", "HEAD"]);
    git(
        &world.project,
        &["update-ref", "refs/remotes/origin/main", &base],
    );
    let set = |definition: Value| {
        let world = &world;
        async move {
            let mut body = definition;
            body["actor"] = person();
            let (status, set) = world.call("POST", "/triggers/set", body).await;
            assert_eq!(status, StatusCode::OK, "{set}");
            let id = set["trigger_id"].as_str().unwrap().to_owned();
            let (status, on) = world
                .call(
                    "POST",
                    "/triggers/on",
                    json!({"trigger_id":id,"actor":person()}),
                )
                .await;
            assert_eq!(status, StatusCode::OK, "{on}");
            assert_eq!(on["state"], "on");
            id
        }
    };
    let opened = set(json!({"name":"Triage tagged tickets",
        "source":{"kind":"ticket_opened","target_id":TARGET,"tag":"triage"},
        "action":{"placement":"pause"}}))
    .await;
    let red = set(json!({"name":"Repair main CI",
        "source":{"kind":"branch_ci_failed","target_id":TARGET,
            "watch":{"repository":"example/widgets","workflow":"ci.yml","branch":"main"}},
        "action":{"placement":"pause"}}))
    .await;

    // The branch watch learns the head and starts its watch Job.
    let jobs = JobStore::at(&world.home);
    let checkout = world.checkout.id.clone();
    let first_job = until(&world, "the branch watch Job", || async {
        let listed = jobs.list(&checkout).ok()?;
        listed.jobs.first().map(|job| json!(job.id))
    })
    .await;

    // A ticket written headlessly into the project's store, with the tag.
    let store = FsStore::open(&world.project_store).unwrap();
    let stamp = || {
        time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap()
    };
    let create = |title: &str, tags: &[&str]| {
        hotsheet_ticketing::ops::create(
            &store,
            hotsheet_model::Ulid::new(),
            "TST",
            stamp().as_str().into(),
            hotsheet_ticketing::NewTicket {
                title: title.into(),
                tags: tags.iter().map(|tag| (*tag).to_owned()).collect(),
                ..Default::default()
            },
        )
        .unwrap()
    };
    let tagged = create("Crash on start", &["triage"]);
    let shown = until(&world, "the ticket delivery", || async {
        let shown = trigger(&world, &opened).await;
        (!history(&shown).is_empty()).then_some(shown)
    })
    .await;
    let key = history(&shown)[0].0.clone();
    assert!(key.ends_with(&tagged.id.to_string()), "{key}");
    // Edited, then an untagged ticket, then a second tagged one: only the second fires
    // (the follower reads events in order, so the edit was seen before it).
    hotsheet_ticketing::ops::update(
        &store,
        &tagged.id,
        stamp().as_str().into(),
        hotsheet_ticketing::TicketPatch {
            title: Some("Crash on start, again".into()),
            ..Default::default()
        },
    )
    .unwrap();
    create("Typo", &[]);
    let second = create("Crash on save", &["triage"]);
    let shown = until(&world, "the second ticket delivery", || async {
        let shown = trigger(&world, &opened).await;
        (history(&shown).len() >= 2).then_some(shown)
    })
    .await;
    let keys: Vec<String> = history(&shown).into_iter().map(|(key, _, _)| key).collect();
    assert_eq!(keys.len(), 2, "{keys:?}");
    assert!(keys[1].ends_with(&second.id.to_string()), "{keys:?}");
    assert!(
        shown["history"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| entry["outcome"] == "applied")
    );

    // The branch's CI fails on a newer commit: one delivery.
    let newer = "b".repeat(40);
    judge(
        &world.home,
        &checkout,
        first_job.as_str().unwrap(),
        &newer,
        9182,
    );
    let shown = until(&world, "the red CI delivery", || async {
        let shown = trigger(&world, &red).await;
        (!history(&shown).is_empty()).then_some(shown)
    })
    .await;
    assert_eq!(
        history(&shown),
        [("ci:github-actions:9182:1".into(), "applied".into(), 1)]
    );
    assert_eq!(shown["history"][0]["evidence"]["commit"], newer.as_str());
    // The watch moved to the newer commit and started its Job; the same run reported
    // there again is already handled, recorded once.
    let second_job = until(&world, "the next watch Job", || async {
        let listed = jobs.list(&checkout).ok()?;
        listed
            .jobs
            .iter()
            .find(|job| {
                job.id != first_job.as_str().unwrap_or_default() && job.title.contains(&newer[..7])
            })
            .map(|job| json!(job.id))
    })
    .await;
    judge(
        &world.home,
        &checkout,
        second_job.as_str().unwrap(),
        &newer,
        9182,
    );
    let shown = until(&world, "the redelivery", || async {
        let shown = trigger(&world, &red).await;
        (history(&shown).len() >= 2).then_some(shown)
    })
    .await;
    assert_eq!(
        history(&shown),
        [
            ("ci:github-actions:9182:1".into(), "applied".into(), 1),
            (
                "ci:github-actions:9182:1".into(),
                "already_handled".into(),
                1
            )
        ]
    );
    // Pausing ends the branch watch and the ticket follower's part.
    for id in [&opened, &red] {
        let (status, paused) = world
            .call(
                "POST",
                "/triggers/pause",
                json!({"trigger_id":id,"actor":person()}),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{paused}");
        assert_eq!(paused["state"], "paused");
    }
    world.host.begin_stopping();
}

/// HS2-W8S3TV: a `commit_landed` trigger learns a landed commit from the branch's head
/// itself, with no CI run reported for it: the server's head watch long-polls the hosted
/// head, which answers when the checkout's remote-tracking ref moves (its ref watch, by
/// kernel events) rather than at the end of its wait. Each move is one delivery; the
/// watch Job the CI watch started is never judged. A hosted head read with the head
/// already known answers early on a move, and at once when it already differs.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_w8s3tv_a_landed_commit_fires_from_the_branch_head_without_ci() {
    let world = World::new(&["tickets_read"], PUSH_AGENT, PUSH_PLUGIN).await;
    world.host.spawn_orchestration_automation();
    git(
        &world.project,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/example/widgets.git",
        ],
    );
    let commit = |message: &str| {
        git(
            &world.project,
            &[
                "-c",
                "user.email=t@example.invalid",
                "-c",
                "user.name=t",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                message,
            ],
        );
        git(&world.project, &["rev-parse", "HEAD"])
    };
    // What a fetch (or a push from this checkout) does: move the remote-tracking ref.
    let land = |sha: &str| {
        git(
            &world.project,
            &["update-ref", "refs/remotes/origin/main", sha],
        )
    };
    let base = commit("base");
    land(&base);

    // The hosted read: an unchanged head waits for the ref to move.
    let checkout = world.checkout.id.clone();
    let head_url = |known: &str, wait_ms: u64| {
        format!(
            "/checkouts/{checkout}/target-branch-head?branch=main&known={known}&wait_ms={wait_ms}"
        )
    };
    let (status, now) = super::orchestration::api(
        world.router.clone(),
        "GET",
        &format!("/checkouts/{checkout}/target-branch-head?branch=main"),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{now}");
    assert_eq!(now["commit"], base.as_str());
    let early = commit("early");
    let started = std::time::Instant::now();
    let (router, url) = (world.router.clone(), head_url(&base, 8_000));
    let waiting = tokio::spawn(async move {
        super::orchestration::api(router, "GET", &url, Value::Null, true).await
    });
    land(&early);
    let (status, moved) = waiting.await.unwrap();
    assert_eq!(status, StatusCode::OK, "{moved}");
    assert_eq!(moved["commit"], early.as_str());
    assert!(
        started.elapsed() < std::time::Duration::from_millis(7_500),
        "the move answered the wait: {:?}",
        started.elapsed()
    );
    // A head that already differs answers at once.
    let started = std::time::Instant::now();
    let (_, differs) = super::orchestration::api(
        world.router.clone(),
        "GET",
        &head_url(&base, 8_000),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(differs["commit"], early.as_str());
    assert!(started.elapsed() < std::time::Duration::from_millis(7_500));

    // The trigger, turned On by a person.
    let (status, set) = world
        .call(
            "POST",
            "/triggers/set",
            json!({"name":"Validate merged commits",
                "source":{"kind":"commit_landed","target_id":TARGET,
                    "watch":{"repository":"example/widgets","workflow":"ci.yml","branch":"main"}},
                "action":{"placement":"pause"},"actor":person()}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{set}");
    let id = set["trigger_id"].as_str().unwrap().to_owned();
    let (status, on) = world
        .call(
            "POST",
            "/triggers/on",
            json!({"trigger_id":id,"actor":person()}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{on}");
    // The head watch learns the head; nothing is delivered for it.
    until(&world, "the head watch's seed", || async {
        let shown = trigger(&world, &id).await;
        (shown["cursor"]["branch_head"] == early.as_str()).then_some(shown)
    })
    .await;
    assert!(history(&trigger(&world, &id).await).is_empty());

    // A commit lands: one delivery, with no CI verdict anywhere.
    let landed = commit("landed");
    land(&landed);
    let shown = until(&world, "the landed commit delivery", || async {
        let shown = trigger(&world, &id).await;
        (!history(&shown).is_empty()).then_some(shown)
    })
    .await;
    assert_eq!(
        history(&shown),
        [(format!("commit:main:{landed}"), "applied".into(), 1)]
    );
    assert_eq!(shown["history"][0]["evidence"]["commit"], landed.as_str());
    let jobs = JobStore::at(&world.home);
    assert!(
        jobs.list(&checkout)
            .map(|listed| listed.jobs.iter().all(|job| !matches!(
                job.status,
                hotsheet_jobs::Status::Succeeded | hotsheet_jobs::Status::Failed
            )))
            .unwrap_or(true),
        "no CI watch was judged"
    );
    // A second commit lands: one more delivery.
    let next = commit("next");
    land(&next);
    let shown = until(&world, "the next landed commit", || async {
        let shown = trigger(&world, &id).await;
        (history(&shown).len() >= 2).then_some(shown)
    })
    .await;
    assert_eq!(
        history(&shown),
        [
            (format!("commit:main:{landed}"), "applied".into(), 1),
            (format!("commit:main:{next}"), "applied".into(), 1)
        ]
    );
    let (status, paused) = world
        .call(
            "POST",
            "/triggers/pause",
            json!({"trigger_id":id,"actor":person()}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{paused}");
    world.host.begin_stopping();
}
