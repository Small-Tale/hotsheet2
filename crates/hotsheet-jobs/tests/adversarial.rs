//! HS2-GCHX8V adversarial sequences: late and out-of-order messages, crashes mid-write,
//! repeated outcomes, lease expiry, cancellation races, retention and refill.

mod common;

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use common::{CHECKOUT, Fixture, LEASE_MS, LOST_AFTER_MS, acquire, external, fixture, new_job};
use hotsheet_jobs::{
    ActiveStatus, Capabilities, Change, EtaSource, ExternalRef, FeedLine, FinalStatus, JobError,
    JobUpdate, JournalProbe, Liveness, NewJob, NoProbe, Outcome, OwnerProbe, OwnerState,
    ProducerFamily, Status, Step, StepStatus, TicketLink,
};
use hotsheet_process_journal::{ExitReason, LaunchMeta, ProcessJournal, Purpose};

fn job_dir(fx: &Fixture, id: &str) -> PathBuf {
    fx.home.path().join("jobs").join(CHECKOUT).join(id)
}

fn summary(text: &str) -> Outcome {
    Outcome {
        summary: Some(text.into()),
        refs: BTreeMap::new(),
    }
}

fn report(seq: u64, value: &str) -> JobUpdate {
    JobUpdate {
        report_seq: Some(seq),
        latest: BTreeMap::from([("current".to_owned(), Some(value.to_owned()))]),
        ..JobUpdate::default()
    }
}

struct Always(OwnerState);

impl OwnerProbe for Always {
    fn owner_state(&self, _job: &hotsheet_jobs::Job) -> OwnerState {
        self.0
    }
}

#[test]
fn a_late_message_from_an_older_lease_cannot_overwrite_a_newer_one() {
    let fx = fixture();
    let job = fx.create(ProducerFamily::Watch);
    let (_, old) = fx
        .store
        .acquire(
            CHECKOUT,
            &job.id,
            acquire("monitor-a", ActiveStatus::Waiting),
        )
        .unwrap();
    fx.lose();
    assert_eq!(fx.get(&job.id).status, Status::Lost);

    let (reattached, new) = fx
        .store
        .acquire(
            CHECKOUT,
            &job.id,
            acquire("monitor-b", ActiveStatus::Waiting),
        )
        .unwrap();
    assert_eq!((old.generation, new.generation), (1, 2));
    assert_eq!(reattached.finished_ms, None);
    fx.store
        .update(CHECKOUT, &job.id, &new, report(1, "newer"))
        .unwrap();

    // Every kind of write from the old generation is refused and changes nothing.
    let before = fx.get(&job.id);
    let stale = |error: JobError| {
        assert!(
            matches!(
                error,
                JobError::StaleLease {
                    presented: 1,
                    current: 2
                }
            ),
            "{error}"
        );
    };
    stale(fx.store.renew(CHECKOUT, &job.id, &old).unwrap_err());
    stale(
        fx.store
            .update(CHECKOUT, &job.id, &old, report(9, "older"))
            .unwrap_err(),
    );
    stale(
        fx.store
            .finish(CHECKOUT, &job.id, &old, FinalStatus::Failed, summary("old"))
            .unwrap_err(),
    );
    stale(
        fx.store
            .confirm_cancel(CHECKOUT, &job.id, &old, summary("old"))
            .unwrap_err(),
    );
    assert_eq!(fx.get(&job.id), before);

    // The newer generation's outcome stands against a later old-generation outcome too.
    fx.store
        .finish(
            CHECKOUT,
            &job.id,
            &new,
            FinalStatus::Succeeded,
            summary("new"),
        )
        .unwrap();
    stale(
        fx.store
            .finish(CHECKOUT, &job.id, &old, FinalStatus::Failed, summary("old"))
            .unwrap_err(),
    );
    let done = fx.get(&job.id);
    assert_eq!(done.status, Status::Succeeded);
    assert_eq!(done.outcome, Some(summary("new")));
    assert_eq!(done.latest["current"], "newer");
}

#[test]
fn a_token_with_the_right_generation_but_another_owner_is_refused() {
    let fx = fixture();
    let job = fx.create(ProducerFamily::Owned);
    let mut token = fx.start(&job.id, "producer-a");
    token.owner = "producer-b".into();
    assert!(matches!(
        fx.store.renew(CHECKOUT, &job.id, &token).unwrap_err(),
        JobError::StaleLease {
            presented: 1,
            current: 1
        }
    ));
}

#[test]
fn a_stalled_watch_lease_can_be_taken_over_but_a_live_one_cannot() {
    let fx = fixture();
    let job = fx.create(ProducerFamily::Watch);
    let (_, first) = fx
        .store
        .acquire(
            CHECKOUT,
            &job.id,
            acquire("monitor-a", ActiveStatus::Waiting),
        )
        .unwrap();
    assert!(matches!(
        fx.store
            .acquire(
                CHECKOUT,
                &job.id,
                acquire("monitor-b", ActiveStatus::Waiting)
            )
            .unwrap_err(),
        JobError::LeaseHeld
    ));
    fx.clock.advance(LEASE_MS);
    assert_eq!(
        fx.get(&job.id).liveness(fx.store.now_ms()),
        Liveness::Stalled
    );
    let (taken, second) = fx
        .store
        .acquire(
            CHECKOUT,
            &job.id,
            acquire("monitor-b", ActiveStatus::Running),
        )
        .unwrap();
    assert_eq!((taken.status, second.generation), (Status::Running, 2));
    assert!(matches!(
        fx.store.renew(CHECKOUT, &job.id, &first).unwrap_err(),
        JobError::StaleLease { .. }
    ));

    // An owned Job has no remote truth: a stalled lease cannot be taken by someone else.
    let owned = fx.create(ProducerFamily::Owned);
    let token = fx.start(&owned.id, "producer-a");
    fx.clock.advance(LEASE_MS);
    assert!(matches!(
        fx.store
            .acquire(
                CHECKOUT,
                &owned.id,
                acquire("producer-b", ActiveStatus::Running)
            )
            .unwrap_err(),
        JobError::InvalidTransition { .. }
    ));
    fx.store.renew(CHECKOUT, &owned.id, &token).unwrap();
}

