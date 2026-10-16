//! Interruptible PTY master I/O (HS2-RA494E).
//!
//! A blocking write to a PTY master parks while the child is not reading stdin and the kernel
//! input buffer is full. On Linux that write is never woken when the child is killed and the
//! parent later releases the slave: `pty_close` wakes the master's writers while the slave
//! still counts as open, the writer finds no room and sleeps again, and nothing wakes it
//! afterwards. The terminal's input writer thread (HS2-FERD7N) and its queued bytes would then
//! leak for the life of the server.
//!
//! The master file description is therefore switched to `O_NONBLOCK`, and both ends that use
//! it wait in `poll(2)` instead of inside the kernel write/read:
//!
//! - [`PtyWriter`] waits for `POLLOUT` in short, local intervals and gives up with
//!   [`BrokenPipe`](std::io::ErrorKind::BrokenPipe) once its `abandoned` check reports that the
//!   child exited or the terminal is gone. Partial writes keep their order through `write_all`.
//! - [`PtyReader`] (the drain) waits for `POLLIN` without a timeout, so it still ends exactly when
//!   the PTY reports EOF or EIO.
//!
//! `O_NONBLOCK` belongs to the shared open file description, so it applies to every duplicate
//! of the master fd (portable-pty's reader and writer included). Each adapter polls its own
//! duplicate, never a descriptor another owner could close and the kernel reuse.

use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};

/// How long the writer waits for input room before re-checking whether to give up. A local
/// `poll(2)` timeout, never a network request.
pub(crate) const WRITE_POLL_MS: libc::c_int = 50;

/// Duplicate `fd` (close-on-exec) so the caller owns a descriptor nobody else can close.
pub(crate) fn dup_owned(fd: RawFd) -> io::Result<OwnedFd> {
    // SAFETY: F_DUPFD_CLOEXEC only reads `fd`; a non-negative result is a new descriptor this
    // process owns exclusively.
    let dup = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 0) };
    if dup < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `dup` is a freshly created descriptor with no other owner.
    Ok(unsafe { OwnedFd::from_raw_fd(dup) })
}

