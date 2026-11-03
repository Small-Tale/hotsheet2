//! HS2-V9BXWF: the headless retire-finished workflow through real CLI processes.
#[path = "support/coordination.rs"]
#[allow(dead_code)] // HS2-V9BXWF: shared process fixture also provides unused waiter helpers.
mod support;

use hotsheet_cli::coordination::Phase;
use hotsheet_model::{NoteKind, Status, Timestamp, Ulid};
use hotsheet_ticketing::ops;
use support::{Fixture, git};

fn retire(f: &Fixture, extra: &[&str]) -> serde_json::Value {
    let mut args = vec!["retire-finished", "--id", "integrator", "--session", "root"];
    args.extend_from_slice(extra);
    let args = f.coord(&args);
    serde_json::from_str(&f.cli(&args.iter().map(String::as_str).collect::<Vec<_>>())).unwrap()
}

fn workers(f: &Fixture, extra: &[&str]) -> serde_json::Value {
    let mut args = vec!["workers"];
    args.extend_from_slice(extra);
    let args = f.coord(&args);
    serde_json::from_str(&f.cli(&args.iter().map(String::as_str).collect::<Vec<_>>())).unwrap()
}

#[test]
fn hs2_v9bxwf_cli_retires_findings_handoff_on_archived_ticket_with_inspection_note() {
    let f = Fixture::new();
    let ticket = f.ticket();
    for (role, id, session) in [
        ("integrator", "integrator", "root"),
        ("worker", "worker", "worker-session"),
    ] {
        f.run(&["register", "--role", role, "--id", id, "--session", session]);
    }
    let base = git(&f.project, &["rev-parse", "HEAD"]);
    f.assign("findings", &ticket, &base, "root");
    let worker = |phase: &str, extra: &[&str]| {
        let mut args = vec![
            "worker",
            "--id",
            "worker",
            "--session",
            "worker-session",
            "--assignment",
            "findings",
            "--phase",
            phase,
        ];
        args.extend_from_slice(extra);
        f.run(&args);
    };
    f.cli(&["claim", &ticket.slug, "--worker", "worker", "--eta", "10m"]);
    worker("accepted", &[]);
    let findings = Ulid::new();
    ops::add_note(
        &f.store,
        &ticket.id,
        findings,
        Timestamp::from_datetime(time::OffsetDateTime::now_utc()),
        NoteKind::Regular,
        "## Findings handoff\nSynthetic verified result; no code change.".into(),
    )
    .unwrap();
    f.cli(&["release", &ticket.slug, "--worker", "worker"]);
    worker("handoff", &["--findings-note", &findings.to_string()]);
    let held = workers(&f, &[]);
    let row = held["workers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "worker")
        .unwrap();
    assert_eq!(row["tickets"].as_array().unwrap().len(), 1);

    let mut archived = f.store.read_ticket(&ticket.id).unwrap();
    archived.status = Status::Archive;
    f.store.write_ticket(&archived).unwrap();

    // The archived ticket's record is terminal for the Workers view at once.
    let view = workers(&f, &["--include-past"]);
    let row = view["workers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "worker")
        .unwrap();
    assert!(row["tickets"].as_array().unwrap().is_empty());
    assert_eq!(view["past_workers"], 0, "a live worker is never past");

    let preview = retire(&f, &["--dry-run"]);
    assert_eq!(preview["dry_run"], true);
    assert_eq!(preview["retired"][0]["assignment"], "findings");
    assert!(preview["retired"][0]["note"].is_null());
    let status = f.run(&["status"]);
    assert_eq!(status.state.assignments["findings"].phase, Phase::Handoff);

    let report = retire(&f, &[]);
    assert_eq!(report["retired"].as_array().unwrap().len(), 1);
    assert_eq!(report["retired"][0]["previous_phase"], "handoff");
    assert_eq!(report["retired"][0]["ticket_status"], "archive");
    let note = report["retired"][0]["note"].as_str().unwrap().to_owned();
    let stored = f.store.read_ticket(&ticket.id).unwrap();
    let written = stored
        .notes
        .iter()
        .find(|n| n.id.to_string() == note)
        .expect("inspection note on the ticket");
    assert_eq!(written.kind, NoteKind::Regular);
    assert!(written.text.starts_with("## Coordination retirement"));
    assert!(written.text.contains(&findings.to_string()));
    assert_eq!(
        stored.status,
        Status::Archive,
        "retirement never reopens a ticket"
    );
    let status = f.run(&["status"]);
    let retired = &status.state.assignments["findings"];
    assert_eq!(retired.phase, Phase::Done);
    assert_eq!(retired.retirement_note.as_deref(), Some(note.as_str()));

    let again = retire(&f, &[]);
    assert!(again["retired"].as_array().unwrap().is_empty());
    f.run(&[
        "finish",
        "--id",
        "integrator",
        "--session",
        "root",
        "--reason",
        "blocked",
    ]);
}
