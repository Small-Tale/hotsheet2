//! The notifier plugin contract and the process runner shared by `speak` and external
//! notifiers.

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::capability::NotifierDescriptor;
use crate::render::Rendered;
use crate::settings::Resolved;

/// A failed delivery. `retryable` errors (network, 5xx, rate limit, timeout) are retried
/// with backoff; permanent ones (bad settings, rejected credentials) are not.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct DeliveryError {
    pub message: String,
    pub retryable: bool,
}

impl DeliveryError {
    pub fn retryable(message: impl Into<String>) -> DeliveryError {
        DeliveryError {
            message: message.into(),
            retryable: true,
        }
    }

    pub fn permanent(message: impl Into<String>) -> DeliveryError {
        DeliveryError {
            message: message.into(),
            retryable: false,
        }
    }
}

/// A notifier plugin: a descriptor plus a delivery function.
pub trait Notifier: Send + Sync {
    fn descriptor(&self) -> &NotifierDescriptor;
    /// Deliver one rendered message for `channel` (the configured channel id). Returns an
    /// optional provider-side message id.
    fn deliver(
        &self,
        channel: &str,
        rendered: &Rendered,
        settings: &Resolved,
    ) -> Result<Option<String>, DeliveryError>;
}

/// The result of a finished child process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessOutput {
    pub success: bool,
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// Runs a program without a shell. Injectable so tests never speak aloud or spawn plugins.
pub trait CommandRunner: Send + Sync {
    fn run(
        &self,
        program: &str,
        args: &[String],
        stdin: Option<&[u8]>,
        timeout: Duration,
    ) -> Result<ProcessOutput, DeliveryError>;
}

/// Spawns real processes, killing one that outlives `timeout`.
#[derive(Debug, Default, Clone, Copy)]
pub struct LiveRunner;

impl CommandRunner for LiveRunner {
    fn run(
        &self,
        program: &str,
        args: &[String],
        stdin: Option<&[u8]>,
        timeout: Duration,
    ) -> Result<ProcessOutput, DeliveryError> {
        let mut child = Command::new(program)
            .args(args)
            .stdin(if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| DeliveryError::permanent(format!("could not start '{program}': {e}")))?;
        if let (Some(input), Some(mut pipe)) = (stdin, child.stdin.take()) {
            // A plugin that exits without reading stdin closes the pipe; its exit status
            // and output then explain the failure, so a write error is not reported here.
            let _ = pipe.write_all(input);
        }
        let mut stdout = child.stdout.take();
        let mut stderr = child.stderr.take();
        let out_reader = std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(pipe) = stdout.as_mut() {
                let _ = pipe.read_to_end(&mut buf);
            }
            buf
        });
        let err_reader = std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(pipe) = stderr.as_mut() {
                let _ = pipe.read_to_end(&mut buf);
            }
            buf
        });
        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if started.elapsed() >= timeout => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(DeliveryError::retryable(format!(
                        "'{program}' did not finish within {}s",
                        timeout.as_secs()
                    )));
                }
                // Local child-process wait; no network request is issued here.
                Ok(None) => std::thread::sleep(Duration::from_millis(20)),
                Err(e) => {
                    return Err(DeliveryError::retryable(format!(
                        "waiting for '{program}': {e}"
                    )));
                }
            }
        };
        Ok(ProcessOutput {
            success: status.success(),
            code: status.code(),
            stdout: out_reader.join().unwrap_or_default(),
            stderr: err_reader.join().unwrap_or_default(),
        })
    }
}

/// The last `max` bytes of a process stream as lossy text, for error messages.
pub(crate) fn tail(bytes: &[u8], max: usize) -> String {
    let text = String::from_utf8_lossy(bytes);
    let text = text.trim();
    if text.len() <= max {
        return text.to_string();
    }
    let mut start = text.len() - max;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    format!("…{}", &text[start..])
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn live_runner_passes_stdin_and_collects_output() {
        let out = LiveRunner
            .run(
                "sh",
                &["-c".into(), "cat; echo err >&2; exit 3".into()],
                Some(b"hello"),
                Duration::from_secs(10),
            )
            .unwrap();
        assert!(!out.success);
        assert_eq!(out.code, Some(3));
        assert_eq!(out.stdout, b"hello");
        assert_eq!(tail(&out.stderr, 100), "err");
    }

    #[test]
    fn live_runner_kills_a_process_past_its_timeout() {
        let err = LiveRunner
            .run(
                "sh",
                &["-c".into(), "sleep 5".into()],
                None,
                Duration::from_millis(100),
            )
            .unwrap_err();
        assert!(
            err.retryable && err.message.contains("did not finish"),
            "{err}"
        );
    }

    #[test]
    fn missing_program_is_permanent() {
        let err = LiveRunner
            .run(
                "hotsheet-no-such-program",
                &[],
                None,
                Duration::from_secs(1),
            )
            .unwrap_err();
        assert!(!err.retryable);
    }

    #[test]
    fn tail_keeps_the_end_on_a_char_boundary() {
        assert_eq!(tail("abcdé".as_bytes(), 2), "…é");
    }
}
