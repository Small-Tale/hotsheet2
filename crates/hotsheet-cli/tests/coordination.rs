//! Real separate-process local coordination flows (HS2-76VMYG).

#[path = "support/source_support.rs"]
mod source_support_cases;

#[path = "support/coordination.rs"]
mod support;

use hotsheet_model::{NoteKind, Timestamp};
use hotsheet_ticketing::ops;
use serde_json::Value;
use std::fs;
use std::time::{Duration, Instant};
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
    let transitions = &refill.state.assignments["operation-one"].transitions;
    assert_eq!(
        transitions
            .iter()
            .map(|event| event.kind)
            .collect::<Vec<_>>(),
        [
            hotsheet_cli::coordination::TransitionKind::Offered,
            hotsheet_cli::coordination::TransitionKind::Accepted,
            hotsheet_cli::coordination::TransitionKind::Handoff,
            hotsheet_cli::coordination::TransitionKind::Integrating,
            hotsheet_cli::coordination::TransitionKind::Ci,
        ]
    );
    assert_eq!(transitions[0].actor.as_ref().unwrap().id, "integrator");
    assert_eq!(transitions[1].actor.as_ref().unwrap().id, "worker");
    assert!(transitions.iter().all(|event| {
        time::OffsetDateTime::parse(&event.at, &time::format_description::well_known::Rfc3339)
            .is_ok()
    }));
    assert_eq!(
        fs::read_to_string(f.project.join("synthetic.txt")).unwrap(),
        "verified synthetic change\n"
    );
    assert!(!f.cli(&["show", &first.slug]).contains("claimed_by:"));
}

