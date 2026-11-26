//! HS2-TZCCBJ: the automatic default-goal re-plan (HS2-BQN0M5) on a real server. An
//! orchestration project plans the standing default goal over one separate target
//! project; a ticket written headlessly into the target's own store reaches the
//! server's follower through the target feed, and starts one background Update Plan in
//! the idle push-capable planner. The planner is a real PTY running the workspace fake
//! agent; no AI tool is launched and nothing leaves loopback.
use super::orchestration::api;
use super::session_work::{
    PUSH_AGENT, PUSH_PLUGIN, REPORT_TOOL, Roomy, TARGET, received_prompt, report_plugin,
};
use super::sessions::{drawer, git_project};
use super::*;
use hotsheet_server::AppState;
use hotsheet_ticketing::checkouts::{Checkout, CheckoutRegistry};
use hotsheet_ticketing::orchestration::ReplanMemoryStore;
use hotsheet_ticketing::{NewTicket, StoreMetadata, TicketPatch, ops};
use std::path::PathBuf;

/// An orchestration project and one separate target project, each with its own linked
/// store, neither of them the store this server serves. Every open ticket of the
/// orchestration project is a goal, so tickets go into the target only.
struct TwoProjects {
    _root: tempfile::TempDir,
    home: PathBuf,
    reports: PathBuf,
    owner: Checkout,
    target_store: PathBuf,
    target: Checkout,
    /// HS2-04WW8B: the serving state, its store and plugins, to restart it.
    host: AppState,
    server_store: PathBuf,
    plugins: PathBuf,
    router: axum::Router,
    base: String,
}

impl TwoProjects {
    async fn new() -> Self {
        assert!(hotsheet_aitools::test_permission_vault_available());
        hotsheet_aitools::enable_test_permission_vault();
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let reports = root.path().join("reports");
        let server_store = root.path().join("server-store");
        let plugins = report_plugin(root.path(), &reports, PUSH_AGENT, PUSH_PLUGIN);
        let host = Self::serving(&server_store, &home, &plugins);
        let registry = CheckoutRegistry::new(home.join("checkouts.json"));
        let project = |name: &str, prefix: &str| {
            let path = root.path().join(name);
            git_project(&path);
            let store = root.path().join(format!("{name}-store"));
            FsStore::init_git(&store, &StoreMetadata::new(prefix)).unwrap();
            let checkout = registry
                .register(&path, None, None, vec![store.clone()])
                .unwrap();
            (checkout, store)
        };
        let (owner, _) = project("orchestration", "ORC");
        let (target, target_store) = project("target", "TGT");
        let base = format!("/checkouts/{}/orchestration", owner.id);
        let world = Self {
            _root: root,
            home,
            reports,
            owner,
            target_store,
            target,
            router: app(host.clone()),
            host,
            server_store,
            plugins,
            base,
        };
        world.bind().await;
        world
    }

    fn serving(
        store: &std::path::Path,
        home: &std::path::Path,
        plugins: &std::path::Path,
    ) -> AppState {
        state(store, home)
            .with_capacity_source(Box::new(Roomy))
            .with_capacity_cache(std::time::Duration::ZERO)
            .with_plugin_dirs(vec![plugins.to_path_buf()])
    }

    /// HS2-04WW8B: the server stops; the terminals it hosts die with its process.
    async fn stop(&self) {
        for tab in drawer(&self.router).await {
            let id = tab["id"].as_str().unwrap();
            let (status, _) = api(
                self.router.clone(),
                "DELETE",
                &format!("/terminals/{id}"),
                Value::Null,
                true,
            )
            .await;
            assert_eq!(status, StatusCode::NO_CONTENT);
        }
        self.host.begin_stopping();
    }

    /// HS2-04WW8B: a new server starts over the same home; its start-up alone follows
    /// the targets again.
    fn start(&mut self) {
        let next = Self::serving(&self.server_store, &self.home, &self.plugins);
        next.spawn_run_recovery();
        self.router = app(next.clone());
        self.host = next;
    }

    async fn call(&self, method: &str, path: &str, body: Value) -> (StatusCode, Value) {
        api(
            self.router.clone(),
            method,
            &format!("{}{path}", self.base),
            body,
            true,
        )
        .await
    }