#[test]
fn a_watch_reattach_must_name_the_same_external_operation() {
    let fx = fixture();
    let (job, _) = fx.job_in(ProducerFamily::Watch, Status::Lost);
    let mut other = acquire("monitor-b", ActiveStatus::Waiting);
    other.external = Some(ExternalRef {
        system: "fake-ci".into(),
        id: "run-2".into(),
    });
    assert!(matches!(
        fx.store.acquire(CHECKOUT, &job.id, other).unwrap_err(),
        JobError::ExternalMismatch
    ));
    let mut unnamed = acquire("monitor-b", ActiveStatus::Waiting);
    unnamed.external = None;
    assert!(matches!(
        fx.store.acquire(CHECKOUT, &job.id, unnamed).unwrap_err(),
        JobError::ExternalMismatch
    ));
    assert_eq!(fx.get(&job.id).status, Status::Lost);

    // A watch Job that never recorded its operation cannot prove identity on reattach,
    // but the first acquire records what it is given.
    let mut anonymous = new_job(ProducerFamily::Watch);
    anonymous.external = None;
    let anonymous = fx.store.create(anonymous).unwrap();
    let (attached, _) = fx
        .store
        .acquire(
            CHECKOUT,
            &anonymous.id,
            acquire("monitor-a", ActiveStatus::Waiting),
        )
        .unwrap();
    assert_eq!(attached.external, Some(external()));
    let mut first_mismatch = new_job(ProducerFamily::Watch);
    first_mismatch.external = Some(ExternalRef {
        system: "fake-ci".into(),
        id: "run-9".into(),
    });
    let first_mismatch = fx.store.create(first_mismatch).unwrap();
    assert!(matches!(
        fx.store
            .acquire(
                CHECKOUT,
                &first_mismatch.id,
                acquire("m", ActiveStatus::Waiting)
            )
            .unwrap_err(),
        JobError::ExternalMismatch
    ));
}

#[test]
fn out_of_order_and_repeated_reports_are_refused() {
    let fx = fixture();
    let job = fx.create(ProducerFamily::Reported);
    let token = fx.start(&job.id, "producer-a");
    fx.store
        .update(CHECKOUT, &job.id, &token, report(5, "five"))
        .unwrap();
    for late in [5, 4, 0] {
        let error = fx
            .store
            .update(CHECKOUT, &job.id, &token, report(late, "late"))
            .unwrap_err();
        assert!(
            matches!(error, JobError::StaleReport { presented, last: 5 } if presented == late),
            "{error}"
        );
    }
    assert_eq!(fx.get(&job.id).latest["current"], "five");
    fx.store
        .update(CHECKOUT, &job.id, &token, report(6, "six"))
        .unwrap();
    // A report without a sequence (a heartbeat) is always accepted and keeps the mark.
    fx.store.renew(CHECKOUT, &job.id, &token).unwrap();
    let job = fx.get(&job.id);
    assert_eq!(job.latest["current"], "six");
    assert_eq!(job.lease.unwrap().last_report_seq, Some(6));
}

#[test]
fn a_crash_mid_write_leaves_the_last_good_snapshot_and_is_swept() {
    let fx = fixture();
    let job = fx.create(ProducerFamily::Owned);
    let token = fx.start(&job.id, "producer-a");
    let good = fx.get(&job.id);

    // A writer died after creating its temp file, and a create died before its snapshot.
    let dir = job_dir(&fx, &job.id);
    fs::write(dir.join(".tmpAbC123"), b"{\"version\":1,\"id\":\"partial").unwrap();
    fs::write(dir.parent().unwrap().join(".tmpZZZ999"), b"7").unwrap();
    let half_created = dir.parent().unwrap().join("01ARZ3NDEKTSV4RRFFQ69G5FAV");
    fs::create_dir(&half_created).unwrap();

    assert_eq!(fx.get(&job.id), good);
    let listing = fx.store.list(CHECKOUT).unwrap();
    assert_eq!(listing.jobs.len(), 1);
    assert!(listing.unreadable.is_empty());
    // The producer keeps working across the leftover files.
    fx.store.renew(CHECKOUT, &job.id, &token).unwrap();

    let result = fx.store.reconcile(CHECKOUT, &NoProbe).unwrap();
    assert_eq!(result.removed, ["01ARZ3NDEKTSV4RRFFQ69G5FAV"]);
    assert!(!dir.join(".tmpAbC123").exists());
    assert!(!dir.parent().unwrap().join(".tmpZZZ999").exists());
    assert!(!half_created.exists());
    assert_eq!(fx.get(&job.id).status, Status::Running);
}

#[test]
fn a_damaged_snapshot_is_reported_then_removed() {
    let fx = fixture();
    let job = fx.create(ProducerFamily::Owned);
    let keep = fx.create(ProducerFamily::Owned);
    fs::write(job_dir(&fx, &job.id).join("job.json"), b"{\"version\":1,").unwrap();
    assert!(matches!(
        fx.store.get(CHECKOUT, &job.id).unwrap_err(),
        JobError::Corrupt
    ));
    let listing = fx.store.list(CHECKOUT).unwrap();
    assert_eq!(listing.jobs.len(), 1);
    assert_eq!(listing.unreadable[0].reason, "damaged");
    let result = fx.store.reconcile(CHECKOUT, &NoProbe).unwrap();
    assert_eq!(result.removed, std::slice::from_ref(&job.id));
    assert_eq!(fx.store.list(CHECKOUT).unwrap().jobs[0].id, keep.id);

    // A snapshot whose identity disagrees with its directory is damaged too.
    let moved = fx.create(ProducerFamily::Owned);
    fs::copy(
        job_dir(&fx, &keep.id).join("job.json"),
        job_dir(&fx, &moved.id).join("job.json"),
    )
    .unwrap();
    assert!(matches!(
        fx.store.get(CHECKOUT, &moved.id).unwrap_err(),
        JobError::Corrupt
    ));
}

