//! HS2-VVHVF0: the runtime makes sure the Job monitor observes the watch Jobs it
//! created. The monitor here is a recording stand-in that holds the machine's real
//! monitor slot, so the adapter's own reading (`watching`) is the one under test.
//! Synthetic repositories and commit ids only.
use std::sync::atomic::AtomicU64;

use hotsheet_jobs::{MonitorClaim, MonitorInfo, MonitorSlot};
use hotsheet_ticketing::orchestration::{GateState, TargetAction};
use serde_json::json;

use super::gates::{judge, sha, with_run};
use super::tickets::{human, worker};
use super::*;

/// Holds the monitor slot the way a running monitor does, and counts starts.
#[derive(Default)]
struct Recording {
    claim: Mutex<Option<MonitorClaim>>,
    starts: AtomicUsize,
    broken: std::sync::atomic::AtomicBool,
}
impl Recording {
    fn starts(&self) -> usize {
        self.starts.load(Ordering::SeqCst)
    }
    /// The monitor process is gone: it crashed, was stopped, or went idle.
    fn stop(&self) {
        self.claim.lock().unwrap().take();
    }
}
impl JobMonitor for Recording {
    fn ensure(&self, home: &Path) -> std::io::Result<bool> {
        self.starts.fetch_add(1, Ordering::SeqCst);
        if self.broken.load(Ordering::SeqCst) {
            return Err(std::io::Error::other("the monitor could not be started"));
        }
        let claim = MonitorSlot::at(home).try_claim(&MonitorInfo {
            owner: "monitor:recording".into(),
            pid: 1,
            started_ms: 1,
        })?;
        let started = claim.is_some();
        if let Some(claim) = claim {
            *self.claim.lock().unwrap() = Some(claim);
        }
        Ok(started)
    }
}

