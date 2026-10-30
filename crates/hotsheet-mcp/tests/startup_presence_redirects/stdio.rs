//! Owned stdio child and source-free project fixtures; never a real server or credential.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use hotsheet_ticketing::{FsStore, StoreMetadata};
use serde_json::{Value, json};

use super::http::{DEADLINE, SECRET};

pub(super) struct Project {
    _root: tempfile::TempDir,
    pub home: PathBuf,
    pub root: PathBuf,
    instance: PathBuf,
    record: String,
}
impl Project {
    pub fn new(origin: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let bootstrap = home.join("server-bootstrap.hs2");
        FsStore::init(&bootstrap, &StoreMetadata::new("SYN")).unwrap();
        let project = root.path().join("project");
        std::fs::create_dir(&project).unwrap();
        hotsheet_ticketing::checkouts::CheckoutRegistry::new(home.join("checkouts.json"))
            .register_sources(&project, None, None, vec![], None)
            .unwrap();
        let instance = hotsheet_cli::external_launch::instance_path(&home, &bootstrap);
        std::fs::create_dir_all(instance.parent().unwrap()).unwrap();
        let record = json!({"pid":std::process::id(), "url":origin, "secret":SECRET,
            "store_path":bootstrap.canonicalize().unwrap()})
        .to_string();
        std::fs::write(&instance, &record).unwrap();
        assert_eq!(
            hotsheet_cli::external_launch::discover_running_server(&bootstrap, &home)
                .unwrap()
                .url,
            origin
        );
        Self {
            _root: root,
            home,
            root: project,
            instance,
            record,
        }
    }
    pub fn assert_no_relocation(&self) {
        assert_eq!(
            std::fs::read_to_string(&self.instance).unwrap(),
            self.record
        );
        // ensure_running_server would journal CLI setup if the live fixture record failed.
        if let Ok(entries) = std::fs::read_dir(self.home.join("processes")) {
            for entry in entries {
                let path = entry.unwrap().path();
                if path
                    .extension()
                    .is_some_and(|extension| extension == "json")
                {
                    let value: Value =
                        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
                    assert_ne!(value["purpose"], "cli_setup");
                }
            }
        }
    }
    pub fn spawn(&self) -> Process {
        self.assert_no_relocation();
        let live = hotsheet_cli::external_launch::discover_running_server(
            &self.home.join("server-bootstrap.hs2"),
            &self.home,
        )
        .unwrap();
        assert_eq!(live.pid, std::process::id());
        Process::start(
            &self.home,
            &["--project", self.root.to_str().unwrap()],
            None,
        )
    }
}

pub(super) struct Completed {
    pub status: ExitStatus,
    pub replies: Vec<Value>,
    pub stderr: String,
}

pub(super) struct Process {
    child: Option<Child>,
    input: Option<ChildStdin>,
    messages: mpsc::Receiver<Value>,
    stdout: Option<JoinHandle<Vec<Value>>>,
    stderr: Option<JoinHandle<Vec<u8>>>,
}
impl Process {
    pub fn start(home: &Path, arguments: &[&str], presence_origin: Option<&str>) -> Self {
        for dir in [
            "user-home",
            "codex-home",
            "cache",
            "config",
            "data",
            "state",
        ] {
            std::fs::create_dir_all(home.join(dir)).unwrap();
        }
        let mut command = Command::new(env!("CARGO_BIN_EXE_hotsheet-mcp"));
        command
            .args(arguments)
            .current_dir(home)
            .env("HOME", home.join("user-home"))
            .env("CODEX_HOME", home.join("codex-home"))
            .env("HOTSHEET_HOME", home)
            .env("HOTSHEET_CACHE_DIR", home.join("cache"))
            .env("XDG_CACHE_HOME", home.join("cache"))
            .env("XDG_CONFIG_HOME", home.join("config"))
            .env("XDG_DATA_HOME", home.join("data"))
            .env("XDG_STATE_HOME", home.join("state"))
            .env("HOTSHEET_AI_TOOL_PROBES", "off")
            .env_remove("HOTSHEET_STORE")
            .env_remove("HOTSHEET_SERVER")
            .env_remove("HOTSHEET_SECRET")
            .env_remove("HOTSHEET_TERMINAL_ID")
            .env_remove("HOTSHEET_AGENT")
            .env_remove("HOTSHEET_WORKER_ID")
            .env_remove("HOTSHEET_ACTOR_ROLE")
            .env_remove("HOTSHEET_ACTOR_ID")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(origin) = presence_origin {
            command
                .env("HOTSHEET_SERVER", origin)
                .env("HOTSHEET_SECRET", SECRET);
        }
        let mut child = command.spawn().unwrap();
        let input = child.stdin.take();
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let (sent, messages) = mpsc::channel();
        let stdout = thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut replies = Vec::new();
            loop {
                let mut line = Vec::new();
                let count = (&mut reader)
                    .take(256 * 1024 + 1)
                    .read_until(b'\n', &mut line)
                    .unwrap();
                if count == 0 {
                    break;
                }
                assert!(
                    line.len() <= 256 * 1024,
                    "synthetic MCP output bound exceeded"
                );
                let value: Value = serde_json::from_slice(&line).unwrap();
                let _ = sent.send(value.clone());
                replies.push(value);
                assert!(replies.len() <= 64, "synthetic MCP reply count exceeded");
            }
            replies
        });
        let stderr = thread::spawn(move || {
            let mut bytes = Vec::new();
            stderr.take(65_537).read_to_end(&mut bytes).unwrap();
            assert!(
                bytes.len() <= 65_536,
                "synthetic MCP diagnostic bound exceeded"
            );
            bytes
        });
        Self {
            child: Some(child),
            input,
            messages,
            stdout: Some(stdout),
            stderr: Some(stderr),
        }
    }
    pub fn send(&mut self, value: Value) -> std::io::Result<()> {
        let input = self.input.as_mut().unwrap();
        writeln!(input, "{value}")?;
        input.flush()
    }
    pub fn message(&self) -> Value {
        self.messages.recv_timeout(DEADLINE).unwrap()
    }
    pub fn wait_before(&mut self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            if self.child.as_mut().unwrap().try_wait().unwrap().is_some() {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
    pub fn finish(mut self) -> Completed {
        self.input.take();
        assert!(
            self.wait_before(Duration::from_secs(15)),
            "owned MCP fixture timed out"
        );
        let status = self.child.take().unwrap().wait().unwrap();
        let replies = self.stdout.take().unwrap().join().unwrap();
        let stderr = String::from_utf8(self.stderr.take().unwrap().join().unwrap()).unwrap();
        Completed {
            status,
            replies,
            stderr,
        }
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        self.input.take();
        if let Some(child) = &mut self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(reader) = self.stdout.take() {
            let _ = reader.join();
        }
        if let Some(reader) = self.stderr.take() {
            let _ = reader.join();
        }
    }
}
