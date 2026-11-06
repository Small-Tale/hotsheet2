//! Per-write change notification for one file on macOS (HS2-TWE913).
//!
//! FSEvents reports a file's content change only when a writer closes it. Codex keeps its
//! session rollout open for the whole session and appends to it, so an FSEvents watch of the
//! rollout saw no append after its first read, and no Codex terminal usage was recorded. A
//! kqueue `EVFILT_VNODE` registration on the file fires on every write and extension instead,
//! whoever holds the file open. It is an event wait, never a timer.

use std::ffi::CString;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::ptr;
use std::thread::JoinHandle;

/// The `EVFILT_USER` identifier [`VnodeWatch`]'s drop triggers to end its thread.
const STOP: usize = 1;

/// Calls its callback after each write to, extension of, or removal of one file, from a
/// background thread. Dropping it stops and joins that thread.
pub(crate) struct VnodeWatch {
    queue: OwnedFd,
    // Held open (event-only) for the registration; closed after the thread ends.
    _file: OwnedFd,
    thread: Option<JoinHandle<()>>,
}

impl VnodeWatch {
    pub(crate) fn start(
        path: &Path,
        mut on_change: impl FnMut() + Send + 'static,
    ) -> io::Result<Self> {
        let c_path = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains NUL"))?;
        // SAFETY: `c_path` is a NUL-terminated string that outlives the call.
        let file = unsafe { libc::open(c_path.as_ptr(), libc::O_EVTONLY | libc::O_CLOEXEC) };
        if file < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `open` returned a new descriptor that nothing else owns.
        let file = unsafe { OwnedFd::from_raw_fd(file) };
        // SAFETY: `kqueue` takes no arguments.
        let queue = unsafe { libc::kqueue() };
        if queue < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `kqueue` returned a new descriptor that nothing else owns.
        let queue = unsafe { OwnedFd::from_raw_fd(queue) };
        let changes = [
            event(
                file.as_raw_fd() as usize,
                libc::EVFILT_VNODE,
                libc::EV_ADD | libc::EV_CLEAR,
                libc::NOTE_WRITE
                    | libc::NOTE_EXTEND
                    | libc::NOTE_DELETE
                    | libc::NOTE_RENAME
                    | libc::NOTE_REVOKE,
            ),
            event(STOP, libc::EVFILT_USER, libc::EV_ADD | libc::EV_CLEAR, 0),
        ];
        // SAFETY: `changes` is a live two-element array; no events are requested back.
        let registered = unsafe {
            libc::kevent(
                queue.as_raw_fd(),
                changes.as_ptr(),
                changes.len() as libc::c_int,
                ptr::null_mut(),
                0,
                ptr::null(),
            )
        };
        if registered < 0 {
            return Err(io::Error::last_os_error());
        }
        let raw_queue = queue.as_raw_fd();
        let gone = libc::NOTE_DELETE | libc::NOTE_RENAME | libc::NOTE_REVOKE;
        let thread = std::thread::spawn(move || {
            loop {
                let mut fired = event(0, 0, 0, 0);
                // SAFETY: the queue stays open until this thread has been joined, and `fired`
                // is one writable event for the call.
                let count = unsafe {
                    libc::kevent(raw_queue, ptr::null(), 0, &raw mut fired, 1, ptr::null())
                };
                if count < 0 {
                    if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                        continue;
                    }
                    return;
                }
                if count == 0 {
                    continue;
                }
                if fired.filter == libc::EVFILT_USER {
                    return;
                }
                on_change();
                if fired.fflags & gone != 0 {
                    return;
                }
            }
        });
        Ok(Self {
            queue,
            _file: file,
            thread: Some(thread),
        })
    }
}

impl Drop for VnodeWatch {
    fn drop(&mut self) {
        let stop = [event(STOP, libc::EVFILT_USER, 0, libc::NOTE_TRIGGER)];
        // SAFETY: the queue is open (its `OwnedFd` closes only after this body), and `stop` is
        // a live one-element array.
        unsafe {
            libc::kevent(
                self.queue.as_raw_fd(),
                stop.as_ptr(),
                1,
                ptr::null_mut(),
                0,
                ptr::null(),
            );
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn event(ident: usize, filter: i16, flags: u16, fflags: u32) -> libc::kevent {
    libc::kevent {
        ident,
        filter,
        flags,
        fflags,
        data: 0,
        udata: ptr::null_mut(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use std::sync::mpsc::channel;
    use std::time::Duration;

    #[test]
    fn hs2_twe913_reports_each_append_while_the_writer_keeps_the_file_open() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rollout.jsonl");
        let mut file = std::fs::File::create(&path).unwrap();
        let (tx, changes) = channel();
        let watch = VnodeWatch::start(&path, move || {
            let _ = tx.send(());
        })
        .unwrap();
        for line in ["a", "b", "c"] {
            writeln!(file, "{line}").unwrap();
            changes
                .recv_timeout(Duration::from_secs(5))
                .expect("an append through an open handle is reported");
            while changes.try_recv().is_ok() {}
        }
        drop(watch);
        writeln!(file, "after").unwrap();
        // The stopped thread released its callback.
        assert_eq!(
            changes.recv_timeout(Duration::from_secs(5)),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected)
        );
    }

    #[test]
    fn hs2_twe913_a_missing_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(VnodeWatch::start(&dir.path().join("absent"), || {}).is_err());
    }
}
