//! Real process claim guards and attempt fences (HS2-MFYYJ9).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use hotsheet_model::{ClaimEventKind, Ticket, Timestamp, Ulid};
use hotsheet_ticketing::{FsStore, NewTicket, StoreError, StoreMetadata, git as hotsheet_git, ops};
use time::OffsetDateTime;

#[path = "../../test-support/first_launch.rs"]
mod first_launch;

/// The `hotsheet-cli` binary, launched once to completion before any window that times its
/// child starts, so the macOS first-launch assessment is not timed (HS2-04D6DW).
fn cli_binary() -> &'static str {
    first_launch::launched(env!("CARGO_BIN_EXE_hotsheet-cli"), &["--version"])
}

/// The upper bound of a wait on state that should arrive, never a timing window under test.
const SAFETY_BOUND: Duration = Duration::from_secs(60);

struct Fixture {
    root: tempfile::TempDir,
    store: FsStore,
    ticket: Ticket,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let store = FsStore::init(root.path().join("store"), &StoreMetadata::new("HS")).unwrap();
        let ticket = ops::create(
            &store,
            Ulid::new(),
            "HS",
            Timestamp::from_datetime(OffsetDateTime::now_utc()),
            NewTicket {
                title: "synthetic guarded command".into(),
                ..NewTicket::default()
            },
        )
        .unwrap();
        Self {
            root,
            store,
            ticket,
        }
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(cli_binary());
        command
            .arg("-C")
            .arg(self.store.root())
            .args(args)
            .env("HOTSHEET_ACTOR_ROLE", "system")
            .env_remove("HOTSHEET_ACTOR_ID");
        command
    }

    fn cli(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }

    fn claim(&self, worker: &str) {
        let output = self.cli(&["claim", &self.ticket.slug, "--worker", worker]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn ticket(&self) -> Ticket {
        self.store.read_ticket(&self.ticket.id).unwrap()
    }

    fn guard(&self, worker: &str, lease_seconds: &str, mode: &str, delay_ms: &str) -> Child {
        self.guard_command(worker, lease_seconds, mode, delay_ms)
            .spawn()
            .unwrap()
    }

    fn guard_command(
        &self,
        worker: &str,
        lease_seconds: &str,
        mode: &str,
        delay_ms: &str,
    ) -> Command {
        let mut command = self.command(&[
            "claim-run",
            &self.ticket.slug,
            "--worker",
            worker,
            "--lease-seconds",
            lease_seconds,
            "--",
        ]);
        command
            .arg(std::env::current_exe().unwrap())
            .args(["--exact", "claim_run_child", "--nocapture"])
            .env("HS2_MFYYJ9_MODE", mode)
            .env("HS2_MFYYJ9_DELAY_MS", delay_ms)
            .env("HS2_MFYYJ9_STARTED", self.root.path().join("started"))
            .env("HS2_MFYYJ9_MARKER", self.root.path().join("completed"))
            .env("HS2_MFYYJ9_RELEASE", self.root.path().join("release"))
            .env("HS2_MFYYJ9_PROJECT", self.root.path().join("project"))
            .env(
                "HS2_MFYYJ9_GRANDCHILD_STARTED",
                self.root.path().join("grandchild-started"),
            )
            .env(
                "HS2_MFYYJ9_GRANDCHILD_GATE",
                self.root.path().join("grandchild-gate"),
            )
            .env(
                "HS2_MFYYJ9_GRANDCHILD_DELAY_MS",
                if mode == "grandchild-exit" {
                    "1200"
                } else {
                    "3500"
                },
            )
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    fn wait_started(&self) {
        self.wait_file("started");
    }

    /// Wait for a marker file. The bound only stops a hung test: a host stall on a loaded CI
    /// runner suspended every process for about 13 s, so it is not a timing window (HS2-EGMJ8G).
    fn wait_file(&self, name: &str) {
        let deadline = Instant::now() + SAFETY_BOUND;
        while !self.root.path().join(name).exists() {
            assert!(Instant::now() < deadline, "{name} did not appear");
            thread::sleep(Duration::from_millis(20));
        }
    }

    /// Hold the ticket lock until the returned sender sends or is dropped (HS2-K1M08D).
    fn hold_ticket_lock_until_released(
        &self,
    ) -> (std::sync::mpsc::Sender<()>, thread::JoinHandle<()>) {
        let store = self.store.clone();
        let marker = self.root.path().join("locked");
        let (release, held) = std::sync::mpsc::channel::<()>();
        let holder = thread::spawn(move || {
            store
                .with_ticket_transaction(|| {
                    fs::write(marker, "held").unwrap();
                    let _ = held.recv();
                    Ok::<(), StoreError>(())
                })
                .unwrap();
        });
        self.wait_file("locked");
        (release, holder)
    }

    fn init_project(&self) -> String {
        let project = self.root.path().join("project");
        fs::create_dir(&project).unwrap();
        git(&project, &["init", "-q", "-b", "main"]);
        git(&project, &["config", "user.name", "Synthetic Worker"]);
        git(
            &project,
            &["config", "user.email", "synthetic@example.invalid"],
        );
        fs::write(project.join("synthetic.txt"), "initial\n").unwrap();
        git(&project, &["add", "synthetic.txt"]);
        git(&project, &["commit", "-qm", "synthetic initial commit"]);
        git(&project, &["rev-parse", "HEAD"])
    }
}

fn renew_count(ticket: &Ticket) -> usize {
    ticket
        .claim_history
        .iter()
        .filter(|event| event.kind == ClaimEventKind::Renew)
        .count()
}

fn git(project: &Path, args: &[&str]) -> String {
    let output = hotsheet_git::command_in(project)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

#[test]
fn claim_run_child() {
    let Ok(started) = std::env::var("HS2_MFYYJ9_STARTED") else {
        return;
    };
    fs::write(started, "started").unwrap();
    let delay = std::env::var("HS2_MFYYJ9_DELAY_MS")
        .unwrap()
        .parse()
        .unwrap();
    thread::sleep(Duration::from_millis(delay));
    let mode = std::env::var("HS2_MFYYJ9_MODE").unwrap();
    match mode.as_str() {
        "grandchild-exit" | "grandchild-wait" | "grandchild-hold" => {
            let mut grandchild = Command::new(std::env::current_exe().unwrap());
            grandchild
                .args(["--exact", "claim_run_grandchild", "--nocapture"])
                .env("HS2_MFYYJ9_GRANDCHILD", "commit")
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit());
            if mode == "grandchild-hold" {
                grandchild.env("HS2_MFYYJ9_GRANDCHILD_GATED", "1");
            }
            #[expect(clippy::zombie_processes, reason = "HS2-MFYYJ9: parent exits first")]
            grandchild.spawn().unwrap();
            let started = PathBuf::from(std::env::var("HS2_MFYYJ9_GRANDCHILD_STARTED").unwrap());
            let deadline = Instant::now() + SAFETY_BOUND;
            while !started.exists() {
                assert!(Instant::now() < deadline, "grandchild did not start");
                thread::sleep(Duration::from_millis(10));
            }
            match mode.as_str() {
                "grandchild-wait" => thread::sleep(Duration::from_millis(delay)),
                // HS2-EGMJ8G: keep the tree running until the guard stops it.
                "grandchild-hold" => wait_for_path(&release_path(), "the guard never stopped"),
                _ => {}
            }
        }
        "commit" => {
            let project = PathBuf::from(std::env::var("HS2_MFYYJ9_PROJECT").unwrap());
            fs::write(project.join("synthetic.txt"), "late commit\n").unwrap();
            git(&project, &["add", "synthetic.txt"]);
            git(&project, &["commit", "-qm", "synthetic late commit"]);
        }
        "fail" => panic!("synthetic child failure"),
        // HS2-K1M08D: exit only once the test releases it, then mark completion.
        "release-wait" => {
            wait_for_path(&release_path(), "the test never released the child");
            fs::write(std::env::var("HS2_MFYYJ9_MARKER").unwrap(), "completed").unwrap();
        }
        _ => fs::write(std::env::var("HS2_MFYYJ9_MARKER").unwrap(), "completed").unwrap(),
    }
}

fn release_path() -> PathBuf {
    PathBuf::from(std::env::var("HS2_MFYYJ9_RELEASE").unwrap())
}

/// Wait in a guarded child for a file the test writes; the bound only ends an orphan.
fn wait_for_path(path: &Path, message: &str) {
    let deadline = Instant::now() + SAFETY_BOUND;
    while !path.exists() {
        assert!(Instant::now() < deadline, "{message}");
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn claim_run_grandchild() {
    if std::env::var("HS2_MFYYJ9_GRANDCHILD").is_err() {
        return;
    }
    fs::write(
        std::env::var("HS2_MFYYJ9_GRANDCHILD_STARTED").unwrap(),
        "started",
    )
    .unwrap();
    if std::env::var_os("HS2_MFYYJ9_GRANDCHILD_GATED").is_some() {
        // HS2-EGMJ8G: commit only once the test opens the gate after the guard exited, so
        // the commit lands exactly when the guard failed to stop this grandchild.
        let gate = PathBuf::from(std::env::var("HS2_MFYYJ9_GRANDCHILD_GATE").unwrap());
        let deadline = Instant::now() + SAFETY_BOUND;
        while !gate.exists() {
            if Instant::now() >= deadline {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
    } else {
        let delay = std::env::var("HS2_MFYYJ9_GRANDCHILD_DELAY_MS")
            .unwrap()
            .parse()
            .unwrap();
        thread::sleep(Duration::from_millis(delay));
    }
    let project = PathBuf::from(std::env::var("HS2_MFYYJ9_PROJECT").unwrap());
    fs::write(project.join("synthetic.txt"), "late grandchild commit\n").unwrap();
    git(&project, &["add", "synthetic.txt"]);
    git(
        &project,
        &["commit", "-qm", "synthetic late grandchild commit"],
    );
}

/// HS2-EGMJ8G: the child runs until the store shows two scheduled renewals after the
/// preflight one, instead of for a fixed 4.5 s against a 3 s lease. The 6-second lease
/// renews 2 s before each deadline; the old 1 s margin failed (correctly, closed) when a CI
/// host stall held the renewal past it.
#[test]
fn hs2_mfyyj9_delayed_child_renews_the_same_claim_attempt_and_reconnects() {
    let f = Fixture::new();
    f.claim("worker");
    let attempt = f.ticket().claim_history.last().unwrap().id;
    let mut child = f.guard("worker", "6", "release-wait", "0");
    let deadline = Instant::now() + SAFETY_BOUND;
    while renew_count(&f.ticket()) < 3 {
        if let Some(status) = child.try_wait().unwrap() {
            let output = child.wait_with_output().unwrap();
            panic!(
                "the guard ended with {status} before two scheduled renewals: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        assert!(Instant::now() < deadline, "scheduled renewals never landed");
        thread::sleep(Duration::from_millis(50));
    }
    fs::write(f.root.path().join("release"), "release").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(f.root.path().join("completed").exists());
    let ticket = f.ticket();
    assert_eq!(ticket.claim_count, 1);
    assert_eq!(
        ticket
            .claim_history
            .iter()
            .filter(|event| event.kind == ClaimEventKind::Claim)
            .count(),
        1
    );
    assert!(renew_count(&ticket) >= 3);
    assert_eq!(ticket.claim_history[0].id, attempt);

    fs::remove_file(f.root.path().join("started")).unwrap();
    fs::remove_file(f.root.path().join("completed")).unwrap();
    let reconnect = f
        .guard("worker", "6", "marker", "50")
        .wait_with_output()
        .unwrap();
    assert!(
        reconnect.status.success(),
        "{}",
        String::from_utf8_lossy(&reconnect.stderr)
    );
    assert!(f.root.path().join("completed").exists());
    assert_eq!(f.ticket().claim_count, 1);
}

#[test]
fn hs2_mfyyj9_expired_renewal_refuses_and_requires_a_new_exact_claim() {
    let f = Fixture::new();
    f.claim("worker");
    let mut ticket = f.ticket();
    ticket.claim_lease_expires_at = Some(Timestamp::from_datetime(
        OffsetDateTime::now_utc() - time::Duration::seconds(1),
    ));
    f.store.write_ticket(&ticket).unwrap();
    let before = f.ticket().claim_history;
    let renew = f.cli(&["renew", &f.ticket.slug, "--worker", "worker"]);
    assert!(!renew.status.success());
    assert!(String::from_utf8_lossy(&renew.stderr).contains("lease expired"));
    assert_eq!(f.ticket().claim_history, before);
    let denied = f
        .guard("worker", "2", "marker", "50")
        .wait_with_output()
        .unwrap();
    assert!(!denied.status.success());
    assert!(!f.root.path().join("started").exists());
    f.claim("worker");
    assert_eq!(f.ticket().claim_count, 2);
    let resumed = f
        .guard("worker", "2", "marker", "50")
        .wait_with_output()
        .unwrap();
    assert!(
        resumed.status.success(),
        "{}",
        String::from_utf8_lossy(&resumed.stderr)
    );
}

#[test]
fn hs2_mfyyj9_takeover_stops_a_delayed_child_before_its_commit() {
    let f = Fixture::new();
    let project = f.root.path().join("project");
    let head = f.init_project();
    f.claim("worker");
    let child = f.guard("worker", "2", "commit", "4000");
    f.wait_started();
    ops::release(
        &f.store,
        &f.ticket.id,
        Timestamp::from_datetime(OffsetDateTime::now_utc()),
        "worker",
        false,
    )
    .unwrap();
    f.claim("replacement");
    let before = f.ticket().claim_history;
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("claim guard renewal failed"));
    assert_eq!(git(&project, &["rev-parse", "HEAD"]), head);
    assert_eq!(f.ticket().claim_history, before);
    assert_eq!(f.ticket().claimed_by.as_deref(), Some("replacement"));
}

#[test]
fn hs2_mfyyj9_reset_attempt_and_failed_child_never_report_authority() {
    let f = Fixture::new();
    f.claim("worker");
    let old = f.ticket().claim_history[0].id;
    let child = f.guard("worker", "2", "marker", "4000");
    f.wait_started();
    ops::release(
        &f.store,
        &f.ticket.id,
        Timestamp::from_datetime(OffsetDateTime::now_utc()),
        "worker",
        false,
    )
    .unwrap();
    f.claim("worker");
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    assert!(!f.root.path().join("completed").exists());
    assert_ne!(
        f.ticket()
            .claim_history
            .iter()
            .rev()
            .find(|event| event.kind == ClaimEventKind::Claim)
            .unwrap()
            .id,
        old
    );

    fs::remove_file(f.root.path().join("started")).unwrap();
    let failed = f
        .guard("worker", "2", "fail", "50")
        .wait_with_output()
        .unwrap();
    assert!(!failed.status.success());
    assert!(String::from_utf8_lossy(&failed.stderr).contains("guarded command failed"));
}

#[test]
fn hs2_mfyyj9_exited_parent_cannot_leave_stdio_holding_grandchild_commit() {
    let f = Fixture::new();
    let head = f.init_project();
    f.claim("worker");
    let output = f
        .guard("worker", "3", "grandchild-exit", "0")
        .wait_with_output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    // The grandchild inherited the guard's stdout/stderr. wait_with_output must
    // complete because the entire owned tree was stopped, not just its parent.
    thread::sleep(Duration::from_millis(1400));
    assert_eq!(
        git(&f.root.path().join("project"), &["rev-parse", "HEAD"]),
        head
    );
}

#[cfg(windows)]
#[test]
fn hs2_mfyyj9_windows_late_helper_open_preserves_start_gate_and_stops_descendant() {
    let f = Fixture::new();
    let head = f.init_project();
    f.claim("worker");
    let output = f
        .guard_command("worker", "3", "grandchild-exit", "0")
        .env(
            "HOTSHEET_TEST_CLAIM_RUN_PARENT_READY_PATH",
            f.root.path().join("parent-ready"),
        )
        .spawn()
        .unwrap()
        .wait_with_output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    f.wait_file("parent-ready");
    f.wait_started();
    thread::sleep(Duration::from_millis(1400));
    assert_eq!(
        git(&f.root.path().join("project"), &["rev-parse", "HEAD"]),
        head
    );
}

#[test]
fn hs2_mfyyj9_takeover_stops_a_grandchild_commit_with_inherited_stdio() {
    let f = Fixture::new();
    let head = f.init_project();
    f.claim("worker");
    let child = f.guard("worker", "2", "grandchild-wait", "4000");
    f.wait_file("grandchild-started");
    ops::release(
        &f.store,
        &f.ticket.id,
        Timestamp::from_datetime(OffsetDateTime::now_utc()),
        "worker",
        false,
    )
    .unwrap();
    f.claim("replacement");
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    thread::sleep(Duration::from_millis(3700));
    assert_eq!(
        git(&f.root.path().join("project"), &["rev-parse", "HEAD"]),
        head
    );
    assert_eq!(f.ticket().claimed_by.as_deref(), Some("replacement"));
}

/// HS2-EGMJ8G: the lock is held until the guard has exited, so the guard can only fail at
/// its own lease deadline; a fixed 2.8 s hold let a stalled guard instead renew after the
/// hold and fail differently. The grandchild commits only once the gate opens after the
/// guard exited, so no fixed grandchild timer races the stop. A 6-second lease leaves the
/// whole start-up before its first scheduled renewal.
#[test]
fn hs2_mfyyj9_held_lock_cannot_revive_expired_attempt_or_allow_grandchild_commit() {
    let f = Fixture::new();
    let head = f.init_project();
    f.claim("worker");
    let child = f.guard("worker", "6", "grandchild-hold", "0");
    f.wait_file("grandchild-started");
    let (release_lock, holder) = f.hold_ticket_lock_until_released();
    // HS2-FDZGFD: snapshot only once the lock is held, so a renewal that commits before the
    // hold begins is part of the baseline.
    let before = f.ticket().claim_history;
    let output = child.wait_with_output().unwrap();
    drop(release_lock);
    holder.join().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(stderr.contains("lease deadline passed"), "{stderr}");
    // A grandchild that outlived the guard commits as soon as the gate opens.
    fs::write(f.root.path().join("grandchild-gate"), "open").unwrap();
    thread::sleep(Duration::from_millis(800));
    assert_eq!(
        git(&f.root.path().join("project"), &["rev-parse", "HEAD"]),
        head
    );
    assert_eq!(f.ticket().claim_history, before);
    let renew = f.cli(&["renew", &f.ticket.slug, "--worker", "worker"]);
    assert!(!renew.status.success());
    f.claim("replacement");
    assert_eq!(f.ticket().claimed_by.as_deref(), Some("replacement"));
}

/// HS2-K1M08D: the child exits only after the test holds the ticket lock, and the lock is
/// held until the guard answers, so the final authority check is the one that waits. A
/// 6-second lease schedules the first renewal 4 seconds after preflight; with the old
/// 2-second lease and a timed 700 ms child, a slow child start under load crossed the
/// 1-second renewal point and that renewal, not the final check, hit the held lock.
#[test]
fn hs2_mfyyj9_held_lock_during_final_check_cannot_report_late_success() {
    let f = Fixture::new();
    f.claim("worker");
    let child = f.guard("worker", "6", "release-wait", "0");
    f.wait_started();
    let (release_lock, holder) = f.hold_ticket_lock_until_released();
    fs::write(f.root.path().join("release"), "release").unwrap();
    let output = child.wait_with_output().unwrap();
    drop(release_lock);
    holder.join().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(stderr.contains("final authority check failed"), "{stderr}");
    assert!(
        f.root.path().join("completed").exists(),
        "the child ran to completion"
    );
    let renews = f
        .ticket()
        .claim_history
        .iter()
        .filter(|event| event.kind == ClaimEventKind::Renew)
        .count();
    assert_eq!(renews, 1, "only the preflight renewal may have succeeded");
    f.claim("replacement");
    assert_eq!(f.ticket().claimed_by.as_deref(), Some("replacement"));
}

#[test]
fn hs2_mfyyj9_source_policy_pins_owned_tree_and_under_lock_clock() {
    let process = include_str!("../src/dispatch/claim_process.rs");
    let claims = include_str!("../src/dispatch/claims.rs");
    let ops = include_str!("../../hotsheet-ticketing/src/ops.rs");
    for phrase in [
        "process_group(0)",
        "libc::kill(-group",
        "AssignProcessToJobObject",
        "TerminateJobObject",
        "JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE",
        "WaitForSingleObject",
        "SetEvent",
        "_gate: StartGate",
        "_gate: gate",
        ".env_remove(START_GATE_ENV)",
        "guarded parent did not return from spawn",
    ] {
        assert!(
            process.contains(phrase),
            "missing owned-tree policy: {phrase}"
        );
    }
    assert!(claims.contains("claim lease deadline passed before renewal completed"));
    assert!(claims.contains("could not mark guarded parent spawn return"));
    assert!(
        claims
            .matches("could not stop guarded process tree")
            .count()
            >= 3
    );
    assert!(ops.contains("let (now, lease_expires) = clock();"));
    // HS2-WSE3FJ: the daily platform workflow's Windows targets run the whole workspace,
    // including this Job Object test, instead of a per-commit Windows subset.
    assert!(
        include_str!("../../../.github/workflows/platforms.yml")
            .contains("cargo nextest run --all-features")
    );
    assert!(
        include_str!("../../../scripts/platform-matrix.mjs").contains("runner: 'windows-latest'")
    );
}
