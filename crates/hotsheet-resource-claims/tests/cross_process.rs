//! HS2-H3WPPY: separate processes contending for one resource through the local
//! backend's files. The helper is this test binary re-run in a role chosen by
//! `HS_RESOURCE_CLAIMS_ROLE`, so no helper binary ships in the crate.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

use hotsheet_resource_claims::{AcquireRequest, ClaimBackend, LocalBackend, ResourceRef};
use hotsheet_ticketing::FileLock;

const ROLE: &str = "HS_RESOURCE_CLAIMS_ROLE";
const HOME: &str = "HS_RESOURCE_CLAIMS_HOME";
const LONG: Duration = Duration::from_secs(120);
const PROMPT: Duration = Duration::from_secs(30);

fn target() -> ResourceRef {
    ResourceRef::parse("vms/mac-1").unwrap()
}

fn gate(home: &Path) -> std::fs::File {
    std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(home.join("gate"))
        .unwrap()
}

/// The helper entry point. As an ordinary test it does nothing.
#[test]
fn helper_process() {
    let Ok(role) = std::env::var(ROLE) else {
        return;
    };
    let home = std::path::PathBuf::from(std::env::var(HOME).unwrap());
    let backend = LocalBackend::machine(&home);
    let say = |line: String| {
        println!("RC:{line}");
        std::io::stdout().flush().unwrap();
    };
    let block_on_stdin = || {
        let mut sink = Vec::new();
        std::io::stdin().read_to_end(&mut sink).unwrap();
    };
    match role.as_str() {
        // Start together with the other contenders, then try once.
        "try" => {
            drop(FileLock::acquire(gate(&home)).unwrap());
            match backend.acquire(&target(), &AcquireRequest::new(LONG)) {
                Ok(grant) => say(format!("granted {}", grant.claim.fencing_token)),
                Err(error) => say(error.code().to_owned()),
            }
        }
        // Queue for the resource, hold it briefly, pass it on.
        "queue" => {
            drop(FileLock::acquire(gate(&home)).unwrap());
            let grant = backend
                .acquire_wait(&target(), &AcquireRequest::new(LONG), LONG)
                .unwrap();
            let seen = backend.get(&target()).unwrap().unwrap();
            assert_eq!(seen.claim_id, grant.claim.claim_id);
            backend.release(&grant.renewal_token).unwrap();
            say(format!("granted {}", grant.claim.fencing_token));
        }
        // Hold until the parent closes stdin, then release.
        "hold" => {
            let grant = backend
                .acquire(&target(), &AcquireRequest::new(LONG))
                .unwrap();
            say(format!("holding {}", grant.renewal_token));
            block_on_stdin();
            backend.release(&grant.renewal_token).unwrap();
            say("released".into());
        }
        // Take a short lease and die without releasing it.
        "crash" => {
            let lease = Duration::from_millis(1_500);
            let grant = backend
                .acquire(&target(), &AcquireRequest::new(lease))
                .unwrap();
            say(format!("holding {}", grant.renewal_token));
            std::process::exit(3);
        }
        other => panic!("unknown helper role {other}"),
    }
}

struct Helper {
    child: Child,
    lines: BufReader<ChildStdout>,
}

fn spawn(role: &str, home: &Path) -> Helper {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "helper_process", "--nocapture"])
        .env(ROLE, role)
        .env(HOME, home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let lines = BufReader::new(child.stdout.take().unwrap());
    Helper { child, lines }
}

impl Helper {
    /// The next `RC:` line the helper printed (the test harness prints others).
    fn line(&mut self) -> String {
        let mut text = String::new();
        loop {
            text.clear();
            assert_ne!(
                self.lines.read_line(&mut text).unwrap(),
                0,
                "helper ended early"
            );
            if let Some((_, line)) = text.split_once("RC:") {
                return line.trim().to_owned();
            }
        }
    }
}

