//! Real CLI route selection and operation replay (HS2-HM9FRV).

#[path = "support/coordination.rs"]
mod support;

use std::fs;
use std::process::Output;

use hotsheet_cli::coordination::ReturnRoute;
use hotsheet_model::Ticket;
use hotsheet_ticketing::git_connection_id;
use support::{Fixture, git};

fn register(f: &Fixture, role: &str, id: &str, generation: &str, route: &str) {
    f.run(&[
        "register",
        "--role",
        role,
        "--id",
        id,
        "--session",
        generation,
        "--route",
        route,
    ]);
}

fn route_note(
    f: &Fixture,
    ticket: &Ticket,
    operation: &str,
    worker: &str,
    generation: &str,
    route: &str,
    author: &str,
) -> String {
    let json = serde_json::json!({
        "operation": operation,
        "worker": worker,
        "generation": generation,
        "route": route,
    });
    f.cli(&[
        "edit",
        &ticket.slug,
        "--note",
        &format!("## Assignment return route\n\n{json}"),
        "--note-kind",
        "regular",
        "--actor-role",
        "ai",
        "--actor-id",
        author,
    ]);
    f.store
        .read_ticket(&ticket.id)
        .unwrap()
        .notes
        .last()
        .unwrap()
        .id
        .to_string()
}

fn assign_output(
    f: &Fixture,
    ticket: &Ticket,
    operation: &str,
    worker: &str,
    generation: &str,
    route: Option<(&str, &str)>,
) -> Output {
    let base = git(&f.project, &["rev-parse", "HEAD"]);
    let connection = git_connection_id(&f.store);
    let native_id = ticket.id.to_string();
    let mut args = vec![
        "assign",
        "--id",
        "integrator",
        "--session",
        "integrator-generation",
        "--worker",
        worker,
        "--worker-session",
        generation,
        "--operation",
        operation,
        "--connection",
        &connection,
        "--native-id",
        &native_id,
        "--slug",
        &ticket.slug,
        "--base",
        &base,
    ];
    if let Some((route, note)) = route {
        args.extend(["--return-route", route, "--route-evidence-note", note]);
    }
    let command = f.coord(&args);
    f.command(&command.iter().map(String::as_str).collect::<Vec<_>>())
        .output()
        .unwrap()
}

fn state_bytes(f: &Fixture) -> Vec<u8> {
    fs::read(
        f.root
            .path()
            .join("project/.git/hotsheet-coordination/state.json"),
    )
    .unwrap()
}

#[test]
fn hs2_hm9frv_default_route_wakes_the_exact_waiter_after_a_released_claim() {
    let f = Fixture::new();
    register(
        &f,
        "integrator",
        "integrator",
        "integrator-generation",
        "terminal:root-connection",
    );
    register(&f, "worker", "worker", "worker-session", "human");
    let ticket = f.ticket();
    f.cli(&["claim", &ticket.slug, "--worker", "worker", "--eta", "10m"]);
    f.cli(&["release", &ticket.slug, "--worker", "worker"]);
    let mut waiter = f.waiting(&[
        "next",
        "--role",
        "worker",
        "--id",
        "worker",
        "--session",
        "worker-session",
        "--route",
        "human",
        "--lease-seconds",
        "3600",
    ]);
    let base = git(&f.project, &["rev-parse", "HEAD"]);
    f.assign("default-operation", &ticket, &base, "integrator-generation");
    let snapshot = waiter.result();
    assert_eq!(
        snapshot.state.assignments["default-operation"].return_route,
        ReturnRoute::Terminal("root-connection".into())
    );
    assert_eq!(
        snapshot.state.assignments["default-operation"]
            .ticket
            .native_id,
        ticket.id.to_string()
    );
}