    /// Add the target project and bind it to the local target checkout, then turn
    /// orchestration on: automatic placement, reads only on the target.
    async fn bind(&self) {
        let added = self
            .call(
                "POST",
                "/targets",
                json!({"target_id":TARGET,"label":"Target","expected_revision":null}),
            )
            .await;
        assert_eq!(added.0, StatusCode::OK, "{}", added.1);
        let candidate = self
            .call(
                "POST",
                "/inspect",
                json!({"destination":{"kind":"local","checkout_id":self.target.id}}),
            )
            .await
            .1;
        let bound = self
            .call(
                "PUT",
                &format!("/targets/{TARGET}/binding"),
                json!({"target":{"owner_checkout_id":self.owner.id,"target_id":TARGET},
                    "expected_manifest_revision":added.1["revision"],
                    "expected_binding_revision":0,"previous_destination":null,
                    "destination":candidate["destination"]}),
            )
            .await;
        assert_eq!(bound.0, StatusCode::OK, "{}", bound.1);
        let current = self.call("GET", "/policy", Value::Null).await.1;
        let stored = self
            .call(
                "PUT",
                "/policy",
                json!({"expected_revision":current["revision"],
                    "policy":{"machine":{"max_sessions":4},
                        "tools":[{"tool":REPORT_TOOL,"max_sessions":4}],
                        "targets":[{"target_id":TARGET,"actions":["tickets_read"],
                            "tools":[{"tool":REPORT_TOOL}],"max_sessions":2}],
                        "placement":{"automatic":true}},
                    "actor":{"role":"human","id":"maintainer"}}),
            )
            .await;
        assert_eq!(stored.0, StatusCode::OK, "{}", stored.1);
    }

    /// A ticket written into the target's store as a headless CLI would, outside this
    /// server.
    fn target_ticket(&self, title: &str) -> hotsheet_model::Ticket {
        ops::create(
            &FsStore::open(&self.target_store).unwrap(),
            hotsheet_model::Ulid::new(),
            "TGT",
            "2026-11-25T00:00:00Z".into(),
            NewTicket {
                title: title.into(),
                ..Default::default()
            },
        )
        .unwrap()
    }

    fn retitle(&self, ticket: &hotsheet_model::Ticket, title: &str) {
        ops::update(
            &FsStore::open(&self.target_store).unwrap(),
            &ticket.id,
            "2026-11-25T00:00:01Z".into(),
            TicketPatch {
                title: Some(title.into()),
                ..Default::default()
            },
        )
        .unwrap();
    }

    async fn audit(&self) -> Vec<Value> {
        let (status, audit) = self.call("GET", "/audit?limit=10000", Value::Null).await;
        assert_eq!(status, StatusCode::OK, "{audit}");
        audit["entries"].as_array().unwrap().clone()
    }

