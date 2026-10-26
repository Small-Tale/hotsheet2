//! Real separate-process local coordination flows (HS2-76VMYG).

#[path = "support/coordination.rs"]
mod support;

use hotsheet_model::Timestamp;
use hotsheet_ticketing::ops;
use std::fs;
use support::{Fixture, git};

#[test]
fn hs2_76vmyg_worker_first_handoff_survives_absence_then_refills_from_new_main() {
    let f = Fixture::new();
    let first = f.ticket();
    let second = f.ticket();
    let mut worker = f.waiting(&[
        "next",
        "--role",
        "worker",
        "--id",
        "worker",
        "--session",
        "worker-session",
    ]);
    let status = f.run(&[
        "register",
        "--role",
        "integrator",
        "--id",
        "integrator",
        "--session",
        "root-one",
        "--route",
        "native:/root",
    ]);
    assert_eq!(status.live_workers, ["worker"]);
    let base = git(&f.project, &["rev-parse", "HEAD"]);
    f.assign("operation-one", &first, &base, "root-one");
    assert_eq!(
        worker
            .result()
            .state
            .active_assignment("worker")
            .unwrap()
            .ticket
            .native_id,
        first.id.to_string()
    );
    f.cli(&["claim", &first.slug, "--worker", "worker", "--eta", "10m"]);
    f.run(&[
        "worker",
        "--id",
        "worker",
        "--session",
        "worker-session",
        "--assignment",
        "operation-one",
        "--phase",
        "accepted",
    ]);
    let checkout = f.root.path().join("worker-one");
    git(
        &f.project,
        &[
            "worktree",
            "add",
            "-qb",
            "worker-one",
            checkout.to_str().unwrap(),
            &base,
        ],
    );
    fs::write(
        checkout.join("synthetic.txt"),
        "verified synthetic change\n",
    )
    .unwrap();
    git(&checkout, &["add", "synthetic.txt"]);
    git(
        &checkout,
        &["commit", "-qm", "HS-SYNTHETIC Change synthetic fixture"],
    );
    let commit = git(&checkout, &["rev-parse", "HEAD"]);
    f.run(&[
        "leave",
        "--role",
        "integrator",
        "--id",
        "integrator",
        "--session",
        "root-one",
    ]);
    f.cli(&[
        "edit",
        &first.slug,
        "--note",
        &format!(
            "## Integration handoff\nVerified synthetic commit {commit} is ready for integration."
        ),
    ]);
    f.cli(&["release", &first.slug, "--worker", "worker"]);
    let handoff = f.run(&[
        "worker",
        "--id",
        "worker",
        "--session",
        "worker-session",
        "--assignment",
        "operation-one",
        "--phase",
        "handoff",
        "--commit",
        &commit,
    ]);
    assert!(!handoff.live_integrator);
    let mut again = f.waiting(&[
        "next",
        "--role",
        "worker",
        "--id",
        "worker",
        "--session",
        "worker-session",
    ]);
    let resumed = f.run(&[
        "register",
        "--role",
        "integrator",
        "--id",
        "integrator",
        "--session",
        "root-two",
    ]);
    assert_eq!(
        resumed.state.assignments["operation-one"].commit.as_deref(),
        Some(commit.as_str())
    );
    f.run(&[
        "mark",
        "--id",
        "integrator",
        "--session",
        "root-two",
        "--assignment",
        "operation-one",
        "--phase",
        "integrating",
    ]);
    git(&f.project, &["cherry-pick", &commit]);
    f.run(&[
        "mark",
        "--id",
        "integrator",
        "--session",
        "root-two",
        "--assignment",
        "operation-one",
        "--phase",
        "ci",
    ]);
    let new_base = git(&f.project, &["rev-parse", "HEAD"]);
    f.assign("operation-two", &second, &new_base, "root-two");
    let refill = again.result();
    assert_eq!(
        refill.state.active_assignment("worker").unwrap().base,
        new_base
    );
    assert_eq!(
        refill.state.assignments["operation-one"].phase,
        hotsheet_cli::coordination::Phase::Ci
    );
    assert_eq!(
        fs::read_to_string(f.project.join("synthetic.txt")).unwrap(),
        "verified synthetic change\n"
    );
    assert!(!f.cli(&["show", &first.slug]).contains("claimed_by:"));
}

#[test]
fn hs2_76vmyg_integrator_first_waits_for_later_worker_and_stop_wakes_it() {
    let f = Fixture::new();
    let initial = f.run(&[
        "register",
        "--role",
        "integrator",
        "--id",
        "integrator",
        "--session",
        "root-one",
    ]);
    let mut integrator = f.waiting(&[
        "next",
        "--role",
        "integrator",
        "--id",
        "integrator",
        "--session",
        "root-one",
        "--after",
        &initial.cursor,
    ]);
    let mut worker = f.waiting(&[
        "next",
        "--role",
        "worker",
        "--id",
        "worker",
        "--session",
        "worker-session",
    ]);
    assert_eq!(integrator.result().live_workers, ["worker"]);
    f.run(&[
        "finish",
        "--id",
        "integrator",
        "--session",
        "root-one",
        "--reason",
        "drained",
    ]);
    assert_eq!(worker.result().state.stopped.as_deref(), Some("drained"));
}

