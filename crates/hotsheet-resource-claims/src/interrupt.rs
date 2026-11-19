//! Letting a stop signal end a wait cleanly (HS2-QGY1NN).
//!
//! A `hotsheet-cli resource acquire --wait` that SIGTERM or SIGINT ends used to die inside the
//! waiter's `park`, never running the `Waiter` drop that removes its registration file; the file
//! then counted against the namespace's 256 waiters until the wait's own deadline plus a grace.
//! An [`InterruptGuard`] fixes that at the source with the self-pipe trick: while it is alive,
//! SIGINT, SIGTERM and SIGHUP write one byte to a pipe instead of ending the process, and
//! `park` polls that pipe beside its socket, so a signal wakes the wait at once and without a
//! race (a signal that lands before the poll leaves the byte waiting). The wait returns
//! [`ClaimError::Interrupted`], the waiter drops and removes its registration, and the guard
//! then restores the default disposition and re-raises the signal, so the process still dies by
//! the signal it was sent. Outside a guard nothing changes: signals keep their default action.

use std::io;

#[cfg(unix)]
mod imp {
    use std::os::fd::RawFd;
    use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

    const SIGNALS: [libc::c_int; 3] = [libc::SIGINT, libc::SIGTERM, libc::SIGHUP];

    /// The pipe's read end, or -1 while no guard is active.
    static READ_FD: AtomicI32 = AtomicI32::new(-1);
    static WRITE_FD: AtomicI32 = AtomicI32::new(-1);
    /// The signal that was caught, 0 for none.
    static CAUGHT: AtomicI32 = AtomicI32::new(0);
    static ACTIVE: AtomicBool = AtomicBool::new(false);

    extern "C" fn on_signal(signal: libc::c_int) {
        CAUGHT.store(signal, Ordering::SeqCst);
        let fd = WRITE_FD.load(Ordering::SeqCst);
        if fd >= 0 {
            let byte = 1u8;
            // SAFETY: `write` is async-signal-safe; the descriptor is the pipe this guard owns.
            unsafe {
                libc::write(fd, (&raw const byte).cast(), 1);
            }
        }
    }

    pub struct Guard {
        previous: Vec<(libc::c_int, libc::sighandler_t)>,
        read: RawFd,
        write: RawFd,
    }

    impl Guard {
        /// `None` when another guard is already active (the outer one covers the wait).
        pub fn new() -> std::io::Result<Option<Self>> {
            if ACTIVE.swap(true, Ordering::SeqCst) {
                return Ok(None);
            }
            let mut fds = [0 as libc::c_int; 2];
            // SAFETY: `fds` is a valid two-element array for `pipe` to fill.
            if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
                ACTIVE.store(false, Ordering::SeqCst);
                return Err(std::io::Error::last_os_error());
            }
            for fd in fds {
                // SAFETY: both descriptors were just created by `pipe`.
                unsafe {
                    let flags = libc::fcntl(fd, libc::F_GETFL);
                    libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
                    libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
                }
            }
            CAUGHT.store(0, Ordering::SeqCst);
            WRITE_FD.store(fds[1], Ordering::SeqCst);
            READ_FD.store(fds[0], Ordering::SeqCst);
            let mut previous = Vec::new();
            for signal in SIGNALS {
                // SAFETY: `on_signal` only calls async-signal-safe functions and stores atomics.
                let old =
                    unsafe { libc::signal(signal, on_signal as *const () as libc::sighandler_t) };
                previous.push((signal, old));
            }
            Ok(Some(Self {
                previous,
                read: fds[0],
                write: fds[1],
            }))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            READ_FD.store(-1, Ordering::SeqCst);
            for (signal, old) in &self.previous {
                // SAFETY: restoring the handler `signal` returned when it was replaced.
                unsafe {
                    libc::signal(*signal, *old);
                }
            }
            WRITE_FD.store(-1, Ordering::SeqCst);
            // SAFETY: the guard owns both ends and nothing reads them after the stores above.
            unsafe {
                libc::close(self.read);
                libc::close(self.write);
            }
            let caught = CAUGHT.swap(0, Ordering::SeqCst);
            ACTIVE.store(false, Ordering::SeqCst);
            if caught != 0 {
                // The process was asked to stop while the guard held the signal: honour it now
                // that the waiter has cleaned up, by the signal's own disposition.
                // SAFETY: re-raising a signal this process caught; the handler is restored.
                unsafe {
                    libc::raise(caught);
                }
            }
        }
    }

    /// The pipe `park` should poll beside its socket, while a guard is active.
    pub fn wake_fd() -> Option<RawFd> {
        let fd = READ_FD.load(Ordering::SeqCst);
        (fd >= 0).then_some(fd)
    }

    /// Wait up to `timeout_ms` for `socket` or the stop pipe. `true` means a stop signal.
    pub fn poll(socket: RawFd, stop: RawFd, timeout_ms: i32) -> std::io::Result<bool> {
        let mut fds = [
            libc::pollfd {
                fd: socket,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: stop,
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        loop {
            // SAFETY: `fds` is a valid array of two `pollfd`s.
            let ready = unsafe { libc::poll(fds.as_mut_ptr(), 2, timeout_ms) };
            if ready >= 0 {
                return Ok(fds[1].revents != 0);
            }
            let error = std::io::Error::last_os_error();
            // A signal this guard does not own interrupted the poll: look again.
            if error.kind() != std::io::ErrorKind::Interrupted {
                return Err(error);
            }
            if CAUGHT.load(Ordering::SeqCst) != 0 {
                return Ok(true);
            }
        }
    }
}

/// While alive, a stop signal wakes a local wait instead of ending the process (unix). On other
/// platforms it does nothing.
#[must_use = "the guard only protects waits while it is held"]
pub struct InterruptGuard {
    #[cfg(unix)]
    _inner: Option<imp::Guard>,
}

impl InterruptGuard {
    pub fn new() -> io::Result<Self> {
        Ok(Self {
            #[cfg(unix)]
            _inner: imp::Guard::new()?,
        })
    }
}

/// Wait on `socket` for a wake-up or `timeout`; `Err(Interrupted)` when a stop signal arrived.
#[cfg(unix)]
pub(crate) fn poll_socket(
    socket: &std::net::UdpSocket,
    timeout: std::time::Duration,
) -> io::Result<Option<bool>> {
    use std::os::fd::AsRawFd;
    let Some(stop) = imp::wake_fd() else {
        return Ok(None);
    };
    let ms = i32::try_from(timeout.as_millis().max(1)).unwrap_or(i32::MAX);
    imp::poll(socket.as_raw_fd(), stop, ms).map(Some)
}
