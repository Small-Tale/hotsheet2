//! HS2-3D8D6V: the orchestration lines of a checkout on a remote server, answered by
//! the machine that runs the orchestration project from its own bindings and
//! timelines. The remote server is never contacted for the read. Synthetic data only.
use hotsheet_ticketing::orchestration::{
    BindingRegistry, ItemState, TargetLocation, TargetRef, UnbindTarget,
};

use crate::orchestration::{
    LINE_PUSH_FIRST_BACKOFF_MS, LINE_PUSH_MIN_GAP_MS, LINE_PUSH_TTL_MS, LinePushReport,
    LinePushState,
};

use serde_json::json;

use super::hosted_gates::{Remote, remote};
use super::*;

fn read(
    r: &Remote,
    registration: &str,
    checkout: &str,
    after: Option<&str>,
    wait_ms: u64,
) -> Result<hotsheet_ticketing::orchestration::TargetLines, OrchestrationError> {
    let mut rng = Rng(900);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    r.runtime
        .orchestration_remote_target_lines(registration, checkout, after, wait_ms, &mut tx)
}

fn peer_checkout(r: &Remote) -> String {
    r.peer.hosted.target.id.clone()
}

/// Plans the fixture's dispatched remote work as a step, so the timeline shows it.
fn plan(r: &Remote, tx: &mut Transaction<'_>) {
    let run = r
        .runtime
        .run_list(&r.owner, false)
        .unwrap()
        .into_iter()
        .find(|run| run.run_id == r.run)
        .unwrap();
    let request: DefinePlanStep = serde_json::from_value(json!({
        "ticket": run.ticket, "target_id": run.target.target_id,
        "connection_id": run.target.connection_id, "native_id": run.target.native_id,
        "prerequisites": [],
    }))
    .unwrap();
    r.runtime
        .readiness_define(&r.owner, request, Some(&super::tickets::worker()), tx)
        .unwrap();
}

#[test]
fn hs2_3d8d6v_remote_target_lines_come_from_this_machines_plan() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let r = remote(&mut tx);
    plan(&r, &mut tx);
    let checkout = peer_checkout(&r);
    let contacted = r.peer.calls.lock().unwrap().len();

    let lines = read(&r, &id(20), &checkout, None, 0).unwrap();
    assert!(lines.orchestrated);
    assert_eq!(lines.lines.len(), 1);
    let line = &lines.lines[0];
    assert_eq!(line.owner_checkout_id, r.owner);
    assert_eq!(line.item.state, ItemState::Running);
    assert_eq!(line.item.location, TargetLocation::Remote);
    assert_eq!(line.item.target.target_id, id(1));
    assert!(!line.proposed);
    // Repeating the read answers the same version; the remote server was not asked.
    assert_eq!(
        read(&r, &id(20), &checkout, None, 0).unwrap().version,
        lines.version
    );
    assert_eq!(r.peer.calls.lock().unwrap().len(), contacted);

    // The same checkout id on another registration, or another checkout on this one,
    // is not orchestrated.
    assert!(!read(&r, &id(21), &checkout, None, 0).unwrap().orchestrated);
    assert!(
        !read(&r, &id(20), "other-0123456789ab", None, 0)
            .unwrap()
            .orchestrated
    );
    // The remote checkout is not a checkout of this machine: the local read refuses it.
    assert!(
        r.runtime
            .orchestration_target_lines(&checkout, None, 0, &mut tx)
            .is_err()
    );
    // Malformed ids are refused before anything is read.
    for (registration, checkout) in [("a/b", checkout.as_str()), (&id(20), ""), (&id(20), "a b")] {
        assert!(matches!(
            read(&r, registration, checkout, None, 0),
            Err(OrchestrationError::Invalid(_))
        ));
    }
    // The current version waits to the deadline; an older one answers at once.
    let started = std::time::Instant::now();
    let same = read(&r, &id(20), &checkout, Some(&lines.version), 300).unwrap();
    assert_eq!(same.version, lines.version);
    assert!(started.elapsed() >= std::time::Duration::from_millis(250));
    let at_once = read(&r, &id(20), &checkout, Some("0a1b2c3d4e5f"), 5_000).unwrap();
    assert_eq!(at_once.version, lines.version);
}

