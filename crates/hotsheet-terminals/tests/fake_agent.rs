//! Integrated terminal E2E (HS2-1GJY50): drive the whole terminal-inference stack —
//! OSC-133 busy/idle, OSC-7 cwd, OSC-9 progress, the spinner hint, scrollback, and exit —
//! with the deterministic `hs-fake-agent` emulator, in one realistic multi-step sequence
//! (not isolated single-op tests).

use std::time::{Duration, Instant};

use hotsheet_terminals::{Activity, TermSpec, Terminal, contains_spinner};

#[path = "../../test-support/first_launch.rs"]
mod first_launch;
#[path = "../../test-support/workspace_bin.rs"]
mod workspace_bin;

/// The `hs-fake-agent` binary, launched once to completion before a test times anything
/// (HS2-W00050, HS2-04D6DW). macOS assesses a newly linked executable on its first launch
/// through one queue for the whole host; under load that queue exceeded the 5 s windows
/// below, so the agent printed nothing and never exited in time although its PTY plumbing
/// was fine.
fn fake_agent() -> String {
    first_launch::launched(workspace_bin::path("hs-fake-agent"), &["--exit", "0"])
        .into_os_string()
        .into_string()
        .expect("UTF-8 target directory")
}

fn wait_until(mut cond: impl FnMut() -> bool, secs: u64) -> bool {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        if cond() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    cond()
}

#[test]
fn hs2_2tt0yg_fake_agent_reports_v1_for_push_capable_fleet_launches() {
    let output = std::process::Command::new(fake_agent())
        .arg("--version")
        .output()
        .expect("probe fake agent version");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "hs-fake-agent 1.0.0\n"
    );
}

#[test]
fn fake_agent_drives_busy_cwd_progress_output_then_idle_and_exit() {
    let mut spec = TermSpec::new(fake_agent());
    spec.args = vec![
        "--cwd".into(),
        "/work/proj".into(),
        "--busy".into(),
        "--print".into(),
        "building the thing".into(),
        "--progress".into(),
        "42".into(),
        "--hold-ms".into(),
        "600".into(), // stay busy while holding
        "--idle".into(),
        "--exit".into(),
        "0".into(),
    ];
    let term = Terminal::spawn(spec).expect("spawn hs-fake-agent");

    // OSC-133 C → busy (observable during the 600ms hold).
    assert!(
        wait_until(|| term.activity() == Activity::Busy, 5),
        "OSC-133 command-start should mark the terminal busy"
    );
    // OSC-7 cwd + OSC-9 progress parsed off the same stream.
    assert!(
        wait_until(|| term.term_state().cwd.as_deref() == Some("/work/proj"), 5),
        "OSC-7 should surface the cwd"
    );
    assert!(
        wait_until(|| term.term_state().progress == Some(42), 5),
        "OSC-9;4 should surface progress"
    );
    // The printed line reached the scrollback.
    assert!(
        wait_until(
            || String::from_utf8_lossy(&term.scrollback()).contains("building the thing"),
            5
        ),
        "printed output should reach the scrollback"
    );

    // OSC-133 D → idle, then the process exits with the chosen code.
    assert!(wait_until(|| !term.is_alive(), 5), "the agent should exit");
    assert_eq!(
        term.activity(),
        Activity::Idle,
        "OSC-133 command-finish should leave the terminal idle"
    );
}

#[test]
fn fake_agent_spinner_is_detected_in_the_output() {
    let mut spec = TermSpec::new(fake_agent());
    spec.args = vec![
        "--spinner".into(),
        "--print".into(),
        "still working".into(),
        "--exit".into(),
        "0".into(),
    ];
    let term = Terminal::spawn(spec).expect("spawn hs-fake-agent");
    assert!(
        wait_until(
            || {
                let sb = term.scrollback();
                contains_spinner(&sb) && String::from_utf8_lossy(&sb).contains("still working")
            },
            5
        ),
        "the spinner glyph + text should reach the scrollback and be detected"
    );
}

#[test]
fn fake_agent_reports_a_nonzero_exit() {
    let mut spec = TermSpec::new(fake_agent());
    spec.args = vec!["--print".into(), "boom".into(), "--exit".into(), "3".into()];
    let term = Terminal::spawn(spec).expect("spawn hs-fake-agent");
    assert!(
        wait_until(|| !term.is_alive(), 5),
        "the agent should exit (non-zero code)"
    );
    assert!(String::from_utf8_lossy(&term.scrollback()).contains("boom"));
}

/// HS2-YM55P3: a program's own OSC-133 output markers drive busy and idle on every PTY,
/// Windows' pseudo console included. Busy holds until a typed line lets the program end its
/// turn.
#[test]
fn hs2_ym55p3_output_markers_hold_busy_until_a_typed_line_ends_the_turn() {
    let mut spec = TermSpec::new(fake_agent());
    spec.args = vec![
        "--busy".into(),
        "--wait-line".into(),
        "--idle".into(),
        "--hold-ms".into(),
        "30000".into(),
    ];
    let term = Terminal::spawn(spec).expect("spawn hs-fake-agent");
    assert!(
        wait_until(|| term.activity() == Activity::Busy, 5),
        "OSC-133 command-start output should mark the terminal busy"
    );
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(
        term.activity(),
        Activity::Busy,
        "busy until a line is typed"
    );
    term.write(b"done\r").expect("type a line");
    assert!(
        wait_until(|| term.activity() == Activity::Idle, 5),
        "OSC-133 command-finished output should mark the terminal idle"
    );
    assert!(term.is_alive(), "the program is still running, only idle");
    let _ = term.kill();
}
