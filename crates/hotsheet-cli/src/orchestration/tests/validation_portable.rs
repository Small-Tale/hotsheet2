//! HS2-2R32SD: a validation step's process control on every platform. The larger
//! validation suites (`validation.rs`, `validation_runner.rs`) script their steps in
//! POSIX `sh` and stay Unix-only; this module runs the same essentials with each
//! platform's own shell, so a Windows target runs them too.
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use hotsheet_ticketing::orchestration::{ExecutionEnd, ExecutionOutcome};

use crate::orchestration::validation::execute;

const EXECUTION: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

/// The platform's shell running one line: `sh -c` on Unix, `cmd.exe /C` on Windows.
fn shell(root: &Path, unix: &str, windows: &str, timeout: Duration) -> ExecutionOutcome {
    let (program, args) = if cfg!(windows) {
        let comspec =
            std::env::var("ComSpec").unwrap_or_else(|_| "C:\\Windows\\System32\\cmd.exe".into());
        (comspec, vec!["/C".to_owned(), windows.to_owned()])
    } else {
        ("/bin/sh".to_owned(), vec!["-c".to_owned(), unix.to_owned()])
    };
    execute(
        root,
        &program,
        &args,
        timeout,
        1024 * 1024,
        7,
        &AtomicBool::new(false),
        EXECUTION,
        None,
    )
}

#[test]
fn hs2_2r32sd_a_step_reports_its_exit_code_and_output_on_every_platform() {
    let checkout = tempfile::tempdir().unwrap();
    let ok = shell(
        checkout.path(),
        "printf ok",
        "echo ok",
        Duration::from_secs(20),
    );
    assert_eq!(
        (ok.end, ok.exit_code),
        (ExecutionEnd::Exited, Some(0)),
        "{ok:?}"
    );
    assert!(ok.stdout.is_some_and(|stream| stream.bytes >= 2));
    let failed = shell(checkout.path(), "exit 7", "exit 7", Duration::from_secs(20));
    assert_eq!(
        (failed.end, failed.exit_code),
        (ExecutionEnd::Exited, Some(7)),
        "{failed:?}"
    );
    let missing = execute(
        checkout.path(),
        "hs2-2r32sd-no-such-program",
        &[],
        Duration::from_secs(5),
        1024,
        7,
        &AtomicBool::new(false),
        EXECUTION,
        None,
    );
    assert_eq!(missing.end, ExecutionEnd::ProgramNotFound);
}

/// A timeout ends the step and everything it started: the streams the descendants
/// inherited close at once, so nothing still holds them (Unix: the process group;
/// Windows: the step's job object).
#[test]
fn hs2_2r32sd_a_timeout_ends_the_steps_whole_process_tree() {
    let checkout = tempfile::tempdir().unwrap();
    let started = Instant::now();
    let family = shell(
        checkout.path(),
        "sleep 30 & sleep 30",
        "start /b ping -n 60 127.0.0.1 & ping -n 60 127.0.0.1",
        Duration::from_millis(500),
    );
    assert_eq!(family.end, ExecutionEnd::TimedOut, "{family:?}");
    assert_eq!(family.signal, None);
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "{:?}",
        started.elapsed()
    );
    // A descendant still running would hold the inherited streams open past the drain
    // grace, and the step would report no digest for them.
    assert!(family.stdout.is_some(), "stdout still held: {family:?}");
    assert!(family.stderr.is_some(), "stderr still held: {family:?}");
}

/// A checkout's own batch script, named without its extension and with a directory
/// part, is found through PATHEXT and runs under `cmd.exe`.
#[cfg(windows)]
#[test]
fn hs2_2r32sd_a_checkout_batch_script_is_found_through_pathext() {
    let checkout = tempfile::tempdir().unwrap();
    let scripts = checkout.path().join("scripts");
    std::fs::create_dir(&scripts).unwrap();
    std::fs::write(scripts.join("check.cmd"), "@exit /b 3\r\n").unwrap();
    let outcome = execute(
        checkout.path(),
        "scripts\\check",
        &[],
        Duration::from_secs(20),
        1024,
        7,
        &AtomicBool::new(false),
        EXECUTION,
        None,
    );
    assert_eq!(
        (outcome.end, outcome.exit_code),
        (ExecutionEnd::Exited, Some(3)),
        "{outcome:?}"
    );
    // Named without a directory part it is not searched in the checkout.
    let bare = execute(
        checkout.path(),
        "check",
        &[],
        Duration::from_secs(5),
        1024,
        7,
        &AtomicBool::new(false),
        EXECUTION,
        None,
    );
    assert_eq!(bare.end, ExecutionEnd::ProgramNotFound);
}
