//! HS2-M7EV18: a hosted validation step runs in a detached runner process (the built
//! `hotsheet-cli`), so it finishes and is collected after the hosting process
//! restarts, never runs twice, can be cancelled from a new host process, and reads
//! as lost when its runner dies. Synthetic commands only.
use std::time::{Duration, Instant};

use hotsheet_ticketing::orchestration::{ExecutionEnd, HostedValidationRegistry, ValidationCheck};

use super::tickets::{machine, unsupported};
use super::*;
use crate::orchestration::step_process::process_started;
use crate::orchestration::validation::{
    HostedExecutionState, HostedValidationCancel, HostedValidationStart, forget_executions,
};

/// SHA-256 of the two bytes `ok`.
const OK_SHA256: &str = "2689367b205c16ce32ed4200942b8b8b1e262dfc70d9bc9fbc77c49699a4f1df";

fn start(n: u128, line: &str, wait_ms: u64) -> HostedValidationStart {
    HostedValidationStart {
        execution_id: id(n),
        check: ValidationCheck::Command {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), line.into()],
        },
        timeout_seconds: 60,
        wait_ms,
    }
}

fn lines(path: &Path) -> usize {
    std::fs::read_to_string(path).map_or(0, |text| text.lines().count())
}

#[test]
fn hs2_m7ev18_a_detached_step_outlives_its_host_process_and_runs_once() {
    let m = machine(10, false, unsupported);
    let runtime = m.runtime.clone().with_detached_validation();
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let checkout = m.target.id.clone();
    let root = Path::new(&m.target.root).to_path_buf();
    let registry = HostedValidationRegistry::new(&m.home);
    let held = "echo x >> runs; while [ ! -f go ]; do sleep 0.05; done; printf ok";

    // It runs in another process than this host.
    let running = runtime
        .hosted_validation_start(&checkout, start(80, held, 0), &mut tx)
        .unwrap();
    assert_eq!(running.state, HostedExecutionState::Running);
    let deadline = Instant::now() + Duration::from_secs(60);
    let step = loop {
        let record = registry.record(&checkout, &id(80)).unwrap().unwrap();
        if let Some(process) = record
            .process
            .filter(|process| process.pid != process.host_pid)
        {
            break process;
        }
        assert!(
            Instant::now() < deadline,
            "the runner never launched the step: {record:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_ne!(
        step.host_pid,
        std::process::id(),
        "a runner process hosts it"
    );
    assert_eq!(lines(&root.join("runs")), 1);

    // The hosting process restarts: a new host still sees it running, not lost.
    forget_executions();
    let after_restart = runtime.hosted_validation(&checkout, &id(80), 0).unwrap();
    assert_eq!(after_restart.state, HostedExecutionState::Running);
    // Starting the same id again starts nothing.
    let again = runtime
        .hosted_validation_start(&checkout, start(80, held, 0), &mut tx)
        .unwrap();
    assert_eq!(again.state, HostedExecutionState::Running);
    // A second runner of the same id finds nothing to claim.
    assert!(matches!(
        crate::orchestration::run_validation_step(&m.home, &checkout, &id(80)),
        Err(OrchestrationError::NotFound)
    ));
    // It ends by itself and the new host collects the result.
    std::fs::write(root.join("go"), "").unwrap();
    let finished = runtime
        .hosted_validation(&checkout, &id(80), MAX_VALIDATION_WAIT_MS)
        .unwrap();
    assert_eq!(
        finished.state,
        HostedExecutionState::Finished,
        "{finished:?}"
    );
    let outcome = finished.outcome.unwrap();
    assert_eq!(outcome.exit_code, Some(0));
    assert_eq!(outcome.stdout.unwrap().sha256, OK_SHA256);
    assert_eq!(lines(&root.join("runs")), 1, "it ran once");
    std::fs::remove_file(root.join("go")).unwrap();

    // A host process that restarted cancels a step its runner still runs.
    runtime
        .hosted_validation_start(&checkout, start(81, held, 0), &mut tx)
        .unwrap();
    forget_executions();
    let cancelled = runtime
        .hosted_validation_cancel(
            &checkout,
            HostedValidationCancel {
                execution_id: id(81),
                wait_ms: MAX_VALIDATION_WAIT_MS,
            },
            &mut tx,
        )
        .unwrap();
    assert_eq!(
        cancelled.state,
        HostedExecutionState::Finished,
        "{cancelled:?}"
    );
    assert_eq!(cancelled.outcome.unwrap().end, ExecutionEnd::Cancelled);

    // The runner dies mid-step: the attempt is lost, and the orphaned step is stopped.
    runtime
        .hosted_validation_start(&checkout, start(82, held, 0), &mut tx)
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    let step = loop {
        let record = registry.record(&checkout, &id(82)).unwrap().unwrap();
        if let Some(process) = record
            .process
            .filter(|process| process.pid != process.host_pid)
        {
            break process;
        }
        assert!(
            Instant::now() < deadline,
            "the runner never launched the step: {record:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    // SAFETY: plain integers; the pid is the runner this test started, still running.
    unsafe {
        libc::kill(libc::pid_t::try_from(step.host_pid).unwrap(), libc::SIGKILL);
    }
    let lost = runtime
        .hosted_validation(&checkout, &id(82), MAX_VALIDATION_WAIT_MS)
        .unwrap();
    assert_eq!(lost.state, HostedExecutionState::Unknown, "{lost:?}");
    assert_eq!(runtime.reap_orphaned_validations(), 1);
    let deadline = Instant::now() + Duration::from_secs(5);
    while process_started(step.pid).is_some() {
        assert!(Instant::now() < deadline, "the orphaned step kept running");
        std::thread::sleep(Duration::from_millis(20));
    }
}