#[test]
fn an_unknown_record_version_is_rejected_and_never_rewritten() {
    let fx = fixture();
    let job = fx.create(ProducerFamily::Watch);
    let path = job_dir(&fx, &job.id).join("job.json");
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["version"] = 2.into();
    value["field_from_the_future"] = true.into();
    let newer = serde_json::to_vec(&value).unwrap();
    fs::write(&path, &newer).unwrap();

    assert!(matches!(
        fx.store.get(CHECKOUT, &job.id).unwrap_err(),
        JobError::UnsupportedVersion(2)
    ));
    assert!(matches!(
        fx.store
            .acquire(CHECKOUT, &job.id, acquire("m", ActiveStatus::Waiting))
            .unwrap_err(),
        JobError::UnsupportedVersion(2)
    ));
    assert!(matches!(
        fx.store.request_cancel(CHECKOUT, &job.id).unwrap_err(),
        JobError::UnsupportedVersion(2)
    ));
    assert_eq!(
        fx.store.list(CHECKOUT).unwrap().unreadable[0].reason,
        "unsupported_version"
    );
    fx.clock.advance(10_000_000);
    fx.store.reconcile(CHECKOUT, &NoProbe).unwrap();
    fx.create(ProducerFamily::Owned);
    assert_eq!(fs::read(&path).unwrap(), newer);
}

#[test]
fn a_repeated_finish_is_idempotent_only_when_identical() {
    let fx = fixture();
    let job = fx.create(ProducerFamily::Owned);
    let token = fx.start(&job.id, "producer-a");
    let first = fx
        .store
        .finish(
            CHECKOUT,
            &job.id,
            &token,
            FinalStatus::Succeeded,
            summary("ok"),
        )
        .unwrap();
    fx.clock.advance(50);
    let again = fx
        .store
        .finish(
            CHECKOUT,
            &job.id,
            &token,
            FinalStatus::Succeeded,
            summary("ok"),
        )
        .unwrap();
    assert_eq!(again, first, "an identical repeat writes nothing");

    for (status, text) in [
        (FinalStatus::Succeeded, "different"),
        (FinalStatus::Failed, "ok"),
    ] {
        let error = fx
            .store
            .finish(CHECKOUT, &job.id, &token, status, summary(text))
            .unwrap_err();
        assert!(
            matches!(error, JobError::InvalidTransition { .. }),
            "{error}"
        );
    }
    assert!(matches!(
        fx.store.renew(CHECKOUT, &job.id, &token).unwrap_err(),
        JobError::NotActive(Status::Succeeded)
    ));
    assert_eq!(fx.get(&job.id), first);
}

#[test]
fn lease_expiry_then_a_late_renew_revives_until_the_job_is_lost() {
    let fx = fixture();
    let job = fx.create(ProducerFamily::Owned);
    let token = fx.start(&job.id, "producer-a");
    let now = || fx.store.now_ms();
    assert_eq!(fx.get(&job.id).liveness(now()), Liveness::Live);

    fx.clock.advance(LEASE_MS - 1);
    assert_eq!(fx.get(&job.id).liveness(now()), Liveness::Live);
    fx.clock.advance(1);
    assert_eq!(fx.get(&job.id).liveness(now()), Liveness::Stalled);

    // Stalled, inside the grace window: reconciliation leaves it, a late renew revives it.
    fx.clock.advance(LOST_AFTER_MS - 1);
    assert!(
        fx.store
            .reconcile(CHECKOUT, &NoProbe)
            .unwrap()
            .lost
            .is_empty()
    );
    assert_eq!(fx.get(&job.id).status, Status::Running);
    fx.store.renew(CHECKOUT, &job.id, &token).unwrap();
    assert_eq!(fx.get(&job.id).liveness(now()), Liveness::Live);

    // Stalled again, and this time the window passes.
    fx.clock.advance(LEASE_MS + LOST_AFTER_MS);
    let result = fx.store.reconcile(CHECKOUT, &NoProbe).unwrap();
    assert_eq!(result.lost, std::slice::from_ref(&job.id));
    let lost = fx.get(&job.id);
    assert_eq!(
        (lost.status, lost.liveness(now())),
        (Status::Lost, Liveness::Finished)
    );
    assert!(matches!(
        fx.store.renew(CHECKOUT, &job.id, &token).unwrap_err(),
        JobError::NotActive(Status::Lost)
    ));
    // Reconciling again does not report the same loss twice.
    assert!(
        fx.store
            .reconcile(CHECKOUT, &NoProbe)
            .unwrap()
            .lost
            .is_empty()
    );
}

#[test]
fn a_late_outcome_lands_while_stalled_but_not_after_lost() {
    let fx = fixture();
    let stalled = fx.create(ProducerFamily::Reported);
    let token = fx.start(&stalled.id, "producer-a");
    fx.clock.advance(LEASE_MS + LOST_AFTER_MS - 1);
    let done = fx
        .store
        .finish(
            CHECKOUT,
            &stalled.id,
            &token,
            FinalStatus::Failed,
            summary("late"),
        )
        .unwrap();
    assert_eq!(done.status, Status::Failed);
    // An outcome is final: the expired lease no longer makes it lost.
    fx.clock.advance(LOST_AFTER_MS);
    assert!(
        fx.store
            .reconcile(CHECKOUT, &NoProbe)
            .unwrap()
            .lost
            .is_empty()
    );

    let (lost, token) = fx.job_in(ProducerFamily::Reported, Status::Lost);
    let error = fx
        .store
        .finish(
            CHECKOUT,
            &lost.id,
            &token.unwrap(),
            FinalStatus::Succeeded,
            summary("too late"),
        )
        .unwrap_err();
    assert!(matches!(
        error,
        JobError::InvalidTransition {
            from: Some(Status::Lost),
            to: Status::Succeeded,
            ..
        }
    ));
    assert_eq!(fx.get(&lost.id).outcome, None);
}

#[test]
fn the_owner_probe_decides_between_expiry_and_the_grace_window() {
    let fx = fixture();
    let job = fx.create(ProducerFamily::Owned);
    fx.start(&job.id, "producer-a");

    // Before expiry even a gone owner is not lost: the lease is the contract.
    assert!(
        fx.store
            .reconcile(CHECKOUT, &Always(OwnerState::Gone))
            .unwrap()
            .lost
            .is_empty()
    );
    fx.clock.advance(LEASE_MS + LOST_AFTER_MS * 10);
    assert!(
        fx.store
            .reconcile(CHECKOUT, &Always(OwnerState::Alive))
            .unwrap()
            .lost
            .is_empty()
    );
    assert_eq!(fx.get(&job.id).status, Status::Running);

    let quick = fx.create(ProducerFamily::Owned);
    fx.start(&quick.id, "producer-b");
    fx.clock.advance(LEASE_MS);
    let result = fx
        .store
        .reconcile(CHECKOUT, &Always(OwnerState::Gone))
        .unwrap();
    assert!(
        result.lost.contains(&quick.id),
        "gone owner is lost at expiry"
    );
}

