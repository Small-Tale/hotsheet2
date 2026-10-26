//! Real CLI authority wiring; precise boundary clocks are injected in the private guard tests.

#[path = "support/coordination.rs"]
mod support;

use hotsheet_model::Timestamp;
use support::{Fixture, git};

fn registered() -> Fixture {
    let f = Fixture::new();
    for (role, id, session) in [
        ("integrator", "integrator", "root"),
        ("worker", "worker", "worker-session"),
    ] {
        f.run(&["register", "--role", role, "--id", id, "--session", session]);
    }
    f
}

fn rejects(f: &Fixture, args: &[&str], expected: &str) {
    let args = f.coord(args);
    let out = f
        .command(&args.iter().map(String::as_str).collect::<Vec<_>>())
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "unexpected successful authority transition"
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains(expected),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn expire(f: &Fixture, id: &hotsheet_model::Ulid) {
    let mut ticket = f.store.read_ticket(id).unwrap();
    // Distant synthetic past, not a wall-clock race or a sleep near a second boundary.
    ticket.claim_lease_expires_at = Some(Timestamp::new("2020-01-01T00:00:00.200Z"));
    f.store.write_ticket(&ticket).unwrap();
}

#[test]
fn hs2_35r7m3_cli_acceptance_rechecks_expiry_release_replacement_and_other_claims() {
    let f = registered();
    let ticket = f.ticket();
    let other = f.ticket();
    let base = git(&f.project, &["rev-parse", "HEAD"]);
    let mut waiting = f.waiting(&[
        "next",
        "--role",
        "worker",
        "--id",
        "worker",
        "--session",
        "worker-session",
    ]);
    f.assign("acceptance", &ticket, &base, "root");
    assert_eq!(
        waiting.result().state.assignments["acceptance"].phase,
        hotsheet_cli::coordination::Phase::Offered
    );
    let accept = [
        "worker",
        "--id",
        "worker",
        "--session",
        "worker-session",
        "--assignment",
        "acceptance",
        "--phase",
        "accepted",
    ];
    rejects(&f, &accept, "claim this exact ticket");
    f.cli(&["claim", &ticket.slug, "--worker", "worker", "--eta", "10m"]);
    f.run(&accept);
    f.run(&accept);
    expire(&f, &ticket.id);
    rejects(&f, &accept, "claim this exact ticket");
    // Expiry never substitutes for the explicit release required by a blocked handoff.
    rejects(
        &f,
        &[
            "worker",
            "--id",
            "worker",
            "--session",
            "worker-session",
            "--assignment",
            "acceptance",
            "--phase",
            "blocked",
        ],
        "release the ticket claim",
    );
    f.cli(&["claim", &ticket.slug, "--worker", "worker", "--eta", "10m"]);
    f.cli(&["renew", &ticket.slug, "--worker", "worker", "--eta", "10m"]);
    f.run(&accept);
    f.cli(&["claim", &other.slug, "--worker", "worker", "--eta", "10m"]);
    rejects(&f, &accept, "worker holds another live ticket claim");
    expire(&f, &other.id);
    f.run(&accept);
    f.cli(&["release", &ticket.slug, "--worker", "worker"]);
    rejects(&f, &accept, "claim this exact ticket");
    f.cli(&[
        "claim",
        &ticket.slug,
        "--worker",
        "replacement",
        "--eta",
        "10m",
    ]);
    rejects(&f, &accept, "claim this exact ticket");
    f.cli(&["release", &ticket.slug, "--worker", "replacement"]);
    f.cli(&["claim", &ticket.slug, "--worker", "worker", "--eta", "10m"]);
    f.run(&accept);
    let snapshot = f.run(&["status"]);
    assert_eq!(snapshot.state.version, 3);
    assert_eq!(
        snapshot.state.assignments["acceptance"].phase,
        hotsheet_cli::coordination::Phase::Accepted
    );
}

#[test]
fn hs2_35r7m3_cli_assignment_and_recovery_require_current_claim_authority() {
    let f = registered();
    let ticket = f.ticket();
    let other = f.ticket();
    let base = git(&f.project, &["rev-parse", "HEAD"]);
    f.cli(&["claim", &other.slug, "--worker", "worker", "--eta", "10m"]);
    let connection = hotsheet_ticketing::git_connection_id(&f.store);
    let id = ticket.id.to_string();
    let assign = [
        "assign",
        "--id",
        "integrator",
        "--session",
        "root",
        "--worker",
        "worker",
        "--worker-session",
        "worker-session",
        "--operation",
        "recovery",
        "--connection",
        &connection,
        "--native-id",
        &id,
        "--slug",
        &ticket.slug,
        "--base",
        &base,
    ];
    rejects(&f, &assign, "worker already holds a live ticket claim");
    expire(&f, &other.id);
    f.cli(&[
        "claim",
        &ticket.slug,
        "--worker",
        "replacement",
        "--eta",
        "10m",
    ]);
    rejects(&f, &assign, "ticket is not eligible");
    expire(&f, &ticket.id);
    f.run(&assign);
    f.cli(&["claim", &ticket.slug, "--worker", "worker", "--eta", "10m"]);
    f.run(&[
        "worker",
        "--id",
        "worker",
        "--session",
        "worker-session",
        "--assignment",
        "recovery",
        "--phase",
        "accepted",
    ]);
    let inspection = f.root.path().join("inspection.md");
    std::fs::write(
        &inspection,
        "Synthetic recovery inspection: no worktree changes, reservation preserved.",
    )
    .unwrap();
    f.cli(&[
        "edit",
        &ticket.slug,
        "--note-file",
        inspection.to_str().unwrap(),
        "--note-kind",
        "regular",
    ]);
    let evidence = f
        .store
        .read_ticket(&ticket.id)
        .unwrap()
        .notes
        .last()
        .unwrap()
        .id
        .to_string();
    f.run(&[
        "leave",
        "--role",
        "worker",
        "--id",
        "worker",
        "--session",
        "worker-session",
    ]);
    let recover = [
        "recover",
        "--id",
        "integrator",
        "--session",
        "root",
        "--assignment",
        "recovery",
        "--evidence-note",
        &evidence,
        "--inspected",
    ];
    rejects(&f, &recover, "a live ticket claim prevents recovery");
    expire(&f, &ticket.id);
    let snapshot = f.run(&recover);
    let recovered = &snapshot.state.assignments["recovery"];
    assert_eq!(recovered.phase, hotsheet_cli::coordination::Phase::Blocked);
    assert_eq!(recovered.recovery_note.as_deref(), Some(evidence.as_str()));
    assert_eq!(recovered.generation, "worker-session");
    assert!(recovered.handoff.is_none());
}