#[test]
fn hs2_a1f615_findings_handoff_refills_while_code_ci_is_pending() {
    let f = Fixture::new();
    let code = f.ticket();
    let findings = f.ticket();
    f.run(&[
        "register",
        "--role",
        "integrator",
        "--id",
        "integrator",
        "--session",
        "root",
    ]);
    f.run(&[
        "register",
        "--role",
        "worker",
        "--id",
        "worker",
        "--session",
        "worker-session",
    ]);
    let base = git(&f.project, &["rev-parse", "HEAD"]);
    f.assign("code", &code, &base, "root");
    f.cli(&["claim", &code.slug, "--worker", "worker", "--eta", "10m"]);
    f.run(&[
        "worker",
        "--id",
        "worker",
        "--session",
        "worker-session",
        "--assignment",
        "code",
        "--phase",
        "accepted",
    ]);
    let checkout = f.root.path().join("code-worker");
    git(
        &f.project,
        &[
            "worktree",
            "add",
            "-qb",
            "code-worker",
            checkout.to_str().unwrap(),
            &base,
        ],
    );
    fs::write(checkout.join("code.txt"), "synthetic code\n").unwrap();
    git(&checkout, &["add", "code.txt"]);
    git(
        &checkout,
        &["commit", "-qm", "HS-SYNTHETIC Record code fixture"],
    );
    let commit = git(&checkout, &["rev-parse", "HEAD"]);
    f.cli(&[
        "edit",
        &code.slug,
        "--note",
        &format!("## Integration handoff\nSynthetic commit {commit}"),
    ]);
    f.cli(&["release", &code.slug, "--worker", "worker"]);
    f.run(&[
        "worker",
        "--id",
        "worker",
        "--session",
        "worker-session",
        "--assignment",
        "code",
        "--phase",
        "handoff",
        "--commit",
        &commit,
    ]);
    f.run(&[
        "mark",
        "--id",
        "integrator",
        "--session",
        "root",
        "--assignment",
        "code",
        "--phase",
        "integrating",
    ]);
    git(&f.project, &["cherry-pick", &commit]);
    f.run(&[
        "mark",
        "--id",
        "integrator",
        "--session",
        "root",
        "--assignment",
        "code",
        "--phase",
        "ci",
    ]);
    let mut waiting = f.waiting(&[
        "next",
        "--role",
        "worker",
        "--id",
        "worker",
        "--session",
        "worker-session",
    ]);
    let new_base = git(&f.project, &["rev-parse", "HEAD"]);
    f.assign("findings", &findings, &new_base, "root");
    assert_eq!(
        waiting
            .result()
            .state
            .active_assignment("worker")
            .unwrap()
            .base,
        new_base
    );
    f.cli(&[
        "claim",
        &findings.slug,
        "--worker",
        "worker",
        "--eta",
        "10m",
    ]);
    f.run(&[
        "worker",
        "--id",
        "worker",
        "--session",
        "worker-session",
        "--assignment",
        "findings",
        "--phase",
        "accepted",
    ]);
    f.cli(&[
        "edit",
        &findings.slug,
        "--note",
        "## Findings handoff\nSynthetic verification evidence.",
    ]);
    f.cli(&[
        "edit",
        &code.slug,
        "--note",
        "## Findings handoff\nEvidence for a different synthetic ticket.",
    ]);
    let wrong_note_id = f
        .store
        .read_ticket(&code.id)
        .unwrap()
        .notes
        .last()
        .unwrap()
        .id
        .to_string();
    let note_id = f
        .store
        .read_ticket(&findings.id)
        .unwrap()
        .notes
        .last()
        .unwrap()
        .id
        .to_string();
    f.cli(&[
        "edit",
        &findings.slug,
        "--note-kind",
        "activity",
        "--note-summary",
        "Synthetic activity evidence",
        "--note",
        "## Findings handoff\nSynthetic activity evidence.",
    ]);
    let activity_note_id = f
        .store
        .read_ticket(&findings.id)
        .unwrap()
        .notes
        .last()
        .unwrap()
        .id
        .to_string();
    let handoff = f.coord(&[
        "worker",
        "--id",
        "worker",
        "--session",
        "worker-session",
        "--assignment",
        "findings",
        "--phase",
        "handoff",
        "--findings-note",
        &note_id,
    ]);
    assert!(
        !f.command(&handoff.iter().map(String::as_str).collect::<Vec<_>>())
            .output()
            .unwrap()
            .status
            .success(),
        "a live worker claim cannot be bypassed"
    );
    f.cli(&["release", &findings.slug, "--worker", "worker"]);
    let wrong_handoff = f.coord(&[
        "worker",
        "--id",
        "worker",
        "--session",
        "worker-session",
        "--assignment",
        "findings",
        "--phase",
        "handoff",
        "--findings-note",
        &wrong_note_id,
    ]);
    assert!(
        !f.command(&wrong_handoff.iter().map(String::as_str).collect::<Vec<_>>())
            .output()
            .unwrap()
            .status
            .success(),
        "a note on another ticket cannot satisfy this handoff"
    );
    let activity_handoff = f.coord(&[
        "worker",
        "--id",
        "worker",
        "--session",
        "worker-session",
        "--assignment",
        "findings",
        "--phase",
        "handoff",
        "--findings-note",
        &activity_note_id,
    ]);
    assert!(
        !f.command(
            &activity_handoff
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
        )
        .output()
        .unwrap()
        .status
        .success(),
        "an activity note cannot satisfy a findings handoff"
    );
    let handed = f.run(&[
        "worker",
        "--id",
        "worker",
        "--session",
        "worker-session",
        "--assignment",
        "findings",
        "--phase",
        "handoff",
        "--findings-note",
        &note_id,
    ]);
    assert_eq!(handed.state.version, 3);
    assert!(handed.state.assignments["findings"].commit.is_none());
    assert_eq!(
        handed.state.assignments["code"].phase,
        hotsheet_cli::coordination::Phase::Ci
    );
    let integrating = f.coord(&[
        "mark",
        "--id",
        "integrator",
        "--session",
        "root",
        "--assignment",
        "findings",
        "--phase",
        "integrating",
    ]);
    assert!(
        !f.command(&integrating.iter().map(String::as_str).collect::<Vec<_>>())
            .output()
            .unwrap()
            .status
            .success()
    );
    let verifying = f.coord(&[
        "mark",
        "--id",
        "integrator",
        "--session",
        "root",
        "--assignment",
        "findings",
        "--phase",
        "verifying",
    ]);
    assert!(
        !f.command(&verifying.iter().map(String::as_str).collect::<Vec<_>>())
            .output()
            .unwrap()
            .status
            .success()
    );
    f.cli(&[
        "claim",
        &findings.slug,
        "--worker",
        "integrator",
        "--eta",
        "10m",
    ]);
    let set_findings_kind = |kind| {
        let mut ticket = f.store.read_ticket(&findings.id).unwrap();
        ticket
            .notes
            .iter_mut()
            .find(|note| note.id.to_string() == note_id)
            .unwrap()
            .kind = kind;
        f.store.write_ticket(&ticket).unwrap();
    };
    set_findings_kind(NoteKind::Activity);
    assert!(
        !f.command(&verifying.iter().map(String::as_str).collect::<Vec<_>>())
            .output()
            .unwrap()
            .status
            .success(),
        "verification must recheck the referenced note kind"
    );
    set_findings_kind(NoteKind::Regular);
    f.run(&[
        "mark",
        "--id",
        "integrator",
        "--session",
        "root",
        "--assignment",
        "findings",
        "--phase",
        "verifying",
    ]);
    f.cli(&["release", &findings.slug, "--worker", "integrator"]);
    assert!(
        !f.command(&verifying.iter().map(String::as_str).collect::<Vec<_>>())
            .output()
            .unwrap()
            .status
            .success(),
        "verification replay requires a live integrator claim after release"
    );
    f.cli(&[
        "claim",
        &findings.slug,
        "--worker",
        "replacement",
        "--eta",
        "10m",
    ]);
    assert!(
        !f.command(&verifying.iter().map(String::as_str).collect::<Vec<_>>())
            .output()
            .unwrap()
            .status
            .success(),
        "verification replay rejects a replacement claim holder"
    );
    f.cli(&["release", &findings.slug, "--worker", "replacement"]);
    f.cli(&[
        "claim",
        &findings.slug,
        "--worker",
        "integrator",
        "--eta",
        "10m",
    ]);
    let mut expired = f.store.read_ticket(&findings.id).unwrap();
    expired.claim_lease_expires_at = Some(Timestamp::from_datetime(
        time::OffsetDateTime::now_utc() - time::Duration::minutes(1),
    ));
    f.store.write_ticket(&expired).unwrap();
    assert!(
        !f.command(&verifying.iter().map(String::as_str).collect::<Vec<_>>())
            .output()
            .unwrap()
            .status
            .success(),
        "verification replay rejects an expired integrator claim"
    );
    f.cli(&[
        "claim",
        &findings.slug,
        "--worker",
        "integrator",
        "--eta",
        "10m",
    ]);
    f.run(&[
        "mark",
        "--id",
        "integrator",
        "--session",
        "root",
        "--assignment",
        "findings",
        "--phase",
        "verifying",
    ]);
    let done = f.coord(&[
        "mark",
        "--id",
        "integrator",
        "--session",
        "root",
        "--assignment",
        "findings",
        "--phase",
        "done",
    ]);
    assert!(
        !f.command(&done.iter().map(String::as_str).collect::<Vec<_>>())
            .output()
            .unwrap()
            .status
            .success()
    );
    f.cli(&[
        "edit",
        &findings.slug,
        "--status",
        "completed",
        "--note",
        "Synthetic findings verified.",
    ]);
    set_findings_kind(NoteKind::Activity);
    assert!(
        !f.command(&done.iter().map(String::as_str).collect::<Vec<_>>())
            .output()
            .unwrap()
            .status
            .success(),
        "completion must recheck the referenced note kind"
    );
    set_findings_kind(NoteKind::Regular);
    let completed = f.run(&[
        "mark",
        "--id",
        "integrator",
        "--session",
        "root",
        "--assignment",
        "findings",
        "--phase",
        "done",
    ]);
    assert_eq!(
        completed.state.assignments["findings"].phase,
        hotsheet_cli::coordination::Phase::Done
    );
    assert_eq!(
        completed.state.assignments["code"].phase,
        hotsheet_cli::coordination::Phase::Ci
    );
    assert_eq!(git(&f.project, &["rev-parse", "HEAD"]), new_base);
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

// HS2-CP6GWA: `next` re-registers its session from its own flags, so the role workflows pass
// the registered return route and availability lease explicitly on every wait. Synthetic
// route identifiers only.

const LONG_LEASE: &str = "3600";

fn now() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

/// The worker's durable session as `status` reports it.
fn worker(f: &Fixture, id: &str) -> Value {
    let args = f.coord(&["status"]);
    let status: Value =
        serde_json::from_str(&f.cli(&args.iter().map(String::as_str).collect::<Vec<_>>())).unwrap();
    status["state"]["workers"][id].clone()
}

fn route_of(session: &Value) -> (String, Option<String>) {
    let route = &session["return_route"];
    (
        route["kind"].as_str().unwrap_or_default().to_string(),
        route["recipient"].as_str().map(str::to_string),
    )
}

/// `next` as the role workflows prescribe it: the same route and lease as `register`.
fn prescribed_next<'a>(
    id: &'a str,
    session: &'a str,
    route: &'a str,
    lease: &'a str,
) -> Vec<&'a str> {
    vec![
        "next",
        "--role",
        "worker",
        "--id",
        id,
        "--session",
        session,
        "--route",
        route,
        "--lease-seconds",
        lease,
    ]
}

