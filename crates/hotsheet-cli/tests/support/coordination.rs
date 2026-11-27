//! Real separate CLI processes, filesystem subscriptions, claims and worktrees.
//! No provider AI session is impersonated by these synthetic workers (HS2-76VMYG).

use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use hotsheet_cli::coordination::Snapshot;
use hotsheet_model::{Timestamp, Ulid};
use hotsheet_ticketing::{FsStore, NewTicket, StoreMetadata, git_connection_id, ops};

#[path = "../../../test-support/first_launch.rs"]
mod first_launch;
#[path = "../../../test-support/workspace_bin.rs"]
mod workspace_bin;

/// The `hotsheet-cli` binary, launched once to completion before any window that times its
/// child starts, so the macOS first-launch assessment is not timed (HS2-04D6DW).
fn cli_binary() -> std::path::PathBuf {
    first_launch::launched(workspace_bin::path("hotsheet-cli"), &["--version"])
}

/// An event-backed wait's idle wake bound. A waiter wakes within about 1 s on an idle or
/// CPU-loaded host (HS2-237JQE measured 500+ waits under CPU and fseventsd churn load).
pub(super) const IDLE_WAKE_BOUND: Duration = Duration::from_secs(10);

/// The default [`load_factor`]. HS2-237JQE: under a deep fseventsd backlog a live stream
/// delivers seconds to minutes late (HS2-P702Y8, HS2-BN9MMJ), so four waits in one full
/// local gate missed the bare 10 s bound together. Six makes the default bound nextest's
/// 60 s slow-timeout period (`.config/nextest.toml`): a missed wake still fails, at the
/// moment nextest would report the test SLOW and long before it terminates it.
pub(super) const DEFAULT_LOAD_FACTOR: u32 = 6;

/// `HOTSHEET_TEST_LOAD_FACTOR` (a positive integer) scales every coordination wake bound
/// for a host known to be slower still; unset or invalid uses [`DEFAULT_LOAD_FACTOR`].
pub(super) fn load_factor() -> u32 {
    load_factor_from(std::env::var("HOTSHEET_TEST_LOAD_FACTOR").ok().as_deref())
}

/// [`load_factor`] for one `HOTSHEET_TEST_LOAD_FACTOR` value.
pub(super) fn load_factor_from(value: Option<&str>) -> u32 {
    value
        .and_then(|value| value.trim().parse::<u32>().ok())
        .filter(|factor| *factor > 0)
        .unwrap_or(DEFAULT_LOAD_FACTOR)
}

/// How long a waiting child may take to subscribe, to wake once its event is written, or
/// to close stderr after exiting (HS2-237JQE).
pub(super) fn wake_bound() -> Duration {
    IDLE_WAKE_BOUND * load_factor()
}

pub(super) struct Fixture {
    pub(super) root: tempfile::TempDir,
    pub(super) project: PathBuf,
    pub(super) store: FsStore,
}

