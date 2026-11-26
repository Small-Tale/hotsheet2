//! HS2-C8J6WS: deterministic evidence retention, not a historical startup-cause reproduction.
//! HS2-KCM23W: a live broker that binds after the original 2 s window is still adopted, and
//! one that exited fails without waiting out the window.
#![cfg(unix)] // HS2-8GVYHQ: this out-of-line test module uses Unix sockets and shell fixtures.

use super::startup::{ContenderState, StartupContender, StartupWindow};
use super::{
    BrokerResponse, SpawnedBroker, TerminalBroker, child_state_with_stderr, tracked_child_state,
    wait_for_broker_startup, wait_for_startup,
};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

fn exited_child(home: &Path) -> hotsheet_process_journal::TrackedChild {
    let mut command = hotsheet_ticketing::git::launch("/bin/sh");
    command
        .args(["-c", "exit 17"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = fixture_child(home, &mut command);
    assert_eq!(child.wait().unwrap().code(), Some(17));
    child
}

fn fixture_child(home: &Path, command: &mut Command) -> hotsheet_process_journal::TrackedChild {
    hotsheet_process_journal::LaunchContext {
        journal: hotsheet_process_journal::ProcessJournal::at(home),
        supervisor: "startup-diagnostic-test",
        purpose: hotsheet_process_journal::Purpose::BrokerSupervisor,
        project_id: Some("synthetic".into()),
        session_id: None,
        binary_revision: None,
    }
    .spawn(command)
    .unwrap()
}

struct HeldChild(hotsheet_process_journal::TrackedChild);

impl Drop for HeldChild {
    fn drop(&mut self) {
        // Only this fixture's /bin/cat, held on its own stdin, is terminated.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Peer {
    path: PathBuf,
    stop: Arc<AtomicBool>,
    probes: Arc<AtomicUsize>,
    thread: Option<JoinHandle<()>>,
}

impl Peer {
    fn bind(path: PathBuf, pong: bool) -> Self {
        let listener = UnixListener::bind(&path).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let probes = Arc::new(AtomicUsize::new(0));
        let stopped = stop.clone();
        let counted = probes.clone();
        let thread = thread::spawn(move || {
            loop {
                let (mut stream, _) = listener.accept().unwrap();
                if stopped.load(Ordering::SeqCst) {
                    break;
                }
                stream
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                let mut request = String::new();
                BufReader::new(&mut stream).read_line(&mut request).unwrap();
                assert_eq!(request.trim(), r#"{"op":"ping"}"#);
                counted.fetch_add(1, Ordering::SeqCst);
                let response = if pong {
                    BrokerResponse::Pong
                } else {
                    BrokerResponse::Ok
                };
                writeln!(stream, "{}", serde_json::to_string(&response).unwrap()).unwrap();
            }
        });
        Self {
            path,
            stop,
            probes,
            thread: Some(thread),
        }
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = UnixStream::connect(&self.path);
        let result = self.thread.take().unwrap().join();
        if !thread::panicking() {
            result.unwrap();
        }
    }
}

#[test]
fn hs2_c8j6ws_exited_child_failure_retains_pid_status_and_absent_probe() {
    let home = tempfile::tempdir_in("/tmp").unwrap();
    let mut child = exited_child(home.path());
    let pid = child.id();
    let error = wait_for_broker_startup(&home.path().join("missing.sock"), &mut child)
        .unwrap_err()
        .to_string();
    assert!(error.contains(&format!("spawned pid {pid}")), "{error}");
    assert!(error.contains("child exited: exit status: 17"), "{error}");
    assert!(error.contains("last probe: absent"), "{error}");
}

#[test]
fn hs2_8gvyhq_detached_broker_failure_includes_child_stderr() {
    let home = tempfile::tempdir_in("/tmp").unwrap();
    let stderr = tempfile::NamedTempFile::new_in(home.path()).unwrap();
    let stderr_path = stderr.path().to_path_buf();
    let mut command = hotsheet_ticketing::git::launch("/bin/sh");
    command.args(["-c", "printf 'synthetic bind failure\\n' >&2; exit 17"]);
    let child = hotsheet_process_journal::LaunchContext {
        journal: hotsheet_process_journal::ProcessJournal::at(home.path()),
        supervisor: "startup-diagnostic-test",
        purpose: hotsheet_process_journal::Purpose::BrokerSupervisor,
        project_id: Some("synthetic".into()),
        session_id: None,
        binary_revision: None,
    }
    .spawn_detached_with_stderr_file(&mut command, stderr.path())
    .unwrap();
    let pid = child.id();
    let mut spawned = SpawnedBroker::Direct { child, stderr };
    let error = wait_for_startup(
        &home.path().join("missing.sock"),
        pid,
        "spawned",
        super::BROKER_STARTUP,
        || spawned.state(),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("child exited: exit status: 17"), "{error}");
    assert!(
        error.contains("broker stderr: synthetic bind failure"),
        "{error}"
    );
    assert!(error.contains("last probe: absent"), "{error}");
    assert!(stderr_path.exists());
    drop(spawned);
    assert!(!stderr_path.exists(), "failed startup kept its capture");
}

#[test]
fn hs2_8gvyhq_successful_startup_removes_its_capture() {
    let home = tempfile::tempdir_in("/tmp").unwrap();
    let stderr = tempfile::NamedTempFile::new_in(home.path()).unwrap();
    let stderr_path = stderr.path().to_path_buf();
    let mut command = hotsheet_ticketing::git::launch("/bin/sh");
    command.args(["-c", "exit 0"]);
    let child = hotsheet_process_journal::LaunchContext {
        journal: hotsheet_process_journal::ProcessJournal::at(home.path()),
        supervisor: "startup-diagnostic-test",
        purpose: hotsheet_process_journal::Purpose::BrokerSupervisor,
        project_id: Some("synthetic".into()),
        session_id: None,
        binary_revision: None,
    }
    .spawn_detached_with_stderr_file(&mut command, stderr.path())
    .unwrap();
    let pid = child.id();
    let mut spawned = SpawnedBroker::Direct { child, stderr };
    let peer = Peer::bind(home.path().join("winner.sock"), true);
    wait_for_startup(&peer.path, pid, "spawned", super::BROKER_STARTUP, || {
        spawned.state()
    })
    .unwrap();
    if let SpawnedBroker::Direct { child, .. } = &mut spawned {
        child.wait().unwrap();
    }
    assert!(stderr_path.exists());
    drop(spawned);
    assert!(!stderr_path.exists(), "healthy startup kept its capture");
}

#[test]
fn hs2_8gvyhq_failure_includes_at_most_four_kib_of_stderr() {
    let home = tempfile::tempdir_in("/tmp").unwrap();
    let stderr = tempfile::NamedTempFile::new_in(home.path()).unwrap();
    std::fs::write(stderr.path(), format!("{}TAIL", "x".repeat(4_096))).unwrap();
    let mut child = exited_child(home.path());
    let state = child_state_with_stderr(&mut child, stderr.path());
    let captured = state.describe().split("broker stderr: ").nth(1).unwrap();
    assert_eq!(captured.len(), 4_096);
    assert!(!captured.contains("TAIL"));
}

#[test]
fn hs2_c8j6ws_exited_contender_still_adopts_a_healthy_winner() {
    let home = tempfile::tempdir_in("/tmp").unwrap();
    let mut child = exited_child(home.path());
    let peer = Peer::bind(home.path().join("winner.sock"), true);
    wait_for_broker_startup(&peer.path, &mut child).unwrap();
    assert_eq!(peer.probes.load(Ordering::SeqCst), 1);
}

#[test]
fn hs2_c8j6ws_accepting_non_pong_retains_probe_kind_and_all_attempts() {
    let home = tempfile::tempdir_in("/tmp").unwrap();
    let mut command = hotsheet_ticketing::git::launch("/bin/cat");
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = HeldChild(fixture_child(home.path(), &mut command));
    let peer = Peer::bind(home.path().join("non-pong.sock"), false);
    // The grace window alone: a live child is otherwise waited for up to the ceiling.
    let window = StartupWindow {
        grace_attempts: 40,
        ceiling: Duration::ZERO,
    };
    let pid = child.0.id();
    let error = wait_for_startup(&peer.path, pid, "spawned", window, || {
        tracked_child_state(&mut child.0)
    })
    .unwrap_err()
    .to_string();
    assert!(error.contains("last probe: InvalidData"), "{error}");
    assert!(error.contains("os error None"), "{error}");
    assert!(error.contains("child still running"), "{error}");
    assert_eq!(peer.probes.load(Ordering::SeqCst), 40);
}

fn live_child(home: &Path) -> HeldChild {
    // /bin/cat on a held stdin pipe stays alive until the fixture drops it.
    let mut command = hotsheet_ticketing::git::launch("/bin/cat");
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    HeldChild(fixture_child(home, &mut command))
}

impl StartupContender for HeldChild {
    fn state(&mut self) -> ContenderState {
        tracked_child_state(&mut self.0)
    }
}

/// Past the original 40 x 50 ms window, as a first macOS launch can be (HS2-04D6DW: 3.6 s).
const SLOW_BIND: Duration = Duration::from_millis(2_500);

#[test]
fn hs2_kcm23w_a_live_broker_that_binds_after_two_seconds_is_adopted() {
    let home = tempfile::tempdir_in("/tmp").unwrap();
    let mut child = live_child(home.path());
    let socket = home.path().join("slow.sock");
    let bind_at = socket.clone();
    let binder = thread::spawn(move || {
        thread::sleep(SLOW_BIND);
        Peer::bind(bind_at, true)
    });
    let started = Instant::now();
    let result = wait_for_broker_startup(&socket, &mut child.0);
    let peer = binder.join().unwrap();
    result.unwrap();
    assert!(started.elapsed() >= SLOW_BIND);
    assert!(peer.probes.load(Ordering::SeqCst) >= 1);
}

#[test]
fn hs2_kcm23w_an_exited_broker_fails_without_waiting_out_the_window() {
    let home = tempfile::tempdir_in("/tmp").unwrap();
    let mut child = exited_child(home.path());
    let started = Instant::now();
    let error = wait_for_broker_startup(&home.path().join("missing.sock"), &mut child)
        .unwrap_err()
        .to_string();
    assert!(started.elapsed() < Duration::from_secs(1), "{error}");
    assert!(error.contains("child exited: exit status: 17"), "{error}");
}

#[test]
fn hs2_kcm23w_a_live_broker_that_never_binds_fails_at_the_ceiling() {
    let home = tempfile::tempdir_in("/tmp").unwrap();
    let mut child = live_child(home.path());
    let pid = child.0.id();
    let window = StartupWindow {
        grace_attempts: 2,
        ceiling: Duration::from_millis(400),
    };
    let started = Instant::now();
    let error = wait_for_startup(
        &home.path().join("never.sock"),
        pid,
        "spawned",
        window,
        || tracked_child_state(&mut child.0),
    )
    .unwrap_err()
    .to_string();
    let elapsed = started.elapsed();
    assert!(elapsed >= Duration::from_millis(400), "{elapsed:?}");
    assert!(elapsed < Duration::from_secs(5), "{elapsed:?}");
    assert!(error.contains("child still running"), "{error}");
    assert!(error.contains("last probe: absent"), "{error}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hs2_kcm23w_a_restart_adopts_a_live_broker_that_binds_after_two_seconds() {
    let home = tempfile::tempdir_in("/tmp").unwrap();
    let socket = home.path().join("restart.sock");
    let broker = TerminalBroker::at(&socket, "proj");
    let bind_at = socket.clone();
    let contender_home = home.path().to_owned();
    let started = Instant::now();
    let mut client = broker
        .connect_or_restart(move || {
            tokio::spawn(async move {
                tokio::time::sleep(SLOW_BIND).await;
                let listener = tokio::net::UnixListener::bind(&bind_at).unwrap();
                hotsheet_terminals::serve_broker(
                    listener,
                    "proj".into(),
                    Arc::new(hotsheet_terminals::TerminalManager::new()),
                )
                .await;
            });
            Ok(live_child(&contender_home))
        })
        .await
        .unwrap();
    assert!(started.elapsed() >= SLOW_BIND);
    assert!(matches!(
        client
            .request(&hotsheet_terminals::BrokerRequest::Ping)
            .await
            .unwrap(),
        BrokerResponse::Pong
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hs2_kcm23w_a_restart_whose_broker_exited_fails_fast() {
    let home = tempfile::tempdir_in("/tmp").unwrap();
    let broker = TerminalBroker::at(home.path().join("gone.sock"), "proj");
    let contender_home = home.path().to_owned();
    let started = Instant::now();
    let error = broker
        .connect_or_restart(move || Ok(HeldChild(exited_child(&contender_home))))
        .await
        .err()
        .expect("an exited broker never comes up")
        .to_string();
    assert!(started.elapsed() < Duration::from_secs(1), "{error}");
    assert!(error.contains("child exited: exit status: 17"), "{error}");
}