/// Put the open file description behind `fd` (and so every duplicate of it) in non-blocking mode.
pub(crate) fn set_nonblocking(fd: &OwnedFd) -> io::Result<()> {
    let raw = fd.as_raw_fd();
    // SAFETY: F_GETFL/F_SETFL on a descriptor the caller owns.
    let flags = unsafe { libc::fcntl(raw, libc::F_GETFL) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    if unsafe { libc::fcntl(raw, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Wait until `fd` reports `events` (or a hangup/error), at most `timeout_ms` (`-1` waits
/// indefinitely). Returns the reported events, `0` on timeout.
fn wait_for(
    fd: &OwnedFd,
    events: libc::c_short,
    timeout_ms: libc::c_int,
) -> io::Result<libc::c_short> {
    let mut pollfd = libc::pollfd {
        fd: fd.as_raw_fd(),
        events,
        revents: 0,
    };
    loop {
        // SAFETY: one valid pollfd for a descriptor the caller owns.
        let ready = unsafe { libc::poll(&mut pollfd, 1, timeout_ms) };
        if ready >= 0 {
            return Ok(if ready == 0 { 0 } else { pollfd.revents });
        }
        let err = io::Error::last_os_error();
        if err.kind() != io::ErrorKind::Interrupted {
            return Err(err);
        }
    }
}

fn invalid(revents: libc::c_short) -> io::Result<()> {
    if revents & libc::POLLNVAL != 0 {
        Err(io::Error::from_raw_os_error(libc::EBADF))
    } else {
        Ok(())
    }
}

/// The input side of a non-blocking PTY master: waits for room in short intervals and gives up
/// once `abandoned` says nobody will ever read the input.
pub(crate) struct PtyWriter {
    inner: Box<dyn Write + Send>,
    poll_fd: OwnedFd,
    abandoned: Box<dyn Fn() -> bool + Send>,
}

impl PtyWriter {
    /// Wrap `inner`, which writes to the same open file description as `poll_fd`. The
    /// description must already be non-blocking ([`set_nonblocking`]).
    pub(crate) fn new(
        inner: Box<dyn Write + Send>,
        poll_fd: OwnedFd,
        abandoned: impl Fn() -> bool + Send + 'static,
    ) -> Self {
        Self {
            inner,
            poll_fd,
            abandoned: Box::new(abandoned),
        }
    }
}

impl Write for PtyWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        loop {
            match self.inner.write(buf) {
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                other => return other,
            }
            if (self.abandoned)() {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "terminal exited before reading its input",
                ));
            }
            invalid(wait_for(&self.poll_fd, libc::POLLOUT, WRITE_POLL_MS)?)?;
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// The output side of a non-blocking PTY master: a read that waits in `poll(2)` instead of in
/// the kernel, ending exactly where a blocking read would (EOF, EIO, or another error).
pub(crate) struct PtyReader {
    inner: Box<dyn Read + Send>,
    poll_fd: OwnedFd,
}

impl PtyReader {
    /// Wrap `inner`, which reads from the same open file description as `poll_fd`.
    pub(crate) fn new(inner: Box<dyn Read + Send>, poll_fd: OwnedFd) -> Self {
        Self { inner, poll_fd }
    }
}

impl Read for PtyReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut hung_up = false;
        loop {
            match self.inner.read(buf) {
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                other => return other,
            }
            if hung_up {
                // The far side is gone and nothing is left to read.
                return Ok(0);
            }
            let revents = wait_for(&self.poll_fd, libc::POLLIN, -1)?;
            invalid(revents)?;
            hung_up = revents & (libc::POLLHUP | libc::POLLERR) != 0 && revents & libc::POLLIN == 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    /// A non-blocking pipe: (read end, write end).
    fn pipe() -> (std::fs::File, std::fs::File) {
        let mut fds = [0; 2];
        // SAFETY: pipe writes two new descriptors into the array.
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        // SAFETY: both descriptors are fresh and owned here.
        let (r, w) = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
        set_nonblocking(&r).unwrap();
        set_nonblocking(&w).unwrap();
        (r.into(), w.into())
    }

    fn writer_for(w: &std::fs::File, abandoned: Arc<AtomicBool>) -> PtyWriter {
        let poll_fd = dup_owned(w.as_raw_fd()).unwrap();
        PtyWriter::new(Box::new(w.try_clone().unwrap()), poll_fd, move || {
            abandoned.load(Ordering::SeqCst)
        })
    }

    #[test]
    fn a_full_buffer_write_waits_then_completes_in_order_once_drained() {
        let (mut r, w) = pipe();
        let mut writer = writer_for(&w, Arc::default());
        let payload: Vec<u8> = (0..1024 * 1024).map(|i| (i % 251) as u8).collect();
        let expected = payload.clone();
        let reader = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100)); // the writer hits a full pipe first
            let mut out = Vec::new();
            let mut buf = [0u8; 8192];
            while out.len() < expected.len() {
                match r.read(&mut buf) {
                    Ok(n) => out.extend_from_slice(&buf[..n]),
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    Err(e) => panic!("{e}"),
                }
            }
            out
        });
        writer.write_all(&payload).unwrap();
        writer.flush().unwrap();
        assert_eq!(reader.join().unwrap(), payload);
    }

    #[test]
    fn a_full_buffer_write_gives_up_promptly_once_abandoned() {
        let (_r, w) = pipe();
        let abandoned = Arc::new(AtomicBool::new(false));
        let mut writer = writer_for(&w, abandoned.clone());
        let flag = abandoned.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            flag.store(true, Ordering::SeqCst);
        });
        let started = Instant::now();
        let err = writer.write_all(&vec![b'x'; 1024 * 1024]).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::BrokenPipe);
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "gave up within a poll interval"
        );
        // Later writes to the abandoned, still-full buffer fail without waiting.
        let started = Instant::now();
        assert!(writer.write_all(b"more").is_err());
        assert!(started.elapsed() < Duration::from_millis(WRITE_POLL_MS as u64 * 4));
    }

    /// The Linux hang at the PTY level: the slave stays open (as the slave keeper holds it), so
    /// nothing ever makes room or reports EIO. A blocking master write would park forever; the
    /// non-blocking writer gives up once abandoned.
    #[test]
    fn a_pty_master_with_a_full_input_buffer_and_an_open_slave_gives_up_once_abandoned() {
        use portable_pty::{PtySize, native_pty_system};
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let fd = pair.master.as_raw_fd().unwrap();
        // Raw mode, like a full-screen program: input is queued, not dropped at a line limit.
        // SAFETY: tcgetattr/cfmakeraw/tcsetattr on the live master with a valid termios.
        unsafe {
            let mut t: libc::termios = std::mem::zeroed();
            assert_eq!(libc::tcgetattr(fd, &mut t), 0);
            libc::cfmakeraw(&mut t);
            assert_eq!(libc::tcsetattr(fd, libc::TCSANOW, &t), 0);
        }
        let poll_fd = dup_owned(fd).unwrap();
        set_nonblocking(&poll_fd).unwrap();
        let abandoned = Arc::new(AtomicBool::new(false));
        let flag = abandoned.clone();
        let mut writer = PtyWriter::new(pair.master.take_writer().unwrap(), poll_fd, move || {
            flag.load(Ordering::SeqCst)
        });
        let setter = abandoned.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            setter.store(true, Ordering::SeqCst);
        });
        let started = Instant::now();
        let err = writer.write_all(&vec![b'x'; 1024 * 1024]).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::BrokenPipe);
        assert!(
            started.elapsed() >= Duration::from_millis(150),
            "it waited for room first"
        );
        assert!(started.elapsed() < Duration::from_secs(3));
        // Dropping portable-pty's writer (which writes an EOF sequence) cannot hang either.
        drop(writer);
        drop(pair.slave);
    }

    #[test]
    fn the_reader_waits_for_data_and_reports_eof_when_the_writer_closes() {
        let (r, w) = pipe();
        let poll_fd = dup_owned(r.as_raw_fd()).unwrap();
        let mut reader = PtyReader::new(Box::new(r), poll_fd);
        let writer = std::thread::spawn(move || {
            let mut w = w;
            std::thread::sleep(Duration::from_millis(50));
            w.write_all(b"late").unwrap();
            std::thread::sleep(Duration::from_millis(50));
            drop(w);
        });
        let mut buf = [0u8; 16];
        let n = reader.read(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"late");
        assert_eq!(
            reader.read(&mut buf).unwrap(),
            0,
            "EOF after the writer closed"
        );
        writer.join().unwrap();
    }

    #[test]
    fn dup_and_nonblocking_report_a_bad_descriptor() {
        assert!(dup_owned(-1).is_err());
        let (r, _w) = pipe();
        let fd = dup_owned(r.as_raw_fd()).unwrap();
        set_nonblocking(&fd).unwrap();
        assert!(invalid(libc::POLLNVAL).is_err());
        assert!(invalid(libc::POLLIN).is_ok());
    }
}