#[test]
fn owned_liveness_follows_the_process_journal_launch_id() {
    let fx = fixture();
    let journal = ProcessJournal::at(fx.home.path());
    let launch = |pid: u32| {
        journal
            .start(LaunchMeta {
                pid,
                supervisor_pid: 1,
                supervisor: "jobs-test".into(),
                purpose: Purpose::ServerCommand,
                project_id: None,
                session_id: None,
                binary_revision: None,
            })
            .unwrap()
            .launch_id
    };
    // Two launches reuse one PID; only the launch id tells them apart.
    let ended = launch(4242);
    let running = launch(4242);
    journal.finish(&ended, ExitReason::Killed).unwrap();

    let mut ids = Vec::new();
    for launch_id in [Some(ended), Some(running), None] {
        let job = fx.create(ProducerFamily::Owned);
        let mut request = acquire("producer", ActiveStatus::Running);
        request.owner = format!("producer-{}", ids.len());
        request.launch_id = launch_id;
        fx.store.acquire(CHECKOUT, &job.id, request).unwrap();
        ids.push(job.id);
    }
    fx.clock.advance(LEASE_MS);
    let probe = JournalProbe::load(&journal).unwrap();
    let result = fx.store.reconcile(CHECKOUT, &probe).unwrap();
    assert_eq!(result.lost, [ids[0].clone()]);
    fx.clock.advance(LOST_AFTER_MS);
    let result = fx.store.reconcile(CHECKOUT, &probe).unwrap();
    assert_eq!(result.lost, [ids[1].clone(), ids[2].clone()]);
}

#[test]
fn cancellation_races_resolve_to_exactly_one_outcome() {
    let fx = fixture();

    // Request, then the producer finishes first: the outcome wins and cancel is refused.
    let job = fx.create(ProducerFamily::Owned);
    let token = fx.start(&job.id, "producer-a");
    let requested = fx.store.request_cancel(CHECKOUT, &job.id).unwrap();
    assert_eq!(
        requested.status,
        Status::Running,
        "a request is not a transition"
    );
    assert!(requested.cancel_requested_ms.is_some());
    let again = fx.store.request_cancel(CHECKOUT, &job.id).unwrap();
    assert_eq!(
        again.revision, requested.revision,
        "a repeated request is a no-op"
    );
    fx.store
        .finish(
            CHECKOUT,
            &job.id,
            &token,
            FinalStatus::Succeeded,
            summary("done"),
        )
        .unwrap();
    assert!(matches!(
        fx.store
            .confirm_cancel(CHECKOUT, &job.id, &token, summary("cancelled"))
            .unwrap_err(),
        JobError::InvalidTransition {
            from: Some(Status::Succeeded),
            to: Status::Cancelled,
            ..
        }
    ));
    assert!(matches!(
        fx.store.request_cancel(CHECKOUT, &job.id).unwrap_err(),
        JobError::NotActive(Status::Succeeded)
    ));
    assert_eq!(fx.get(&job.id).outcome, Some(summary("done")));

    // Request, then the producer confirms first: the late outcome is refused.
    let job = fx.create(ProducerFamily::Owned);
    let token = fx.start(&job.id, "producer-b");
    fx.store.request_cancel(CHECKOUT, &job.id).unwrap();
    let cancelled = fx
        .store
        .confirm_cancel(CHECKOUT, &job.id, &token, summary("stopped"))
        .unwrap();
    assert_eq!(cancelled.status, Status::Cancelled);
    assert!(matches!(
        fx.store
            .finish(
                CHECKOUT,
                &job.id,
                &token,
                FinalStatus::Succeeded,
                summary("done")
            )
            .unwrap_err(),
        JobError::InvalidTransition {
            from: Some(Status::Cancelled),
            ..
        }
    ));
    let repeat = fx
        .store
        .confirm_cancel(CHECKOUT, &job.id, &token, summary("stopped"))
        .unwrap();
    assert_eq!(repeat, cancelled);

    // Queued: nothing has started, so the request itself cancels, and a start loses.
    let job = fx.create(ProducerFamily::Reported);
    let cancelled = fx.store.request_cancel(CHECKOUT, &job.id).unwrap();
    assert_eq!(cancelled.status, Status::Cancelled);
    assert!(matches!(
        fx.store
            .acquire(CHECKOUT, &job.id, acquire("late", ActiveStatus::Running))
            .unwrap_err(),
        JobError::InvalidTransition {
            from: Some(Status::Cancelled),
            ..
        }
    ));

    // A lost Job has no producer to confirm a cancel.
    let (lost, _) = fx.job_in(ProducerFamily::Watch, Status::Lost);
    assert!(matches!(
        fx.store.request_cancel(CHECKOUT, &lost.id).unwrap_err(),
        JobError::NotActive(Status::Lost)
    ));
}

#[test]
fn a_failed_cancel_leaves_the_job_in_its_prior_state() {
    let fx = fixture();
    let job = fx.create(ProducerFamily::Watch);
    let (_, token) = fx
        .store
        .acquire(
            CHECKOUT,
            &job.id,
            acquire("monitor-a", ActiveStatus::Waiting),
        )
        .unwrap();
    fx.store.request_cancel(CHECKOUT, &job.id).unwrap();
    let update = JobUpdate {
        cancel_failed: Some("provider refused".into()),
        ..JobUpdate::default()
    };
    let job = fx.store.update(CHECKOUT, &job.id, &token, update).unwrap();
    assert_eq!(job.status, Status::Waiting);
    assert_eq!(job.cancel_requested_ms, None);
    assert_eq!(job.cancel_failure.as_deref(), Some("provider refused"));
    // A new request clears the old failure.
    let job = fx.store.request_cancel(CHECKOUT, &job.id).unwrap();
    assert!(job.cancel_requested_ms.is_some());
    assert_eq!(job.cancel_failure, None);
}

