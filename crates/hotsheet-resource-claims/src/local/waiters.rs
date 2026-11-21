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
use std::time::{Duration, Instant};

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
        hotsheet_sync::files::persist(staged, &path)?;
        Ok(Some(Self { socket, path }))
    }

    /// Sleep until a wake-up arrives or `timeout` passes, whichever is first.
    pub(crate) fn park(&self, timeout: Duration) -> io::Result<()> {
        park_for(timeout, |left| self.wait_once(left))
    }

    /// One timed wait on the socket. `true` means a wake-up arrived; `false` means the
    /// OS timer returned, which may be before `timeout` has elapsed.
    fn wait_once(&self, timeout: Duration) -> io::Result<bool> {
        let mut byte = [0u8; 1];
        // HS2-QGY1NN: under an `InterruptGuard`, poll the stop pipe beside the socket so a
        // SIGTERM or SIGINT ends the wait and this waiter's drop removes its registration.
        #[cfg(unix)]
        let polled = match crate::interrupt::poll_socket(&self.socket, timeout)? {
            Some(true) => return Err(io::Error::from(io::ErrorKind::Interrupted)),
            Some(false) => true,
            None => false,
        };
        #[cfg(not(unix))]
        let polled = false;
        let mut woken = false;
        if !polled {
            self.socket.set_nonblocking(false)?;
            self.socket
                .set_read_timeout(Some(timeout.max(Duration::from_millis(1))))?;
            match self.socket.recv(&mut byte) {
                Ok(_) => woken = true,
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) => {}
                Err(error) => return Err(error),
            }
        }
        // Coalesce a burst of wake-ups into the one re-inspection that follows.
        self.socket.set_nonblocking(true)?;
        while self.socket.recv(&mut byte).is_ok() {
            woken = true;
        }
        Ok(woken)
    }
}

/// HS2-H9DNG6: park until `wait` reports a wake-up or `timeout` has fully elapsed on the
/// monotonic clock. A timed socket wait can return before its timeout (Windows ARM64 did,
/// and Windows truncates a read timeout to whole milliseconds); returning then would make
/// the caller re-inspect claim state for nothing, so an early timer return waits out the
/// remainder instead. Only a wake-up or the planned instant costs an inspection.
fn park_for(
    timeout: Duration,
    mut wait: impl FnMut(Duration) -> io::Result<bool>,
) -> io::Result<()> {
    let until = Instant::now() + timeout.max(Duration::from_millis(1));
    loop {
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Ok(());
        }
        // Whole milliseconds, rounded up: a truncated timeout would wake a hair early.
        let millis = left.as_nanos().div_ceil(1_000_000);
        if wait(Duration::from_millis(
            u64::try_from(millis).unwrap_or(u64::MAX),
        ))? {
            return Ok(());
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// HS2-H9DNG6: a timer that returns early (as `SO_RCVTIMEO` did on Windows ARM64)
    /// must not end the park before its timeout; the caller would re-inspect for nothing.
    #[test]
    fn hs2_h9dng6_an_early_timer_return_waits_out_the_remainder() {
        let calls = Cell::new(0u32);
        let started = Instant::now();
        let timeout = Duration::from_millis(120);
        park_for(timeout, |left| {
            calls.set(calls.get() + 1);
            // The first two timed waits return at once, reporting no wake-up; later
            // ones sleep what was asked. Nothing here depends on the OS sleep's accuracy.
            if calls.get() > 2 {
                std::thread::sleep(left);
            }
            Ok(false)
        })
        .unwrap();
        assert!(started.elapsed() >= timeout, "returned before the timeout");
        assert!(calls.get() >= 3, "each early return was re-waited");
    }

    #[test]
    fn hs2_h9dng6_a_wake_up_ends_the_park_at_once() {
        let calls = Cell::new(0u32);
        let started = Instant::now();
        park_for(Duration::from_secs(60), |_| {
            calls.set(calls.get() + 1);
            Ok(true)
        })
        .unwrap();
        assert_eq!(calls.get(), 1);
        assert!(started.elapsed() < Duration::from_secs(30));
    }

    #[test]
    fn hs2_h9dng6_waits_are_whole_milliseconds_and_errors_propagate() {
        let mut asked = Vec::new();
        let error = park_for(Duration::from_micros(1_500), |left| {
            asked.push(left);
            Err(io::Error::from(io::ErrorKind::ConnectionReset))
        })
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::ConnectionReset);
        assert_eq!(asked.len(), 1);
        assert_eq!(asked[0].subsec_nanos() % 1_000_000, 0);
        assert!(asked[0] >= Duration::from_millis(1));
    }

    /// A real socket: with no datagram the park lasts its whole timeout; a datagram sent
    /// first ends it at once.
    #[test]
    fn hs2_h9dng6_a_real_waiter_parks_for_its_timeout_or_until_woken() {
        let dir = tempfile::tempdir().unwrap();
        let waiter = Waiter::register(dir.path(), u64::MAX / 2).unwrap().unwrap();
        let timeout = Duration::from_millis(150);
        let started = Instant::now();
        waiter.park(timeout).unwrap();
        assert!(started.elapsed() >= timeout);

        wake_all(dir.path(), 0).unwrap();
        let started = Instant::now();
        waiter.park(Duration::from_secs(60)).unwrap();
        assert!(started.elapsed() < Duration::from_secs(30));
    }
}
