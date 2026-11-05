//! Real separate-process worker preferences, requests and refusals (HS2-6WFCZR).
//! Every pool here is a disposable, independently initialized project and ticket store.

#[allow(dead_code)] // HS2-6WFCZR: shared process fixture includes unrelated helpers.
#[path = "support/coordination.rs"]
mod support;

use hotsheet_model::{Timestamp, Ulid};
use hotsheet_ticketing::{NewTicket, git_connection_id, ops};
use support::{Fixture, git};

fn output(f: &Fixture, args: &[&str]) -> std::process::Output {
    let args = f.coord(args);
    f.command(&args.iter().map(String::as_str).collect::<Vec<_>>())
        .output()
        .unwrap()
}

fn json(f: &Fixture, args: &[&str]) -> serde_json::Value {
    let out = output(f, args);
    assert!(
        out.status.success(),
        "CLI failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

fn refused(f: &Fixture, args: &[&str], reason: &str) {
    let out = output(f, args);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "expected refusal for {args:?}");
    assert!(stderr.contains(reason), "{stderr}");
}

fn tagged(f: &Fixture, title: &str, tags: &[&str]) -> hotsheet_model::Ticket {
    ops::create(
        &f.store,
        Ulid::new(),
        "HS",
        Timestamp::from_datetime(time::OffsetDateTime::now_utc()),
        NewTicket {
            title: title.into(),
            up_next: true,
            tags: tags.iter().map(|t| (*t).to_owned()).collect(),
            ..NewTicket::default()
        },
    )
    .unwrap()
}

const WORKER: [&str; 11] = [
    "--role",
    "worker",
    "--id",
    "worker",
    "--session",
    "worker-session",
    "--route",
    "human",
    "--lease-seconds",
    "600",
    "--all-participants-upgraded",
];

fn worker(verb: &str, extra: &[&str]) -> Vec<String> {
    [vec![verb], WORKER.to_vec(), extra.to_vec()]
        .concat()
        .into_iter()
        .map(str::to_owned)
        .collect()
}

fn assign_args(
    f: &Fixture,
    operation: &str,
    ticket: &hotsheet_model::Ticket,
    base: &str,
) -> Vec<String> {
    [
        "assign",
        "--id",
        "integrator",
        "--session",
        "integrator-session",
        "--worker",
        "worker",
        "--worker-session",
        "worker-session",
        "--operation",
        operation,
        "--connection",
        &git_connection_id(&f.store),
        "--native-id",
        &ticket.id.to_string(),
        "--slug",
        &ticket.slug,
        "--base",
        base,
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn strs(args: &[String]) -> Vec<&str> {
    args.iter().map(String::as_str).collect()
}

#[test]
fn hs2_6wfczr_cli_states_preferences_behind_the_schema_nine_upgrade() {
    let f = Fixture::new();
    let capabilities = json(&f, &["capabilities"]);
    assert_eq!(capabilities["schema_written_with_worker_preferences"], 9);
    assert_eq!(
        capabilities["preference_filter_kinds"],
        serde_json::json!(["tag", "category", "priority", "tier"])
    );
    let base = git(&f.project, &["rev-parse", "HEAD"]);
    json(
        &f,
        &[
            "register",
            "--role",
            "integrator",
            "--id",
            "integrator",
            "--session",
            "integrator-session",
        ],
    );
    let plain = [
        "register",
        "--role",
        "worker",
        "--id",
        "worker",
        "--session",
        "worker-session",
    ];
    refused(
        &f,
        &[&plain[..], &["--only", "tag:glance"]].concat(),
        "--all-participants-upgraded",
    );
    refused(
        &f,
        &[&plain[..], &["--only", "glance"]].concat(),
        "filter must be",
    );
    refused(
        &f,
        &[
            "register",
            "--role",
            "integrator",
            "--id",
            "integrator",
            "--session",
            "integrator-session",
            "--prefer",
            "tag:glance",
        ],
        "only a worker",
    );
    assert_eq!(json(&f, &["status"])["state"]["version"], 3);
    let glance = tagged(&f, "Glance item", &["glance"]);
    let other = tagged(&f, "Other item", &[]);
    let status = json(
        &f,
        &strs(&worker(
            "register",
            &["--only", "tag:glance", "--prefer", "category:bug"],
        )),
    );
    assert_eq!(status["state"]["version"], 9);
    assert_eq!(
        status["state"]["workers"]["worker"]["preferences"],
        serde_json::json!({"prefer": ["category:bug"], "only": ["tag:glance"]})
    );
    let matching = &status["worker_matches"]["worker"];
    assert_eq!(matching["candidates"][0]["slug"], glance.slug);
    assert_eq!(matching["candidates"].as_array().unwrap().len(), 1);
    assert_eq!(status["unmatched_tickets"], serde_json::json!([other.slug]));
    // A plain renewal keeps the stated set.
    json(&f, &plain);
    let status = json(&f, &["status"]);
    assert_eq!(
        status["state"]["workers"]["worker"]["preferences"]["only"][0],
        "tag:glance"
    );
    // The assignment authority cannot override a strict filter.
    let args = assign_args(&f, "op-other", &other, base.trim());
    refused(&f, &strs(&args), "strict preferences");
    // The workers view (also REST /workers and MCP hotsheet_workers) carries the same facts.
    let view = json(&f, &["workers"]);
    let row = view["workers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "worker")
        .unwrap();
    assert_eq!(row["preferences"]["only"][0], "tag:glance");
    assert_eq!(row["matching"]["candidates"][0]["slug"], glance.slug);
    assert_eq!(view["unmatched_tickets"], serde_json::json!([other.slug]));
    // A clear drops the set; the schema is never lowered.
    let status = json(&f, &[&plain[..], &["--clear-preferences"]].concat());
    assert!(
        status["state"]["workers"]["worker"]
            .get("preferences")
            .is_none()
    );
    assert_eq!(status["state"]["version"], 9);
}

#[test]
fn hs2_6wfczr_cli_refusal_wakes_the_worker_and_an_honored_request_is_consumed() {
    let f = Fixture::new();
    let base = git(&f.project, &["rev-parse", "HEAD"]);
    json(
        &f,
        &[
            "register",
            "--role",
            "integrator",
            "--id",
            "integrator",
            "--session",
            "integrator-session",
            "--lease-seconds",
            "600",
        ],
    );
    let wanted = tagged(&f, "Wanted item", &[]);
    let mut waiting = f.waiting(&strs(&worker("next", &["--request", &wanted.slug])));
    // Only the assignment authority may refuse, and only the exact pending request.
    refused(
        &f,
        &[
            "refuse-request",
            "--id",
            "intruder",
            "--session",
            "intruder-session",
            "--worker",
            "worker",
            "--worker-session",
            "worker-session",
            "--slug",
            &wanted.slug,
            "--reason",
            "overlap",
        ],
        "exact integrator session",
    );
    json(
        &f,
        &[
            "refuse-request",
            "--id",
            "integrator",
            "--session",
            "integrator-session",
            "--worker",
            "worker",
            "--worker-session",
            "worker-session",
            "--slug",
            &wanted.slug,
            "--reason",
            "overlap",
            "--detail",
            "scope synthetic.txt is in flight",
        ],
    );
    let snapshot = waiting.result();
    assert!(snapshot.state.active_assignment("worker").is_none());
    let refusal = snapshot.state.workers["worker"]
        .preferences
        .refusal
        .clone()
        .unwrap();
    assert_eq!(refusal.slug, wanted.slug);
    assert_eq!(
        refusal.detail.as_deref(),
        Some("scope synthetic.txt is in flight")
    );
    // Restating the request keeps the seen refusal, so the next wait does not loop on it.
    let mut waiting = f.waiting(&strs(&worker("next", &["--request", &wanted.slug])));
    let args = assign_args(&f, "op-wanted", &wanted, base.trim());
    json(&f, &strs(&args));
    let snapshot = waiting.result();
    assert_eq!(
        snapshot.state.active_assignment("worker").unwrap().id,
        "op-wanted"
    );
    let preferences = &snapshot.state.workers["worker"].preferences;
    assert!(preferences.request.is_none() && preferences.refusal.is_none());
}