#[test]
fn hs2_hm9frv_mixed_routes_keep_the_integrator_registration_and_default() {
    let f = Fixture::new();
    register(
        &f,
        "integrator",
        "integrator",
        "integrator-generation",
        "terminal:root-connection",
    );
    for worker in ["terminal-worker", "native-worker", "relay-worker"] {
        register(&f, "worker", worker, "worker-generation", "human");
    }
    let tickets = [f.ticket(), f.ticket(), f.ticket()];
    let native_note = route_note(
        &f,
        &tickets[1],
        "native-operation",
        "native-worker",
        "worker-generation",
        "native:/root",
        "integrator",
    );
    let relay_note = route_note(
        &f,
        &tickets[2],
        "relay-operation",
        "relay-worker",
        "worker-generation",
        "human",
        "integrator",
    );
    for (ticket, operation, worker, route) in [
        (&tickets[0], "terminal-operation", "terminal-worker", None),
        (
            &tickets[1],
            "native-operation",
            "native-worker",
            Some(("native:/root", native_note.as_str())),
        ),
        (
            &tickets[2],
            "relay-operation",
            "relay-worker",
            Some(("human", relay_note.as_str())),
        ),
    ] {
        let output = assign_output(&f, ticket, operation, worker, "worker-generation", route);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let state = f.run(&["status"]).state;
    assert_eq!(state.version, 3);
    assert_eq!(
        state.integrator.unwrap().return_route,
        ReturnRoute::Terminal("root-connection".into())
    );
    assert_eq!(
        state.assignments["terminal-operation"].return_route,
        ReturnRoute::Terminal("root-connection".into())
    );
    assert_eq!(
        state.assignments["native-operation"].return_route,
        ReturnRoute::NativeParent("/root".into())
    );
    assert_eq!(
        state.assignments["relay-operation"].return_route,
        ReturnRoute::HumanRelay
    );
}

#[test]
fn hs2_hm9frv_refusals_preserve_pool_bytes_and_replay_keeps_the_original_route() {
    let f = Fixture::new();
    register(
        &f,
        "integrator",
        "integrator",
        "integrator-generation",
        "terminal:root-connection",
    );
    register(&f, "worker", "worker", "worker-generation", "human");
    let ticket = f.ticket();
    let other_ticket = f.ticket();
    let before = state_bytes(&f);
    let missing = assign_output(
        &f,
        &ticket,
        "operation",
        "worker",
        "worker-generation",
        Some(("native:/root", "01M00000000000000000000000")),
    );
    assert!(!missing.status.success());
    assert_eq!(state_bytes(&f), before);

    f.cli(&[
        "edit",
        &ticket.slug,
        "--note",
        "## Assignment return route\n\nnot-json",
        "--note-kind",
        "regular",
        "--actor-role",
        "ai",
        "--actor-id",
        "integrator",
    ]);
    let malformed = f
        .store
        .read_ticket(&ticket.id)
        .unwrap()
        .notes
        .last()
        .unwrap()
        .id
        .to_string();
    let before = state_bytes(&f);
    assert!(
        !assign_output(
            &f,
            &ticket,
            "operation",
            "worker",
            "worker-generation",
            Some(("native:/root", &malformed)),
        )
        .status
        .success()
    );
    assert_eq!(state_bytes(&f), before);

    for (operation, worker, generation, route, author) in [
        (
            "wrong-operation",
            "worker",
            "worker-generation",
            "native:/root",
            "integrator",
        ),
        (
            "operation",
            "other-worker",
            "worker-generation",
            "native:/root",
            "integrator",
        ),
        (
            "operation",
            "worker",
            "old-generation",
            "native:/root",
            "integrator",
        ),
        (
            "operation",
            "worker",
            "worker-generation",
            "human",
            "integrator",
        ),
        (
            "operation",
            "worker",
            "worker-generation",
            "native:/root",
            "other-author",
        ),
    ] {
        let note = route_note(&f, &ticket, operation, worker, generation, route, author);
        let before = state_bytes(&f);
        let output = assign_output(
            &f,
            &ticket,
            "operation",
            "worker",
            "worker-generation",
            Some(("native:/root", &note)),
        );
        assert!(!output.status.success());
        assert_eq!(state_bytes(&f), before);
    }
    let foreign_note = route_note(
        &f,
        &other_ticket,
        "operation",
        "worker",
        "worker-generation",
        "native:/root",
        "integrator",
    );
    let before = state_bytes(&f);
    assert!(
        !assign_output(
            &f,
            &ticket,
            "operation",
            "worker",
            "worker-generation",
            Some(("native:/root", &foreign_note)),
        )
        .status
        .success()
    );
    assert_eq!(state_bytes(&f), before);

    let note = route_note(
        &f,
        &ticket,
        "operation",
        "worker",
        "worker-generation",
        "native:/root",
        "integrator",
    );
    let accepted = assign_output(
        &f,
        &ticket,
        "operation",
        "worker",
        "worker-generation",
        Some(("native:/root", &note)),
    );
    assert!(
        accepted.status.success(),
        "{}",
        String::from_utf8_lossy(&accepted.stderr)
    );
    register(
        &f,
        "integrator",
        "integrator",
        "integrator-generation",
        "human",
    );
    let replay = assign_output(
        &f,
        &ticket,
        "operation",
        "worker",
        "worker-generation",
        None,
    );
    assert!(
        replay.status.success(),
        "{}",
        String::from_utf8_lossy(&replay.stderr)
    );
    assert_eq!(
        f.run(&["status"]).state.assignments["operation"].return_route,
        ReturnRoute::NativeParent("/root".into())
    );

    let conflicting_note = route_note(
        &f,
        &ticket,
        "operation",
        "worker",
        "worker-generation",
        "human",
        "integrator",
    );
    let before = state_bytes(&f);
    let conflicting = assign_output(
        &f,
        &ticket,
        "operation",
        "worker",
        "worker-generation",
        Some(("human", &conflicting_note)),
    );
    assert!(!conflicting.status.success());
    assert_eq!(state_bytes(&f), before);
}

#[test]
fn hs2_hm9frv_stale_generation_and_live_claim_cannot_be_rerouted() {
    let f = Fixture::new();
    register(
        &f,
        "integrator",
        "integrator",
        "integrator-generation",
        "human",
    );
    register(&f, "worker", "worker", "worker-generation", "human");
    let stale = f.ticket();
    let note = route_note(
        &f,
        &stale,
        "stale-operation",
        "worker",
        "old-generation",
        "native:/root",
        "integrator",
    );
    let before = state_bytes(&f);
    assert!(
        !assign_output(
            &f,
            &stale,
            "stale-operation",
            "worker",
            "old-generation",
            Some(("native:/root", &note)),
        )
        .status
        .success()
    );
    assert_eq!(state_bytes(&f), before);

    let claimed = f.ticket();
    let note = route_note(
        &f,
        &claimed,
        "claimed-operation",
        "worker",
        "worker-generation",
        "native:/root",
        "integrator",
    );
    f.cli(&[
        "claim",
        &claimed.slug,
        "--worker",
        "other-worker",
        "--eta",
        "10m",
    ]);
    let before = state_bytes(&f);
    assert!(
        !assign_output(
            &f,
            &claimed,
            "claimed-operation",
            "worker",
            "worker-generation",
            Some(("native:/root", &note)),
        )
        .status
        .success()
    );
    assert_eq!(state_bytes(&f), before);
}
