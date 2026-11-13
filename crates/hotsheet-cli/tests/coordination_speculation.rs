//! Real separate-process provisional readiness (HS2-2SJPCH): the unchanged default, the
//! policy opt-in, a provisional reservation with its recorded base and gate, an
//! event-driven wake on a red result, opt-out, rebase onto the fix and the branch record.
//! Every pool here is a disposable, independently initialized project and ticket store.

#[allow(dead_code)] // HS2-2SJPCH: shared process fixture includes unrelated helpers.
#[path = "support/coordination.rs"]
mod support;

use hotsheet_model::{NoteKind, StartedPhase, Status, Ticket, Timestamp, Ulid};
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

fn now() -> Timestamp {
    Timestamp::from_datetime(time::OffsetDateTime::now_utc())
}

fn dependent(f: &Fixture, blocker: &Ticket) -> Ticket {
    ops::create(
        &f.store,
        Ulid::new(),
        "HS",
        now(),
        NewTicket {
            title: "synthetic dependent task".into(),
            up_next: true,
            blocked_by: vec![blocker.id],
            ..NewTicket::default()
        },
    )
    .unwrap()
}

fn gate_note(f: &Fixture, ticket: &Ticket, commit: &str) {
    ops::add_note(
        &f.store,
        &ticket.id,
        Ulid::new(),
        now(),
        NoteKind::Regular,
        format!("Pushed; waiting on CI.\n\nCI gate commit: {commit}\n"),
    )
    .unwrap();
}

/// A prerequisite pushed to `main` and waiting on remote CI.
fn awaiting_ci(f: &Fixture, commit: &str) -> Ticket {
    let created = f.ticket();
    let mut ticket = f.store.read_ticket(&created.id).unwrap();
    ticket.status = Status::Started;
    ticket.started_phase = Some(StartedPhase::FinalTesting);
    f.store.write_ticket(&ticket).unwrap();
    gate_note(f, &ticket, commit);
    f.store.read_ticket(&created.id).unwrap()
}

fn register(f: &Fixture, role: &str, id: &str, session: &str) {
    json(
        f,
        &[
            "register",
            "--role",
            role,
            "--id",
            id,
            "--session",
            session,
            "--lease-seconds",
            "600",
        ],
    );
}

