//! The `codex-queue` session-push adapter (HS2-FKM97G): `codex queue --thread <id> --message
//! <text>` (stable since codex 0.149.0, no feature flag) queues a user turn for an existing
//! session. An idle session starts a turn with it; a busy one takes it as its next turn.
//!
//! The address is **reported, not launch-wired**: the Codex `SessionStart` hook reports the
//! thread id (`session_id`) and the `CODEX_HOME` it runs under, and the host registers this
//! adapter for the terminal then ([`super::adapter_for_reported`]).
//!
//! Observed on codex 0.162.1 (`tests/fixtures/codex-0.162.1-session-queue.json`):
//!
//! - `SessionStart` fires lazily, when the first prompt is submitted, and its `session_id` is
//!   the thread id `--thread` accepts.
//! - Success exits 0 within about 0.1 s with `Queued message <id> for thread <id>.`; the TUI
//!   shows the message as a user turn and fires `UserPromptSubmit`, so a halt clears as for
//!   typed input.
//! - The queue is per `CODEX_HOME`, so a TUI started with `--no-daemon` under the same home is
//!   still reached. A thread the home does not know (including one under a different home)
//!   exits 1 with `no rollout found for thread id`.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::{PushError, PushMessage, PushReceipt, SessionPush};

/// The manifest mechanism name.
pub const MECHANISM: &str = "codex-queue";

/// The reported address field naming the session's `CODEX_HOME` (absent = the default home).
pub const CODEX_HOME_FIELD: &str = "codex_home";

/// The longest prompt accepted, in bytes; far below the platform's argument limit.
pub const MAX_MESSAGE_BYTES: usize = 32 * 1024;

/// How long `codex queue` may run before it is killed and reported unreachable.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(20);

/// Pushes into one Codex thread through `codex queue`.
#[derive(Debug, Clone)]
pub struct CodexQueuePush {
    program: PathBuf,
    thread_id: String,
    codex_home: Option<PathBuf>,
    timeout: Duration,
}

impl CodexQueuePush {
    /// `None` when `thread_id` cannot be passed to codex safely (empty, option-shaped, or
    /// containing anything but a UUID's or session name's characters).
    pub fn new(
        program: impl Into<PathBuf>,
        thread_id: &str,
        codex_home: Option<PathBuf>,
    ) -> Option<Self> {
        let thread_id = thread_id.trim();
        is_thread_id(thread_id).then(|| Self {
            program: program.into(),
            thread_id: thread_id.to_owned(),
            codex_home,
            timeout: DEFAULT_TIMEOUT,
        })
    }

    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// The exact command a prompt runs. The `--flag=value` forms keep text that starts with
    /// `-` from being read as an option; no shell is involved.
    pub fn command(&self, text: &str) -> Result<Command, PushError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(PushError::Invalid("the message is empty".into()));
        }
        if text.len() > MAX_MESSAGE_BYTES {
            return Err(PushError::Invalid(format!(
                "the message is longer than {MAX_MESSAGE_BYTES} bytes"
            )));
        }
        let mut cmd = hotsheet_ticketing::git::launch(&self.program);
        cmd.arg("queue")
            .arg(format!("--thread={}", self.thread_id))
            .arg(format!("--message={text}"))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        match &self.codex_home {
            Some(home) => cmd.env("CODEX_HOME", home),
            // The session used the default home; the server's own override must not leak in.
            None => cmd.env_remove("CODEX_HOME"),
        };
        Ok(cmd)
    }
}

impl SessionPush for CodexQueuePush {
    fn mechanism(&self) -> &'static str {
        MECHANISM
    }

    fn session_id(&self) -> Option<String> {
        Some(self.thread_id.clone())
    }

    fn message(&self, message: &PushMessage) -> Result<PushReceipt, PushError> {
        // Codex has no banner; the queued turn itself is visible in the TUI.
        let PushMessage::Prompt { text, .. } = message else {
            return Err(PushError::Unsupported(
                "Codex sessions accept prompts, not UI commands".into(),
            ));
        };
        let output = run_bounded(self.command(text)?, self.timeout).map_err(|error| {
            PushError::Unreachable(match error {
                Bounded::Timeout => format!("codex queue timed out after {:?}", self.timeout),
                Bounded::Io(reason) => {
                    format!("could not run {}: {reason}", self.program.display())
                }
            })
        })?;
        if output.success && queued_message_id(&output.stdout).is_some() {
            return Ok(PushReceipt {
                mechanism: MECHANISM,
                session_id: Some(self.thread_id.clone()),
                confirmed: true,
            });
        }
        if output.success {
            // Exit 0 without the confirmation line: accepted, but not confirmed queued.
            return Ok(PushReceipt {
                mechanism: MECHANISM,
                session_id: Some(self.thread_id.clone()),
                confirmed: false,
            });
        }
        // An unknown thread (or one under another home) is unreachable from here.
        Err(PushError::Unreachable(codex_error_line(
            &output.stderr,
            &output.stdout,
        )))
    }
}

