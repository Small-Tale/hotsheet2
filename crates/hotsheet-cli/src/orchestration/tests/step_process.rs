//! HS2-HTS89S: a validation step left running by a hosting process that died is
//! stopped by its recorded process group, and nothing else is. Synthetic commands only.
use std::os::unix::process::CommandExt;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use hotsheet_ticketing::orchestration::{
    HostedValidationKnown, HostedValidationRegistry, StepProcess,
};

use super::tickets::{machine, unsupported};
use super::*;
use crate::orchestration::step_process::process_started;
use crate::orchestration::validation::{
    HostedExecutionState, HostedValidationCancel, HostedValidationStart,
};

/// A synthetic step: its own process group, as the host launches steps.
fn group(line: &str, dir: &Path) -> Child {
    Command::new("/bin/sh")
        .args(["-c", line])
        .current_dir(dir)
        .process_group(0)
        .spawn()
        .unwrap()
}

/// A hosting process that is gone: a pid that ran and was reaped.
fn dead_host() -> (u32, u64) {
    let mut gone = Command::new("/bin/sh")
        .args(["-c", "exit 0"])
        .spawn()
        .unwrap();
    let pid = gone.id();
    gone.wait().unwrap();
    (pid, 1)
}

fn record(registry: &HostedValidationRegistry, checkout: &str, n: u128, process: StepProcess) {
    registry.accept(checkout, &id(n), 1).unwrap();
    registry.launched(checkout, &id(n), process).unwrap();
}

/// Whether the child ended within a few seconds (it was stopped).
fn ends(child: &mut Child) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if child.try_wait().unwrap().is_some() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

/// Whether a pid we are not the parent of is gone within a few seconds.
fn gone(pid: u32) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if process_started(pid).is_none() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

#[test]
fn hs2_hts89s_an_orphaned_step_group_is_stopped_and_nothing_else_is() {
    let m = machine(10, false, unsupported);
    let root = Path::new(&m.target.root).to_path_buf();
    let registry = HostedValidationRegistry::new(&m.home);
    let checkout = m.target.id.as_str();
    let (dead_pid, dead_started) = dead_host();
    let me = (
        std::process::id(),
        process_started(std::process::id()).unwrap(),
    );

    // 1. Its host is gone and its leader is the recorded process: stopped.
    let mut orphan = group("sleep 30", &root);
    record(
        &registry,
        checkout,
        60,
        StepProcess {
            pid: orphan.id(),
            started: process_started(orphan.id()).unwrap(),
            host_pid: dead_pid,
            host_started: dead_started,
        },
    );
    // 2. Its leader ended, a member still runs: the group can only be the step's.
    let mut leader = group("sleep 30 & echo $! > member; exit 0", &root);
    let leader_pid = leader.id();
    let leader_started = process_started(leader_pid).unwrap();
    leader.wait().unwrap();
    let member: u32 = std::fs::read_to_string(root.join("member"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(process_started(member).is_some());
    record(
        &registry,
        checkout,
        61,
        StepProcess {
            pid: leader_pid,
            started: leader_started,
            host_pid: dead_pid,
            host_started: dead_started,
        },
    );
    // 3. The pid now names a process that started at another time: not the step's.
    let mut stranger = group("sleep 30", &root);
    record(
        &registry,
        checkout,
        62,
        StepProcess {
            pid: stranger.id(),
            started: process_started(stranger.id()).unwrap() + 1,
            host_pid: dead_pid,
            host_started: dead_started,
        },
    );
    // 4. Its host still runs (this process): left to it.
    let mut hosted = group("sleep 30", &root);
    record(
        &registry,
        checkout,
        63,
        StepProcess {
            pid: hosted.id(),
            started: process_started(hosted.id()).unwrap(),
            host_pid: me.0,
            host_started: me.1,
        },
    );

    assert_eq!(m.runtime.reap_orphaned_validations(), 2);
    assert!(ends(&mut orphan), "the orphaned step was stopped");
    assert!(
        gone(member),
        "the remaining member of the group was stopped"
    );
    std::thread::sleep(Duration::from_millis(200));
    assert!(
        stranger.try_wait().unwrap().is_none(),
        "another process is never stopped"
    );
    assert!(
        hosted.try_wait().unwrap().is_none(),
        "a live host's step is its own"
    );
    // The stopped and the foreign records forget the group; the attempts stay unknown,
    // never a made-up outcome. The live host's record keeps its group.
    for n in [60, 61, 62] {
        let kept = registry.record(checkout, &id(n)).unwrap().unwrap();
        assert_eq!((kept.process, kept.outcome), (None, None));
        assert_eq!(
            registry.known(checkout, &id(n)).unwrap(),
            HostedValidationKnown::Accepted
        );
    }
    assert!(
        registry
            .record(checkout, &id(63))
            .unwrap()
            .unwrap()
            .process
            .is_some()
    );
    // Reaping again does nothing more.
    assert_eq!(m.runtime.reap_orphaned_validations(), 0);
    let _ = stranger.kill();
    let _ = hosted.kill();
    let _ = (stranger.wait(), hosted.wait());
}

#[test]
fn hs2_hts89s_a_running_step_records_its_group_and_a_cancel_stops_an_orphan() {
    let m = machine(10, false, unsupported);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let registry = HostedValidationRegistry::new(&m.home);
    let checkout = m.target.id.as_str();
    let root = Path::new(&m.target.root).to_path_buf();
    // A step this process runs records its group while it runs and forgets it after.
    let started = m
        .runtime
        .hosted_validation_start(
            checkout,
            HostedValidationStart {
                execution_id: id(70),
                check: hotsheet_ticketing::orchestration::ValidationCheck::Command {
                    program: "/bin/sh".into(),
                    args: vec!["-c".into(), "while [ ! -f go ]; do sleep 0.05; done".into()],
                },
                timeout_seconds: 20,
                wait_ms: 0,
            },
            &mut tx,
        )
        .unwrap();
    assert_eq!(started.state, HostedExecutionState::Running);
    let deadline = Instant::now() + Duration::from_secs(5);
    let process = loop {
        if let Some(process) = registry.record(checkout, &id(70)).unwrap().unwrap().process {
            break process;
        }
        assert!(Instant::now() < deadline, "the group was never recorded");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(process.host_pid, std::process::id());
    assert_eq!(process_started(process.pid), Some(process.started));
    std::fs::write(root.join("go"), "").unwrap();
    let finished = m
        .runtime
        .hosted_validation(checkout, &id(70), 8_000)
        .unwrap();
    assert_eq!(finished.state, HostedExecutionState::Finished);
    assert_eq!(
        registry.record(checkout, &id(70)).unwrap().unwrap().process,
        None
    );

    // A cancel of a step whose host is gone stops its group and records the cancel.
    let (dead_pid, dead_started) = dead_host();
    let mut orphan = group("sleep 30", &root);
    record(
        &registry,
        checkout,
        71,
        StepProcess {
            pid: orphan.id(),
            started: process_started(orphan.id()).unwrap(),
            host_pid: dead_pid,
            host_started: dead_started,
        },
    );
    let cancelled = m
        .runtime
        .hosted_validation_cancel(
            checkout,
            HostedValidationCancel {
                execution_id: id(71),
                wait_ms: 0,
            },
            &mut tx,
        )
        .unwrap();
    assert_eq!(cancelled.state, HostedExecutionState::Unknown);
    assert!(ends(&mut orphan));
    let kept = registry.record(checkout, &id(71)).unwrap().unwrap();
    assert_eq!(kept.process, None);
}