#[test]
fn undeclared_capabilities_are_absent_actions() {
    let fx = fixture();
    let mut plain = new_job(ProducerFamily::Owned);
    plain.capabilities = Capabilities::default();
    let job = fx.store.create(plain.clone()).unwrap();
    let token = fx.start(&job.id, "producer-a");
    let unsupported = |error: JobError, name: &str| {
        assert!(
            matches!(error, JobError::Unsupported(found) if found == name),
            "{error}"
        );
    };
    unsupported(
        fx.store.request_cancel(CHECKOUT, &job.id).unwrap_err(),
        "cancel",
    );
    let with_url = JobUpdate {
        url: Some("https://ci.example.test/run/1".into()),
        ..JobUpdate::default()
    };
    unsupported(
        fx.store
            .update(CHECKOUT, &job.id, &token, with_url)
            .unwrap_err(),
        "open_url",
    );
    plain.url = Some("https://ci.example.test/run/1".into());
    unsupported(fx.store.create(plain).unwrap_err(), "open_url");
    fx.store
        .finish(
            CHECKOUT,
            &job.id,
            &token,
            FinalStatus::Failed,
            Outcome::default(),
        )
        .unwrap();
    unsupported(fx.store.retry(CHECKOUT, &job.id).unwrap_err(), "retry");
    assert_eq!(fx.get(&job.id).status, Status::Failed);
}

#[test]
fn retry_makes_a_new_linked_job_and_keeps_the_old_outcome() {
    let fx = fixture();
    let mut new = new_job(ProducerFamily::Reported);
    new.ticket = Some(TicketLink {
        connection_id: "git".into(),
        native_id: "HS2-SYNTH1".into(),
    });
    let job = fx.store.create(new).unwrap();
    assert!(matches!(
        fx.store.retry(CHECKOUT, &job.id).unwrap_err(),
        JobError::Unfinished(Status::Queued)
    ));
    let token = fx.start(&job.id, "producer-a");
    fx.store
        .finish(
            CHECKOUT,
            &job.id,
            &token,
            FinalStatus::Failed,
            summary("red"),
        )
        .unwrap();
    let retried = fx.store.retry(CHECKOUT, &job.id).unwrap();
    assert_ne!(retried.id, job.id);
    assert_eq!(retried.retry_of.as_deref(), Some(job.id.as_str()));
    assert_eq!(retried.status, Status::Queued);
    assert_eq!(retried.ticket, fx.get(&job.id).ticket);
    assert_eq!(retried.job_key, job.job_key);
    let old = fx.get(&job.id);
    assert_eq!(
        (old.status, old.outcome),
        (Status::Failed, Some(summary("red")))
    );
}

#[test]
fn finished_jobs_leave_after_their_window_and_can_be_dismissed_sooner() {
    let fx = fixture();
    let config = fx.store.config();
    let (succeeded, _) = fx.job_in(ProducerFamily::Owned, Status::Succeeded);
    let (cancelled, _) = fx.job_in(ProducerFamily::Owned, Status::Cancelled);
    let (failed, _) = fx.job_in(ProducerFamily::Owned, Status::Failed);
    let queued = fx.create(ProducerFamily::Owned);

    fx.clock.advance(config.retain_succeeded_ms - 1);
    assert!(
        fx.store
            .reconcile(CHECKOUT, &NoProbe)
            .unwrap()
            .removed
            .is_empty()
    );
    fx.clock.advance(1);
    let mut removed = fx.store.reconcile(CHECKOUT, &NoProbe).unwrap().removed;
    removed.sort();
    let mut expected = vec![succeeded.id.clone(), cancelled.id.clone()];
    expected.sort();
    assert_eq!(removed, expected);
    assert!(matches!(
        fx.store.get(CHECKOUT, &succeeded.id).unwrap_err(),
        JobError::NotFound
    ));

    // A failure stays readable for its longer window; a queued Job never expires.
    fx.clock
        .advance(config.retain_failed_ms - config.retain_succeeded_ms - 1);
    assert!(
        fx.store
            .reconcile(CHECKOUT, &NoProbe)
            .unwrap()
            .removed
            .is_empty()
    );
    fx.clock.advance(1);
    assert_eq!(
        fx.store.reconcile(CHECKOUT, &NoProbe).unwrap().removed,
        std::slice::from_ref(&failed.id)
    );
    assert_eq!(fx.store.list(CHECKOUT).unwrap().jobs[0].id, queued.id);

    assert!(matches!(
        fx.store.dismiss(CHECKOUT, &queued.id).unwrap_err(),
        JobError::Unfinished(Status::Queued)
    ));
    let (done, _) = fx.job_in(ProducerFamily::Owned, Status::Failed);
    fx.store.dismiss(CHECKOUT, &done.id).unwrap();
    assert!(matches!(
        fx.store.dismiss(CHECKOUT, &done.id).unwrap_err(),
        JobError::NotFound
    ));
}

#[test]
fn the_hard_cap_evicts_old_finished_jobs_and_refuses_when_all_are_unfinished() {
    let fx = fixture();
    let cap = fx.store.config().max_jobs;
    let mut finished = Vec::new();
    for _ in 0..3 {
        finished.push(fx.job_in(ProducerFamily::Owned, Status::Failed).0.id);
        fx.clock.advance(10);
    }
    for _ in 3..cap {
        fx.create(ProducerFamily::Owned);
    }
    assert_eq!(fx.store.list(CHECKOUT).unwrap().jobs.len(), cap);

    // Each create over the cap evicts the oldest finished Job, well inside its window.
    for evicted in &finished {
        fx.create(ProducerFamily::Owned);
        let ids: Vec<String> = fx
            .store
            .list(CHECKOUT)
            .unwrap()
            .jobs
            .into_iter()
            .map(|job| job.id)
            .collect();
        assert_eq!(ids.len(), cap);
        assert!(!ids.contains(evicted));
    }
    assert!(matches!(
        fx.store.create(new_job(ProducerFamily::Owned)).unwrap_err(),
        JobError::CheckoutFull
    ));
    assert_eq!(fx.store.list(CHECKOUT).unwrap().jobs.len(), cap);
}