    /// Waits, woken by the machine home (where the audit journal lives) changing, until
    /// the audit satisfies `done`; answers that audit.
    async fn audit_until(&self, what: &str, done: impl Fn(&[Value]) -> bool) -> Vec<Value> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        loop {
            let waiter = hotsheet_jobs::ChangeWaiter::starting(&[self.home.as_path()]).unwrap();
            let audit = self.audit().await;
            if done(&audit) {
                return audit;
            }
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            assert!(
                !remaining.is_zero(),
                "the audit never showed {what}: {audit:?}"
            );
            tokio::task::block_in_place(|| waiter.wait(remaining));
        }
    }

    /// HS2-H41MBC: waits, woken by the machine home changing, until the follower's first
    /// step listed the target's open tickets as the re-plan's baseline (HS2-04WW8B).
    /// Every ticket open at that listing counts as already seen, so a ticket written
    /// before it ends would be seeded rather than re-planned. The follower authorizes
    /// its reads (the first audited read) before that step's page and listing, so the
    /// audit alone does not say the seed was taken.
    async fn seeded(&self) {
        let store = ReplanMemoryStore::new(&self.home, &self.owner.id).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        loop {
            let waiter = hotsheet_jobs::ChangeWaiter::starting(&[self.home.as_path()]).unwrap();
            if store.read().is_ok_and(|memory| memory.seeded(TARGET)) {
                return;
            }
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            assert!(
                !remaining.is_zero(),
                "the follower never seeded the target's baseline"
            );
            tokio::task::block_in_place(|| waiter.wait(remaining));
        }
    }

    /// `plan show` once the server sees the planner idle in its open tab.
    async fn replannable(&self) -> Value {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        loop {
            let waiter = hotsheet_jobs::ChangeWaiter::starting(&[self.home.as_path()]).unwrap();
            let (_, shown) = self.call("POST", "/planning/show", json!({})).await;
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
}

/// A follower's read of the target: one per step that saw a ticket no step names (or a
/// refresh of planned tickets), recorded before any update that step starts.
fn is_target_read(entry: &Value) -> bool {
    entry["action"] == "tickets_read" && entry["target_id"] == TARGET
}

fn is_auto(entry: &Value) -> bool {
    entry["action"] == "plan_update_auto"
}

fn count(audit: &[Value], wanted: fn(&Value) -> bool) -> usize {
    audit.iter().filter(|entry| wanted(entry)).count()
}

/// HS2-TZCCBJ (HS2-BQN0M5): on a real server, a new ticket in a separate target project
/// starts one automatic Update Plan in the idle planner, audited `plan_update_auto`. A
/// change to that ticket while the update drafts, and again after the planner finished
/// the update without it, starts nothing more; the next new ticket starts the next one.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_tzccbj_a_new_target_ticket_replans_the_default_goal_on_a_real_server() {
    let world = TwoProjects::new().await;
    let planned = world.target_ticket("Synthetic planned work");
    let person = json!({"role":"human","id":"maintainer"});

    // Propose Plan with no goal ticket open: the standing default goal.
    let (status, proposed) = world
        .call(
            "POST",
            "/planning/propose",
            json!({"tool":REPORT_TOOL,"operation_id":"press-1","actor":person}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{proposed}");
    assert_eq!(proposed["outcome"], "started", "{proposed}");
    assert_eq!(proposed["goals"][0]["default_goal"], true, "{proposed}");
    let goal = proposed["goals"][0]["native_id"]
        .as_str()
        .unwrap()
        .to_owned();
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

    // The planner plans one target ticket under the default goal. From then on the
    // server follows the target; its first step reads the planned ticket.
    let (status, defined) = world
        .call(
            "POST",
            "/readiness/define",
            json!({"actor":planner,
                "ticket":{"connection_id":world.owner.sources[0].connection_id,"native_id":goal},
                "target_id":TARGET,"connection_id":world.target.sources[0].connection_id,
                "native_id":planned.slug,"prerequisites":[]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{defined}");
    world
        .audit_until("the follower's first read", |audit| {
            count(audit, is_target_read) > 0
        })
        .await;
    world.replannable().await;
    world.seeded().await;
    assert!(count(&world.audit().await, is_auto) == 0);

    // A new ticket in the target, written outside the server: one automatic update,
    // pushed into the idle planner's open tab, acting as the orchestrator.
    let first = world.target_ticket("Synthetic new work");
    let audit = world
        .audit_until("an automatic update", |audit| count(audit, is_auto) == 1)
        .await;
    let auto = audit.iter().find(|entry| is_auto(entry)).unwrap();
    assert_eq!(auto["outcome"], "succeeded", "{auto}");
    assert_eq!(auto["reason"], "plan_update_started", "{auto}");
    assert!(
        auto["actor_id"]
            .as_str()
            .is_some_and(|actor| actor.starts_with("orchestrator")),
        "{auto}"
    );
    let (_, shown) = world.call("POST", "/planning/show", json!({})).await;
    assert_eq!(shown["update"]["state"], "drafting", "{shown}");
    let update_id = shown["update"]["update_id"].as_str().unwrap().to_owned();
    let prompt = received_prompt(
        &world.reports,
        &terminal,
        &format!("plan update `{update_id}`"),
    );
    assert!(
        prompt.contains(&format!("--plan-update {update_id}")),
        "{prompt}"
    );
    assert_eq!(drawer(&world.router).await.len(), 1, "no second planner");

    // The same ticket changes while the update drafts: it was handed already.
    let reads = count(&audit, is_target_read);
    world.retitle(&first, "Synthetic new work, renamed");
    world
        .audit_until("the follower's read of the change", |audit| {
            count(audit, is_target_read) > reads
        })
        .await;

    // The planner finishes the update without a step for it: skipped.
    let (status, applied) = world
        .call(
            "POST",
            "/planning/update_finish",
            json!({"actor":planner,"update_id":update_id}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{applied}");
    assert_eq!(applied["state"], "applied", "{applied}");
    world.replannable().await;

    // The skipped ticket changes again: still nothing.
    let reads = count(&world.audit().await, is_target_read);
    world.retitle(&first, "Synthetic new work, renamed again");
    let audit = world
        .audit_until("the follower's read of the skipped ticket", |audit| {
            count(audit, is_target_read) > reads
        })
        .await;
    let reads = count(&audit, is_target_read);

    // The next new ticket starts the next update. Steps run one after another and a
    // step records its read before the update it starts, so the second update follows
    // the read of this ticket's step: the skipped ticket's step started none.
    world.target_ticket("Synthetic later work");
    let audit = world
        .audit_until("the second automatic update", |audit| {
            count(audit, is_auto) == 2
        })
        .await;
    let second = audit
        .iter()
        .enumerate()
        .filter(|(_, entry)| is_auto(entry))
        .nth(1)
        .map(|(at, _)| at)
        .unwrap();
    assert!(
        count(&audit[..second], is_target_read) > reads,
        "an update started before the new ticket was read: {audit:?}"
    );
    assert_eq!(audit[second]["reason"], "plan_update_started", "{audit:?}");
    let (_, shown) = world.call("POST", "/planning/show", json!({})).await;
    assert_eq!(shown["update"]["state"], "drafting", "{shown}");
    assert_ne!(shown["update"]["update_id"], update_id.as_str(), "{shown}");
    assert_eq!(drawer(&world.router).await.len(), 1, "no second planner");
}

/// The follower's authorization of its target reads, as the orchestrator: the first
/// thing a follower step records.
fn is_follower_authorized(entry: &Value) -> bool {
    is_target_read(entry)
        && entry["outcome"] == "allowed"
        && entry["actor_id"]
            .as_str()
            .is_some_and(|actor| actor.starts_with("orchestrator"))
}

/// HS2-TZCCBJ (HS2-BQN0M5): with no run, no session on the target and no plan step
/// naming a target ticket, the server still follows the target while the default goal
/// is re-planned automatically: `ensure_watchers` follows `replan_targets`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_tzccbj_the_server_follows_a_target_only_for_the_default_goal() {
    let world = TwoProjects::new().await;
    // Orchestration is on but nothing is planned: no goal, nothing followed.
    assert_eq!(count(&world.audit().await, is_follower_authorized), 0);
    let (status, proposed) = world
        .call(
            "POST",
            "/planning/propose",
            json!({"tool":REPORT_TOOL,"operation_id":"press-1",
                "actor":{"role":"human","id":"maintainer"}}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{proposed}");
    assert_eq!(proposed["goals"][0]["default_goal"], true, "{proposed}");
    world
        .audit_until("the target followed for the default goal", |audit| {
            count(audit, is_follower_authorized) > 0
        })
        .await;
}

/// HS2-04WW8B: the follower's catch-up listing of the target's open tickets (and any
/// other read it records), succeeded.
fn is_target_read_done(entry: &Value) -> bool {
    is_target_read(entry) && entry["outcome"] == "succeeded"
}

/// HS2-04WW8B: on a real server, a ticket written headlessly into the target while the
/// server is down is caught up when it starts again: its start-up follows the target,
/// the follower's handshake lists the target's open tickets, and exactly one automatic
/// Update Plan starts. The ticket that existed before the re-plan applied was only
/// seeded, so editing it starts nothing (HS2-2AZQD2), and another restart starts
/// nothing more.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_04ww8b_a_ticket_created_while_the_server_was_down_replans_once() {
    let mut world = TwoProjects::new().await;
    let existing = world.target_ticket("Synthetic existing work");
    let (status, proposed) = world
        .call(
            "POST",
            "/planning/propose",
            json!({"tool":REPORT_TOOL,"operation_id":"press-1",
                "actor":{"role":"human","id":"maintainer"}}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{proposed}");
    assert_eq!(proposed["goals"][0]["default_goal"], true, "{proposed}");
    let terminal = proposed["session"]["session"]["session_id"]
        .as_str()
        .unwrap()
        .to_owned();
    received_prompt(&world.reports, &terminal, "press-1");
    // The follower's first step seeds the baseline: a listing, and no update.
    world
        .audit_until("the seeding listing", |audit| {
            count(audit, is_target_read_done) > 0
        })
        .await;
    world.replannable().await;
    assert_eq!(count(&world.audit().await, is_auto), 0);

    // HS2-2AZQD2: the seeded ticket, which no step names, is edited. The follower
    // reads the change and starts nothing: the update below follows a later read.
    let reads = count(&world.audit().await, is_target_read_done);
    world.retitle(&existing, "Synthetic existing work, renamed");
    world
        .audit_until("the follower's read of the edit", |audit| {
            count(audit, is_target_read_done) > reads
        })
        .await;

    // The server stops. A ticket is written into the target's store meanwhile, as a
    // headless CLI would; no server sees it happen.
    world.stop().await;
    let reads = count(&world.audit().await, is_target_read_done);
    world.target_ticket("Synthetic work created while the server was down");

    // The server starts again: exactly one automatic update, after the new follower's
    // catch-up listing.
    world.start();
    let audit = world
        .audit_until("the catch-up's automatic update", |audit| {
            count(audit, is_auto) == 1
        })
        .await;
    let auto = audit.iter().position(is_auto).unwrap();
    assert!(
        count(&audit[..auto], is_target_read_done) > reads,
        "the update started before the restarted follower listed the target: {audit:?}"
    );
    assert_eq!(audit[auto]["outcome"], "succeeded", "{audit:?}");
    assert_eq!(audit[auto]["reason"], "plan_update_started", "{audit:?}");
    assert!(
        audit[auto]["actor_id"]
            .as_str()
            .is_some_and(|actor| actor.starts_with("orchestrator")),
        "{audit:?}"
    );

    // Another restart with nothing new: the next follower lists the target again and
    // starts nothing more.
    world.stop().await;
    let reads = count(&world.audit().await, is_target_read_done);
    world.start();
    let audit = world
        .audit_until("the second restart's listing", |audit| {
            count(audit, is_target_read_done) > reads
        })
        .await;
    assert_eq!(count(&audit, is_auto), 1, "{audit:?}");
    world.host.begin_stopping();
}
