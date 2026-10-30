//! HS2-C8J6WS: deterministic evidence retention, not a historical startup-cause reproduction.

use super::{BrokerResponse, wait_for_broker_startup};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

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
    let error = wait_for_broker_startup(&peer.path, &mut child.0)
        .unwrap_err()
        .to_string();
    assert!(error.contains("last probe: InvalidData"), "{error}");
    assert!(error.contains("os error None"), "{error}");
    assert!(error.contains("child still running"), "{error}");
    assert_eq!(peer.probes.load(Ordering::SeqCst), 40);
}
