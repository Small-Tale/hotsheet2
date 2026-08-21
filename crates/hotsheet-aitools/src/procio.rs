//! Shared child-process stdio plumbing for the **stream transports** (codex app-server,
//! claude channel): spawn a process with piped stdio and split it into newline-delimited
//! [`RpcWriter`]/[`RpcReader`] halves. The reader owns the [`Child`] so the process stays
//! alive for the connection's lifetime and is reaped when the reader thread ends.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use crate::ports::{RpcReader, RpcWriter};

/// A spawned tool process whose stdio carries newline-delimited JSON.
pub(crate) struct StreamChild {
    child: Child,
}

impl StreamChild {
    /// Spawn `program args…` in `cwd` (with extra env), piping stdin/stdout for JSON.
    pub(crate) fn spawn(
        program: &str,
        args: &[&str],
        cwd: &Path,
        envs: &[(String, String)],
    ) -> std::io::Result<Self> {
        let mut cmd = Command::new(program);
        cmd.args(args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        for (k, v) in envs {
            cmd.env(k, v);
        }
        Ok(Self {
            child: cmd.spawn()?,
        })
    }

    /// Split into the write half and the read half (the latter owns the [`Child`]).
    pub(crate) fn into_halves(mut self) -> (Box<dyn RpcWriter>, Box<dyn RpcReader>) {
        let stdin = self.child.stdin.take().expect("piped stdin");
        let stdout = self.child.stdout.take().expect("piped stdout");
        (
            Box::new(PipeWriter { stdin }),
            Box::new(PipeReader {
                lines: BufReader::new(stdout),
                _child: self.child,
            }),
        )
    }
}

struct PipeWriter {
    stdin: ChildStdin,
}
impl RpcWriter for PipeWriter {
    fn send(&mut self, msg: &str) -> std::io::Result<()> {
        self.stdin.write_all(msg.as_bytes())?;
        self.stdin.write_all(b"\n")?;
        self.stdin.flush()
    }
}

struct PipeReader {
    lines: BufReader<ChildStdout>,
    _child: Child,
}
impl RpcReader for PipeReader {
    fn recv(&mut self) -> std::io::Result<Option<String>> {
        let mut line = String::new();
        match self.lines.read_line(&mut line)? {
            0 => Ok(None), // EOF
            _ => Ok(Some(line.trim_end().to_string())),
        }
    }
}
