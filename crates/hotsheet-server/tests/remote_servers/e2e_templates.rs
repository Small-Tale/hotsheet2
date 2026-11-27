//! HS2-36XDDX: a template's Wait for Accept on real servers. Under Auto placement the
//! step the planner defines for a template's goal is held (the scheduler's own loop
//! places nothing for it, explain and the timeline say why) until a person accepts the
//! proposed plan through `templates/accept`; then the loop places it as usual. A
//! template with the setting off is placed at once. Synthetic tickets only; the
//! sessions are the Unix fake agent.
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
