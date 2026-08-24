//! Integrated terminal E2E (HS2-1GJY50): drive the whole terminal-inference stack —
//! OSC-133 busy/idle, OSC-7 cwd, OSC-9 progress, the spinner hint, scrollback, and exit —
//! with the deterministic `hs-fake-agent` emulator, in one realistic multi-step sequence
//! (not isolated single-op tests).

use std::time::{Duration, Instant};

use hotsheet_terminals::{Activity, TermSpec, Terminal, contains_spinner};

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
fn fake_agent_drives_busy_cwd_progress_output_then_idle_and_exit() {
    let exe = env!("CARGO_BIN_EXE_hs-fake-agent");
    let mut spec = TermSpec::new(exe);
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
    let exe = env!("CARGO_BIN_EXE_hs-fake-agent");
    let mut spec = TermSpec::new(exe);
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
    let exe = env!("CARGO_BIN_EXE_hs-fake-agent");
    let mut spec = TermSpec::new(exe);
    spec.args = vec!["--print".into(), "boom".into(), "--exit".into(), "3".into()];
    let term = Terminal::spawn(spec).expect("spawn hs-fake-agent");
    assert!(
        wait_until(|| !term.is_alive(), 5),
        "the agent should exit (non-zero code)"
    );
    assert!(String::from_utf8_lossy(&term.scrollback()).contains("boom"));
}
