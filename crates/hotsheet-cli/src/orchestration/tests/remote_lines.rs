//! HS2-3D8D6V: the orchestration lines of a checkout on a remote server, answered by
//! the machine that runs the orchestration project from its own bindings and
//! timelines. The remote server is never contacted for the read. Synthetic data only.
use hotsheet_ticketing::orchestration::{
    BindingRegistry, ItemState, TargetLocation, TargetRef, UnbindTarget,
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
    let (unbound, waited) = waiting.join().unwrap();
    assert!(!unbound.orchestrated && unbound.lines.is_empty());
    assert_ne!(unbound.version, bound.version);
    // The bindings file is not a watched journal: the safety re-check finds it.
    assert!(waited < std::time::Duration::from_secs(5), "{waited:?}");
    // Repeated reads of the empty answer agree.
    assert_eq!(
        read(&r, &id(20), &checkout, None, 0).unwrap().version,
        unbound.version
    );
}
