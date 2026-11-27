//! HS2-36XDDX: a template's Wait for Accept on real servers. Under Auto placement the
//! step the planner defines for a template's goal is held (the scheduler's own loop
//! places nothing for it, explain and the timeline say why) until a person accepts the
//! proposed plan through `templates/accept`; then the loop places it as usual. A
//! template with the setting off is placed at once. Synthetic tickets only; the
//! sessions are the Unix fake agent.
//!
//! HS2-38MFXT: a second server with its own machine home runs the same orchestration
//! project's scheduler from the same checked-in plan (as another machine does once the
//! plan is pulled). It has no apply record, yet it holds the step for the same reason,
//! and an accept on either machine releases the step on both.
use super::e2e::{ACTOR, Fleet, Side, TOOL_A};
use super::e2e_loop::placed;
use super::*;

/// Store one template and apply it as a person: the goal ticket's slug and the apply.
async fn applied(
    fleet: &Fleet,
    name: &str,
    wait_for_accept: bool,
    operation: &str,
) -> (String, Value) {
    let mut template = json!({
        "name": name,
        "goal": {"title": format!("{name} goal")},
        "steps": [{"key": "work", "target": super::e2e::TARGET_A, "title": format!("{name} work")}]
    });
    if !wait_for_accept {
        template["wait_for_accept"] = json!(false);
    }
    let set = fleet
        .ok(
            "POST",
            "/templates/set",
            json!({"templates":[template],"actor":Fleet::person()}),
        )
        .await;
    let template_id = set["templates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|known| known["name"] == name)
        .unwrap()["template_id"]
        .clone();
    let apply = fleet
        .ok(
            "POST",
            "/templates/apply",
            json!({"template_id":template_id,"tool":TOOL_A,"operation_id":operation,
                "actor":Fleet::person()}),
        )
        .await;
    assert_eq!(apply["goal"]["created"], true, "{apply}");
    (apply["goal"]["slug"].as_str().unwrap().to_owned(), apply)
}

/// The planner's step for the goal on a new ticket of target A.
async fn planned_step(fleet: &Fleet, goal: &str, title: &str) -> String {
    let work = fleet.target_ticket(Side::Controller, title);
    let defined = fleet
        .ok(
            "POST",
            "/readiness/define",
            json!({"actor":Fleet::ai(),"ticket":{"native_id":goal},
                "target_id":super::e2e::TARGET_A,"connection_id":fleet.connection(Side::Controller),
                "native_id":work.slug}),
        )
        .await;
    assert_eq!(defined["level"], "confirmed", "{defined}");
    defined["step"]["step_id"].as_str().unwrap().to_owned()
}

async fn item_state(fleet: &Fleet, step: &str) -> Value {
    let timeline = fleet.ok("GET", "/timeline", Value::Null).await;
    timeline["waves"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|wave| wave["items"].as_array().unwrap().clone())
        .find(|item| item["step_id"] == step)
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn hs2_36xddx_a_held_template_step_is_placed_once_a_person_accepts() {
    let mut fleet = Fleet::new().await;
    // Auto placement: unset, so it is on once the plan has a step.
    fleet
        .policy(json!({"placement":{"actor":ACTOR,"cooldown_minutes":0}}))
        .await;
    let (goal, _) = applied(&fleet, "Repair the release", true, "tpl-wait").await;
    let step = planned_step(&fleet, &goal, "Synthetic held change").await;

    // A pass of the loop places nothing for it, and says why.
    let tick = fleet
        .ok("POST", "/scheduler/tick", json!({"actor":Fleet::ai()}))
        .await;
    assert_eq!(tick["mode"], "applied", "{tick}");
    assert!(
        !fleet
            .decisions()
            .await
            .iter()
            .any(|entry| entry["kind"] == "start" && entry["step_id"] == step.as_str()),
        "a held step was placed"
    );
    let explained = fleet
        .ok(
            "POST",
            "/scheduler/explain",
            json!({"actor":Fleet::ai(),"step_id":step}),
        )
        .await;
    assert_eq!(explained["standing"], "not_placed", "{explained}");
    assert_eq!(
        explained["reason"], "template_plan_not_accepted",
        "{explained}"
    );
    let item = item_state(&fleet, &step).await;
    assert_eq!(item["state"], "waits_for_accept", "{item}");
    assert_eq!(item["accept"]["operation_id"], "tpl-wait", "{item}");
    assert!(
        fleet.sessions().await.is_empty(),
        "no session was started for it"
    );

    // An AI session cannot accept; a person does, through the plan review.
    let (status, refused) = fleet
        .call(
            "POST",
            "/templates/accept",
            json!({"operation_id":"tpl-wait","actor":Fleet::ai()}),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{refused}");
    assert_eq!(refused["code"], "forbidden", "{refused}");
    let accepted = fleet
        .ok(
            "POST",
            "/templates/accept",
            json!({"operation_id":"tpl-wait","actor":Fleet::person()}),
        )
        .await;
    assert_eq!(accepted["outcome"], "accepted", "{accepted}");
    assert_eq!(accepted["released"], json!([step]), "{accepted}");
    // Nobody ticks: the accept wakes the server's own loop, which places the step.
    let placed = placed(&fleet, &step, Side::Controller).await;
    assert!(!placed.run_id.is_empty());
    let again = fleet
        .ok(
            "POST",
            "/templates/accept",
            json!({"operation_id":"tpl-wait","actor":Fleet::person()}),
        )
        .await;
    assert_eq!(again["outcome"], "existing", "{again}");
    let audit = fleet.ok("GET", "/audit", Value::Null).await;
    let reasons: Vec<&str> = audit["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["action"] == "template_accept")
        .map(|entry| entry["reason"].as_str().unwrap())
        .collect();
    assert_eq!(
        reasons,
        ["template_accepted", "template_accept_existing"],
        "{audit}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn hs2_36xddx_a_template_that_does_not_wait_is_placed_at_once() {
    let mut fleet = Fleet::new().await;
    fleet
        .policy(json!({"placement":{"actor":ACTOR,"cooldown_minutes":0}}))
        .await;
    let (goal, apply) = applied(&fleet, "Bump a dependency", false, "tpl-now").await;
    assert_eq!(apply["snapshot"]["wait_for_accept"], false, "{apply}");
    let step = planned_step(&fleet, &goal, "Synthetic immediate change").await;
    // Nobody accepts and nobody ticks: the loop places it as before the setting.
    let placed = placed(&fleet, &step, Side::Controller).await;
    assert!(!placed.run_id.is_empty());
    let item = item_state(&fleet, &step).await;
    assert_ne!(item["state"], "waits_for_accept", "{item}");
}

/// HS2-38MFXT: another machine of the same orchestration project: its own home and
/// server, the same checkout (the checked-in plan, policy and manifest) and the same
/// stores, with its own binding of target A, so it reads the target ticket itself. It
/// remembers no rule, so its own loop asks a person before it could start anything.
struct OtherMachine {
    home: std::path::PathBuf,
    state: hotsheet_server::AppState,
    router: axum::Router,
    base: String,
}

impl OtherMachine {
    async fn new(fleet: &Fleet) -> Self {
        let home = fleet.root.path().join("machine-b-home");
        std::fs::create_dir_all(&home).unwrap();
        let registry =
            hotsheet_ticketing::checkouts::CheckoutRegistry::new(home.join("checkouts.json"));
        let owner = registry
            .register(
                std::path::Path::new(&fleet.owner.root),
                None,
                None,
                vec![fleet.owner_store.clone()],
            )
            .unwrap();
        assert_eq!(
            owner.id, fleet.owner.id,
            "a checkout id is its path's identity"
        );
        let a_checkout = registry
            .register(&fleet.a_project, None, None, vec![fleet.a_store.clone()])
            .unwrap();
        let memory = Arc::new(Memory::default());
        let state = state(&fleet.a_store, &home).with_orchestration_runtime(
            super::orchestration::shared_runtime(&fleet.a_store, &home, &memory),
        );
        let machine = Self {
            router: app(state.clone()),
            state,
            home,
            base: format!("/checkouts/{}/orchestration", owner.id),
        };
        // This machine's own binding of target A (bindings are machine-local).
        let manifest = machine.ok("GET", "/targets", Value::Null).await;
        let candidate = machine
            .ok(
                "POST",
                "/inspect",
                json!({"destination":{"kind":"local","checkout_id":a_checkout.id}}),
            )
            .await;
        let bound = machine
            .ok(
                "PUT",
                &format!("/targets/{}/binding", super::e2e::TARGET_A),
                json!({"target":{"owner_checkout_id":owner.id,"target_id":super::e2e::TARGET_A},
                    "expected_manifest_revision":manifest["revision"],"expected_binding_revision":0,
                    "previous_destination":null,"destination":candidate["destination"]}),
            )
            .await;
        assert_eq!(bound["usable"], true, "{bound}");
        machine
    }

    /// Its scheduling loop ends with the test, not at its next bounded wait: the loop
    /// waits on files, so after the stop its marker is moved to end that wait.
    async fn stop(self) {
        self.state.begin_stopping();
        hotsheet_cli::orchestration::OrchestrationRuntime::scheduler_wake_at(&self.home);
        self.state
            .shutdown_orchestration(std::time::Duration::from_secs(10))
            .await;
    }

    async fn call(&self, method: &str, path: &str, body: Value) -> (StatusCode, Value) {
        super::orchestration::api(
            self.router.clone(),
            method,
            &format!("{}{path}", self.base),
            body,
            true,
        )
        .await
    }

    async fn ok(&self, method: &str, path: &str, body: Value) -> Value {
        let (status, value) = self.call(method, path, body).await;
        assert_eq!(status, StatusCode::OK, "{method} {path}: {value}");
        value
    }

    /// Why this machine's scheduler leaves the step unplaced, after reading the
    /// target ticket itself (readiness comes only from what a machine confirmed).
    async fn explain(&self, step: &str) -> Value {
        let refreshed = self
            .ok("POST", "/readiness/refresh", json!({"actor":Fleet::ai()}))
            .await;
        assert_eq!(refreshed["counts"]["confirmed"], 1, "{refreshed}");
        self.ok(
            "POST",
            "/scheduler/explain",
            json!({"actor":Fleet::ai(),"step_id":step}),
        )
        .await
    }

    async fn item_state(&self, step: &str) -> Value {
        let timeline = self.ok("GET", "/timeline", Value::Null).await;
        timeline["waves"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|wave| wave["items"].as_array().unwrap().clone())
            .find(|item| item["step_id"] == step)
            .unwrap()
    }

    /// Nothing this machine's scheduler decided started the step.
    async fn started_nothing_for(&self, step: &str) {
        let decisions = self
            .ok(
                "POST",
                "/scheduler/decisions",
                json!({"after": 0, "limit": 200}),
            )
            .await;
        assert!(
            !decisions["entries"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["kind"] == "start" && entry["step_id"] == step),
            "{decisions}"
        );
    }
}

/// Both machines hold the step for the template's accept and say so.
async fn held_on_both(fleet: &Fleet, other: &OtherMachine, step: &str, operation: &str) {
    for explained in [
        fleet
            .ok(
                "POST",
                "/scheduler/explain",
                json!({"actor":Fleet::ai(),"step_id":step}),
            )
            .await,
        other.explain(step).await,
    ] {
        assert_eq!(explained["standing"], "not_placed", "{explained}");
        assert_eq!(
            explained["reason"], "template_plan_not_accepted",
            "{explained}"
        );
    }
    let item = other.item_state(step).await;
    assert_eq!(item["state"], "waits_for_accept", "{item}");
    assert_eq!(item["accept"]["operation_id"], operation, "{item}");
    other.started_nothing_for(step).await;
    assert!(fleet.sessions().await.is_empty(), "nothing was started");
}

/// Neither machine holds the step any longer.
async fn released_on_both(fleet: &Fleet, other: &OtherMachine, step: &str) {
    let explained = other.explain(step).await;
    assert_ne!(
        explained["reason"], "template_plan_not_accepted",
        "{explained}"
    );
    assert_ne!(other.item_state(step).await["state"], "waits_for_accept");
    // The machine that applied places it by itself: its loop sees the checked-in
    // accept (another machine's write) without anyone ticking.
    let placed = placed(fleet, step, Side::Controller).await;
    assert!(!placed.run_id.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn hs2_38mfxt_another_machine_holds_the_step_and_its_accept_releases_both() {
    let mut fleet = Fleet::new().await;
    fleet
        .policy(json!({"placement":{"actor":ACTOR,"cooldown_minutes":0}}))
        .await;
    let other = OtherMachine::new(&fleet).await;
    let (goal, _) = applied(&fleet, "Repair the release", true, "tpl-shared").await;
    let step = planned_step(&fleet, &goal, "Synthetic shared hold").await;
    held_on_both(&fleet, &other, &step, "tpl-shared").await;

    // A person accepts on the other machine, which has no apply record of its own.
    let (status, refused) = other
        .call(
            "POST",
            "/templates/accept",
            json!({"operation_id":"tpl-shared","actor":Fleet::ai()}),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{refused}");
    let accepted = other
        .ok(
            "POST",
            "/templates/accept",
            json!({"operation_id":"tpl-shared","actor":Fleet::person()}),
        )
        .await;
    assert_eq!(accepted["outcome"], "accepted", "{accepted}");
    assert_eq!(accepted["released"], json!([step]), "{accepted}");
    assert_eq!(accepted["hold"]["operation_id"], "tpl-shared", "{accepted}");
    assert!(accepted.get("snapshot").is_none(), "{accepted}");
    released_on_both(&fleet, &other, &step).await;
    // The machine that applied answers the accept as already made.
    let again = fleet
        .ok(
            "POST",
            "/templates/accept",
            json!({"operation_id":"tpl-shared","actor":Fleet::person()}),
        )
        .await;
    assert_eq!(again["outcome"], "existing", "{again}");
    other.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn hs2_38mfxt_an_accept_on_the_applying_machine_releases_the_other_one() {
    let mut fleet = Fleet::new().await;
    fleet
        .policy(json!({"placement":{"actor":ACTOR,"cooldown_minutes":0}}))
        .await;
    let other = OtherMachine::new(&fleet).await;
    let (goal, _) = applied(&fleet, "Bump a dependency", true, "tpl-home").await;
    let step = planned_step(&fleet, &goal, "Synthetic home accept").await;
    held_on_both(&fleet, &other, &step, "tpl-home").await;
    let accepted = fleet
        .ok(
            "POST",
            "/templates/accept",
            json!({"operation_id":"tpl-home","actor":Fleet::person()}),
        )
        .await;
    assert_eq!(accepted["outcome"], "accepted", "{accepted}");
    assert_eq!(
        accepted["snapshot"]["name"], "Bump a dependency",
        "{accepted}"
    );
    released_on_both(&fleet, &other, &step).await;
    let again = other
        .ok(
            "POST",
            "/templates/accept",
            json!({"operation_id":"tpl-home","actor":Fleet::person()}),
        )
        .await;
    assert_eq!(again["outcome"], "existing", "{again}");
    other.stop().await;
}
