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
pub(super) fn sync_published(store: &Path) {
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
pub(super) fn converge(fleet: &Fleet) {
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
pub(super) struct Placed {
    pub(super) session_id: String,
    pub(super) run_id: Option<String>,
    pub(super) terminal: String,
    pub(super) worker: String,
}

pub(super) async fn placed_from(fleet: &Fleet, start: &Value, site: &str) -> Placed {
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
                    // HS2-4JQ41V: an integrator's start names the handoff it is for.
                    && if role == "integrator" {
                        entry["reason"] == "integrator_started"
                            && entry["detail"] == "handoff_pending"
                    } else {
                        entry["reason"] == "role_needed" && entry["detail"] == role
                    }
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

pub(super) async fn claim(
    fleet: &Fleet,
    side: Side,
    ticket: &hotsheet_model::Ticket,
    worker: &str,
) -> u16 {
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

pub(super) fn ticket_in(store: &Path, ticket: &hotsheet_model::Ticket) -> hotsheet_model::Ticket {
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
    // HS2-R2N33M: the project chooses no role sessions.
    peer_outage_takes_no_second_claim(Some(json!([{"target_id":TARGET_A,"roles":[]}]))).await;
}

/// HS2-70Z2Z6: the same outage with the project unlisted, so Auto may start both role
/// sessions on the home. The home's one slot goes to the Linux-only step the peer
/// cannot run, not to a dispatcher, and no pass starts a role session in its place.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn hs2_70z2z6_a_peer_outage_with_auto_role_sessions_takes_no_second_claim() {
    peer_outage_takes_no_second_claim(None).await;
}

/// The peer-outage scenario; `role_sessions` is the policy's list, or `None` for Auto.
async fn peer_outage_takes_no_second_claim(role_sessions: Option<Value>) {
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
    // One worker session on each machine.
    let mut placement = json!({"automatic":true,"actor":ACTOR,"cooldown_minutes":0});
    if let Some(role_sessions) = role_sessions {
        placement["role_sessions"] = role_sessions;
    }
    fleet
        .policy(json!({
            "targets":[
                {"target_id":TARGET_A,"actions":ACTIONS,"tools":[{"tool":TOOL_A}],"max_sessions":1},
                {"target_id":TARGET_B,"actions":ACTIONS,"tools":[{"tool":TOOL_A}],"max_sessions":1}],
            "placement":placement
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

/// HS2-SK9B3T: rewrite the home's pool as its people's own commands would leave it.
fn edit_home_pool(fleet: &Fleet, change: impl FnOnce(&mut State)) {
    let mut state = home_pool(fleet).state().unwrap();
    change(&mut state);
    state.revision += 1;
    std::fs::write(
        fleet
            .a_project
            .join(".git")
            .join("hotsheet-coordination")
            .join("state.json"),
        serde_json::to_vec(&state).unwrap(),
    )
    .unwrap();
}

/// HS2-SK9B3T: the request the peer recorded on a hosted worker session: its kind and
/// the return note it names.
async fn request_on_peer(fleet: &Fleet, placed: &Placed) -> (String, String) {
    let operation = fleet.session(&placed.session_id).await["operation_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let hosted = hotsheet_ticketing::orchestration::HostedSessionRegistry::new(&fleet.peer_home)
        .get(&fleet.b_checkout.id, &operation)
        .unwrap()
        .expect("the peer hosts the worker");
    let request = hosted.request.expect("a request was recorded");
    (
        request.kind.code().to_owned(),
        request.commit.unwrap_or_default(),
    )
}

/// HS2-SK9B3T (docs/26 §26.24): a worker on the peer, a site that is not its project's
/// home, hands off work the home's dispatcher assigned it. The dispatcher then leaves,
/// and the home's integrator (which does not assign under dispatcher authority) returns
/// the handoff for corrections. The home's `target-pool` answer reports that return by
/// the home's own connection id, the peer's ticket names the same ticket under its own,
/// and the controller's loop claims the ticket for the same peer session again and tells
/// it which return to correct. Before the return, while the handoff awaits review,
/// nothing is claimed. Waits on event streams and file changes, never on time.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn hs2_sk9b3t_a_site_workers_returned_handoff_is_claimed_again_for_its_session() {
    site_workers_returned_handoff(SiteWorker::RegistersLocally).await;
}

/// HS2-26Z8KB (docs/26 §26.24, §26.30): the same, for a site worker that never
/// registers in its own site clone's pool. The relay makes it a member of the home's
/// pool, and that membership is what makes it joined: the peer's own listing never
/// says so, yet its correction is claimed for it all the same.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn hs2_26z8kb_a_site_worker_joined_only_through_the_home_pool_is_claimed_for() {
    site_workers_returned_handoff(SiteWorker::RelayOnly).await;
}

/// HS2-RHKSXQ (docs/26 §26.24): the same, but the site worker's session is lost (its
/// terminal dies on the peer) while its handoff awaits review. A pass then places
/// nothing: the run is in review, not missing work. Once the home's integrator returns
/// it, the loop reads the home's answer, starts a new session for the step on the peer
/// on the same run, and claims the correction for that session's worker.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn hs2_rhksxq_a_lost_site_session_is_placed_again_on_its_returned_run() {
    site_workers_returned_handoff(SiteWorker::LostInReview).await;
}

/// How the peer's worker behaves in [`site_workers_returned_handoff`].
#[derive(Clone, Copy, PartialEq, Eq)]
enum SiteWorker {
    /// Registers in its own clone's pool, as the worker skill does.
    RegistersLocally,
    /// HS2-26Z8KB: never registers there; only the relay joins it to the home's pool.
    RelayOnly,
    /// HS2-RHKSXQ: registers locally, and its session is lost while in review.
    LostInReview,
}

async fn site_workers_returned_handoff(variant: SiteWorker) {
    let registers_locally = variant != SiteWorker::RelayOnly;
    const FIRST: &str = "3333333333333333333333333333333333333333";
    let mut fleet = Fleet::sites().await;
    // Each site names the shared store by its own connection id.
    assert_ne!(
        fleet.connection(Side::Controller),
        fleet.connection(Side::Peer)
    );
    // The home's pool runs under dispatcher authority, with a live dispatcher and a live
    // integrator (both people's: the loop starts no role session here).
    let pool = home_pool(&fleet);
    let now = now_s();
    pool.register(
        Role::Integrator,
        "integrator-1",
        "integrate-1",
        now,
        3600,
        ReturnRoute::HumanRelay,
    )
    .unwrap();
    pool.set_authority(
        "integrator-1",
        "integrate-1",
        now,
        Authority::Dispatcher,
        true,
    )
    .unwrap();
    pool.register(
        Role::Dispatcher,
        "dispatcher-1",
        "dispatch-1",
        now,
        3600,
        ReturnRoute::HumanRelay,
    )
    .unwrap();
    let orchestration = fleet.orchestration_ticket("Coordinate the synthetic project");
    let windows = fleet.target_ticket(Side::Controller, "Windows-only change to correct");
    converge(&fleet);
    // Only the peer meets the step; a correction nobody assigns is claimed at once.
    fleet
        .policy(json!({
            "targets":[
                {"target_id":TARGET_A,"actions":ACTIONS,"tools":[{"tool":TOOL_A}],"max_sessions":1},
                {"target_id":TARGET_B,"actions":ACTIONS,"tools":[{"tool":TOOL_A}],"max_sessions":1}],
            "placement":{"automatic":true,"actor":ACTOR,"cooldown_minutes":0,
                "dispatcher_wait_minutes":0,
                "role_sessions":[{"target_id":TARGET_A,"roles":[]}]}
        }))
        .await;
    let defined = fleet
        .ok(
            "POST",
            "/readiness/define",
            json!({"actor":Fleet::ai(),"ticket":{"native_id":orchestration.slug},
                "target_id":TARGET_A,"connection_id":fleet.connection(Side::Controller),
                "native_id":windows.slug,"prerequisites":[],
                "requirements":{"os":["windows"]}}),
        )
        .await;
    let step = defined["step"]["step_id"].as_str().unwrap().to_owned();
    let w = placed_step(&fleet, &step, TARGET_B, "os:windows").await;
    let w_run = w.run_id.clone().unwrap();

    // The worker takes up its role on its own machine, as the worker skill does (or,
    // HS2-26Z8KB, never registers there), and the relay joins it to the home's pool. It
    // claims the ticket the dispatcher gave it.
    received_prompt(&fleet.reports, &w.terminal, "`hotsheet-worker`");
    if registers_locally {
        fleet.join_pool(Side::Peer, &w.worker);
        let synced = fleet
            .ok(
                "POST",
                &format!("/sessions/{}/sync", w.session_id),
                json!({"actor":Fleet::ai()}),
            )
            .await;
        assert!(synced["joined_at_ms"].as_u64().is_some(), "{synced}");
    }
    pool_until(&fleet, "joined by the peer's worker", |state| {
        state
            .workers
            .get(&w.worker)
            .is_some_and(|member| member.generation == w.session_id)
    });
    if !registers_locally {
        // The peer's own clone has no such member, so its listing says not joined.
        let synced = fleet
            .ok(
                "POST",
                &format!("/sessions/{}/sync", w.session_id),
                json!({"actor":Fleet::ai()}),
            )
            .await;
        let peer_pool = hotsheet_cli::coordination::Coordinator::open_with_registry_home(
            fleet.project(Side::Peer),
            &fleet.b_store,
            fleet.peer_home.clone(),
        )
        .unwrap()
        .state()
        .ok();
        assert!(
            peer_pool.is_none_or(|state| !state.workers.contains_key(&w.worker)),
            "{synced}"
        );
        // An applied pass relays the session to the home, whose answer is that it is a
        // member: from then on it is joined (only the first time is kept).
        let tick = fleet
            .ok("POST", "/scheduler/tick", json!({"actor":Fleet::ai()}))
            .await;
        assert_eq!(tick["mode"], "applied", "{tick}");
        let joined = fleet.session(&w.session_id).await;
        assert!(joined["joined_at_ms"].as_u64().is_some(), "{joined}");
    }
    assert_eq!(claim(&fleet, Side::Peer, &windows, &w.worker).await, 200);
    fleet
        .run_until(&w_run, "dispatched", |status| {
            status["state"] == "dispatched"
        })
        .await;

    // The home records its handoff (as the relay does from a pushed branch); the
    // dispatcher leaves; the worker releases its claim and its machine publishes that.
    let handed: Assignment = serde_json::from_value(json!({
        "id":"site-original","worker":w.worker,"generation":w.session_id,
        "ticket":{"connection_id":fleet.connection(Side::Controller),
            "native_id":windows.id.to_string()},
        "slug":windows.slug,"base":"0".repeat(40),"phase":"handoff","commit":FIRST,
        "handoff":{"kind":"code","commit":FIRST},
        "return_route":{"kind":"human_relay"}
    }))
    .unwrap();
    edit_home_pool(&fleet, |state| {
        state.assignments.insert(handed.id.clone(), handed.clone());
        state.dispatcher.as_mut().unwrap().expires_at = now_s() - 1;
    });
    release(&fleet, Side::Peer, &windows, &w.worker).await;
    sync_published(&fleet.b_store);
    fleet
        .run_until(&w_run, "released", |status| status["state"] == "intervened")
        .await;
    assert_eq!(fleet.readiness(&step).await["level"], "in_progress");

    if variant == SiteWorker::LostInReview {
        // HS2-RHKSXQ: the worker's terminal dies on the peer while it is in review. The
        // loop learns the loss from the peer's feed; a pass places nothing for it.
        let (status, _) = api(
            fleet.side_router(Side::Peer),
            "DELETE",
            &format!("/terminals/{}", w.terminal),
            Value::Null,
            true,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let deadline = std::time::Instant::now() + 2 * MINUTE;
        let mut cursor = 0;
        loop {
            let synced = fleet
                .ok(
                    "POST",
                    &format!("/sessions/{}/sync", w.session_id),
                    json!({"actor":Fleet::ai()}),
                )
                .await;
            if synced["state"] == "lost" || synced["state"] == "stopped" {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the session was never lost: {synced}"
            );
            cursor = fleet.next_event(Some(&w_run), cursor).await;
        }
    }

    // Awaiting review is not a return: a pass claims nothing.
    let tick = fleet
        .ok("POST", "/scheduler/tick", json!({"actor":Fleet::ai()}))
        .await;
    assert_eq!(tick["mode"], "applied", "{tick}");
    assert!(
        !fleet
            .decisions()
            .await
            .iter()
            .any(|entry| { entry["kind"] == "assignment" && entry["detail"] == "review_returned" }),
        "a handoff awaiting review was claimed"
    );
    assert_eq!(
        fleet
            .ok("GET", &format!("/runs/{w_run}/status"), Value::Null)
            .await["state"],
        "intervened"
    );
    // Nor does it place the step again while it is in review, its session lost or not.
    let starts = |log: &[Value]| {
        log.iter()
            .filter(|entry| {
                entry["kind"] == "start"
                    && entry["outcome"] == "started"
                    && entry["step_id"] == step.as_str()
            })
            .count()
    };
    assert_eq!(starts(&fleet.decisions().await), 1);
    assert_eq!(fleet.sessions().await.len(), 1);

    // 2. The home's integrator returns it: a `## Review return` note on the ticket in
    // the home's clone, and the return in the home's pool. Both machines' sync loops
    // publish the note, and its arrival on the peer's change feed wakes the loop.
    converge(&fleet);
    let note = hotsheet_model::Ulid::new();
    hotsheet_ticketing::ops::add_note(
        &FsStore::open(&fleet.a_store).unwrap(),
        &windows.id,
        note,
        hotsheet_model::Timestamp::from_datetime(time::OffsetDateTime::now_utc()),
        hotsheet_model::NoteKind::Regular,
        format!("## Review return\nsite-original {FIRST}\nCorrect the synthetic behaviour."),
    )
    .unwrap();
    pool.review_return(
        "integrator-1",
        "integrate-1",
        time::OffsetDateTime::now_utc(),
        "site-original",
        &note.to_string(),
    )
    .unwrap();
    // The home's pool route reports the return by the home's connection id, and that
    // nobody will assign it.
    let (status, reply) = api(
        fleet.router.clone(),
        "GET",
        &format!("/checkouts/{}/target-pool", fleet.a_checkout.id),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(reply["assigner_live"], false, "{reply}");
    let returned = reply["returned"].as_array().unwrap();
    assert_eq!(returned.len(), 1, "{reply}");
    assert_eq!(
        returned[0]["connection_id"],
        fleet.connection(Side::Controller).as_str()
    );
    assert_eq!(returned[0]["native_id"], windows.id.to_string().as_str());
    assert_eq!(returned[0]["review_note"], note.to_string().as_str());
    converge(&fleet);

    // HS2-RHKSXQ: the lost session's step is placed again on the peer, on the same run,
    // and its new session takes up its role there.
    let w = if variant == SiteWorker::LostInReview {
        let again = fleet
            .decided(
                "the returned step placed again",
                |entry| {
                    entry["kind"] == "start"
                        && entry["outcome"] == "started"
                        && entry["step_id"] == step.as_str()
                        && entry["session_id"] != w.session_id.as_str()
                },
                2 * MINUTE,
            )
            .await;
        assert_eq!(again["run_id"], w_run.as_str(), "{again}");
        let next = placed_from(&fleet, &again, TARGET_B).await;
        assert_eq!(next.run_id.as_deref(), Some(w_run.as_str()));
        received_prompt(&fleet.reports, &next.terminal, "`hotsheet-worker`");
        fleet.join_pool(Side::Peer, &next.worker);
        next
    } else {
        w
    };

    let claimed = fleet
        .decided(
            "the site worker's correction claimed",
            |entry| {
                entry["kind"] == "assignment"
                    && entry["detail"] == "review_returned"
                    && entry["outcome"] == "claimed"
            },
            2 * MINUTE,
        )
        .await;
    assert_eq!(claimed["run_id"], w_run.as_str(), "{claimed}");
    assert_eq!(claimed["session_id"], w.session_id.as_str(), "{claimed}");
    let run = fleet
        .run_until(&w_run, "claimed again", |status| {
            status["state"] == "dispatched"
        })
        .await;
    assert_eq!(run["worker_id"], w.worker.as_str(), "{run}");
    assert_eq!(
        request_on_peer(&fleet, &w).await,
        ("returned".to_owned(), note.to_string())
    );
    // The claim is the peer worker's, taken on the peer and published to the home.
    sync_published(&fleet.b_store);
    sync_published(&fleet.a_store);
    for store in [&fleet.b_store, &fleet.a_store] {
        let read = ticket_in(store, &windows);
        assert_eq!(
            read.claimed_by.as_deref(),
            Some(w.worker.as_str()),
            "{read:?}"
        );
    }
    assert_eq!(fleet.readiness(&step).await["level"], "in_progress");
    // One run, one worker session (two when the first was lost), nothing started twice;
    // the return is claimed once.
    assert_eq!(fleet.runs().await.len(), 1);
    let sessions = if variant == SiteWorker::LostInReview {
        2
    } else {
        1
    };
    assert_eq!(fleet.sessions().await.len(), sessions);
    assert_eq!(starts(&fleet.decisions().await), sessions);
    let claims = fleet
        .decisions()
        .await
        .into_iter()
        .filter(|entry| {
            entry["kind"] == "assignment"
                && entry["detail"] == "review_returned"
                && entry["outcome"] == "claimed"
        })
        .count();
    assert_eq!(claims, 1);
    fleet.shut_down().await;
}

/// HS2-EX594V: the home-outage notifications the controller's inbox holds, oldest first,
/// as `(severity, title)`.
pub(super) async fn home_notifications(fleet: &Fleet) -> Vec<(String, String)> {
    let (status, listed) = api(
        fleet.router.clone(),
        "GET",
        "/notifications",
        Value::Null,
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    let mut found: Vec<&Value> = listed
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| {
            entry["dedupe_key"]
                .as_str()
                .is_some_and(|key| key.starts_with("home-"))
        })
        .collect();
    found.sort_by_key(|entry| entry["created_at_ms"].as_u64());
    found
        .into_iter()
        .map(|entry| {
            assert_eq!(entry["checkout"], fleet.owner.id.as_str(), "{entry}");
            (
                entry["severity"].as_str().unwrap().to_owned(),
                entry["message"]
                    .as_str()
                    .unwrap()
                    .lines()
                    .next()
                    .unwrap()
                    .to_owned(),
            )
        })
        .collect()
}

/// HS2-EQ7K35, the home-outage variant: the peer's target B is the project's home and
/// the controller, which runs the orchestrator, holds its other site A. With the home's
/// machine off the network no dispatcher or integrator starts anywhere (there is no
/// failover, HS2-EX594V), yet the work is placed on the site, whose run claims its ticket
/// for its own session after the bounded wait. The outage is reported on every surface
/// and announced once; when the home is back it clears and its return is announced once.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn hs2_eq7k35_a_home_outage_starts_no_role_session_and_is_announced_once_each_way() {
    // The delay before an announcement is the unit tests' (HS2-EX594V); here it is none.
    let mut fleet = Fleet::sites_peer_home(Duration::ZERO).await;
    let orchestration = fleet.orchestration_ticket("Coordinate the synthetic project");
    let change = fleet.target_ticket(Side::Controller, "Change the site can make");
    converge(&fleet);

    // The home's machine drops off the network before any work is planned.
    fleet.peer_offline().await;
    // Both roles are Auto on the home; a run nobody assigns claims at once.
    fleet
        .policy(json!({
            "targets":[
                {"target_id":TARGET_A,"actions":ACTIONS,"tools":[{"tool":TOOL_A}],"max_sessions":1},
                {"target_id":TARGET_B,"actions":ACTIONS,"tools":[{"tool":TOOL_A}],"max_sessions":1}],
            "placement":{"automatic":true,"actor":ACTOR,"cooldown_minutes":0,
                "dispatcher_wait_minutes":0}
        }))
        .await;
    let defined = fleet
        .ok(
            "POST",
            "/readiness/define",
            json!({"actor":Fleet::ai(),"ticket":{"native_id":orchestration.slug},
                "target_id":TARGET_A,"connection_id":fleet.connection(Side::Controller),
                "native_id":change.slug,"prerequisites":[]}),
        )
        .await;
    let step = defined["step"]["step_id"].as_str().unwrap().to_owned();

    // The work is still placed, on the site the orchestrator can reach.
    let start = fleet.started_for(&step, 2 * MINUTE).await;
    let worker = placed_from(&fleet, &start, TARGET_A).await;
    let run_id = worker.run_id.clone().unwrap();
    let run = fleet
        .ok("GET", &format!("/runs/{run_id}"), Value::Null)
        .await;
    assert_eq!(run["mode"], "dispatcher", "{run}");
    // Its worker takes up its role in its own site's pool (it can join no other).
    received_prompt(&fleet.reports, &worker.terminal, "`hotsheet-worker`");
    fleet.join_pool(Side::Controller, &worker.worker);
    let synced = fleet
        .ok(
            "POST",
            &format!("/sessions/{}/sync", worker.session_id),
            json!({"actor":Fleet::ai()}),
        )
        .await;
    assert!(synced["joined_at_ms"].as_u64().is_some(), "{synced}");

    // Nobody can assign it, so after the bounded wait its session claims the ticket.
    let tick = fleet
        .ok("POST", "/scheduler/tick", json!({"actor":Fleet::ai()}))
        .await;
    assert_eq!(tick["mode"], "applied", "{tick}");
    let claimed = fleet
        .decided(
            "the site's run claimed for its own session",
            |entry| {
                entry["kind"] == "assignment"
                    && entry["outcome"] == "claimed"
                    && entry["run_id"] == run_id.as_str()
            },
            2 * MINUTE,
        )
        .await;
    assert_eq!(
        claimed["session_id"],
        worker.session_id.as_str(),
        "{claimed}"
    );
    let run = fleet
        .run_until(&run_id, "claimed for its session", |status| {
            status["state"] == "dispatched"
        })
        .await;
    assert_eq!(run["worker_id"], worker.worker.as_str(), "{run}");
    let read = ticket_in(&fleet.a_store, &change);
    assert_eq!(
        read.claimed_by.as_deref(),
        Some(worker.worker.as_str()),
        "{read:?}"
    );

    // No dispatcher or integrator was started anywhere, and only the worker runs.
    let log = fleet.decisions().await;
    let roles: Vec<_> = log
        .iter()
        .filter(|entry| {
            entry["kind"] == "start"
                && entry["operation_id"]
                    .as_str()
                    .is_some_and(|id| id.starts_with("sched-role-"))
        })
        .collect();
    assert!(roles.is_empty(), "{roles:#?}");
    let sessions = fleet.sessions().await;
    assert_eq!(sessions.len(), 1, "{sessions:#?}");
    assert_eq!(sessions[0]["role"], "worker", "{sessions:#?}");

    // Every surface names the outage: the plan, the explanation and the timeline.
    let plan = fleet
        .ok("POST", "/scheduler/plan", json!({"actor":Fleet::ai()}))
        .await;
    let outage = &plan["home_outages"][0];
    assert_eq!(outage["home_target_id"], TARGET_B, "{plan:#}");
    assert_eq!(outage["sites"], json!([TARGET_A]), "{plan:#}");
    assert_eq!(outage["reason"], "unreachable", "{plan:#}");
    assert_eq!(outage["announced"], true, "{plan:#}");
    let since = outage["since_ms"].as_u64().unwrap();
    let explained = fleet
        .ok(
            "POST",
            "/scheduler/explain",
            json!({"step_id":step,"actor":Fleet::ai()}),
        )
        .await;
    assert_eq!(
        explained["home_outage"]["home_target_id"], TARGET_B,
        "{explained:#}"
    );
    let timeline = fleet.ok("GET", "/timeline", Value::Null).await;
    assert_eq!(
        timeline["home_outages"][0]["since_ms"].as_u64(),
        Some(since),
        "{timeline:#}"
    );
    // Announced once, however many passes saw it.
    fleet
        .ok("POST", "/scheduler/tick", json!({"actor":Fleet::ai()}))
        .await;
    let told = home_notifications(&fleet).await;
    assert_eq!(told.len(), 1, "{told:?}");
    assert_eq!(told[0].0, "warning");
    assert_eq!(
        told[0].1,
        format!("Home site Target {TARGET_B} is unavailable")
    );

    // The home is back: the next pass that reads it clears the outage and says so, once.
    fleet.peer_online().await;
    let mut cleared = false;
    for _ in 0..3 {
        let tick = fleet
            .ok("POST", "/scheduler/tick", json!({"actor":Fleet::ai()}))
            .await;
        if tick.get("home_outages").is_none() {
            cleared = true;
            break;
        }
    }
    assert!(cleared, "the home never read as back");
    let told = home_notifications(&fleet).await;
    assert_eq!(told.len(), 2, "{told:?}");
    assert_eq!(
        told[1],
        (
            "success".to_owned(),
            format!("Home site Target {TARGET_B} is back")
        )
    );
    let timeline = fleet.ok("GET", "/timeline", Value::Null).await;
    assert!(timeline.get("home_outages").is_none(), "{timeline:#}");
    let explained = fleet
        .ok(
            "POST",
            "/scheduler/explain",
            json!({"step_id":step,"actor":Fleet::ai()}),
        )
        .await;
    assert!(explained.get("home_outage").is_none(), "{explained:#}");
    fleet
        .ok("POST", "/scheduler/tick", json!({"actor":Fleet::ai()}))
        .await;
    assert_eq!(home_notifications(&fleet).await.len(), 2);
    fleet.shut_down().await;
}