#[test]
fn hs2_76vmyg_lost_cue_replays_durable_assignment_and_duplicate_wait_is_rejected() {
    let f = Fixture::new();
    let ticket = f.ticket();
    f.run(&[
        "register",
        "--role",
        "integrator",
        "--id",
        "integrator",
        "--session",
        "root",
    ]);
    let mut first_wait = f.waiting(&[
        "next",
        "--role",
        "worker",
        "--id",
        "worker",
        "--session",
        "worker-session",
    ]);
    let duplicate = f.coord(&[
        "next",
        "--role",
        "worker",
        "--id",
        "worker",
        "--session",
        "worker-session",
    ]);
    let rejected = f
        .command(&duplicate.iter().map(String::as_str).collect::<Vec<_>>())
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("another active wait"));
    first_wait.child.kill().unwrap();
    first_wait.child.wait().unwrap();
    let base = git(&f.project, &["rev-parse", "HEAD"]);
    f.assign("durable-offer", &ticket, &base, "root");
    // No notification is delivered while the worker is absent. A later call reads the
    // same durable offer, fenced to the same session generation.
    let resumed = f.run(&[
        "next",
        "--role",
        "worker",
        "--id",
        "worker",
        "--session",
        "worker-session",
    ]);
    assert_eq!(
        resumed.state.active_assignment("worker").unwrap().id,
        "durable-offer"
    );
}

#[test]
fn hs2_76vmyg_known_claim_expiry_wakes_integrator_without_a_file_write() {
    let f = Fixture::new();
    let ticket = f.ticket();
    let at = time::OffsetDateTime::now_utc()
        .replace_nanosecond(750_000_000)
        .unwrap();
    ops::claim(
        &f.store,
        &ticket.id,
        &Timestamp::from_datetime(at),
        Timestamp::from_datetime(at + time::Duration::seconds(5)),
        "other-worker",
        None,
    )
    .unwrap();
    let initial = f.run(&[
        "register",
        "--role",
        "integrator",
        "--id",
        "integrator",
        "--session",
        "root",
    ]);
    assert_eq!(
        initial.local_queue.as_ref().unwrap()[0].claim_expires_at,
        Some(at.unix_timestamp() + 6)
    );
    assert_eq!(
        initial.local_queue.as_ref().unwrap()[0]
            .claimed_by
            .as_deref(),
        Some("other-worker")
    );
    let mut waiting = f.waiting(&[
        "next",
        "--role",
        "integrator",
        "--id",
        "integrator",
        "--session",
        "root",
        "--after",
        &initial.cursor,
    ]);
    let after = waiting.result();
    assert!(after.local_queue.unwrap()[0].claimed_by.is_none());
}

#[test]
fn hs2_76vmyg_provider_scope_change_wakes_without_ticket_polling() {
    let f = Fixture::new();
    let initial = f.run(&[
        "register",
        "--role",
        "integrator",
        "--id",
        "integrator",
        "--session",
        "root",
    ]);
    let mut waiting = f.waiting(&[
        "next",
        "--role",
        "integrator",
        "--id",
        "integrator",
        "--session",
        "root",
        "--after",
        &initial.cursor,
    ]);
    hotsheet_ticketing::ProviderConfigRegistry::new(f.store.root().join("providers.json"))
        .save(&[hotsheet_ticketing::ProviderConnection {
            id: "synthetic-remote".into(),
            provider: "github".into(),
            locator: "synthetic/project".into(),
            name: None,
            default: false,
            settings: serde_json::json!({}),
            disabled: false,
        }])
        .unwrap();
    assert_eq!(waiting.result().uncovered_sources, ["synthetic-remote"]);
}

#[test]
fn hs2_gbspp8_corrupt_provider_event_preserves_the_waiter_exit_cause() {
    let f = Fixture::new();
    let initial = f.run(&[
        "register",
        "--role",
        "integrator",
        "--id",
        "integrator",
        "--session",
        "root",
    ]);
    let mut waiting = f.waiting(&[
        "next",
        "--role",
        "integrator",
        "--id",
        "integrator",
        "--session",
        "root",
        "--after",
        &initial.cursor,
    ]);
    // An in-place writer exposes this state after creation/truncation and before bytes arrive.
    // Keep it empty until the event-backed reader exits: no scheduling delay or race is needed.
    std::fs::write(f.store.root().join("providers.json"), b"").unwrap();
    let output = waiting.exit_output();
    let error = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{error}");
    assert!(error.contains("invalid provider configuration"), "{error}");
    assert!(error.contains("EOF while parsing a value"), "{error}");
    assert!(output.stdout.is_empty());
}

#[test]
fn hs2_gbspp8_atomic_provider_publications_wake_create_replace_empty_and_refill() {
    let f = Fixture::new();
    let mut initial = f.run(&[
        "register",
        "--role",
        "integrator",
        "--id",
        "integrator",
        "--session",
        "root",
    ]);
    let registry =
        hotsheet_ticketing::ProviderConfigRegistry::new(f.store.root().join("providers.json"));
    let source = hotsheet_ticketing::ProviderConnection {
        id: "synthetic-remote".into(),
        provider: "github".into(),
        locator: "synthetic/project".into(),
        name: None,
        default: false,
        settings: serde_json::json!({}),
        disabled: false,
    };
    let mut disabled = source.clone();
    disabled.disabled = true;
    for connections in [
        vec![source.clone()],
        vec![disabled],
        vec![source.clone()],
        vec![],
        vec![source],
    ] {
        let mut waiting = f.waiting(&[
            "next",
            "--role",
            "integrator",
            "--id",
            "integrator",
            "--session",
            "root",
            "--after",
            &initial.cursor,
        ]);
        registry.save(&connections).unwrap();
        let expected: Vec<_> = connections
            .iter()
            .filter(|source| !source.disabled)
            .map(|source| source.id.clone())
            .collect();
        initial = waiting.result();
        assert_eq!(initial.uncovered_sources, expected);
    }
}
