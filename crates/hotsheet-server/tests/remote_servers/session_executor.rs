//! HS2-95SWQ4: an automatic policy on a real server starts a session for confirmed
//! ready work and winds it down at a safe point once the work is finished. The
//! session is a real PTY running the workspace fake agent; no AI tool is launched and
//! nothing leaves loopback. Every wait in this test is on the decision log changing.
use super::orchestration::api;
use super::sessions::{IDLE_TOOL, Roomy, drawer, fake_plugins, git_project};
use super::*;
use hotsheet_ticketing::checkouts::CheckoutRegistry;
use hotsheet_ticketing::{NewTicket, TicketPatch, ops};
use std::path::Path;

const TARGET: &str = "01ARZ3NDEKTSV4RRFFQ69G5FH0";
const NOW: &str = "2026-10-15T00:00:00Z";

fn ticket(store: &FsStore, title: &str) -> hotsheet_model::Ticket {
    ops::create(
        store,
        hotsheet_model::Ulid::new(),
        "TST",
        NOW.into(),
        NewTicket {
            title: title.into(),
            ..Default::default()
        },
    )
    .unwrap()
}

/// One idle fake tool on the target, the shortest idle time and no cooldown.
fn policy(automatic: bool) -> Value {
    json!({
        "machine": {"max_sessions": 4},
        "tools": [{"tool": IDLE_TOOL, "max_sessions": 2}],
        "targets": [{"target_id": TARGET,
            "actions": ["tickets_read", "gates_observe", "sessions_manage", "work_start"],
            "tools": [{"tool": IDLE_TOOL}], "max_sessions": 1}],
        "wind_down": {"idle_minutes": 1, "keep_warm": 0},
        "placement": {"automatic": automatic, "actor": "scheduler-1", "cooldown_minutes": 0}
    })
}

/// The project targets itself, so its checked-in orchestration files are committed
/// like any other project file: uncommitted work would hold the session.
fn commit_all(project: &Path) {
    for arguments in [
        &["add", "-A"][..],
        &[
            "-c",
            "user.name=Synthetic",
            "-c",
            "user.email=synthetic@example.test",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "Synthetic orchestration files",
        ],
    ] {
        let status = hotsheet_ticketing::git::launch("git")
            .arg("-C")
            .arg(project)
            .args(arguments)
            .status()
            .unwrap();
        assert!(status.success(), "{arguments:?}");
    }
}

async fn decisions(router: &axum::Router, base: &str) -> Vec<Value> {
    let mut entries = Vec::new();
    let mut after = 0;
    loop {
        let page = api(
            router.clone(),
            "POST",
            &format!("{base}/scheduler/decisions"),
            json!({"after": after, "limit": 200}),
            true,
        )
        .await
        .1;
        entries.extend(page["entries"].as_array().unwrap().iter().cloned());
        after = page["cursor"].as_u64().unwrap();
        if page["more"] != true {
            return entries;
        }
    }
}