fn assign_args(f: &Fixture, worker: &str, operation: &str, t: &Ticket, base: &str) -> Vec<String> {
    [
        "assign",
        "--id",
        "integrator",
        "--session",
        "root",
        "--worker",
        worker,
        "--worker-session",
        "worker-session",
        "--operation",
        operation,
        "--connection",
        &git_connection_id(&f.store),
        "--native-id",
        &t.id.to_string(),
        "--slug",
        &t.slug,
        "--base",
        base,
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect()
}

fn notes_with(f: &Fixture, t: &Ticket, heading: &str) -> usize {
    f.store
        .read_ticket(&t.id)
        .unwrap()
        .notes
        .iter()
        .filter(|note| note.text.starts_with(heading))
        .count()
}

#[test]
fn hs2_2sjpch_default_refusal_is_unchanged_and_opt_in_reserves_provisionally() {
    let f = Fixture::new();
    let pushed = git(&f.project, &["rev-parse", "HEAD"]);
    let blocker = awaiting_ci(&f, &pushed);
    let waiting = dependent(&f, &blocker);
    register(&f, "integrator", "integrator", "root");
    register(&f, "worker", "worker", "worker-session");
    let ledger = f
        .project
        .join(".git/hotsheet-coordination/speculation.json");
    let assign = assign_args(&f, "worker", "op", &waiting, &pushed);
    let assign: Vec<&str> = assign.iter().map(String::as_str).collect();

    // Default: confirmed readiness only, with the refusal exactly as before the policy.
    let policy = json(&f, &["speculation-policy"]);
    assert_eq!(
        policy,
        serde_json::json!({"enabled": false, "max_depth": 1, "integration": "push-per-ticket"})
    );
    let out = output(&f, &assign);
    assert!(!out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stderr).trim(),
        format!(
            "Error: ticket {} depends on {} ({}, started); close them or remove the dependency before assigning it",
            waiting.slug, blocker.slug, blocker.id
        )
    );
    let status = json(&f, &["status"]);
    assert!(status.get("speculation").is_none());
    assert!(
        status["state"]["assignments"]
            .as_object()
            .unwrap()
            .is_empty()
    );
    let explained = json(&f, &["speculation-explain", "--ticket", &waiting.slug]);
    assert_eq!(explained["readiness"]["level"], "blocked");
    assert!(
        explained["explain"]
            .as_str()
            .unwrap()
            .contains("speculation_disabled")
    );
    assert!(!ledger.exists());
    assert!(
        !std::fs::read_to_string(f.project.join(".hotsheet2/settings.json"))
            .unwrap_or_default()
            .contains("speculation")
    );

    // Opt in. The policy is committed shared settings, validated before it is written.
    refused(
        &f,
        &["speculation-policy", "--enable", "--max-depth", "0"],
        "max_depth must be between 1 and 8",
    );
    refused(
        &f,
        &["speculation-policy", "--integration", "sometimes"],
        "push-per-ticket or parked",
    );
    let policy = json(
        &f,
        &["speculation-policy", "--enable", "--base-branch", "main"],
    );
    assert_eq!(policy["enabled"], true);
    assert_eq!(policy["base_branch"], "main");
    assert!(
        std::fs::read_to_string(f.project.join(".hotsheet2/settings.json"))
            .unwrap()
            .contains("\"speculation\"")
    );
    let explained = json(&f, &["speculation-explain", "--ticket", &waiting.slug]);
    assert_eq!(explained["readiness"]["level"], "provisional");
    let explain = format!("provisional on {} CI unknown", blocker.slug);
    assert_eq!(explained["explain"], explain.as_str());
    assert_eq!(
        explained["readiness"]["prerequisites"][0]["readiness"]["base_commit"],
        pushed.as_str()
    );

    // An integrator waiting on the pool wakes on events, not on a timer.
    let cursor = json(&f, &["status"])["cursor"].as_str().unwrap().to_owned();
    let mut waiter = f.waiting(&[
        "next",
        "--role",
        "integrator",
        "--id",
        "integrator",
        "--session",
        "root",
        "--after",
        &cursor,
    ]);
    json(&f, &assign);
    let woke = waiter.result();
    let row = &woke.speculation.as_ref().unwrap().records[0];
    assert_eq!(row.assignment, "op");
    assert_eq!(row.base, pushed);
    assert_eq!(row.gates[0].commit, pushed);
    assert_eq!(row.explain, explain);
    // The reservation replays without a second record, and both read surfaces agree.
    json(&f, &assign);
    let status = json(&f, &["speculation-status"]);
    assert_eq!(status["status"]["records"].as_array().unwrap().len(), 1);
    assert_eq!(status["status"]["records"][0]["state"], "provisional");
    let view = json(&f, &["workers"]);
    assert_eq!(view["speculation"], status["status"]);
    // An unwatched gate carries no information; a bounded wait on nothing returns.
    let synced = json(&f, &["speculation-sync", "--wait-ms", "50"]);
    assert!(synced["changes"].as_array().unwrap().is_empty());
    assert_eq!(synced["event_source"], "none");
    refused(
        &f,
        &["speculation-sync", "--wait-ms", "999999999"],
        "call again to keep waiting",
    );

    // An opted-out ticket is explained and refused, in CLI terms an ordinary tag edit.
    let cautious = dependent(&f, &blocker);
    f.cli(&["edit", &cautious.slug, "--tag", "speculate:never"]);
    register(&f, "worker", "second", "worker-session");
    let explained = json(&f, &["speculation-explain", "--ticket", &cautious.slug]);
    assert_eq!(explained["opted_out"], true);
    let second = assign_args(&f, "second", "op-2", &cautious, &pushed);
    refused(
        &f,
        &second.iter().map(String::as_str).collect::<Vec<_>>(),
        "speculation_opted_out",
    );

    // Red: a waiter wakes, each dependent and the prerequisite get one note naming the run.
    let cursor = json(&f, &["status"])["cursor"].as_str().unwrap().to_owned();
    let mut waiter = f.waiting(&[
        "next",
        "--role",
        "integrator",
        "--id",
        "integrator",
        "--session",
        "root",
        "--after",
        &cursor,
    ]);
    let red = json(
        &f,
        &[
            "speculation-result",
            "--gate-commit",
            &pushed,
            "--result",
            "red",
            "--run",
            "501.1",
        ],
    );
    assert_eq!(red["changes"][0]["to"], "at_risk");
    assert_eq!(red["changes"][0]["recovery"], "rebase_onto_fix");
    assert_eq!(red["status"]["repair_first"][0], blocker.slug.as_str());
    let woke = waiter.result();
    assert_eq!(
        woke.speculation.unwrap().repair_first,
        vec![blocker.slug.clone()]
    );
    assert_eq!(notes_with(&f, &waiting, "## Provisional work at risk"), 1);
    assert_eq!(
        notes_with(&f, &blocker, "## CI red with provisional dependents"),
        1
    );
    let replay = json(
        &f,
        &[
            "speculation-result",
            "--gate-commit",
            &pushed,
            "--result",
            "red",
            "--run",
            "501.1",
        ],
    );
    assert!(replay["changes"].as_array().unwrap().is_empty());
    assert_eq!(notes_with(&f, &waiting, "## Provisional work at risk"), 1);
    refused(
        &f,
        &[
            "speculation-result",
            "--gate-commit",
            "abc",
            "--result",
            "red",
        ],
        "full 40-character SHA",
    );

    // The fix lands on main; the parked work is rebased onto it and recorded.
    std::fs::write(f.project.join("synthetic.txt"), "repaired\n").unwrap();
    git(&f.project, &["commit", "-qam", "synthetic repair"]);
    let fix = git(&f.project, &["rev-parse", "HEAD"]);
    gate_note(&f, &blocker, &fix);
    refused(
        &f,
        &[
            "speculation-rebase",
            "--assignment",
            "op",
            "--replaces",
            &pushed,
            "--gate-commit",
            &fix,
            "--base",
            &pushed,
        ],
        "new base does not contain the repaired gate commit",
    );
    let rebased = json(
        &f,
        &[
            "speculation-rebase",
            "--assignment",
            "op",
            "--replaces",
            &pushed,
            "--gate-commit",
            &fix,
            "--base",
            &fix,
        ],
    );
    assert_eq!(rebased["state"], "provisional");
    assert_eq!(rebased["base"], fix.as_str());
    let branch = json(
        &f,
        &[
            "speculation-branch",
            "--assignment",
            "op",
            "--worktree",
            &f.project.display().to_string(),
        ],
    );
    assert_eq!(branch["branch"], "main");
    assert_eq!(branch["base"], fix.as_str());
    assert_eq!(
        git(
            &f.project,
            &["config", "--get", "branch.main.hotsheet-pending-gate"]
        ),
        format!("{} {fix} unknown -", blocker.slug)
    );
    let green = json(
        &f,
        &[
            "speculation-result",
            "--gate-commit",
            &fix,
            "--result",
            "green",
            "--run",
            "502.1",
        ],
    );
    assert_eq!(green["changes"][0]["to"], "confirmed");
    assert!(
        green["status"]["repair_first"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    refused(
        &f,
        &[
            "speculation-reopen",
            "--assignment",
            "op",
            "--reason",
            "synthetic",
        ],
        "is confirmed",
    );
    // With its base confirmed, the once-refused sibling is still opted out; removing the
    // tag makes it assignable, now provisional on the repaired commit's green gate.
    f.cli(&["edit", &cautious.slug, "--tag", "other"]);
    let explained = json(&f, &["speculation-explain", "--ticket", &cautious.slug]);
    assert_eq!(explained["opted_out"], false);
    assert_eq!(explained["readiness"]["level"], "provisional");
}
