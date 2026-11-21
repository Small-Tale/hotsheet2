//! HS2-RCR4A6: one code project on two servers, end to end. The fleet of
//! [`Layout::Sites`](super::e2e::Layout::Sites): each server holds its own clone of one
//! bare ticket store (bound to a `git_ref` claim backend, HS2-WJF0D8) and of one bare
//! code repository; target A on the controller is the project's home and target B on
//! the peer, reached over mTLS, is another site of it (HS2-SHH7WW). The controller
//! reports a Linux machine with both fake tools, the peer a Windows machine with one.
//!
//! The orchestrator's own loop places every step on a site that meets its requirements
//! (HS2-9FKTB3), starts the one dispatcher and the one integrator on the home, and relays
//! the peer's worker into the home's coordination pool (HS2-5P112Z). Where the test
//! plays an agent (the dispatcher assigning, a worker claiming, committing, pushing,
//! noting and releasing, the integrator landing commits), it does what the role skills
//! do, through the hosting server's own routes and the shared remotes. Each machine's
//! sync loop is played by one `sync_pass` of its store clone. Nothing is waited for by
//! time: every wait is woken by an event stream or a file change, with a deadline.
use super::e2e::{ACTIONS, ACTOR, Fleet, Side, TARGET_A, TARGET_B, TOOL_A};
use super::orchestration::api;
use super::session_work::{launch_report, now_ms, received_prompt};
use super::*;
use hotsheet_cli::coordination::{
    Assignment, Authority, Coordinator, Phase, ReturnRoute, Role, State,
};
use hotsheet_model::ClaimEventKind;
use std::path::Path;
use std::time::Duration;

const MINUTE: Duration = Duration::from_secs(60);
/// The generation the home site's own workers register under, as the worker skill does.
const LOCAL_GENERATION: &str = "synthetic-generation";