/// A Codex thread id (a UUID) or exact session name: bounded, not option-shaped, and limited
/// to UUID/name characters.
fn is_thread_id(id: &str) -> bool {
    !id.is_empty()
        && !id.starts_with('-')
        && id.len() <= 256
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
}

/// The message id from `Queued message <id> for thread <id>.`
pub fn queued_message_id(stdout: &str) -> Option<String> {
    stdout.lines().find_map(|line| {
        let rest = line.trim().strip_prefix("Queued message ")?;
        let id = rest.split_whitespace().next()?;
        (!id.is_empty()).then(|| id.to_owned())
    })
}

/// The readable reason from a failed `codex queue`: its `Error:` line without the prefix, else
/// the last non-empty line. Codex prints unrelated `WARNING:` lines first.
pub fn codex_error_line(stderr: &str, stdout: &str) -> String {
    let lines = || {
        stderr
            .lines()
            .chain(stdout.lines())
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with("WARNING:"))
    };
    lines()
        .find_map(|line| line.strip_prefix("Error:").map(str::trim))
        .map(str::to_owned)
        .or_else(|| lines().next_back().map(str::to_owned))
        .unwrap_or_else(|| "codex queue failed without a message".to_owned())
}

struct BoundedOutput {
    success: bool,
    stdout: String,
    stderr: String,
}

enum Bounded {
    Timeout,
    Io(String),
}