#[test]
fn hs2_h3wppy_concurrent_processes_get_exactly_one_grant() {
    let home = tempfile::tempdir().unwrap();
    // Hold the gate so every contender is running before any of them tries.
    let closed = FileLock::acquire(gate(home.path())).unwrap();
    let mut helpers: Vec<Helper> = (0..8).map(|_| spawn("try", home.path())).collect();
    drop(closed);

    let mut outcomes: Vec<String> = helpers.iter_mut().map(Helper::line).collect();
    for helper in &mut helpers {
        assert!(helper.child.wait().unwrap().success());
    }
    outcomes.sort();
    let mut expected = vec!["held_by".to_owned(); 7];
    expected.insert(0, "granted 1".to_owned());
    assert_eq!(outcomes, expected);
    assert_eq!(
        LocalBackend::machine(home.path())
            .get(&target())
            .unwrap()
            .unwrap()
            .fencing_token,
        1
    );
}

#[test]
fn hs2_h3wppy_queued_processes_each_hold_once_with_rising_tokens() {
    let home = tempfile::tempdir().unwrap();
    let closed = FileLock::acquire(gate(home.path())).unwrap();
    let mut helpers: Vec<Helper> = (0..6).map(|_| spawn("queue", home.path())).collect();
    drop(closed);

    let mut tokens: Vec<String> = helpers.iter_mut().map(Helper::line).collect();
    for helper in &mut helpers {
        assert!(helper.child.wait().unwrap().success());
    }
    tokens.sort();
    let expected: Vec<String> = (1..=6).map(|n| format!("granted {n}")).collect();
    assert_eq!(tokens, expected);
    assert_eq!(
        LocalBackend::machine(home.path()).get(&target()).unwrap(),
        None
    );
}

#[test]
fn hs2_h3wppy_a_release_in_another_process_wakes_this_waiter() {
    let home = tempfile::tempdir().unwrap();
    let mut holder = spawn("hold", home.path());
    let token = holder.line().strip_prefix("holding ").unwrap().to_owned();
    let backend = LocalBackend::machine(home.path());
    assert!(backend.get(&target()).unwrap().is_some());

    let started = Instant::now();
    let waiter = std::thread::scope(|scope| {
        let waiting =
            scope.spawn(|| backend.acquire_wait(&target(), &AcquireRequest::new(LONG), LONG));
        while backend.inspections() < 2 {
            assert!(started.elapsed() < PROMPT);
            std::thread::yield_now();
        }
        // Closing stdin tells the holder process to release.
        drop(holder.child.stdin.take());
        waiting.join().unwrap()
    });
    let grant = waiter.unwrap();
    assert!(started.elapsed() < PROMPT, "woken by the other process");
    assert_eq!(grant.claim.fencing_token, 2);
    assert_eq!(holder.line(), "released");
    assert!(holder.child.wait().unwrap().success());
    // The other process's token was revoked by its own release and then superseded.
    assert_eq!(backend.release(&token).unwrap_err().code(), "not_holder");
}

#[test]
fn hs2_h3wppy_a_crashed_holder_keeps_its_lease_until_expiry_only() {
    let home = tempfile::tempdir().unwrap();
    let mut crashed = spawn("crash", home.path());
    let token = crashed.line().strip_prefix("holding ").unwrap().to_owned();
    assert_eq!(crashed.child.wait().unwrap().code(), Some(3));

    let backend = LocalBackend::machine(home.path());
    // The process is gone but the lease is not: nobody may take the resource early.
    let dead = backend
        .get(&target())
        .unwrap()
        .expect("lease survives the crash");
    let error = backend
        .acquire(&target(), &AcquireRequest::new(LONG))
        .unwrap_err();
    assert_eq!(error.holder().unwrap().claim_id, dead.claim_id);

    let grant = backend
        .acquire_wait(&target(), &AcquireRequest::new(LONG), LONG)
        .unwrap();
    assert!(grant.claim.acquired_at_ms >= dead.expires_at_ms);
    assert_eq!(grant.claim.fencing_token, dead.fencing_token + 1);
    // If the crashed task comes back with its old token, it is fenced out.
    let renew = backend.renew(&token, LONG);
    assert_eq!(renew.unwrap_err().code(), "not_holder");
    assert_eq!(
        backend
            .verify_fencing(&target(), dead.fencing_token)
            .unwrap_err()
            .code(),
        "not_holder"
    );
}