fn now_s() -> i64 {
    i64::try_from(now_ms() / 1000).unwrap()
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = hotsheet_ticketing::git::launch("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=Synthetic",
            "-c",
            "user.email=synthetic@example.test",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

/// One commit adding `file` with a subject naming the ticket, on the current branch.
fn commit_file(dir: &Path, file: &str, subject: &str) -> String {
    let path = dir.join(file);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, format!("{subject}\n")).unwrap();
    git(dir, &["add", file]);
    git(dir, &["commit", "-q", "-m", subject]);
    git(dir, &["rev-parse", "HEAD"])
}

/// The home site's one coordination pool: target A's checkout.
fn home_pool(fleet: &Fleet) -> Coordinator {
    Coordinator::open_with_registry_home(&fleet.a_project, &fleet.a_store, fleet.home.clone())
        .unwrap()
}

/// How many passes one machine's sync loop may take when each push loses a race.
const RACED_PASSES: usize = 5;

/// One pass of a machine's sync loop, repeated at once (as the product's loop repasses
/// promptly) while its push loses every in-cycle retry of the documented race to another
/// pusher in the fleet: a peer server's own sync or claim push may land between this
/// clone's fetch and push (HS2-457FGS). Since HS2-R2BC3N a pass retries a raced push
/// itself and reports `PushRaced` only when every attempt lost, so a repetition here is
/// rare. Any other outcome ends the repetition.
fn sync_pass(store: &Path) -> hotsheet_ticketing::sync::SyncReport {
    use hotsheet_ticketing::sync::{SyncReport, sync_once};
    let mut report = sync_once(store);
    for _ in 1..RACED_PASSES {
        let raced = report == SyncReport::PushRaced;
        if !raced {
            break;
        }
        report = sync_once(store);
    }
    report
}

/// One machine's sync loop publishing what its clone holds and taking what the shared
/// remote holds: [`sync_pass`], which must end synced or up to date.
///
/// HS2-7FGY44: every pass the test plays goes through here. A bare `sync_once` whose
/// push lost the race to the home's own pull-and-push (the relay's `freshen`) left the
/// peer's release only in the peer's clone. A real sync loop pushes it on its next
/// pass; the test has no next pass, so the home waited on `claim_held` until the
/// deadline.
fn sync_published(store: &Path) {
    let report = sync_pass(store);
    assert!(
        matches!(
            report,
            hotsheet_ticketing::sync::SyncReport::Synced { .. }
                | hotsheet_ticketing::sync::SyncReport::UpToDate
        ),
        "{}: {report:?}\n{}\n{}\n{}\n{}",
        store.display(),
        git(store, &["status", "--short"]),
        git(store, &["log", "--oneline", "--stat", "-6", "--all"]),
        std::fs::read_to_string(store.join(".git/hotsheet-sync-health.json")).unwrap_or_default(),
        git(store, &["diff", "HEAD", "origin/main"]),
    );
}

/// One pass of each machine's sync loop, the home's last, so both clones hold every
/// commit either published.
fn converge(fleet: &Fleet) {
    for store in [&fleet.a_store, &fleet.b_store, &fleet.a_store] {
        sync_published(store);
    }
}

/// The home pool once `wanted` holds, woken by the pool's own file changing.
fn pool_until(fleet: &Fleet, what: &str, wanted: impl Fn(&State) -> bool) -> State {
    let directory = fleet.a_project.join(".git").join("hotsheet-coordination");
    std::fs::create_dir_all(&directory).unwrap();
    let waiter = hotsheet_jobs::ChangeWaiter::starting(&[directory.as_path()]).unwrap();
    let deadline = std::time::Instant::now() + 2 * MINUTE;
    loop {
        let state = home_pool(fleet).state().unwrap();
        if wanted(&state) {
            return state;
        }
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        assert!(!remaining.is_zero(), "home pool never {what}: {state:#?}");
        tokio::task::block_in_place(|| waiter.wait(remaining.min(Duration::from_secs(5))));
    }
}

/// A session the loop started, with its run (none for a role session).
struct Placed {
    session_id: String,
    run_id: Option<String>,
    terminal: String,
    worker: String,
}

async fn placed_from(fleet: &Fleet, start: &Value, site: &str) -> Placed {
    assert_eq!(start["mode"], "applied", "{start}");
    assert_eq!(start["target_id"], site, "{start}");
    let (session, terminal, worker) = fleet.started_session(start).await;
    assert_eq!(session["target_id"], site, "{session}");
    assert_eq!(session["tool"], TOOL_A, "{session}");
    let side = if site == TARGET_A {
        Side::Controller
    } else {
        Side::Peer
    };
    // The session runs on the machine of its site, in that machine's own clone.
    let report = launch_report(&fleet.reports, &terminal);
    assert_eq!(
        Path::new(report["cwd"].as_str().unwrap()),
        hotsheet_sync::paths::canonicalize(fleet.project(side)).unwrap(),
        "{report}"
    );
    assert!(
        fleet
            .drawer(side)
            .await
            .iter()
            .any(|tab| tab["id"] == terminal.as_str())
    );
    Placed {
        session_id: session["session_id"].as_str().unwrap().to_owned(),
        run_id: start["run_id"].as_str().map(str::to_owned),
        terminal,
        worker,
    }
}

/// The worker session the loop placed for `step`, on `site`, with why every other site
/// was passed over.
async fn placed_step(fleet: &Fleet, step: &str, site: &str, passed: &str) -> Placed {
    let start = fleet.started_for(step, 2 * MINUTE).await;
    let other = if site == TARGET_A { TARGET_B } else { TARGET_A };
    let sites = start["sites"].as_array().unwrap();
    assert!(
        sites
            .iter()
            .any(|entry| entry["target_id"] == other && entry["detail"] == passed),
        "{start}"
    );
    let placed = placed_from(fleet, &start, site).await;
    let run = placed.run_id.clone().expect("a step's start names its run");
    let run = fleet.ok("GET", &format!("/runs/{run}"), Value::Null).await;
    assert_eq!(run["mode"], "dispatcher", "{run}");
    assert_eq!(run["target"]["target_id"], site, "{run}");
    assert_eq!(run["operation_id"], format!("sched-run-{step}").as_str());
    placed
}

/// The role session the loop started, by role: always on the home.
async fn role_started(fleet: &Fleet, role: &str) -> Placed {
    let start = fleet
        .decided(
            &format!("a started {role}"),
            |entry| {
                entry["kind"] == "start"
                    && entry["outcome"] == "started"
                    && entry["reason"] == "role_needed"
                    && entry["detail"] == role
            },
            3 * MINUTE,
        )
        .await;
    assert_eq!(
        start["operation_id"],
        format!("sched-role-{role}-{TARGET_A}").as_str(),
        "{start}"
    );
    placed_from(fleet, &start, TARGET_A).await
}

fn assignment(
    fleet: &Fleet,
    id: &str,
    worker: &Placed,
    generation: &str,
    ticket: &hotsheet_model::Ticket,
    base: &str,
    scope: &str,
) -> Assignment {
    serde_json::from_value(json!({
        "id": id, "worker": worker.worker, "generation": generation,
        "ticket": {"connection_id": fleet.connection(Side::Controller),
            "native_id": ticket.id.to_string()},
        "slug": ticket.slug, "base": base, "phase": "offered",
        "return_route": {"kind": "human_relay"}, "scope": [scope],
    }))
    .unwrap()
}

async fn claim(fleet: &Fleet, side: Side, ticket: &hotsheet_model::Ticket, worker: &str) -> u16 {
    let (status, _) = api(
        fleet.side_router(side),
        "POST",
        &format!("/tickets/{}/claim", ticket.id),
        json!({"worker": worker, "lease_minutes": 30}),
        true,
    )
    .await;
    status.as_u16()
}

async fn release(fleet: &Fleet, side: Side, ticket: &hotsheet_model::Ticket, worker: &str) {
    let (status, body) = api(
        fleet.side_router(side),
        "POST",
        &format!("/tickets/{}/release", ticket.id),
        json!({"worker": worker}),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

async fn patch(fleet: &Fleet, side: Side, ticket: &hotsheet_model::Ticket, body: Value) {
    let (status, reply) = api(
        fleet.side_router(side),
        "PATCH",
        &format!("/tickets/{}", ticket.id),
        body,
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
}

/// The integrator lands one handed-off commit on `main` once, linearly: a fast-forward
/// when `main` has not moved since the work's base, else the commit replayed on top.
/// Then it publishes `main`, records the assignment done and completes the ticket.
async fn integrate(
    fleet: &Fleet,
    pool: &Coordinator,
    integrator: &Placed,
    id: &str,
    commit: &str,
    ticket: &hotsheet_model::Ticket,
) -> String {
    let project = &fleet.a_project;
    pool.mark(
        &integrator.worker,
        "integrate-1",
        now_s(),
        id,
        Phase::Integrating,
    )
    .unwrap();
    let head = git(project, &["rev-parse", "HEAD"]);
    let fast_forward = git(project, &["merge-base", head.as_str(), commit]) == head;
    if fast_forward {
        git(project, &["merge", "-q", "--ff-only", commit]);
    } else {
        git(project, &["cherry-pick", commit]);
    }
    git(project, &["push", "-q", "origin", "main"]);
    pool.mark(&integrator.worker, "integrate-1", now_s(), id, Phase::Ci)
        .unwrap();
    pool.mark(&integrator.worker, "integrate-1", now_s(), id, Phase::Done)
        .unwrap();
    patch(
        fleet,
        Side::Controller,
        ticket,
        json!({"status":"completed"}),
    )
    .await;
    let base = git(project, &["rev-parse", "HEAD"]);
    pool.publish(
        &integrator.worker,
        "integrate-1",
        &now_s,
        &base,
        Vec::new(),
        &[],
        &[],
        false,
    )
    .unwrap();
    base
}

/// A worker on the home site does its work in a worktree of the home checkout from
/// `base`, hands it off in the pool and releases its claim.
async fn work_on_home(
    fleet: &Fleet,
    pool: &Coordinator,
    worker: &Placed,
    id: &str,
    ticket: &hotsheet_model::Ticket,
    base: &str,
    file: &str,
) -> String {
    assert_eq!(
        claim(fleet, Side::Controller, ticket, &worker.worker).await,
        200
    );
    pool.worker_transition(&worker.worker, LOCAL_GENERATION, id, Phase::Accepted, None)
        .unwrap();
    let tree = fleet.root.path().join(format!("worktree-{id}"));
    git(
        &fleet.a_project,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            &format!("work/{id}"),
            tree.to_str().unwrap(),
            base,
        ],
    );
    let commit = commit_file(&tree, file, &format!("{} synthetic change", ticket.slug));
    pool.worker_transition(
        &worker.worker,
        LOCAL_GENERATION,
        id,
        Phase::Handoff,
        Some(commit.clone()),
    )
    .unwrap();
    release(fleet, Side::Controller, ticket, &worker.worker).await;
    commit
}

fn ticket_in(store: &Path, ticket: &hotsheet_model::Ticket) -> hotsheet_model::Ticket {
    FsStore::open(store)
        .unwrap()
        .read_ticket(&ticket.id)
        .unwrap()
}

/// HS2-RCR4A6: the whole path across two machines. Three steps of one project are
/// planned through its home; each is placed only on a site that meets its OS or tool
/// requirement; one dispatcher and one integrator run on the home; the peer's worker
/// joins the home's pool through the relay, is told its assignment, and hands off over
/// a pushed branch whose release wakes the home's loop through the run event stream;
/// overlapping scopes on the two machines are serialized by the one pool; every commit
/// lands on a linear `main` once; every ticket completes, with one run, one session and
/// one claim each; both store clones converge.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn hs2_rcr4a6_one_project_on_two_servers_end_to_end() {
    let mut fleet = Fleet::sites().await;
    // The project is one project of two sites, both bound to one store.
    let sites = fleet.ok("GET", "/sites", Value::Null).await;
    let projects = sites["projects"].as_array().unwrap();
    assert_eq!(projects.len(), 1, "{sites}");
    assert_eq!(projects[0]["project_id"], TARGET_A, "{sites}");
    assert_eq!(projects[0]["sites"].as_array().unwrap().len(), 2, "{sites}");
    assert_eq!(
        projects[0]["sites"][0]["destination"]["checkout_authority"],
        projects[0]["sites"][1]["destination"]["checkout_authority"],
        "{sites}"
    );

    // A person set the home's pool up for dispatcher authority from the published base;
    // the integrator who did it has since left (its lease is long expired).
    let pool = home_pool(&fleet);
    let mut base = git(&fleet.a_project, &["rev-parse", "HEAD"]);
    let earlier = now_s() - 7200;
    pool.register(
        Role::Integrator,
        "person-root",
        "root-1",
        earlier,
        60,
        ReturnRoute::HumanRelay,
    )
    .unwrap();
    pool.set_authority(
        "person-root",
        "root-1",
        earlier,
        Authority::Dispatcher,
        true,
    )
    .unwrap();
    pool.publish(
        "person-root",
        "root-1",
        &|| earlier,
        &base,
        Vec::new(),
        &[],
        &[],
        false,
    )
    .unwrap();

    // Synthetic tickets in the shared store, created on the home and published.
    let orchestration = fleet.orchestration_ticket("Coordinate the synthetic project");
    let windows = fleet.target_ticket(Side::Controller, "Windows-only change");
    let linux = fleet.target_ticket(Side::Controller, "Linux-only change");
    let tooled = fleet.target_ticket(Side::Controller, "Change that needs the codex fake");
    let decoy = fleet.target_ticket(Side::Controller, "Ticket both machines claim at once");
    converge(&fleet);

    // 1. Two machines claim one ticket at the same instant through their own servers:
    //    the git-ref claim backend lets exactly one hold it, and both clones agree.
    let (on_home, on_peer) = tokio::join!(
        claim(&fleet, Side::Controller, &decoy, "rival-home"),
        claim(&fleet, Side::Peer, &decoy, "rival-peer"),
    );
    assert_eq!(
        [on_home, on_peer].iter().filter(|s| **s == 200).count(),
        1,
        "{on_home} {on_peer}"
    );
    let (winner, winning_side) = if on_home == 200 {
        ("rival-home", Side::Controller)
    } else {
        ("rival-peer", Side::Peer)
    };
    converge(&fleet);
    for store in [&fleet.a_store, &fleet.b_store] {
        let held = ticket_in(store, &decoy);
        assert_eq!(held.claimed_by.as_deref(), Some(winner), "{held:?}");
    }
    release(&fleet, winning_side, &decoy, winner).await;

    // 2. Plan the three steps through the home site, each with a requirement only one
    //    site meets, and turn automatic placement on with the home's role sessions.
    let mut steps = Vec::new();
    for (ticket, requirements) in [
        (&windows, json!({"os":["windows"]})),
        (&linux, json!({"os":["linux"]})),
        (&tooled, json!({"tools":["fake-codex"]})),
    ] {
        let defined = fleet
            .ok(
                "POST",
                "/readiness/define",
                json!({"actor":Fleet::ai(),"ticket":{"native_id":orchestration.slug},
                    "target_id":TARGET_A,"connection_id":fleet.connection(Side::Controller),
                    "native_id":ticket.slug,"prerequisites":[],"requirements":requirements}),
            )
            .await;
        assert_eq!(defined["level"], "confirmed", "{defined}");
        steps.push(defined["step"]["step_id"].as_str().unwrap().to_owned());
    }
    let [step_w, step_l, step_x] = [&steps[0], &steps[1], &steps[2]];
    fleet
        .policy(json!({
            "machine":{"max_sessions":6},
            "tools":[{"tool":TOOL_A,"max_sessions":8}],
            "targets":[
                {"target_id":TARGET_A,"actions":ACTIONS,"tools":[{"tool":TOOL_A}],"max_sessions":4},
                {"target_id":TARGET_B,"actions":ACTIONS,"tools":[{"tool":TOOL_A}],"max_sessions":1}],
            "placement":{"automatic":true,"actor":ACTOR,"cooldown_minutes":0,
                "role_sessions":[{"target_id":TARGET_A,"roles":["dispatcher","integrator"]}]}
        }))
        .await;

    // 3. The dispatcher starts on the home, and each step on the one site that meets it.
    let dispatcher = role_started(&fleet, "dispatcher").await;
    let w = placed_step(&fleet, step_w, TARGET_B, "os:windows").await;
    let l = placed_step(&fleet, step_l, TARGET_A, "os:linux").await;
    let x = placed_step(&fleet, step_x, TARGET_A, "tool:fake-codex").await;
    // HS2-6GE7WM: the timeline, which the Orchestration view's cards read, shows each
    // step where its run was placed: the Windows step on the peer, the others at home,
    // although all three steps name the home site.
    let timeline = fleet.ok("GET", "/timeline", Value::Null).await;
    let item = |step: &str| {
        timeline["waves"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|wave| wave["items"].as_array().unwrap())
            .find(|item| item["step_id"] == step)
            .cloned()
            .unwrap_or_else(|| panic!("no item for {step}: {timeline}"))
    };
    let on_peer = item(step_w);
    assert_eq!(on_peer["target"]["target_id"], TARGET_A, "{on_peer}");
    assert_eq!(
        on_peer["run"]["run_id"],
        w.run_id.as_deref().unwrap(),
        "{on_peer}"
    );
    assert_eq!(on_peer["site_id"], TARGET_B, "{on_peer}");
    assert_eq!(on_peer["location"], "remote", "{on_peer}");
    assert_eq!(on_peer["machine_label"], "Synthetic peer", "{on_peer}");
    assert_eq!(
        on_peer["project_label"],
        format!("Target {TARGET_B}").as_str(),
        "{on_peer}"
    );
    for (step, placed) in [(step_l, &l), (step_x, &x)] {
        let at_home = item(step);
        assert_eq!(at_home["run"]["run_id"], placed.run_id.as_deref().unwrap());
        assert_eq!(at_home["site_id"], TARGET_A, "{at_home}");
        assert_eq!(at_home["location"], "local", "{at_home}");
        assert!(at_home.get("machine_label").is_none(), "{at_home}");
        assert_eq!(
            at_home["project_label"],
            format!("Target {TARGET_A}").as_str(),
            "{at_home}"
        );
    }
    pool.register(
        Role::Dispatcher,
        &dispatcher.worker,
        "dispatch-1",
        now_s(),
        3600,
        ReturnRoute::HumanRelay,
    )
    .unwrap();
    // The home's workers join its pool themselves; each takes up its role.
    for worker in [&l, &x] {
        received_prompt(&fleet.reports, &worker.terminal, "`hotsheet-worker`");
        fleet.join_pool(Side::Controller, &worker.worker);
    }
    received_prompt(&fleet.reports, &w.terminal, "`hotsheet-worker`");
    // The peer's worker joins the home's pool through the orchestrator's relay, under its
    // session as its generation, with its run's exact ticket as its request.
    pool_until(&fleet, "joined by the peer's worker", |state| {
        state
            .workers
            .get(&w.worker)
            .is_some_and(|member| member.generation == w.session_id)
    });

    // 4. The dispatcher offers the windows and linux work. The tooled work overlaps the
    //    windows scope held on the other machine, so the one pool refuses it for now.
    pool.assign(
        &dispatcher.worker,
        "dispatch-1",
        now_s(),
        assignment(
            &fleet,
            "assign-l",
            &l,
            LOCAL_GENERATION,
            &linux,
            &base,
            "src/library",
        ),
    )
    .unwrap();
    pool.assign(
        &dispatcher.worker,
        "dispatch-1",
        now_s(),
        assignment(
            &fleet,
            "assign-w",
            &w,
            &w.session_id,
            &windows,
            &base,
            "src/shared",
        ),
    )
    .unwrap();
    let refused = pool
        .assign(
            &dispatcher.worker,
            "dispatch-1",
            now_s(),
            assignment(
                &fleet,
                "assign-x",
                &x,
                LOCAL_GENERATION,
                &tooled,
                &base,
                "src/shared/tool",
            ),
        )
        .unwrap_err();
    assert!(refused.to_string().contains("overlaps"), "{refused}");

    // 5. The home's linux worker claims, works and hands off on its own machine.
    let l_commit = work_on_home(
        &fleet,
        &pool,
        &l,
        "assign-l",
        &linux,
        &base,
        "src/library/linux.txt",
    )
    .await;

    // 6. The peer's worker is told by its own host that the home assigned it its ticket.
    received_prompt(&fleet.reports, &w.terminal, "home site assigned you");
    assert_eq!(claim(&fleet, Side::Peer, &windows, &w.worker).await, 200);
    sync_published(&fleet.b_store);
    // It works in its own clone, pushes its branch, notes the handoff and only then
    // releases its claim, through its own server.
    let peer = &fleet.b_project;
    git(peer, &["checkout", "-q", "-b", "site/windows", &base]);
    let w_commit = commit_file(
        peer,
        "src/shared/windows.txt",
        &format!("{} synthetic change", windows.slug),
    );
    git(peer, &["push", "-q", "origin", "site/windows"]);
    patch(
        &fleet,
        Side::Peer,
        &windows,
        json!({"note_kind":"regular","note":format!(
            "## Integration handoff\nBranch: site/windows\nCommit: {w_commit}\nSite: {TARGET_B}"
        )}),
    )
    .await;
    let released_at = now_ms();
    release(&fleet, Side::Peer, &windows, &w.worker).await;
    sync_published(&fleet.b_store);

    // 7. The release reaches the controller on the peer's change feed; that run event
    //    wakes the home's loop, whose relay records the handoff with the pushed commit,
    //    fetched into the home checkout. Nobody here asks the controller to sync or tick.
    let state = pool_until(&fleet, "given the peer's handoff", |state| {
        state
            .assignments
            .get("assign-w")
            .is_some_and(|a| a.phase == Phase::Handoff)
    });
    let handed = &state.assignments["assign-w"];
    assert_eq!(handed.commit.as_deref(), Some(w_commit.as_str()));
    let kinds: Vec<String> = handed
        .transitions
        .iter()
        .map(|t| {
            serde_json::to_value(t.kind)
                .unwrap()
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    assert_eq!(kinds, ["offered", "accepted", "handoff"], "{handed:#?}");
    assert_eq!(
        git(
            &fleet.a_project,
            &["rev-parse", "refs/hotsheet/site-handoffs/assign-w"]
        ),
        w_commit
    );
    // HS2-ZNET66: the run sees the release on the peer's feed, and the home may record
    // the handoff first: any pass relays every unfinished run and pulls the home's
    // clone, so the two are not ordered. Wait for the run on its event stream.
    let w_run = w.run_id.clone().unwrap();
    let deadline = std::time::Instant::now() + 2 * MINUTE;
    let mut cursor = 0;
    let (run, intervened) = loop {
        let run = fleet
            .ok("GET", &format!("/runs/{w_run}"), Value::Null)
            .await;
        let seen = run["history"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["to"] == "intervened")
            .cloned();
        if let Some(intervened) = seen {
            break (run, intervened);
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the run never saw the release: {run:#}"
        );
        cursor = fleet.next_event(Some(&w_run), cursor).await;
    };
    assert!(
        intervened["at_ms"].as_u64().unwrap() >= released_at,
        "{run:#}"
    );
    // The home recorded the handoff only after the peer released, whichever pass saw
    // the release first: the run's event or another pass pulling the home's clone.
    let handed_at = handed
        .transitions
        .iter()
        .find(|t| t.kind == hotsheet_cli::coordination::TransitionKind::Handoff)
        .map(|t| {
            time::OffsetDateTime::parse(&t.at, &time::format_description::well_known::Rfc3339)
                .unwrap()
                .unix_timestamp_nanos()
                / 1_000_000
        })
        .unwrap();
    assert!(handed_at >= i128::from(released_at), "{handed:#?} {run:#}");

    // 8. Handoffs are pending in the home's pool: the integrator starts on the home and
    //    lands each commit once, in order, on a linear main.
    let integrator = role_started(&fleet, "integrator").await;
    pool.register(
        Role::Integrator,
        &integrator.worker,
        "integrate-1",
        now_s(),
        3600,
        ReturnRoute::HumanRelay,
    )
    .unwrap();
    integrate(&fleet, &pool, &integrator, "assign-l", &l_commit, &linux).await;
    base = integrate(&fleet, &pool, &integrator, "assign-w", &w_commit, &windows).await;

    // 9. The overlapping scope is free now: the dispatcher offers the tooled work.
    pool.assign(
        &dispatcher.worker,
        "dispatch-1",
        now_s(),
        assignment(
            &fleet,
            "assign-x",
            &x,
            LOCAL_GENERATION,
            &tooled,
            &base,
            "src/shared/tool",
        ),
    )
    .unwrap();
    let x_commit = work_on_home(
        &fleet,
        &pool,
        &x,
        "assign-x",
        &tooled,
        &base,
        "src/shared/tool/codex.txt",
    )
    .await;
    integrate(&fleet, &pool, &integrator, "assign-x", &x_commit, &tooled).await;

    // 10. Both clones converge; every run follows its own site's change feed to done.
    converge(&fleet);
    for placed in [&w, &l, &x] {
        fleet
            .run_until(placed.run_id.as_deref().unwrap(), "completed", |status| {
                status["state"] == "completed"
            })
            .await;
    }

    // One run per step, on the site it was placed on, and one worker session each; one
    // dispatcher and one integrator, both on the home; nothing else started.
    let runs = fleet.runs().await;
    assert_eq!(runs.len(), 3, "{runs:#?}");
    let sessions = fleet.sessions().await;
    let count = |role: &str, site: &str| {
        sessions
            .iter()
            .filter(|session| session["role"] == role && session["target_id"] == site)
            .count()
    };
    assert_eq!(
        (
            count("worker", TARGET_A),
            count("worker", TARGET_B),
            count("dispatcher", TARGET_A),
            count("integrator", TARGET_A),
            sessions.len()
        ),
        (2, 1, 1, 1, 5),
        "{sessions:#?}"
    );
    let log = fleet.decisions().await;
    let started: Vec<_> = log
        .iter()
        .filter(|entry| entry["kind"] == "start" && entry["outcome"] == "started")
        .collect();
    assert_eq!(started.len(), 5, "{log:#?}");

    // Every ticket is complete in both clones, which hold the same history, and each was
    // claimed exactly once, by its own run's worker.
    let a_head = git(&fleet.a_store, &["rev-parse", "HEAD"]);
    assert_eq!(git(&fleet.b_store, &["rev-parse", "HEAD"]), a_head);
    for (ticket, placed) in [(&windows, &w), (&linux, &l), (&tooled, &x)] {
        for store in [&fleet.a_store, &fleet.b_store] {
            let read = ticket_in(store, ticket);
            assert_eq!(
                serde_json::to_value(read.status).unwrap(),
                "completed",
                "{read:?}"
            );
            let claims: Vec<_> = read
                .claim_history
                .iter()
                .filter(|event| event.kind == ClaimEventKind::Claim)
                .map(|event| event.worker.clone())
                .collect();
            assert_eq!(claims, std::slice::from_ref(&placed.worker), "{read:?}");
        }
    }

    // A linear main holding each commit once, the same on the remote and both machines.
    let main = git(&fleet.a_project, &["rev-parse", "main"]);
    git(&fleet.b_project, &["fetch", "-q", "origin"]);
    assert_eq!(git(&fleet.b_project, &["rev-parse", "origin/main"]), main);
    assert_eq!(git(&fleet.a_project, &["rev-list", "--merges", "main"]), "");
    let subjects = git(&fleet.a_project, &["log", "--format=%s", "main"]);
    for ticket in [&windows, &linux, &tooled] {
        let wanted = format!("{} synthetic change", ticket.slug);
        assert_eq!(
            subjects.lines().filter(|line| *line == wanted).count(),
            1,
            "{subjects}"
        );
    }
    fleet.shut_down().await;
}

/// HS2-RCR4A6, the peer-outage variant: a step running on the peer keeps its one run and
/// its one claim while the peer is down. The home has room again, yet no pass places the
/// step a second time there, and nothing claims its ticket a second time.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn hs2_rcr4a6_a_peer_outage_takes_no_second_claim() {
    let mut fleet = Fleet::sites().await;
    let orchestration = fleet.orchestration_ticket("Coordinate the synthetic project");
    let linux = fleet.target_ticket(Side::Controller, "Linux-only change");
    let free = fleet.target_ticket(Side::Controller, "Change either machine can make");
    converge(&fleet);
    let connection = fleet.connection(Side::Controller);
    let define = |ticket: &hotsheet_model::Ticket, requirements: Value| {
        json!({"actor":Fleet::ai(),"ticket":{"native_id":orchestration.slug},
            "target_id":TARGET_A,"connection_id":connection,
            "native_id":ticket.slug,"prerequisites":[],"requirements":requirements})
    };
    // One session on each machine.
    fleet
        .policy(json!({
            "targets":[
                {"target_id":TARGET_A,"actions":ACTIONS,"tools":[{"tool":TOOL_A}],"max_sessions":1},
                {"target_id":TARGET_B,"actions":ACTIONS,"tools":[{"tool":TOOL_A}],"max_sessions":1}],
            "placement":{"automatic":true,"actor":ACTOR,"cooldown_minutes":0}
        }))
        .await;
    let defined = fleet
        .ok(
            "POST",
            "/readiness/define",
            define(&linux, json!({"os":["linux"]})),
        )
        .await;
    let step_l = defined["step"]["step_id"].as_str().unwrap().to_owned();
    let l = placed_step(&fleet, &step_l, TARGET_A, "os:linux").await;
    // The home is full, so work either machine could do is placed on the peer.
    let defined = fleet
        .ok("POST", "/readiness/define", define(&free, json!({})))
        .await;
    let step_f = defined["step"]["step_id"].as_str().unwrap().to_owned();
    let start = fleet.started_for(&step_f, 2 * MINUTE).await;
    let f = placed_from(&fleet, &start, TARGET_B).await;
    let f_run = f.run_id.clone().unwrap();

    // The peer's worker claims its ticket; its machine publishes the claim.
    received_prompt(&fleet.reports, &f.terminal, "`hotsheet-worker`");
    assert_eq!(claim(&fleet, Side::Peer, &free, &f.worker).await, 200);
    fleet
        .run_until(&f_run, "dispatched", |status| {
            status["state"] == "dispatched"
        })
        .await;
    converge(&fleet);

    // The peer drops off the network with its session and claim in place.
    fleet.peer_offline().await;

    // The home's work finishes, so the home has a free session again.
    assert_eq!(
        claim(&fleet, Side::Controller, &linux, &l.worker).await,
        200
    );
    patch(
        &fleet,
        Side::Controller,
        &linux,
        json!({"status":"completed"}),
    )
    .await;
    release(&fleet, Side::Controller, &linux, &l.worker).await;
    fleet
        .run_until(l.run_id.as_deref().unwrap(), "completed", |status| {
            status["state"] == "completed"
        })
        .await;

    // A pass with room on the home places nothing for the peer's step.
    let tick = fleet
        .ok("POST", "/scheduler/tick", json!({"actor":Fleet::ai()}))
        .await;
    assert_eq!(tick["mode"], "applied", "{tick}");
    let log = fleet.decisions().await;
    let starts: Vec<_> = log
        .iter()
        .filter(|entry| entry["kind"] == "start" && entry["outcome"] == "started")
        .collect();
    assert_eq!(starts.len(), 2, "{log:#?}");
    let runs = fleet.runs().await;
    assert_eq!(runs.len(), 2, "{runs:#?}");
    let run = fleet
        .ok("GET", &format!("/runs/{f_run}"), Value::Null)
        .await;
    assert_eq!(run["target"]["target_id"], TARGET_B, "{run}");
    assert_eq!(run["state"], "dispatched", "{run}");
    assert_eq!(fleet.sessions().await.len(), 2);
    // The shared store still names one holder, claimed once.
    sync_published(&fleet.a_store);
    let read = ticket_in(&fleet.a_store, &free);
    assert_eq!(
        read.claimed_by.as_deref(),
        Some(f.worker.as_str()),
        "{read:?}"
    );
    assert_eq!(
        read.claim_history
            .iter()
            .filter(|event| event.kind == ClaimEventKind::Claim)
            .count(),
        1,
        "{read:?}"
    );
    fleet.peer_online().await;
    fleet.shut_down().await;
}

/// HS2-EPRWB6: a ticket of the project is one checked-in plan step whichever site
/// defines it. The checked-in plan names it by the home's connection id, which every
/// machine binding the home shares; the orchestrator reads the step back with its own
/// id for the site the step names, and a prerequisite named through the peer is an
/// edge to the step planned through the home.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn hs2_eprwb6_one_plan_step_whichever_site_defines_it() {
    let fleet = Fleet::sites().await;
    let orchestration = fleet.orchestration_ticket("Coordinate the synthetic project");
    let first = fleet.target_ticket(Side::Controller, "Change made first");
    let then = fleet.target_ticket(Side::Controller, "Change made after it");
    converge(&fleet);
    let (home, peer) = (
        fleet.connection(Side::Controller),
        fleet.connection(Side::Peer),
    );
    assert_ne!(
        home, peer,
        "each machine names the shared store its own way"
    );
    let define = |side: Side| {
        let fleet = &fleet;
        let orchestration = &orchestration;
        let then = &then;
        let first = &first;
        async move {
            fleet
                .ok(
                    "POST",
                    "/readiness/define",
                    json!({"actor":Fleet::ai(),"ticket":{"native_id":orchestration.slug},
                        "target_id":Fleet::target(side),"connection_id":fleet.connection(side),
                        "native_id":then.slug,"prerequisites":[fleet.after(side, first)]}),
                )
                .await
        }
    };
    let checked_in = || -> Value {
        let path = Path::new(&fleet.owner.root).join(".hotsheet2/orchestration-plan.json");
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    };

    // Through the peer, then the home, then the peer again: one step throughout.
    let through_peer = define(Side::Peer).await;
    let step = through_peer["step"]["step_id"].as_str().unwrap().to_owned();
    let plan = checked_in();
    assert_eq!(plan["steps"].as_array().unwrap().len(), 1, "{plan}");
    assert_eq!(plan["steps"][0]["target"]["target_id"], TARGET_B, "{plan}");
    assert_eq!(
        plan["steps"][0]["target"]["connection_id"],
        home.as_str(),
        "{plan}"
    );
    assert_eq!(
        plan["steps"][0]["prerequisites"][0]["connection_id"],
        home.as_str(),
        "{plan}"
    );
    let through_home = define(Side::Controller).await;
    assert_eq!(
        through_home["step"]["step_id"],
        step.as_str(),
        "{through_home}"
    );
    assert_eq!(through_home["step"]["revision"], 2, "{through_home}");
    let again = define(Side::Peer).await;
    assert_eq!(again["step"]["step_id"], step.as_str(), "{again}");
    assert_eq!(checked_in()["steps"].as_array().unwrap().len(), 1);
    assert_eq!(again["step"]["revision"], 3, "{again}");
    let repeated = define(Side::Peer).await;
    assert_eq!(repeated["step"]["revision"], 3, "{repeated}");
    // Read back with the peer's own id, so a dispatch on the peer finds the ticket.
    let shown = fleet.readiness(&step).await;
    assert_eq!(shown["step"]["target"]["target_id"], TARGET_B, "{shown}");
    assert_eq!(
        shown["step"]["target"]["connection_id"],
        peer.as_str(),
        "{shown}"
    );
    // Its prerequisite, named through the peer, is the step planned through the home.
    let before = fleet
        .plan(&orchestration, Side::Controller, &first, json!([]))
        .await;
    let before = before["step"]["step_id"].as_str().unwrap().to_owned();
    let shown = fleet.readiness(&before).await;
    assert_eq!(shown["unblocks"], json!([step]), "{shown}");
    assert_eq!(fleet.readiness(&step).await["level"], "blocked");
    assert_eq!(checked_in()["steps"].as_array().unwrap().len(), 2);
    fleet.shut_down().await;
}

/// HS2-6NQKP9: the policy requires a different resource of each site. A session on the
/// home holds the home's VM through the home's own claim route, so the loop passes the
/// home over by that name and places the step on the peer, judged by the peer's own
/// requirement read from the peer's backend. The peer's session is told to acquire
/// that resource only. Synthetic resource names.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn hs2_6nqkp9_each_site_is_judged_by_its_own_policy_resources() {
    let mut fleet = Fleet::sites().await;
    let orchestration = fleet.orchestration_ticket("Coordinate the synthetic project");
    let either = fleet.target_ticket(Side::Controller, "Change either machine can make");
    converge(&fleet);
    let (status, grant) = api(
        fleet.router.clone(),
        "POST",
        "/resource-claims/vm/home-vm/acquire",
        json!({"duration_ms": 1_800_000}),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{grant}");
    assert_eq!(grant["acquired"], true, "{grant}");
    let step = fleet
        .plan(&orchestration, Side::Controller, &either, json!([]))
        .await;
    let step = step["step"]["step_id"].as_str().unwrap().to_owned();
    fleet
        .policy(json!({
            "machine":{"max_sessions":6},
            "tools":[{"tool":TOOL_A,"max_sessions":8}],
            "targets":[
                {"target_id":TARGET_A,"actions":ACTIONS,"tools":[{"tool":TOOL_A}],"max_sessions":2},
                {"target_id":TARGET_B,"actions":ACTIONS,"tools":[{"tool":TOOL_A}],"max_sessions":2}],
            "placement":{"automatic":true,"actor":ACTOR,"cooldown_minutes":0,
                "resources":[
                    {"target_id":TARGET_A,"resources":[{"namespace":"vm","resource":"home-vm"}]},
                    {"target_id":TARGET_B,"resources":[{"namespace":"vm","resource":"peer-vm"}]}]}
        }))
        .await;

    let start = fleet.started_for(&step, 2 * MINUTE).await;
    assert_eq!(start["target_id"], TARGET_B, "{start}");
    assert!(
        start["sites"].as_array().unwrap().iter().any(|entry| {
            entry["target_id"] == TARGET_A
                && entry["reason"] == "resource_held"
                && entry["detail"] == "vm/home-vm"
        }),
        "{start}"
    );
    let placed = placed_from(&fleet, &start, TARGET_B).await;
    let prompt = received_prompt(&fleet.reports, &placed.terminal, "`vm/peer-vm`");
    assert!(!prompt.contains("home-vm"), "{prompt}");
    fleet.shut_down().await;
}

/// HS2-XA9DZA: a branch pushed from the peer's clone of the project is an event on the
/// peer's hosted feed, the one the controller's follower holds open for the site: the
/// feed says its remote-tracking refs changed, naming no ref or commit. Synthetic
/// commits only.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn hs2_xa9dza_a_push_on_a_site_reaches_the_followed_feed() {
    let fleet = Fleet::sites().await;
    let feed = format!("/checkouts/{}/target-events", fleet.b_checkout.id);
    let read = |since: u64| {
        let (router, path) = (
            fleet.peer_api.clone(),
            format!("{feed}?since={since}&wait_ms=8000"),
        );
        async move {
            let (status, page) =
                super::orchestration::api(router, "GET", &path, Value::Null, true).await;
            assert_eq!(status, StatusCode::OK, "{page}");
            page
        }
    };
    // Read until a page says the refs changed, as a follower does, within a deadline.
    let changed = |mut cursor: u64, what: &'static str| {
        let read = &read;
        async move {
            let deadline = std::time::Instant::now() + MINUTE;
            loop {
                let page = read(cursor).await;
                cursor = page["cursor"].as_u64().unwrap();
                if page["remote_refs_changed"] == true {
                    assert!(page.get("events").is_some(), "{page}");
                    return cursor;
                }
                assert!(std::time::Instant::now() < deadline, "{what}: {page}");
            }
        }
    };
    // The handshake starts the watch; starting it records nothing.
    let (status, handshake) =
        super::orchestration::api(fleet.peer_api.clone(), "GET", &feed, Value::Null, true).await;
    assert_eq!(status, StatusCode::OK, "{handshake}");
    assert!(
        handshake.get("remote_refs_changed").is_none(),
        "{handshake}"
    );
    let cursor = handshake["cursor"].as_u64().unwrap();

    // A site worker commits its handoff on a branch and pushes it to the shared remote.
    let peer = fleet.project(Side::Peer).to_path_buf();
    git(&peer, &["checkout", "-q", "-b", "hs2-xa9dza-handoff"]);
    commit_file(&peer, "src/handoff.txt", "Synthetic handoff");
    git(&peer, &["push", "-q", "origin", "hs2-xa9dza-handoff"]);
    changed(cursor, "the push").await;
    fleet.shut_down().await;
}