struct At(AtomicU64);
impl hotsheet_ticketing::Clock for At {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

fn github(commit: &str) -> LinkGate {
    serde_json::from_value(json!({"commit": commit, "repository": "example/widgets"})).unwrap()
}

#[test]
fn hs2_vvhvf0_a_link_starts_the_monitor_once_and_only_for_a_gate_that_waits() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let (mut m, run) = with_run(&[TargetAction::GatesObserve], &mut tx);
    let monitor = Arc::new(Recording::default());
    m.runtime = m.runtime.clone().with_job_monitor(monitor.clone());
    let (a, b, owner) = (sha('a'), sha('b'), m.owner.id.clone());
    assert!(m.runtime.gate_owners().is_empty());
    // A link nobody may make starts nothing.
    assert!(
        m.runtime
            .gate_link(&owner, &run, github(&a), Some(&human()), &mut tx)
            .is_err()
    );
    assert_eq!(monitor.starts(), 0);
    // The link creates the watch Job and the runtime starts the monitor for it.
    let linked = m
        .runtime
        .gate_link(&owner, &run, github(&a), Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!(
        (linked.gate.state, linked.watching, monitor.starts()),
        (GateState::Pending, Some(true), 1)
    );
    assert_eq!(m.runtime.gate_owners(), std::slice::from_ref(&owner));
    // Repeating the link finds the running monitor and starts nothing.
    let again = m
        .runtime
        .gate_link(&owner, &run, github(&a), Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!((again.watching, monitor.starts()), (Some(true), 1));
    // The monitor is gone and the gate still waits: the same link starts it again.
    monitor.stop();
    let again = m
        .runtime
        .gate_link(&owner, &run, github(&a), Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!((again.watching, monitor.starts()), (Some(true), 2));
    // A settled gate needs no observer: with the monitor gone, a repeat starts none.
    let job = linked.gate.watch_ref.clone().unwrap();
    judge(&m.home, &owner, &job, "green", &a, 7);
    monitor.stop();
    let passed = m
        .runtime
        .gate_link(&owner, &run, github(&a), Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!(
        (passed.gate.state, passed.watching, monitor.starts()),
        (GateState::Passed, Some(false), 2)
    );
    assert!(m.runtime.gate_owners().is_empty());
    // A monitor that cannot be started leaves the link standing and says so.
    monitor.broken.store(true, Ordering::SeqCst);
    let unwatched = m
        .runtime
        .gate_link(&owner, &run, github(&b), Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!(
        (unwatched.gate.state, unwatched.watching, monitor.starts()),
        (GateState::Pending, Some(false), 3)
    );
    // Without a monitor port the adapter only reports, as before.
    let plain = m
        .runtime
        .clone()
        .with_gate_provider(Arc::new(GithubActionsGates::new(&m.home)));
    let reported = plain
        .gate_link(&owner, &run, github(&b), Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!((reported.watching, monitor.starts()), (Some(false), 3));
}

#[test]
fn hs2_vvhvf0_a_follower_and_a_sync_restart_a_missing_monitor_and_back_off_a_failing_one() {
    let clock = At(AtomicU64::new(1_000));
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let (mut m, run) = with_run(&[TargetAction::GatesObserve], &mut tx);
    let monitor = Arc::new(Recording::default());
    m.runtime = m.runtime.clone().with_job_monitor(monitor.clone());
    let (a, owner) = (sha('a'), m.owner.id.clone());
    let mut follow = GateFollowState::default();
    let step = |follow: &mut GateFollowState, tx: &mut Transaction<'_>| {
        m.runtime.gate_follow_step(&owner, follow, 0, tx)
    };
    // No gate waits: an idle follower starts no monitor.
    let idle = step(&mut follow, &mut tx);
    assert_eq!(
        (idle.active, idle.watching, monitor.starts()),
        (false, None, 0)
    );
    let linked = m
        .runtime
        .gate_link(&owner, &run, github(&a), Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!((linked.watching, monitor.starts()), (Some(true), 1));
    // A running monitor is found by a lock probe; nothing is launched.
    for _ in 0..3 {
        let watched = step(&mut follow, &mut tx);
        assert_eq!((watched.active, watched.watching), (true, Some(true)));
    }
    assert_eq!(monitor.starts(), 1);
    // The monitor dies and comes back, twice: each next step restarts it at once.
    for restart in 1..=2 {
        monitor.stop();
        assert_eq!(step(&mut follow, &mut tx).watching, Some(true));
        assert_eq!(monitor.starts(), 1 + restart);
    }
    // A fresh follower, as after a server restart, does the same from nothing.
    monitor.stop();
    let mut fresh = GateFollowState::default();
    assert_eq!(step(&mut fresh, &mut tx).watching, Some(true));
    assert_eq!(monitor.starts(), 4);
    // A headless sync that waits on the gate makes sure of the monitor first.
    monitor.stop();
    let synced = m
        .runtime
        .gate_sync(
            &owner,
            &run,
            &SyncGates { wait_ms: 0 },
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert_eq!((synced.watching, monitor.starts()), (Some(true), 5));
    // The monitor cannot be started any more. The follower tries, fails, and does not
    // try again until its delay has passed, however often it steps. Each failure
    // doubles the delay.
    monitor.stop();
    monitor.broken.store(true, Ordering::SeqCst);
    let failed = step(&mut follow, &mut tx);
    assert_eq!(
        (failed.active, failed.watching, monitor.starts()),
        (true, Some(false), 6)
    );
    for _ in 0..3 {
        assert_eq!(step(&mut follow, &mut tx).watching, None);
    }
    assert_eq!(monitor.starts(), 6);
    clock.0.store(1_000 + 29_999, Ordering::SeqCst);
    assert_eq!(step(&mut follow, &mut tx).watching, None);
    clock.0.store(1_000 + 30_000, Ordering::SeqCst);
    assert_eq!(step(&mut follow, &mut tx).watching, Some(false));
    assert_eq!(monitor.starts(), 7);
    clock.0.store(1_000 + 30_000 + 59_999, Ordering::SeqCst);
    assert_eq!(step(&mut follow, &mut tx).watching, None);
    assert_eq!(monitor.starts(), 7);
    // Once it can be started, the next due step starts it, and the delay is forgotten:
    // the next loss is repaired at once.
    monitor.broken.store(false, Ordering::SeqCst);
    clock.0.store(1_000 + 30_000 + 60_000, Ordering::SeqCst);
    assert_eq!(step(&mut follow, &mut tx).watching, Some(true));
    assert_eq!(monitor.starts(), 8);
    monitor.stop();
    assert_eq!(step(&mut follow, &mut tx).watching, Some(true));
    assert_eq!(monitor.starts(), 9);
    // A follower whose linking actor lost the grant observes nothing and starts nothing.
    monitor.stop();
    let grant = m.runtime.grants(&m.reference(1), &mut tx).unwrap()[0]
        .grant
        .grant_id
        .clone();
    m.runtime
        .revoke(&m.reference(1), &grant, Some(&human()), &mut tx)
        .unwrap();
    let mut denied = GateFollowState::default();
    let left = step(&mut denied, &mut tx);
    assert_eq!(
        (left.active, left.watching, monitor.starts()),
        (true, None, 9)
    );
    // So does a sync the gate denies.
    assert!(
        m.runtime
            .gate_sync(
                &owner,
                &run,
                &SyncGates { wait_ms: 0 },
                Some(&worker()),
                &mut tx,
            )
            .is_err()
    );
    assert_eq!(monitor.starts(), 9);
    assert_eq!(m.runtime.gate_owners(), std::slice::from_ref(&owner));
}
