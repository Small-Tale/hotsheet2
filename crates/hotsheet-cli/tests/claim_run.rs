//! Real process claim guards and attempt fences (HS2-MFYYJ9).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use hotsheet_model::{ClaimEventKind, Ticket, Timestamp, Ulid};
use hotsheet_ticketing::{FsStore, NewTicket, StoreError, StoreMetadata, git as hotsheet_git, ops};
use time::OffsetDateTime;

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
        let mut command = Command::new(env!("CARGO_BIN_EXE_hotsheet-cli"));
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
            .env("HS2_MFYYJ9_PROJECT", self.root.path().join("project"))
            .env(
                "HS2_MFYYJ9_GRANDCHILD_STARTED",
                self.root.path().join("grandchild-started"),
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

    fn wait_file(&self, name: &str) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !self.root.path().join(name).exists() {
            assert!(Instant::now() < deadline, "{name} did not appear");
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn hold_ticket_lock(&self, duration: Duration) -> thread::JoinHandle<()> {
        let store = self.store.clone();
        let marker = self.root.path().join("locked");
        let holder = thread::spawn(move || {
            store
                .with_ticket_transaction(|| {
                    fs::write(marker, "held").unwrap();
                    thread::sleep(duration);
                    Ok::<(), StoreError>(())
                })
                .unwrap();
        });
        self.wait_file("locked");
        holder
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
    match std::env::var("HS2_MFYYJ9_MODE").unwrap().as_str() {
        "grandchild-exit" | "grandchild-wait" => {
            let mut grandchild = Command::new(std::env::current_exe().unwrap());
            grandchild
                .args(["--exact", "claim_run_grandchild", "--nocapture"])
                .env("HS2_MFYYJ9_GRANDCHILD", "commit")
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit());
            #[expect(clippy::zombie_processes, reason = "HS2-MFYYJ9: parent exits first")]
            grandchild.spawn().unwrap();
            let started = PathBuf::from(std::env::var("HS2_MFYYJ9_GRANDCHILD_STARTED").unwrap());
            let deadline = Instant::now() + Duration::from_secs(5);
            while !started.exists() {
                assert!(Instant::now() < deadline, "grandchild did not start");
                thread::sleep(Duration::from_millis(10));
            }
            if std::env::var("HS2_MFYYJ9_MODE").unwrap() == "grandchild-wait" {
                thread::sleep(Duration::from_millis(delay));
            }
        }
        "commit" => {
            let project = PathBuf::from(std::env::var("HS2_MFYYJ9_PROJECT").unwrap());
            fs::write(project.join("synthetic.txt"), "late commit\n").unwrap();
            git(&project, &["add", "synthetic.txt"]);
            git(&project, &["commit", "-qm", "synthetic late commit"]);
        }
        "fail" => panic!("synthetic child failure"),
        _ => fs::write(std::env::var("HS2_MFYYJ9_MARKER").unwrap(), "completed").unwrap(),
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
    let delay = std::env::var("HS2_MFYYJ9_GRANDCHILD_DELAY_MS")
        .unwrap()
        .parse()
        .unwrap();
    thread::sleep(Duration::from_millis(delay));
    let project = PathBuf::from(std::env::var("HS2_MFYYJ9_PROJECT").unwrap());
    fs::write(project.join("synthetic.txt"), "late grandchild commit\n").unwrap();
    git(&project, &["add", "synthetic.txt"]);
    git(
        &project,
        &["commit", "-qm", "synthetic late grandchild commit"],
    );
}

#[test]
fn hs2_mfyyj9_delayed_child_renews_the_same_claim_attempt_and_reconnects() {
    let f = Fixture::new();
    f.claim("worker");
    let attempt = f.ticket().claim_history.last().unwrap().id;
    let output = f
        .guard("worker", "3", "marker", "4500")
        .wait_with_output()
        .unwrap();
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
    assert!(
        ticket
            .claim_history
            .iter()
            .filter(|event| event.kind == ClaimEventKind::Renew)
            .count()
            >= 3
    );
    assert_eq!(ticket.claim_history[0].id, attempt);

    fs::remove_file(f.root.path().join("started")).unwrap();
    fs::remove_file(f.root.path().join("completed")).unwrap();
    let reconnect = f
        .guard("worker", "3", "marker", "50")
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

#[test]
fn hs2_mfyyj9_held_lock_cannot_revive_expired_attempt_or_allow_grandchild_commit() {
    let f = Fixture::new();
    let head = f.init_project();
    f.claim("worker");
    let child = f.guard("worker", "2", "grandchild-wait", "4000");
    f.wait_file("grandchild-started");
    let holder = f.hold_ticket_lock(Duration::from_millis(2800));
    // HS2-FDZGFD: snapshot only once the lock is held. The guard renews every second, so
    // a snapshot taken earlier can miss a renewal that commits before the hold begins.
    let before = f.ticket().claim_history;
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("lease deadline passed"));
    holder.join().unwrap();
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

#[test]
fn hs2_mfyyj9_held_lock_during_final_check_cannot_report_late_success() {
    let f = Fixture::new();
    f.claim("worker");
    let child = f.guard("worker", "2", "marker", "700");
    f.wait_started();
    let holder = f.hold_ticket_lock(Duration::from_millis(2800));
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("final authority check failed"));
    holder.join().unwrap();
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
    assert!(
        include_str!("../../../.github/workflows/ci.yml")
            .contains("cargo test -p hotsheet-cli --test claim_run")
    );
}