#[test]
fn hs2_cp6gwa_explicit_route_and_lease_survive_wait_and_reconnect() {
    let f = Fixture::new();
    for (id, route, kind, recipient) in [
        (
            "terminal-worker",
            "terminal:01SYNTHETICTERMINAL",
            "terminal",
            "01SYNTHETICTERMINAL",
        ),
        (
            "native-worker",
            "native:/root/synthetic-agent",
            "native_parent",
            "/root/synthetic-agent",
        ),
    ] {
        let expected = (kind.to_string(), Some(recipient.to_string()));
        // A long gate registers the long availability lease first.
        f.run(&[
            "register",
            "--role",
            "worker",
            "--id",
            id,
            "--session",
            "generation-one",
            "--route",
            route,
            "--lease-seconds",
            LONG_LEASE,
        ]);
        let registered = worker(&f, id);
        assert_eq!(route_of(&registered), expected, "{id} register");
        assert!(registered["expires_at"].as_i64().unwrap() - now() > 3_500);

        // The prescribed wait keeps both while it holds availability.
        let waiting = f.waiting(&prescribed_next(id, "generation-one", route, LONG_LEASE));
        let during = worker(&f, id);
        assert_eq!(route_of(&during), expected, "{id} during next");
        assert!(during["expires_at"].as_i64().unwrap() - now() > 3_500);
        assert_eq!(during["generation"], "generation-one");

        // A reconnect (the waiter ended and the session runs next again) keeps them too.
        drop(waiting);
        let reconnected = f.waiting(&prescribed_next(id, "generation-one", route, LONG_LEASE));
        let after = worker(&f, id);
        assert_eq!(route_of(&after), expected, "{id} after reconnect");
        assert!(after["expires_at"].as_i64().unwrap() - now() > 3_500);
        drop(reconnected);
    }
}

