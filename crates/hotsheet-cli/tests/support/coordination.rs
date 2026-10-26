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

    pub(super) fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_hotsheet-cli"));
        cmd.arg("-C")
            .arg(self.store.root())
            .args(args)
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
        let stderr = child.stderr.take().unwrap();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stderr);
            let mut line = String::new();
            let _ = reader.read_line(&mut line);
            let _ = ready_tx.send(line);
        });
        let line = ready_rx
            .recv_timeout(Duration::from_secs(10))
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
        Waiting { child, output }
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
}

impl Waiting {
    pub(super) fn result(&mut self) -> Snapshot {
        let bytes = self
            .output
            .recv_timeout(Duration::from_secs(10))
            .expect("event-backed wait did not wake");
        assert!(self.child.wait().unwrap().success());
        serde_json::from_slice(&bytes).unwrap()
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
