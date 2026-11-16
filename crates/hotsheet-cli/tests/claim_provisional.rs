//! HS2-XYF4YT: separate-process `claim`, `claim-next` and `renew` of a dependent whose
//! prerequisite awaits CI. Off, the answer is unchanged; on, it names the prerequisite
//! and its gate commit, and a red gate is reported once per claim. Gate readings come
//! from the machine's CI watch Job store the way the monitor records them; no CI
//! provider is contacted. Synthetic tickets and commits only.

#[allow(dead_code)] // HS2-XYF4YT: shared process fixture includes unrelated helpers.
#[path = "support/coordination.rs"]
mod support;

use std::path::Path;

use hotsheet_jobs::{
    Acquire, ActiveStatus, CiTarget, CiWatchOptions, FinalStatus, JobStore, Outcome,
};
use hotsheet_model::{NoteKind, StartedPhase, Status, Ticket, Timestamp, Ulid};
use hotsheet_ticketing::provisional::SpeculationPolicy;
use hotsheet_ticketing::{NewTicket, Settings, checkouts, ops};
use support::{Fixture, git};

fn now() -> Timestamp {
    Timestamp::from_datetime(time::OffsetDateTime::now_utc())
}

fn run(f: &Fixture, home: &Path, args: &[&str]) -> String {
    let out = f
        .command(args)
        .current_dir(&f.project)
        .env("HOTSHEET_HOME", home)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "CLI failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

fn awaiting_ci(f: &Fixture, commit: &str) -> Ticket {
    let created = f.ticket();
    let mut ticket = f.store.read_ticket(&created.id).unwrap();
    ticket.status = Status::Started;
    ticket.started_phase = Some(StartedPhase::FinalTesting);
    ticket.up_next = false;
    f.store.write_ticket(&ticket).unwrap();
    ops::add_note(
        &f.store,
        &ticket.id,
        Ulid::new(),
        now(),
        NoteKind::Regular,
        format!("Pushed; waiting on CI.\n\nCI gate commit: {commit}\n"),
    )
    .unwrap();
    f.store.read_ticket(&created.id).unwrap()
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

/// Record the red verdict a monitor writes when the gate's run fails.
fn judge_red(home: &Path, checkout: &str, job_id: &str, head: &str) {
    let store = JobStore::at(home);
    let (_, token) = store
        .acquire(
            checkout,
            job_id,
            Acquire {
                owner: "monitor:test".into(),
                status: ActiveStatus::Running,
                launch_id: None,
                external: None,
            },
        )
        .unwrap();
    let refs = [
        ("gate", "red"),
        ("run_id", "9001"),
        ("run_attempt", "1"),
        ("run_head_sha", head),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value.to_owned()))
    .collect();
    store
        .finish(
            checkout,
            job_id,
            &token,
            FinalStatus::Failed,
            Outcome {
                summary: None,
                refs,
            },
        )
        .unwrap();
}

#[test]
fn hs2_xyf4yt_cli_claim_and_renew_name_a_prerequisite_awaiting_ci() {
    let f = Fixture::new();
    let home = f.root.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let commit = git(&f.project, &["rev-parse", "HEAD"]);
    let blocker = awaiting_ci(&f, &commit);
    let first = dependent(&f, &blocker);
    let slug = first.slug.as_str();
    let checkout = checkouts::checkout_id(&f.project).unwrap();
    let jobs = JobStore::at(&home);
    let watch = hotsheet_jobs::ensure_ci_watch(
        &jobs,
        &checkout,
        &CiTarget::new("synthetic/repo", &commit, "CI", "main").unwrap(),
        &CiWatchOptions::default(),
    )
    .unwrap();

    // Off: the claim answer is exactly the two lines it always was.
    let off = run(&f, &home, &["claim", slug, "--worker", "agent"]);
    assert_eq!(
        off.lines().collect::<Vec<_>>(),
        vec![
            format!("Claimed {slug} for agent (lease 30m)").as_str(),
            format!(
                "Phase: analyzing. Record each transition as it happens with `edit {slug} --started-phase <phase>` \
                 (set initial_testing before running tests, integrating before merging/pushing, \
                 final_testing while waiting on CI)."
            )
            .as_str(),
        ]
    );
    let renew_off = run(&f, &home, &["renew", slug, "--worker", "agent"]);
    assert!(!renew_off.contains("Prerequisite"), "{renew_off}");
    assert!(!home.join("provisional-claim-warnings.json").exists());

    // On: claim and every renew name the prerequisite, its commit and the rule.
    SpeculationPolicy {
        enabled: true,
        base_branch: Some("main".into()),
        ..SpeculationPolicy::default()
    }
    .write(&Settings::for_project(&f.project))
    .unwrap();
    for _ in 0..2 {
        let pending = run(&f, &home, &["renew", slug, "--worker", "agent"]);
        assert!(
            pending.contains(&format!(
                "Prerequisite {} awaits CI (pending): build on a base that contains its CI gate commit {commit}; integrate push-per-ticket",
                blocker.slug
            )),
            "{pending}"
        );
        assert!(!pending.contains("Warning"), "{pending}");
    }

    // Red: one warning per claim; a repeat renew is quiet.
    judge_red(&home, &checkout, &watch.id, &commit);
    let red = run(&f, &home, &["renew", slug, "--worker", "agent"]);
    assert!(
        red.contains(&format!(
            "Warning: the CI gate of prerequisite {} reads red (red run 9001.1) at {commit}",
            blocker.slug
        )),
        "{red}"
    );
    let quiet = run(&f, &home, &["renew", slug, "--worker", "agent"]);
    assert!(
        !quiet.contains("Warning") && !quiet.contains("Prerequisite"),
        "{quiet}"
    );

    // claim-next of another dependent warns once for its own claim.
    run(&f, &home, &["release", slug, "--worker", "agent"]);
    let mut done = f.store.read_ticket(&first.id).unwrap();
    done.up_next = false;
    f.store.write_ticket(&done).unwrap();
    let second = dependent(&f, &blocker);
    let next = run(&f, &home, &["claim-next", "--worker", "agent"]);
    assert!(next.contains(&format!("Claimed {}", second.slug)), "{next}");
    assert!(
        next.contains("Warning: the CI gate of prerequisite"),
        "{next}"
    );
    let quiet = run(&f, &home, &["renew", &second.slug, "--worker", "agent"]);
    assert!(!quiet.contains("Warning"), "{quiet}");

    // Closed prerequisite: confirmed, nothing more to say.
    let mut closed = f.store.read_ticket(&blocker.id).unwrap();
    closed.status = Status::Completed;
    f.store.write_ticket(&closed).unwrap();
    let confirmed = run(&f, &home, &["renew", &second.slug, "--worker", "agent"]);
    assert!(
        !confirmed.contains("Prerequisite") && !confirmed.contains("Warning"),
        "{confirmed}"
    );
}