#[test]
fn an_emptied_checkout_refills_and_keeps_its_history_and_revision() {
    let fx = fixture();
    let config = fx.store.config();
    assert_eq!(fx.store.revision(CHECKOUT).unwrap(), 0);
    assert!(fx.store.list(CHECKOUT).unwrap().jobs.is_empty());
    assert!(fx.store.checkouts().unwrap().is_empty());

    // Two successes are not enough history; the third gives a median.
    for (round, duration) in [4_000_u64, 1_000, 2_000].into_iter().enumerate() {
        let job = fx.create(ProducerFamily::Owned);
        let token = fx.start(&job.id, "producer");
        assert_eq!(fx.get(&job.id).history_estimate_ms, None, "round {round}");
        assert_eq!(fx.get(&job.id).eta(), None);
        fx.clock.advance(duration);
        fx.store
            .finish(
                CHECKOUT,
                &job.id,
                &token,
                FinalStatus::Succeeded,
                Outcome::default(),
            )
            .unwrap();
    }
    // A failure contributes no duration.
    let failed = fx.create(ProducerFamily::Owned);
    let token = fx.start(&failed.id, "producer");
    fx.clock.advance(90_000);
    fx.store
        .finish(
            CHECKOUT,
            &failed.id,
            &token,
            FinalStatus::Failed,
            Outcome::default(),
        )
        .unwrap();

    fx.clock.advance(config.retain_failed_ms);
    let before_empty = fx.store.revision(CHECKOUT).unwrap();
    assert_eq!(
        fx.store
            .reconcile(CHECKOUT, &NoProbe)
            .unwrap()
            .removed
            .len(),
        4
    );
    assert!(fx.store.list(CHECKOUT).unwrap().jobs.is_empty());
    assert!(fx.store.revision(CHECKOUT).unwrap() > before_empty);
    assert_eq!(fx.store.checkouts().unwrap(), [CHECKOUT]);

    // Refill: a new Job gets a fresh id, the surviving history and a growing revision.
    let emptied = fx.store.revision(CHECKOUT).unwrap();
    let job = fx.create(ProducerFamily::Owned);
    let token = fx.start(&job.id, "producer");
    let job = fx.get(&job.id);
    assert_eq!(job.history_estimate_ms, Some(2_000));
    let eta = job.eta().unwrap();
    assert_eq!(eta.source, EtaSource::History);
    assert_eq!(eta.finish_at_ms, job.started_ms.unwrap() + 2_000);
    assert!(fx.store.revision(CHECKOUT).unwrap() > emptied);

    // A producer ETA overrides history until it is cleared; a finished Job has none.
    let set = JobUpdate {
        eta: Change::Set(42),
        ..JobUpdate::default()
    };
    let job = fx.store.update(CHECKOUT, &job.id, &token, set).unwrap();
    assert_eq!(job.eta().unwrap().source, EtaSource::Producer);
    assert_eq!(job.eta().unwrap().finish_at_ms, 42);
    let clear = JobUpdate {
        eta: Change::Clear,
        ..JobUpdate::default()
    };
    let job = fx.store.update(CHECKOUT, &job.id, &token, clear).unwrap();
    assert_eq!(job.eta().unwrap().source, EtaSource::History);
    let job = fx
        .store
        .finish(
            CHECKOUT,
            &job.id,
            &token,
            FinalStatus::Succeeded,
            Outcome::default(),
        )
        .unwrap();
    assert_eq!(job.eta(), None);

    // Another job key has no history of its own.
    let mut other = new_job(ProducerFamily::Owned);
    other.job_key = "fixture:other".into();
    let other = fx.store.create(other).unwrap();
    fx.start(&other.id, "producer");
    assert_eq!(fx.get(&other.id).history_estimate_ms, None);
}

#[test]
fn history_is_bounded_and_survives_damage() {
    let fx = fixture();
    let run = |duration: u64| {
        let job = fx.create(ProducerFamily::Owned);
        let token = fx.start(&job.id, "producer");
        fx.clock.advance(duration);
        fx.store
            .finish(
                CHECKOUT,
                &job.id,
                &token,
                FinalStatus::Succeeded,
                Outcome::default(),
            )
            .unwrap();
        fx.store.dismiss(CHECKOUT, &job.id).unwrap();
    };
    // Twenty old slow runs, then sixteen fast ones: only the newest sixteen count.
    for _ in 0..20 {
        run(9_000);
    }
    for _ in 0..16 {
        run(100);
    }
    assert_eq!(
        fx.store.history_estimate(CHECKOUT, "fixture:key"),
        Some(100)
    );
    // An even sample count averages the middle pair.
    let history = fx
        .home
        .path()
        .join("jobs")
        .join(CHECKOUT)
        .join("history.json");
    let even = serde_json::json!({
        "version": 1,
        "keys": { "fixture:key": { "updated_ms": 1, "durations_ms": [10, 40, 20, 31] } }
    });
    fs::write(&history, serde_json::to_vec(&even).unwrap()).unwrap();
    assert_eq!(fx.store.history_estimate(CHECKOUT, "fixture:key"), Some(25));

    // Damage gives no estimate and is replaced by the next success.
    fs::write(&history, b"not json").unwrap();
    assert_eq!(fx.store.history_estimate(CHECKOUT, "fixture:key"), None);
    run(300);
    run(300);
    assert_eq!(fx.store.history_estimate(CHECKOUT, "fixture:key"), None);
    run(300);
    assert_eq!(
        fx.store.history_estimate(CHECKOUT, "fixture:key"),
        Some(300)
    );

    // A newer history format is left alone.
    let newer = br#"{"version":2,"keys":{},"extra":true}"#;
    fs::write(&history, newer).unwrap();
    run(300);
    assert_eq!(fs::read(&history).unwrap(), newer);
    assert_eq!(fx.store.history_estimate(CHECKOUT, "fixture:key"), None);
}

#[test]
fn steps_latest_values_and_progress_round_trip_through_the_snapshot() {
    let fx = fixture();
    let job = fx.create(ProducerFamily::Owned);
    let token = fx.start(&job.id, "producer-a");
    assert_eq!(fx.get(&job.id).progress(), None);

    let mut build = Step::new("build", "Build");
    build.status = StepStatus::Succeeded;
    let mut tests = Step::new("tests", "Tests");
    tests.status = StepStatus::Active;
    tests.done = Some(1);
    tests.total = Some(4);
    tests.unit = Some("tests".into());
    tests.weight = 3;
    let update = JobUpdate {
        steps: Some(vec![build, tests]),
        latest: BTreeMap::from([("current_test".to_owned(), Some("alpha".to_owned()))]),
        ..JobUpdate::default()
    };
    let job = fx.store.update(CHECKOUT, &job.id, &token, update).unwrap();
    let progress = job.progress().unwrap();
    assert!((progress.fraction - (1.0 + 0.75) / 4.0).abs() < 1e-9);
    assert_eq!(fx.get(&job.id), job);

    // An invalid tree is refused as a whole and the previous tree stays.
    let mut bad = Step::new("build", "Build");
    bad.total = Some(0);
    let update = JobUpdate {
        steps: Some(vec![bad]),
        ..JobUpdate::default()
    };
    assert!(matches!(
        fx.store
            .update(CHECKOUT, &job.id, &token, update)
            .unwrap_err(),
        JobError::Invalid("steps.total")
    ));
    assert_eq!(fx.get(&job.id).steps, job.steps);

    // Latest values are a small bounded map; `None` removes a key.
    let remove = JobUpdate {
        latest: BTreeMap::from([("current_test".to_owned(), None)]),
        ..JobUpdate::default()
    };
    assert!(
        fx.store
            .update(CHECKOUT, &job.id, &token, remove)
            .unwrap()
            .latest
            .is_empty()
    );
    let too_many = JobUpdate {
        latest: (0..9)
            .map(|index| (format!("k{index}"), Some("v".to_owned())))
            .collect(),
        ..JobUpdate::default()
    };
    assert!(matches!(
        fx.store
            .update(CHECKOUT, &job.id, &token, too_many)
            .unwrap_err(),
        JobError::Invalid("latest")
    ));
    assert!(fx.get(&job.id).latest.is_empty());
}