/// Waits, woken by the decision log itself changing, for the first decision the
/// caller is looking for, and returns it with the whole log.
async fn decided(
    router: &axum::Router,
    base: &str,
    home: &Path,
    what: &str,
    wanted: impl Fn(&Value) -> bool,
    within: std::time::Duration,
) -> (Value, Vec<Value>) {
    let directory = home.join("orchestration-scheduler-decisions");
    std::fs::create_dir_all(&directory).unwrap();
    let waiter = hotsheet_jobs::ChangeWaiter::new(&directory).unwrap();
    let deadline = std::time::Instant::now() + within;
    loop {
        let log = decisions(router, base).await;
        if let Some(found) = log.iter().find(|entry| wanted(entry)) {
            return (found.clone(), log);
        }
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        assert!(!remaining.is_zero(), "no {what}: {log:?}");
        tokio::task::block_in_place(|| waiter.wait(remaining));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_95swq4_an_automatic_policy_starts_a_session_for_ready_work_and_winds_it_down() {
    assert!(hotsheet_aitools::test_permission_vault_available());
    hotsheet_aitools::enable_test_permission_vault();
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let source = root.path().join("store");
    let host = state(&source, &home)
        .with_capacity_source(Box::new(Roomy))
        .with_capacity_cache(std::time::Duration::ZERO)
        .with_plugin_dirs(vec![fake_plugins(root.path())]);
    let project = root.path().join("project");
    git_project(&project);
    let checkout = CheckoutRegistry::new(home.join("checkouts.json"))
        .register(&project, None, None, vec![source.clone()])
        .unwrap();
    let connection = checkout.sources[0].connection_id.clone();
    let store = FsStore::open(&source).unwrap();
    let orchestration = ticket(&store, "Coordinate the synthetic placement");
    let work = ticket(&store, "Synthetic ready work");
    let router = app(host.clone());
    let base = format!("/checkouts/{}/orchestration", checkout.id);
    let person = json!({"role":"human","id":"maintainer"});
    let actor = json!({"role":"ai","id":"scheduler-1"});

    let added = api(
        router.clone(),
        "POST",
        &format!("{base}/targets"),
        json!({"target_id":TARGET,"label":"Self","expected_revision":null}),
        true,
    )
    .await;
    assert_eq!(added.0, StatusCode::OK, "{}", added.1);
    let candidate = api(
        router.clone(),
        "POST",
        &format!("{base}/inspect"),
        json!({"destination":{"kind":"local","checkout_id":checkout.id}}),
        true,
    )
    .await
    .1;
    let bound = api(
        router.clone(),
        "PUT",
        &format!("{base}/targets/{TARGET}/binding"),
        json!({"target":{"owner_checkout_id":checkout.id,"target_id":TARGET},
            "expected_manifest_revision":added.1["revision"],"expected_binding_revision":0,
            "previous_destination":null,"destination":candidate["destination"]}),
        true,
    )
    .await;
    assert_eq!(bound.0, StatusCode::OK, "{}", bound.1);
    let expires = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
        + 3_600_000;
    let granted = api(
        router.clone(),
        "POST",
        &format!("{base}/targets/{TARGET}/grants"),
        json!({"actor_id":"scheduler-1",
            "actions":["tickets_read","gates_observe","sessions_manage","work_start"],
            "session_scope":{"tools":[{"tool":IDLE_TOOL}],"max_concurrency":2},
            "expires_at_ms":expires,"actor":person}),
        true,
    )
    .await;
    assert_eq!(granted.0, StatusCode::OK, "{}", granted.1);
    let defined = api(
        router.clone(),
        "POST",
        &format!("{base}/readiness/define"),
        json!({"actor":actor,"ticket":{"native_id":orchestration.slug},"target_id":TARGET,
            "connection_id":connection,"native_id":work.slug}),
        true,
    )
    .await;
    assert_eq!(defined.0, StatusCode::OK, "{}", defined.1);
    assert_eq!(defined.1["level"], "confirmed", "{}", defined.1);
    let step_id = defined.1["step"]["step_id"].as_str().unwrap().to_owned();
    let operation = format!("sched-start-{step_id}");

    // Without automatic placement the work is planned and nothing is started: a tick
    // is a dry run, and no loop runs.
    let stored = api(
        router.clone(),
        "PUT",
        &format!("{base}/policy"),
        json!({"expected_revision":null,"policy":policy(false),"actor":person}),
        true,
    )
    .await;
    assert_eq!(stored.0, StatusCode::OK, "{}", stored.1);
    let dry = api(
        router.clone(),
        "POST",
        &format!("{base}/scheduler/tick"),
        json!({"actor":actor}),
        true,
    )
    .await;
    assert_eq!(dry.0, StatusCode::OK, "{}", dry.1);
    assert_eq!(dry.1["mode"], "dry_run");
    assert_eq!(dry.1["inputs"]["executor"], "available");
    assert_eq!(dry.1["placements"][0]["action"], "start", "{}", dry.1);
    assert_eq!(dry.1["outcomes"], json!([]));
    assert!(drawer(&router).await.is_empty());
    assert!(!home.join("orchestration-sessions.json").exists());

    // A person enables automatic placement. The server's loop starts and places one
    // session of the fake tool for the confirmed step.
    let enabled = api(
        router.clone(),
        "PUT",
        &format!("{base}/policy"),
        json!({"expected_revision":stored.1["revision"],"policy":policy(true),"actor":person}),
        true,
    )
    .await;
    assert_eq!(enabled.0, StatusCode::OK, "{}", enabled.1);
    commit_all(&project);
    let minute = std::time::Duration::from_secs(60);
    let started = |entry: &Value| entry["kind"] == "start" && entry["outcome"] == "started";
    let (start, _) = decided(&router, &base, &home, "started session", started, minute).await;
    assert_eq!(start["mode"], "applied");
    assert_eq!(start["actor_id"], "scheduler-1");
    assert_eq!(start["step_id"], step_id);
    assert_eq!(start["tool"], IDLE_TOOL);
    assert_eq!(start["operation_id"], operation);
    let (status, sessions) = api(
        router.clone(),
        "GET",
        &format!("{base}/sessions"),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let records = sessions.as_array().unwrap();
    assert_eq!(records.len(), 1, "{sessions}");
    let session = &records[0];
    assert_eq!(session["session_id"], start["session_id"]);
    assert_eq!(session["operation_id"], operation);
    assert_eq!(session["state"], "running");
    // HS2-YC2P7K: the step's run was dispatched for the target's dispatcher and the
    // session works it.
    let run_id = session["run_id"].as_str().unwrap().to_owned();
    assert_eq!(start["run_id"], run_id.as_str());
    // It is an ordinary AI terminal of this server, labelled for the project's run.
    let terminals = drawer(&router).await;
    assert_eq!(terminals.len(), 1, "{terminals:?}");
    assert_eq!(terminals[0]["id"], session["hosted"]["session_id"]);
    assert_eq!(terminals[0]["tool"], IDLE_TOOL);
    assert_eq!(
        terminals[0]["name"],
        format!("{}: run {}", checkout.alias, &run_id[run_id.len() - 8..]).as_str()
    );

    // The terminal is killed from outside, which is what a crash looks like to the
    // orchestrator. Its exit wakes the loop: the session is recorded as lost and the
    // step, still ready, gets a new session that replaces it.
    let first = session.clone();
    let killed = api(
        router.clone(),
        "DELETE",
        &format!(
            "/terminals/{}",
            first["hosted"]["session_id"].as_str().unwrap()
        ),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(killed.0, StatusCode::NO_CONTENT);
    let (restart, _) = decided(
        &router,
        &base,
        &home,
        "replacement session",
        |entry| started(entry) && entry["session_id"] != first["session_id"],
        minute,
    )
    .await;
    assert_eq!(restart["operation_id"], operation);
    let (_, sessions) = api(
        router.clone(),
        "GET",
        &format!("{base}/sessions"),
        Value::Null,
        true,
    )
    .await;
    let records = sessions.as_array().unwrap();
    assert_eq!(records.len(), 2, "{sessions}");
    assert_eq!(records[0]["session_id"], first["session_id"]);
    assert_eq!(records[0]["state"], "lost");
    assert_eq!(
        records[0]["history"].as_array().unwrap().last().unwrap()["reason"],
        "host_lost"
    );
    let session = &records[1];
    assert_eq!(session["session_id"], restart["session_id"]);
    assert_eq!(session["operation_id"], format!("{operation}.2").as_str());
    assert_eq!(session["replaces"], first["session_id"]);
    assert_eq!(session["state"], "running");
    assert_eq!(
        session["run_id"],
        run_id.as_str(),
        "the same run, not a second"
    );
    let terminals = drawer(&router).await;
    assert_eq!(terminals.len(), 1, "{terminals:?}");
    assert_eq!(terminals[0]["id"], session["hosted"]["session_id"]);

    // The work is finished in the target, and a refresh confirms it. That readiness
    // event is what tells the loop the session has nothing left to do.
    ops::update(
        &store,
        &work.id,
        NOW.into(),
        TicketPatch {
            status: Some(hotsheet_model::Status::Completed),
            ..Default::default()
        },
    )
    .unwrap();
    // The run learns it, as the target's follower would on the change.
    let synced = api(
        router.clone(),
        "POST",
        &format!("{base}/runs/{run_id}/sync"),
        json!({"actor":actor}),
        true,
    )
    .await;
    assert_eq!(synced.1["state"], "completed", "{}", synced.1);
    let refreshed = api(
        router.clone(),
        "POST",
        &format!("{base}/readiness/refresh"),
        json!({"actor":actor}),
        true,
    )
    .await;
    assert_eq!(refreshed.0, StatusCode::OK, "{}", refreshed.1);
    assert_eq!(
        refreshed.1["counts"],
        json!({"finished": 1}),
        "{}",
        refreshed.1
    );
    commit_all(&project);

    // The session idles out. The loop waits for the time its own pass named, then
    // asks for the wind-down, and the host stops it at its safe point.
    let (stop, log) = decided(
        &router,
        &base,
        &home,
        "wind-down",
        |entry| entry["kind"] == "wind_down" && entry["outcome"] == "wind_down_requested",
        3 * minute,
    )
    .await;
    assert_eq!(stop["session_id"], session["session_id"]);
    assert_eq!(stop["reason"], "idle");
    assert_eq!(stop["trigger"], "retry", "{stop}");
    assert_eq!(
        stop["operation_id"],
        format!("sched-stop-{}", session["session_id"].as_str().unwrap()).as_str()
    );
    // Before that, a pass kept it for its idle time and named when that ends.
    let grace = log
        .iter()
        .find(|entry| entry["kind"] == "keep" && entry["reason"] == "idle_grace")
        .unwrap_or_else(|| panic!("no idle grace was logged: {log:?}"));
    let named = grace["retry_at_ms"].as_u64().unwrap();
    assert!(stop["at_ms"].as_u64().unwrap() >= named, "{stop} {grace}");
    assert!(drawer(&router).await.is_empty());
    let (_, after) = api(
        router.clone(),
        "GET",
        &format!(
            "{base}/sessions/{}",
            session["session_id"].as_str().unwrap()
        ),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(after["state"], "stopped", "{after}");
    assert_eq!(
        after["history"].as_array().unwrap().last().unwrap()["reason"],
        "wound_down"
    );
    assert_eq!(
        after["history"].as_array().unwrap().last().unwrap()["control"],
        "wind_down"
    );

    // No interval drove any of this. Every pass reads each running session once,
    // which is audited, so the journal counts the passes: a few for each start, turn
    // end and exit, the readiness event and the named retry, over more than a
    // minute. A loop on a timer would have left hundreds.
    let (_, audit) = api(
        router.clone(),
        "GET",
        &format!("{base}/audit?after=0&limit=500"),
        Value::Null,
        true,
    )
    .await;
    let entries = audit["entries"].as_array().unwrap();
    let reads = entries
        .iter()
        .filter(|entry| entry["action"] == "sessions_manage" && entry["outcome"] == "allowed")
        .count();
    assert!((4..=40).contains(&reads), "{reads} session reads: {audit}");
    assert_eq!(audit["more"], false, "{audit}");
    let ticks = log.iter().filter(|entry| entry["kind"] == "tick").count();
    assert!(ticks <= 16, "{ticks} logged passes: {log:?}");
    // Nothing was forced, and the stop passed the gate under the scheduler's id.
    assert!(entries.iter().any(|entry| {
        entry["action"] == "scheduler_wind_down"
            && entry["outcome"] == "succeeded"
            && entry["operation_id"] == stop["operation_id"]
    }));

    // Automatic placement is turned off again: the loop ends.
    let off = api(
        router.clone(),
        "PUT",
        &format!("{base}/policy"),
        json!({"expected_revision":enabled.1["revision"],"policy":policy(false),"actor":person}),
        true,
    )
    .await;
    assert_eq!(off.0, StatusCode::OK, "{}", off.1);
}