#[test]
fn hs2_3d8d6v_unbinding_the_remote_target_ends_its_line_and_a_waiting_read() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let r = remote(&mut tx);
    plan(&r, &mut tx);
    let checkout = peer_checkout(&r);
    let bound = read(&r, &id(20), &checkout, None, 0).unwrap();
    assert_eq!(bound.lines.len(), 1);
    let target = TargetRef {
        owner_checkout_id: r.owner.clone(),
        target_id: id(1),
    };
    let view = BindingRegistry::new(&r.runtime.home).get(&target).unwrap();
    // A reader waits on the bound version while the binding is removed.
    let waiting = {
        let runtime = r.runtime.clone();
        let (checkout, version) = (checkout.clone(), bound.version.clone());
        std::thread::spawn(move || {
            let mut rng = Rng(901);
            let mut tx = Transaction {
                clock: &Clock,
                rng: &mut rng,
            };
            let started = std::time::Instant::now();
            let lines = runtime
                .orchestration_remote_target_lines(
                    &id(20),
                    &checkout,
                    Some(&version),
                    15_000,
                    &mut tx,
                )
                .unwrap();
            (lines, started.elapsed())
        })
    };
    std::thread::sleep(std::time::Duration::from_millis(200));
    OrchestrationRuntime::unbind_at(
        &r.runtime.home,
        UnbindTarget {
            target,
            expected_binding_revision: view.revision,
            previous_destination: view.destination,
        },
        &mut tx,
    )
    .unwrap();
    let (unbound, _waited) = waiting.join().unwrap();
    assert!(!unbound.orchestrated && unbound.lines.is_empty());
    assert_ne!(unbound.version, bound.version);
    // The bindings file is not a watched journal: the safety re-check finds it.
    // HS2-5EZZPH: how soon is not asserted here; a bound on it measured how fast a
    // loaded machine re-reads (a loaded run took 15.7 s) and the one-second re-check
    // itself is pinned by `hs2_f8174x_target_lines_find_an_unreported_change_*`.
    // Repeated reads of the empty answer agree.
    assert_eq!(
        read(&r, &id(20), &checkout, None, 0).unwrap().version,
        unbound.version
    );
}

/// A clock a test moves.
struct At(u64);
impl hotsheet_ticketing::Clock for At {
    fn now_ms(&self) -> u64 {
        self.0
    }
}

fn pass(r: &Remote, state: &mut LinePushState, now_ms: u64) -> LinePushReport {
    let mut rng = Rng(950);
    let clock = At(now_ms);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    r.runtime.push_target_lines(state, None, &mut tx).unwrap()
}

fn outcomes(report: &LinePushReport) -> Vec<(&str, u32)> {
    report
        .destinations
        .iter()
        .map(|outcome| (outcome.outcome.as_str(), outcome.lines))
        .collect()
}

fn hosted_lines(r: &Remote) -> hotsheet_ticketing::orchestration::TargetLines {
    let mut rng = Rng(960);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    r.peer
        .hosted
        .runtime
        .orchestration_target_lines(&r.peer.hosted.target.id, None, 0, &mut tx)
        .unwrap()
}

fn pushes(r: &Remote) -> usize {
    r.calls("/target-line-observations").len()
}

