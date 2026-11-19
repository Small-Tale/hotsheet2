//! Runs a plugin command once (HS2-322ZSZ): the envelope on stdin, one result line on
//! stdout, killed when it outlives its timeout. No shell is involved.

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// What a finished plugin process left behind.
#[derive(Debug, Clone)]
pub struct ProcessOutput {
    pub success: bool,
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RunError {
    #[error("could not start '{program}': {message}")]
    Spawn { program: String, message: String },
    #[error("'{program}' did not finish within {seconds}s and was stopped")]
    Timeout { program: String, seconds: u64 },
    #[error("waiting for '{program}': {message}")]
    Wait { program: String, message: String },
}

/// Injected so unit tests never start a process.
pub trait CommandRunner: Send + Sync {
    fn run(
        &self,
        program: &str,
        args: &[String],
        stdin: &[u8],
        timeout: Duration,
    ) -> Result<ProcessOutput, RunError>;
}

/// Spawns real processes.
#[derive(Debug, Default, Clone, Copy)]
pub struct LiveRunner;

impl CommandRunner for LiveRunner {
    fn run(
        &self,
        program: &str,
        args: &[String],
        stdin: &[u8],
        timeout: Duration,
    ) -> Result<ProcessOutput, RunError> {
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| RunError::Spawn {
                program: program.to_owned(),
                message: e.to_string(),
            })?;
        if let Some(mut pipe) = child.stdin.take() {
            // A plugin that exits without reading stdin closes the pipe; its exit status
            // and output then explain the failure, so a write error is not reported here.
            let _ = pipe.write_all(stdin);
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
                    return Err(RunError::Timeout {
                        program: program.to_owned(),
                        seconds: timeout.as_secs(),
                    });
                }
                // A local child-process wait; no network request is issued here.
                Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                Err(e) => {
                    return Err(RunError::Wait {
                        program: program.to_owned(),
                        message: e.to_string(),
                    });
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
        return text.to_owned();
    }
    let mut start = text.len() - max;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    format!("…{}", &text[start..])
}