impl Fixture {
    pub(super) fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        fs::create_dir(&project).unwrap();
        git(&project, &["init", "-q", "-b", "main"]);
        // HS2-9TD8Q5: preserve fixture bytes through Windows worktree/cherry-pick flows.
        git(&project, &["config", "core.autocrlf", "false"]);
        fs::write(project.join("synthetic.txt"), "initial\n").unwrap();
        git(&project, &["add", "synthetic.txt"]);
        git(&project, &["commit", "-qm", "synthetic initial commit"]);
        let store = FsStore::init(root.path().join("store"), &StoreMetadata::new("HS")).unwrap();
        Self {
            root,
            project,
            store,
        }
    }

    /// This fixture's own machine home. HS2-7QPJ05: provider configuration is one file per
    /// machine home, so fixtures never share the test process's home.
    pub(super) fn home(&self) -> PathBuf {
        self.root.path().join("home")
    }

    pub(super) fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(cli_binary());
        cmd.arg("-C")
            .arg(self.store.root())
            .args(args)
            .env("HOTSHEET_HOME", self.home())
            .env("HOTSHEET_ACTOR_ROLE", "system")
            .env_remove("HOTSHEET_ACTOR_ID")
            .env_remove("HOTSHEET_WORKER_ID");
        cmd
    }

    pub(super) fn cli(&self, args: &[&str]) -> String {
        let out = self.command(args).output().unwrap();
        assert!(
            out.status.success(),
            "CLI failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }

    pub(super) fn coord(&self, args: &[&str]) -> Vec<String> {
        [
            vec![
                "coordination".into(),
                "--project".into(),
                self.project.display().to_string(),
            ],
            args.iter().map(|s| s.to_string()).collect(),
        ]
        .concat()
    }

    pub(super) fn run(&self, args: &[&str]) -> Snapshot {
        let args = self.coord(args);
        serde_json::from_str(&self.cli(&args.iter().map(String::as_str).collect::<Vec<_>>()))
            .unwrap()
    }

    pub(super) fn waiting(&self, args: &[&str]) -> Waiting {
        let args = self.coord(args);
        let mut child = self
            .command(&args.iter().map(String::as_str).collect::<Vec<_>>())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let (ready_tx, ready_rx) = mpsc::channel();
        let (stderr_tx, errors) = mpsc::channel();
        let stderr = child.stderr.take().unwrap();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stderr);
            let mut line = String::new();
            let _ = reader.read_line(&mut line);
            let _ = ready_tx.send(line.clone());
            let mut bytes = line.into_bytes();
            let _ = reader.read_to_end(&mut bytes);
            let _ = stderr_tx.send(bytes);
        });
        let line = ready_rx
            .recv_timeout(wake_bound())
            .expect("wait process readiness");
        assert!(
            line.starts_with("Waiting on local"),
            "wait did not subscribe: {line}"
        );
        let stdout = child.stdout.take().unwrap();
        let (tx, output) = mpsc::channel();
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = BufReader::new(stdout).read_to_end(&mut bytes);
            let _ = tx.send(bytes);
        });
        Waiting {
            child,
            output,
            errors,
        }
    }

    pub(super) fn ticket(&self) -> hotsheet_model::Ticket {
        ops::create(
            &self.store,
            Ulid::new(),
            "HS",
            Timestamp::from_datetime(time::OffsetDateTime::now_utc()),
            NewTicket {
                title: "synthetic independent task".into(),
                up_next: true,
                ..NewTicket::default()
            },
        )
        .unwrap()
    }

    pub(super) fn assign(
        &self,
        operation: &str,
        t: &hotsheet_model::Ticket,
        base: &str,
        root_generation: &str,
    ) {
        self.run(&[
            "assign",
            "--id",
            "integrator",
            "--session",
            root_generation,
            "--worker",
            "worker",
            "--worker-session",
            "worker-session",
            "--operation",
            operation,
            "--connection",
            &git_connection_id(&self.store),
            "--native-id",
            &t.id.to_string(),
            "--slug",
            &t.slug,
            "--base",
            base,
        ]);
    }
}

pub(super) struct Waiting {
    pub(super) child: Child,
    output: Receiver<Vec<u8>>,
    errors: Receiver<Vec<u8>>,
}

impl Waiting {
    pub(super) fn exit_output(&mut self) -> std::process::Output {
        self.exit_output_within(Duration::ZERO)
    }

    /// [`Self::exit_output`] for a wait that is due only after `due` (a known deadline,
    /// such as a claim expiry): the usual [`wake_bound`] starts when the deadline passes
    /// (HS2-2982P2).
    pub(super) fn exit_output_within(&mut self, due: Duration) -> std::process::Output {
        let stdout = self
            .output
            .recv_timeout(due + wake_bound())
            .expect("event-backed wait did not wake");
        let status = self.child.wait().unwrap();
        let stderr = self
            .errors
            .recv_timeout(wake_bound())
            .expect("wait process stderr did not close");
        std::process::Output {
            status,
            stdout,
            stderr,
        }
    }

    pub(super) fn result(&mut self) -> Snapshot {
        let output = self.exit_output();
        Self::snapshot_of(output)
    }

    /// [`Self::result`] for a wait that is due only after `due`.
    #[allow(dead_code)] // HS2-2982P2: only the claim-expiry wake test knows its deadline.
    pub(super) fn result_within(&mut self, due: Duration) -> Snapshot {
        let output = self.exit_output_within(due);
        Self::snapshot_of(output)
    }

    fn snapshot_of(output: std::process::Output) -> Snapshot {
        assert!(
            output.status.success(),
            "coordination waiter failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
}

impl Drop for Waiting {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub(super) fn git(root: &Path, args: &[&str]) -> String {
    let output = hotsheet_ticketing::git::launch("git")
        .arg("-C")
        .arg(root)
        .args([
            "-c",
            "user.name=Synthetic Worker",
            "-c",
            "user.email=worker@example.invalid",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}