/// HS2-KB2Y51: a pass pushes changed lines, skips unchanged ones, renews at half the
/// lifetime, backs off after a failure and re-asserts once the peer answers again,
/// and withdraws a destination that is no longer bound.
#[test]
fn hs2_kb2y51_a_push_pass_sends_only_changes_renewals_and_retries() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let r = remote(&mut tx);
    let before = hosted_lines(&r);
    let mut state = LinePushState::default();
    // Bound but nothing planned: an empty push, which the target keeps as nothing.
    assert_eq!(outcomes(&pass(&r, &mut state, 10_000)), [("pushed", 0)]);
    assert_eq!(hosted_lines(&r).version, before.version);
    plan(&r, &mut tx);
    // Absent -> pushed: the line shows on the target, as a display fact.
    let report = pass(&r, &mut state, 20_000);
    assert_eq!(outcomes(&report), [("pushed", 1)]);
    assert_eq!(report.next_due_ms, Some(20_000 + LINE_PUSH_TTL_MS / 2));
    let shown = hosted_lines(&r);
    assert!(shown.orchestrated);
    let line = shown
        .lines
        .iter()
        .find(|line| line.observed_from.is_some())
        .unwrap();
    assert_eq!(line.observed_from.as_deref(), Some(id(10).as_str()));
    assert_eq!(line.owner_checkout_id, r.owner);
    assert_eq!(line.item.location, TargetLocation::Local);
    assert!(line.item.run.is_none() && line.item.session_id.is_none());
    // Unchanged -> nothing sent, repeatedly.
    let sent = pushes(&r);
    for now in [20_001, 25_000, 30_000] {
        assert_eq!(outcomes(&pass(&r, &mut state, now)), [("unchanged", 1)]);
    }
    assert_eq!(pushes(&r), sent);
    // Half the lifetime later the same lines are renewed.
    let renew = 20_000 + LINE_PUSH_TTL_MS / 2;
    assert_eq!(outcomes(&pass(&r, &mut state, renew)), [("pushed", 1)]);
    assert_eq!(pushes(&r), sent + 1);
    // The peer fails: the renewal is retried with a doubling backoff, never sooner.
    let fail_at = renew + LINE_PUSH_TTL_MS / 2;
    *r.peer.status_override.lock().unwrap() = Some(503);
    let failed = pass(&r, &mut state, fail_at);
    assert_eq!(failed.destinations[0].outcome, "failed");
    assert_eq!(failed.destinations[0].code.as_deref(), Some("status_503"));
    assert_eq!(
        failed.next_due_ms,
        Some(fail_at + LINE_PUSH_FIRST_BACKOFF_MS)
    );
    assert_eq!(
        outcomes(&pass(&r, &mut state, fail_at + 1)),
        [("waiting", 1)]
    );
    let second = pass(&r, &mut state, fail_at + LINE_PUSH_FIRST_BACKOFF_MS);
    assert_eq!(second.destinations[0].outcome, "failed");
    assert_eq!(
        second.next_due_ms,
        Some(fail_at + 3 * LINE_PUSH_FIRST_BACKOFF_MS)
    );
    // Reconnected: the next due pass re-asserts the lines.
    *r.peer.status_override.lock().unwrap() = None;
    assert_eq!(
        outcomes(&pass(
            &r,
            &mut state,
            fail_at + 3 * LINE_PUSH_FIRST_BACKOFF_MS
        )),
        [("pushed", 1)]
    );
    // A fresh state (a restarted server) re-asserts once, then is quiet again.
    let mut restarted = LinePushState::default();
    let later = fail_at + 10 * LINE_PUSH_FIRST_BACKOFF_MS;
    assert_eq!(outcomes(&pass(&r, &mut restarted, later)), [("pushed", 1)]);
    assert_eq!(
        outcomes(&pass(&r, &mut restarted, later + 1)),
        [("unchanged", 1)]
    );
    // Unbound -> withdrawn: the target shows no pushed line any more.
    let target = TargetRef {
        owner_checkout_id: r.owner.clone(),
        target_id: id(1),
    };
    let view = BindingRegistry::new(&r.runtime.home).get(&target).unwrap();
    OrchestrationRuntime::unbind_at(
        &r.runtime.home,
        UnbindTarget {
            target,
            expected_binding_revision: view.revision,
            previous_destination: view.destination,
        },
        &mut tx,
    )
    .unwrap();
    let withdrawn = pass(&r, &mut restarted, later + 5_000);
    assert_eq!(outcomes(&withdrawn), [("withdrawn", 0)]);
    assert!(
        hosted_lines(&r)
            .lines
            .iter()
            .all(|line| line.observed_from.is_none())
    );
    // Nothing is left to do or tell.
    let idle = pass(&r, &mut restarted, later + 10_000);
    assert!(idle.destinations.is_empty() && idle.next_due_ms.is_none());
}

/// HS2-KB2Y51: changes closer together than the minimum gap coalesce into one push.
#[test]
fn hs2_kb2y51_quick_changes_coalesce_and_the_hosted_put_is_bounded() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let r = remote(&mut tx);
    let mut state = LinePushState::default();
    assert_eq!(outcomes(&pass(&r, &mut state, 50_000)), [("pushed", 0)]);
    plan(&r, &mut tx);
    let quick = pass(&r, &mut state, 50_500);
    assert_eq!(outcomes(&quick), [("waiting", 1)]);
    assert_eq!(quick.next_due_ms, Some(50_000 + LINE_PUSH_MIN_GAP_MS));
    assert_eq!(
        outcomes(&pass(&r, &mut state, 50_000 + LINE_PUSH_MIN_GAP_MS)),
        [("pushed", 1)]
    );
    // The hosted half refuses what is not a bounded push for a checkout hosted there.
    let hosted = &r.peer.hosted.runtime;
    for (path, body) in [
        (
            format!(
                "/checkouts/{}/target-line-observations",
                r.peer.hosted.target.id
            ),
            json!({"owner_server_id": "not-a-ulid", "sequence": 1, "lines": []}),
        ),
        (
            format!(
                "/checkouts/{}/target-line-observations",
                r.peer.hosted.target.id
            ),
            json!({"owner_server_id": id(30), "sequence": 1, "lines": [], "extra": 1}),
        ),
        (
            "/checkouts/unknown-0123456789ab/target-line-observations".to_owned(),
            json!({"owner_server_id": id(30), "sequence": 1, "lines": []}),
        ),
    ] {
        assert!(
            hosted.request_now("PUT", &path, &body).is_err(),
            "{path} {body}"
        );
    }
    // A push for this machine's own checkout from a local caller works the same way.
    assert_eq!(
        hosted
            .request_now(
                "PUT",
                &format!(
                    "/checkouts/{}/target-line-observations",
                    r.peer.hosted.target.id
                ),
                &json!({"owner_server_id": id(30), "sequence": 1, "lines": []}),
            )
            .unwrap()["outcome"],
        "withdrawn"
    );
}
