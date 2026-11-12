//! HS2-GCHX8V transition matrix through the real store: every (from, to, family) pair of
//! `docs/33-jobs.md` §33.2 is attempted with the API a caller would use. A listed edge
//! lands; an unlisted edge is refused and leaves the snapshot byte-identical.

mod common;

use common::{CHECKOUT, Fixture, acquire, fixture};
use hotsheet_jobs::{
    ActiveStatus, FinalStatus, Job, JobError, JobUpdate, LeaseToken, NoProbe, Outcome,
    ProducerFamily, Status, transition_allowed,
};

fn snapshot_bytes(fx: &Fixture, id: &str) -> Vec<u8> {
    let path = fx
        .home
        .path()
        .join("jobs")
        .join(CHECKOUT)
        .join(id)
        .join("job.json");
    std::fs::read(path).unwrap()
}

fn stale_token() -> LeaseToken {
    LeaseToken {
        owner: "producer-a".into(),
        generation: 1,
    }
}

/// Every way a caller could try to move `job` to `to`. Each returns the store's answer.
fn attempts(
    fx: &Fixture,
    job: &Job,
    token: Option<&LeaseToken>,
    to: Status,
) -> Vec<Result<Job, JobError>> {
    let id = job.id.as_str();
    let held = token.cloned().unwrap_or_else(stale_token);
    match to {
        // Nothing moves a Job back to queued: a retry creates a different Job.
        Status::Queued => Vec::new(),
        Status::Running | Status::Waiting => {
            let active = if to == Status::Running {
                ActiveStatus::Running
            } else {
                ActiveStatus::Waiting
            };
            let update = JobUpdate {
                status: Some(active),
                ..JobUpdate::default()
            };
            let by_update = fx.store.update(CHECKOUT, id, &held, update);
            if job.status.is_active() {
                // A second producer cannot take a live lease, so the update is the only path.
                vec![by_update]
            } else {
                let by_acquire = fx
                    .store
                    .acquire(CHECKOUT, id, acquire("producer-b", active))
                    .map(|(job, _)| job);
                vec![by_update, by_acquire]
            }
        }
        Status::Succeeded | Status::Failed => {
            let outcome = if to == Status::Succeeded {
                FinalStatus::Succeeded
            } else {
                FinalStatus::Failed
            };
            let summary = Outcome {
                summary: Some("attempted".into()),
                ..Outcome::default()
            };
            vec![fx.store.finish(CHECKOUT, id, &held, outcome, summary)]
        }
        Status::Cancelled => {
            let summary = Outcome {
                summary: Some("attempted".into()),
                ..Outcome::default()
            };
            let confirmed = fx.store.confirm_cancel(CHECKOUT, id, &held, summary);
            if job.status == Status::Queued {
                vec![confirmed, fx.store.request_cancel(CHECKOUT, id)]
            } else {
                // A request on an active Job is recorded but is not a transition.
                vec![confirmed]
            }
        }
        Status::Lost => {
            fx.clock.advance(common::LEASE_MS + common::LOST_AFTER_MS);
            let lost = fx.store.reconcile(CHECKOUT, &NoProbe).map(|result| {
                let job = fx.store.get(CHECKOUT, id).unwrap();
                assert_eq!(result.lost.contains(&job.id), job.status == Status::Lost);
                job
            });
            vec![lost]
        }
    }
}

#[test]
fn every_edge_through_the_store_matches_the_table() {
    let mut landed = 0;
    let mut refused = 0;
    for family in ProducerFamily::ALL {
        for from in Status::ALL {
            for to in Status::ALL {
                if from == to {
                    continue;
                }
                let fx = fixture();
                let (job, token) = fx.job_in(family, from);
                let before = snapshot_bytes(&fx, &job.id);
                let expected = transition_allowed(Some(from), to, family);
                let results = attempts(&fx, &job, token.as_ref(), to);
                let after = fx.get(&job.id);
                let label = format!("{from} -> {to} ({family})");
                if expected {
                    assert_eq!(after.status, to, "{label} should land");
                    assert!(results.iter().any(Result::is_ok), "{label}");
                    landed += 1;
                } else {
                    if to == Status::Lost {
                        // Reconciliation succeeds without touching an ineligible Job.
                        assert!(results.iter().all(Result::is_ok), "{label}");
                    } else {
                        for result in &results {
                            assert!(result.is_err(), "{label} should be refused: {result:?}");
                        }
                    }
                    assert_eq!(after.status, from, "{label} must not move");
                    assert_eq!(snapshot_bytes(&fx, &job.id), before, "{label} wrote");
                    refused += 1;
                }
            }
        }
    }
    // 13 shared edges for each family plus 3 watch-only edges, minus `new -> queued`,
    // which `create` covers below.
    assert_eq!(landed, 12 * 3 + 3);
    assert_eq!(landed + refused, 7 * 6 * 3);
}

#[test]
fn create_is_the_only_edge_from_new_and_it_lands_queued() {
    for family in ProducerFamily::ALL {
        let fx = fixture();
        let job = fx.create(family);
        assert_eq!(job.status, Status::Queued);
        assert_eq!((job.revision, job.lease, job.started_ms), (1, None, None));
        for to in Status::ALL {
            assert_eq!(
                transition_allowed(None, to, family),
                to == Status::Queued,
                "new -> {to}"
            );
        }
    }
}

#[test]
fn refused_edges_report_the_transition_and_family() {
    let fx = fixture();
    let (job, token) = fx.job_in(ProducerFamily::Owned, Status::Succeeded);
    let error = fx
        .store
        .finish(
            CHECKOUT,
            &job.id,
            &token.unwrap(),
            FinalStatus::Failed,
            Outcome::default(),
        )
        .unwrap_err();
    assert!(matches!(
        error,
        JobError::InvalidTransition {
            from: Some(Status::Succeeded),
            to: Status::Failed,
            family: ProducerFamily::Owned,
        }
    ));

    let (job, _) = fx.job_in(ProducerFamily::Reported, Status::Queued);
    let error = fx
        .store
        .acquire(CHECKOUT, &job.id, acquire("p", ActiveStatus::Waiting))
        .unwrap_err();
    assert!(matches!(
        error,
        JobError::InvalidTransition {
            from: Some(Status::Queued),
            to: Status::Waiting,
            family: ProducerFamily::Reported,
        }
    ));

    for family in [ProducerFamily::Owned, ProducerFamily::Reported] {
        let (job, _) = fx.job_in(family, Status::Lost);
        let error = fx
            .store
            .acquire(CHECKOUT, &job.id, acquire("p2", ActiveStatus::Running))
            .unwrap_err();
        assert!(
            matches!(
                error,
                JobError::InvalidTransition {
                    from: Some(Status::Lost),
                    ..
                }
            ),
            "{family}: {error}"
        );
    }
}

#[test]
fn updates_inside_one_state_are_not_transitions() {
    let fx = fixture();
    for status in [Status::Running, Status::Waiting] {
        let (job, token) = fx.job_in(ProducerFamily::Watch, status);
        let token = token.unwrap();
        let same = JobUpdate {
            status: Some(if status == Status::Running {
                ActiveStatus::Running
            } else {
                ActiveStatus::Waiting
            }),
            ..JobUpdate::default()
        };
        let updated = fx.store.update(CHECKOUT, &job.id, &token, same).unwrap();
        assert_eq!(updated.status, status);
        assert_eq!(updated.revision, job.revision + 1);
        assert_eq!(updated.lease.unwrap().generation, 1);
    }
}