#[test]
fn the_feed_stays_out_of_the_snapshot_and_reports_gaps_after_compaction() {
    let fx = fixture();
    let job = fx.create(ProducerFamily::Owned);
    let token = fx.start(&job.id, "producer-a");
    let marker = "synthetic-feed-marker";
    let line = format!("{marker} {}", "z".repeat(400));
    let mut cursor = 0;
    let mut seen = 0;
    for _ in 0..20 {
        let update = JobUpdate {
            feed: vec![FeedLine::out(line.clone()); 5],
            ..JobUpdate::default()
        };
        fx.store.update(CHECKOUT, &job.id, &token, update).unwrap();
        // A reader that keeps up never sees a gap.
        let page = fx.store.read_feed(CHECKOUT, &job.id, cursor, 100).unwrap();
        assert_eq!(page.gap, None);
        seen += page.entries.len();
        cursor = page.cursor;
    }
    assert_eq!((seen, cursor), (100, 100));

    let dir = job_dir(&fx, &job.id);
    assert!(fs::metadata(dir.join("feed.jsonl")).unwrap().len() <= 16 * 1024);
    let snapshot = fs::read_to_string(dir.join("job.json")).unwrap();
    assert!(
        !snapshot.contains(marker),
        "feed text must stay out of job.json"
    );

    // A reader that starts late gets one explicit gap, then the retained tail in order.
    let job = fx.get(&job.id);
    assert_eq!(job.feed.next_seq, 101);
    let late = fx.store.read_feed(CHECKOUT, &job.id, 0, 1_000).unwrap();
    let gap = late.gap.unwrap();
    assert_eq!((gap.from_seq, gap.to_seq), (1, job.feed.first_seq - 1));
    assert_eq!(late.entries[0].seq, job.feed.first_seq);
    assert_eq!(late.cursor, 100);
    assert!(
        late.entries
            .windows(2)
            .all(|pair| pair[1].seq == pair[0].seq + 1)
    );

    let too_many = JobUpdate {
        feed: vec![FeedLine::info("x"); 257],
        ..JobUpdate::default()
    };
    assert!(matches!(
        fx.store
            .update(CHECKOUT, &job.id, &token, too_many)
            .unwrap_err(),
        JobError::Invalid("feed")
    ));
}

#[test]
fn identifiers_that_could_leave_the_store_are_refused_without_being_echoed() {
    let fx = fixture();
    for checkout in [
        "",
        ".",
        "..",
        "../x",
        "a/b",
        "a\\b",
        ".hidden",
        "a:b",
        "caf\u{e9}",
    ] {
        let mut new = new_job(ProducerFamily::Owned);
        new.checkout_id = checkout.into();
        let error = fx.store.create(new).unwrap_err();
        assert!(
            matches!(error, JobError::Invalid("checkout_id")),
            "{checkout:?}"
        );
        assert!(fx.store.list(checkout).is_err());
        assert!(fx.store.checkout_dir(checkout).is_err());
    }
    for id in [
        "",
        "..",
        "not-a-ulid",
        "01arz3ndektsv4rrffq69g5fav",
        "../../x",
    ] {
        assert!(
            matches!(
                fx.store.get(CHECKOUT, id).unwrap_err(),
                JobError::Invalid("job_id")
            ),
            "{id:?}"
        );
        assert!(fx.store.dismiss(CHECKOUT, id).is_err());
        assert!(fx.store.read_feed(CHECKOUT, id, 0, 1).is_err());
    }
    assert!(
        !fx.home.path().join("jobs").exists(),
        "a refused write creates nothing"
    );

    let secret = "private\u{7}title";
    let mut new = new_job(ProducerFamily::Owned);
    new.title = secret.into();
    let error = fx.store.create(new).unwrap_err();
    assert_eq!(error.to_string(), "invalid job field: title");

    type Change = fn(&mut NewJob);
    let cases: [(&str, Change); 6] = [
        ("title", |new| new.title = "t".repeat(201)),
        ("job_key", |new| new.job_key = String::new()),
        ("kind.name", |new| new.kind.name = "has space".into()),
        ("job_id", |new| new.retry_of = Some("nope".into())),
        ("url", |new| new.url = Some("file:///etc/hosts".into())),
        ("ticket.native_id", |new| {
            new.ticket = Some(TicketLink {
                connection_id: "git".into(),
                native_id: String::new(),
            });
        }),
    ];
    for (field, change) in cases {
        let mut new = new_job(ProducerFamily::Owned);
        change(&mut new);
        match fx.store.create(new).unwrap_err() {
            JobError::Invalid(found) => assert_eq!(found, field),
            other => panic!("expected {field}, got {other}"),
        }
    }
    assert!(matches!(
        fx.store
            .get(CHECKOUT, "01ARZ3NDEKTSV4RRFFQ69G5FAV")
            .unwrap_err(),
        JobError::NotFound
    ));
    assert!(matches!(
        fx.store
            .request_cancel(CHECKOUT, "01ARZ3NDEKTSV4RRFFQ69G5FAV")
            .unwrap_err(),
        JobError::NotFound
    ));
}

