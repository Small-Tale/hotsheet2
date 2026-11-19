//! Cross-process wake-ups for waiters, without polling.
//!
//! A waiter binds a loopback datagram socket and publishes its port as a small file in
//! the namespace's `waiters` directory *before* it inspects claim state. Whoever frees a
//! resource sends one datagram to every published port *after* publishing the new state.
//! Either the waiter's inspection sees the freed state or the releaser sees the waiter,
//! so no wake-up is lost. A datagram carries no information: a waiter that receives one
//! re-reads the authoritative state, so a stray or forged datagram costs one read.

use std::fs;
use std::io::{self, Write};
use std::net::{Ipv4Addr, UdpSocket};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Most waiters one namespace registers at a time.
pub(crate) const MAX_WAITERS: usize = 256;
/// How long after its own deadline a crashed waiter's registration is kept.
const STALE_GRACE_MS: u64 = 60_000;

pub(crate) struct Waiter {
    socket: UdpSocket,
    path: PathBuf,
}

impl Waiter {
    /// Publish a waiter that stops waiting at `deadline_ms` (backend clock).
    /// `Ok(None)` means the namespace already has [`MAX_WAITERS`] waiters.
    pub(crate) fn register(dir: &Path, deadline_ms: u64) -> io::Result<Option<Self>> {
        fs::create_dir_all(dir)?;
        let registered = fs::read_dir(dir)?
            .filter_map(Result::ok)
            .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
            .count();
        if registered >= MAX_WAITERS {
            return Ok(None);
        }
        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))?;
        let port = socket.local_addr()?.port();
        let path = dir.join(ulid::Ulid::new().to_string());
        // Publish a complete record: a releaser must never read half a port number.
        let mut staged = tempfile::Builder::new()
            .prefix(".waiter-")
            .suffix(".tmp")
            .tempfile_in(dir)?;
        write!(staged, "{port} {deadline_ms}")?;
        staged.persist(&path).map_err(|error| error.error)?;
        Ok(Some(Self { socket, path }))
    }

    /// Sleep until a wake-up arrives or `timeout` passes, whichever is first.
    pub(crate) fn park(&self, timeout: Duration) -> io::Result<()> {
        let mut byte = [0u8; 1];
        // HS2-QGY1NN: under an `InterruptGuard`, poll the stop pipe beside the socket so a
        // SIGTERM or SIGINT ends the wait and this waiter's drop removes its registration.
        #[cfg(unix)]
        if let Some(stopped) = crate::interrupt::poll_socket(&self.socket, timeout)? {
            if stopped {
                return Err(io::Error::from(io::ErrorKind::Interrupted));
            }
            self.socket.set_nonblocking(true)?;
            while self.socket.recv(&mut byte).is_ok() {}
            return Ok(());
        }
        self.socket.set_nonblocking(false)?;
        self.socket
            .set_read_timeout(Some(timeout.max(Duration::from_millis(1))))?;
        match self.socket.recv(&mut byte) {
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) => {}
            Err(error) => return Err(error),
        }
        // Coalesce a burst of wake-ups into the one re-inspection that follows.
        self.socket.set_nonblocking(true)?;
        while self.socket.recv(&mut byte).is_ok() {}
        Ok(())
    }
}

impl Drop for Waiter {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Wake every registered waiter once, and drop registrations long past their deadline.
pub(crate) fn wake_all(dir: &Path, now_ms: u64) -> io::Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    let mut sender = None;
    for entry in entries.filter_map(Result::ok) {
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let path = entry.path();
        let record = fs::read_to_string(&path).unwrap_or_default();
        let mut fields = record.split_ascii_whitespace();
        let (Some(port), Some(deadline_ms)) = (
            fields.next().and_then(|port| port.parse::<u16>().ok()),
            fields.next().and_then(|at| at.parse::<u64>().ok()),
        ) else {
            let _ = fs::remove_file(&path);
            continue;
        };
        if deadline_ms.saturating_add(STALE_GRACE_MS) < now_ms {
            let _ = fs::remove_file(&path);
            continue;
        }
        if sender.is_none() {
            sender = Some(UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))?);
        }
        if let Some(sender) = &sender {
            // A waiter that already left has closed its port; that is not a failure.
            let _ = sender.send_to(b"w", (Ipv4Addr::LOCALHOST, port));
        }
    }
    Ok(())
}