#[test]
fn hs2_cp6gwa_heartbeat_renewal_keeps_the_explicit_route() {
    let f = Fixture::new();
    let route = "terminal:01SYNTHETICTERMINAL";
    // The shortest accepted lease renews at half its length.
    let waiting = f.waiting(&prescribed_next("worker", "generation-one", route, "10"));
    let first = worker(&f, "worker")["expires_at"].as_i64().unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let renewed = loop {
        let session = worker(&f, "worker");
        if session["expires_at"].as_i64().unwrap() > first {
            break session;
        }
        assert!(Instant::now() < deadline, "availability was never renewed");
        std::thread::sleep(Duration::from_millis(250));
    };
    assert_eq!(
        route_of(&renewed),
        (
            "terminal".to_string(),
            Some("01SYNTHETICTERMINAL".to_string())
        )
    );
    drop(waiting);
}

#[test]
fn hs2_cp6gwa_next_without_flags_re_registers_with_cli_defaults() {
    // Why the workflows must repeat the flags: the public CLI keeps its defaults, so a bare
    // next silently replaces a verified route with human relay and the lease with 120 s.
    let f = Fixture::new();
    f.run(&[
        "register",
        "--role",
        "worker",
        "--id",
        "worker",
        "--session",
        "generation-one",
        "--route",
        "terminal:01SYNTHETICTERMINAL",
        "--lease-seconds",
        LONG_LEASE,
    ]);
    let waiting = f.waiting(&[
        "next",
        "--role",
        "worker",
        "--id",
        "worker",
        "--session",
        "generation-one",
    ]);
    let session = worker(&f, "worker");
    assert_eq!(route_of(&session), ("human_relay".to_string(), None));
    assert!(session["expires_at"].as_i64().unwrap() - now() <= 120);
    drop(waiting);
}