#[test]
fn a_ticket_link_is_qualified_and_an_outcome_carries_only_bounded_identifiers() {
    let fx = fixture();
    let mut new = new_job(ProducerFamily::Watch);
    new.ticket = Some(TicketLink {
        connection_id: "github-main".into(),
        native_id: "1234".into(),
    });
    new.url = Some("https://ci.example.test/run/1".into());
    let job = fx.store.create(new).unwrap();
    let stored = fx.get(&job.id);
    let ticket = stored.ticket.unwrap();
    assert_eq!(
        (ticket.connection_id.as_str(), ticket.native_id.as_str()),
        ("github-main", "1234")
    );
    assert_eq!(stored.url.as_deref(), Some("https://ci.example.test/run/1"));

    let (_, token) = fx
        .store
        .acquire(
            CHECKOUT,
            &job.id,
            acquire("monitor-a", ActiveStatus::Waiting),
        )
        .unwrap();
    let mut outcome = summary("run 1 passed");
    outcome.refs.insert("run_id".into(), "1".into());
    let too_many = Outcome {
        summary: None,
        refs: (0..17).map(|i| (format!("k{i}"), "v".to_owned())).collect(),
    };
    assert!(matches!(
        fx.store
            .finish(CHECKOUT, &job.id, &token, FinalStatus::Succeeded, too_many)
            .unwrap_err(),
        JobError::Invalid("outcome.refs")
    ));
    let done = fx
        .store
        .finish(
            CHECKOUT,
            &job.id,
            &token,
            FinalStatus::Succeeded,
            outcome.clone(),
        )
        .unwrap();
    assert_eq!(done.outcome, Some(outcome));
    assert!(done.finished_ms.is_some());
}

#[test]
fn an_observed_cancellation_needs_no_cancel_capability() {
    let fx = fixture();
    let mut plain = new_job(ProducerFamily::Watch);
    plain.capabilities = Capabilities::default();
    let job = fx.store.create(plain).unwrap();
    let (_, token) = fx
        .store
        .acquire(
            CHECKOUT,
            &job.id,
            acquire("monitor-a", ActiveStatus::Waiting),
        )
        .unwrap();
    let done = fx
        .store
        .confirm_cancel(CHECKOUT, &job.id, &token, summary("cancelled upstream"))
        .unwrap();
    assert_eq!(done.status, Status::Cancelled);
}

#[test]
fn stop_watching_is_a_local_action_distinct_from_cancel() {
    let fx = fixture();
    // Offered for every watch Job, even one without the cancel capability.
    let mut plain = new_job(ProducerFamily::Watch);
    plain.capabilities = Capabilities::default();
    let job = fx.store.create(plain.clone()).unwrap();
    let (_, token) = fx
        .store
        .acquire(
            CHECKOUT,
            &job.id,
            acquire("monitor-a", ActiveStatus::Waiting),
        )
        .unwrap();
    let requested = fx.store.stop_watching(CHECKOUT, &job.id).unwrap();
    assert_eq!(requested.status, Status::Waiting);
    assert!(requested.stop_watching_requested_ms.is_some());
    assert_eq!(requested.cancel_requested_ms, None);
    let again = fx.store.stop_watching(CHECKOUT, &job.id).unwrap();
    assert_eq!(again.revision, requested.revision);
    let stopped = fx
        .store
        .confirm_cancel(
            CHECKOUT,
            &job.id,
            &token,
            hotsheet_jobs::stopped_watching_outcome(),
        )
        .unwrap();
    assert_eq!(stopped.status, Status::Cancelled);
    assert_eq!(stopped.outcome.unwrap().refs["cancel"], "stopped_watching");
    assert!(matches!(
        fx.store.stop_watching(CHECKOUT, &job.id).unwrap_err(),
        JobError::NotActive(Status::Cancelled)
    ));

    // Queued: nothing is observing yet, so it ends at once.
    let queued = fx.store.create(plain).unwrap();
    let stopped = fx.store.stop_watching(CHECKOUT, &queued.id).unwrap();
    assert_eq!(stopped.status, Status::Cancelled);
    assert_eq!(stopped.outcome.unwrap().refs["cancel"], "stopped_watching");

    // Lost: no monitor can confirm. Other families have no such action.
    let (lost, _) = fx.job_in(ProducerFamily::Watch, Status::Lost);
    assert!(matches!(
        fx.store.stop_watching(CHECKOUT, &lost.id).unwrap_err(),
        JobError::NotActive(Status::Lost)
    ));
    for family in [ProducerFamily::Owned, ProducerFamily::Reported] {
        let job = fx.create(family);
        assert!(matches!(
            fx.store.stop_watching(CHECKOUT, &job.id).unwrap_err(),
            JobError::Unsupported("stop_watching")
        ));
    }
}

#[test]
fn a_released_lease_reads_as_stalled_and_hands_a_watch_job_over_at_once() {
    let fx = fixture();
    let job = fx.create(ProducerFamily::Watch);
    let (_, first) = fx
        .store
        .acquire(
            CHECKOUT,
            &job.id,
            acquire("monitor-a", ActiveStatus::Waiting),
        )
        .unwrap();
    let released = fx.store.release(CHECKOUT, &job.id, &first).unwrap();
    assert_eq!(released.status, Status::Waiting);
    assert_eq!(released.liveness(fx.store.now_ms()), Liveness::Stalled);
    let again = fx.store.release(CHECKOUT, &job.id, &first).unwrap();
    assert_eq!(again.revision, released.revision);

    let (_, second) = fx
        .store
        .acquire(
            CHECKOUT,
            &job.id,
            acquire("monitor-b", ActiveStatus::Waiting),
        )
        .unwrap();
    assert_eq!(second.generation, 2);
    assert!(matches!(
        fx.store.release(CHECKOUT, &job.id, &first).unwrap_err(),
        JobError::StaleLease { .. }
    ));
    // The releasing producer may also change its mind and renew, while nobody took over.
    let other = fx.create(ProducerFamily::Owned);
    let token = fx.start(&other.id, "producer-a");
    fx.store.release(CHECKOUT, &other.id, &token).unwrap();
    let renewed = fx.store.renew(CHECKOUT, &other.id, &token).unwrap();
    assert_eq!(renewed.liveness(fx.store.now_ms()), Liveness::Live);
    fx.store
        .finish(
            CHECKOUT,
            &other.id,
            &token,
            FinalStatus::Succeeded,
            Outcome::default(),
        )
        .unwrap();
    assert!(matches!(
        fx.store.release(CHECKOUT, &other.id, &token).unwrap_err(),
        JobError::NotActive(Status::Succeeded)
    ));
}