/// Run `cmd` to completion, killing it after `timeout`. Output is drained on helper threads
/// so a chatty child never blocks on a full pipe.
fn run_bounded(mut cmd: Command, timeout: Duration) -> Result<BoundedOutput, Bounded> {
    let mut child = cmd.spawn().map_err(|e| Bounded::Io(e.to_string()))?;
    fn drain(pipe: Option<impl Read + Send + 'static>) -> std::thread::JoinHandle<String> {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut bytes);
            }
            String::from_utf8_lossy(&bytes).into_owned()
        })
    }
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Bounded::Timeout);
            }
            // A local child-process wait, not a network poll.
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(Bounded::Io(e.to_string())),
        }
    };
    Ok(BoundedOutput {
        success: status.success(),
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::super::{ReportedSession, adapter_for_reported, is_reported, launch_wiring};
    use super::*;
    use std::path::Path;
    use std::sync::Arc;

    const THREAD: &str = "01a14736-2ae1-7e92-8da0-5f8db11b19b0";

    #[cfg(unix)]
    fn prompt(text: &str) -> PushMessage {
        PushMessage::Prompt {
            text: text.into(),
            banner: Some("ignored".into()),
        }
    }

    fn spec() -> hotsheet_plugins::SessionPushSpec {
        let plugin = hotsheet_plugins::find_in("codex", &[]).unwrap();
        plugin.manifest.session_push.unwrap()
    }

    #[test]
    fn the_codex_manifest_declares_a_reported_mechanism_without_launch_wiring() {
        let spec = spec();
        assert_eq!(spec.mechanism, MECHANISM);
        assert!(is_reported(&spec));
        assert!(super::super::is_supported(&spec));
        let launch = super::super::PushLaunch {
            port: 1,
            password: "p".into(),
            directory: "/d".into(),
            model: None,
            launched_at_ms: 0,
        };
        assert_eq!(launch_wiring(&spec, &launch), None);
        assert!(
            super::super::adapter_for(&spec, launch, Arc::new(super::super::UreqHttp::default()))
                .is_none()
        );
    }

    #[test]
    fn a_report_needs_a_valid_thread_id_and_carries_its_home() {
        let spec = spec();
        let reported = |session: Option<&str>, home: Option<&str>| ReportedSession {
            session_id: session.map(str::to_owned),
            fields: home
                .map(|home| [(CODEX_HOME_FIELD.to_owned(), home.to_owned())].into())
                .unwrap_or_default(),
        };
        let program = Path::new("/opt/codex");
        for bad in [None, Some(""), Some("--remote=x"), Some("a b")] {
            assert!(
                adapter_for_reported(&spec, &reported(bad, None), program).is_none(),
                "{bad:?}"
            );
        }
        let adapter =
            adapter_for_reported(&spec, &reported(Some(THREAD), Some("/h")), program).unwrap();
        assert_eq!(adapter.mechanism(), MECHANISM);
        assert_eq!(adapter.session_id().as_deref(), Some(THREAD));
        let opencode = hotsheet_plugins::find_in("opencode", &[])
            .and_then(|plugin| plugin.manifest.session_push)
            .unwrap();
        assert!(adapter_for_reported(&opencode, &reported(Some(THREAD), None), program).is_none());
    }

    #[test]
    fn the_command_passes_thread_message_and_home_without_a_shell() {
        let push = CodexQueuePush::new("/opt/codex", THREAD, Some("/h".into())).unwrap();
        let cmd = push.command("  -please check HS2-1  ").unwrap();
        assert_eq!(cmd.get_program(), "/opt/codex");
        let args: Vec<_> = cmd.get_args().map(|a| a.to_string_lossy()).collect();
        assert_eq!(
            args,
            [
                "queue".to_owned(),
                format!("--thread={THREAD}"),
                "--message=-please check HS2-1".to_owned(),
            ]
        );
        assert!(
            cmd.get_envs()
                .any(|(key, value)| key == "CODEX_HOME" && value == Some("/h".as_ref()))
        );
        let default_home = CodexQueuePush::new("codex", THREAD, None)
            .unwrap()
            .command("hi")
            .unwrap();
        assert!(
            default_home
                .get_envs()
                .any(|(key, value)| key == "CODEX_HOME" && value.is_none())
        );
        assert!(matches!(push.command(" \n "), Err(PushError::Invalid(_))));
        assert!(matches!(
            push.command(&"x".repeat(MAX_MESSAGE_BYTES + 1)),
            Err(PushError::Invalid(_))
        ));
        assert!(push.command(&"x".repeat(MAX_MESSAGE_BYTES)).is_ok());
    }

    #[test]
    fn queue_output_and_error_lines_match_codex_0_162_1() {
        assert_eq!(
            queued_message_id(&format!("Queued message m-1 for thread {THREAD}.\n")).as_deref(),
            Some("m-1")
        );
        assert_eq!(queued_message_id("something else\n"), None);
        assert_eq!(
            codex_error_line(
                "WARNING: proceeding\nError: failed to queue session message: no rollout found\n",
                ""
            ),
            "failed to queue session message: no rollout found"
        );
        assert_eq!(codex_error_line("", "boom\n\n"), "boom");
        assert_eq!(
            codex_error_line("WARNING: only\n", ""),
            "codex queue failed without a message"
        );
    }

    #[cfg(unix)]
    fn fake_codex(dir: &Path, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("codex");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[cfg(unix)]
    #[test]
    fn messages_are_queued_confirmed_refused_unreachable_or_timed_out() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("argv");
        let ok = fake_codex(
            dir.path(),
            &format!(
                "printf '%s\\n' \"$CODEX_HOME\" \"$@\" > '{}'\necho 'WARNING: noise' >&2\necho 'Queued message m-1 for thread {THREAD}.'",
                log.display()
            ),
        );
        let push = CodexQueuePush::new(&ok, THREAD, Some("/tmp/h".into())).unwrap();
        let receipt = push.message(&prompt("line one\nline two")).unwrap();
        assert_eq!(
            receipt,
            PushReceipt {
                mechanism: MECHANISM,
                session_id: Some(THREAD.into()),
                confirmed: true
            }
        );
        assert_eq!(
            std::fs::read_to_string(&log).unwrap(),
            format!("/tmp/h\nqueue\n--thread={THREAD}\n--message=line one\nline two\n")
        );
        assert!(matches!(
            push.message(&PushMessage::Command {
                command: "agent_cycle".into()
            }),
            Err(PushError::Unsupported(_))
        ));

        let silent = fake_codex(dir.path(), "exit 0");
        assert!(
            !CodexQueuePush::new(&silent, THREAD, None)
                .unwrap()
                .message(&prompt("hi"))
                .unwrap()
                .confirmed
        );

        let refused = fake_codex(
            dir.path(),
            "echo 'Error: failed to queue session message: no rollout found' >&2\nexit 1",
        );
        assert_eq!(
            CodexQueuePush::new(&refused, THREAD, None)
                .unwrap()
                .message(&prompt("hi"))
                .unwrap_err(),
            PushError::Unreachable("failed to queue session message: no rollout found".into())
        );

        let missing = dir.path().join("absent-codex");
        assert!(matches!(
            CodexQueuePush::new(&missing, THREAD, None)
                .unwrap()
                .message(&prompt("hi")),
            Err(PushError::Unreachable(_))
        ));

        let slow = fake_codex(dir.path(), "exec sleep 30");
        let started = Instant::now();
        let timed_out = CodexQueuePush::new(&slow, THREAD, None)
            .unwrap()
            .with_timeout(Duration::from_millis(200))
            .message(&prompt("hi"))
            .unwrap_err();
        assert!(
            matches!(timed_out, PushError::Unreachable(ref reason) if reason.contains("timed out"))
        );
        assert!(started.elapsed() < Duration::from_secs(10));
    }
}
