//! Ordered PTY input off the async workers (HS2-FERD7N).
//!
//! A PTY master write blocks while the child is not reading stdin and the kernel input buffer
//! is full (a stopped or busy foreground program receiving a large paste). Writing directly
//! from an attach task would park that tokio worker until the child drains, stalling unrelated
//! async work and the attach's own control frames (resize claims, liveness probes).
//!
//! Each terminal therefore owns one [`InputQueue`] drained by one dedicated writer thread, so
//! every caller shares a single FIFO and input order is preserved across viewers and paths:
//!
//! - [`InputQueue::queue`] (attaches): waits only for queue budget, never for the PTY.
//! - [`InputQueue::write_async`] (request/response paths): waits for the write result without
//!   holding any thread.
//! - [`InputQueue::write`] (tool threads, tests): the synchronous form.
//!
//! The queue holds at most [`INPUT_BUDGET_BYTES`] of unwritten input; a chunk larger than the
//! budget is still accepted into an empty queue so it can never deadlock.

use hotsheet_sync::LockExt;
use std::collections::VecDeque;
use std::io::Write;
use std::sync::{Arc, Condvar, Mutex};

/// Unwritten input a terminal accepts before callers wait for the PTY to drain.
pub const INPUT_BUDGET_BYTES: usize = 1024 * 1024;

/// How a queued chunk reports its write result.
enum Ack {
    /// Fire-and-forget (an attach's keystrokes; a write error is reported as the exit).
    None,
    Async(tokio::sync::oneshot::Sender<std::io::Result<()>>),
    Sync(std::sync::mpsc::SyncSender<std::io::Result<()>>),
}

impl Ack {
    fn send(self, result: std::io::Result<()>) {
        match self {
            Ack::None => {}
            Ack::Async(tx) => {
                let _ = tx.send(result);
            }
            Ack::Sync(tx) => {
                let _ = tx.send(result);
            }
        }
    }
}

struct Chunk {
    bytes: Vec<u8>,
    ack: Ack,
}

#[derive(Default)]
struct State {
    chunks: VecDeque<Chunk>,
    /// Bytes queued or being written right now.
    queued_bytes: usize,
    closed: bool,
}

impl State {
    fn has_room(&self, len: usize) -> bool {
        self.queued_bytes == 0 || self.queued_bytes + len <= INPUT_BUDGET_BYTES
    }
}

struct Shared {
    state: Mutex<State>,
    /// Wakes the writer thread (new work, close) and sync callers waiting for budget.
    cvar: Condvar,
    /// Wakes async callers waiting for budget.
    drained: tokio::sync::Notify,
}

/// Why input could not be queued: the terminal is gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputClosed;

impl std::fmt::Display for InputClosed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("terminal input closed")
    }
}

impl std::error::Error for InputClosed {}

impl From<InputClosed> for std::io::Error {
    fn from(_: InputClosed) -> Self {
        std::io::Error::new(std::io::ErrorKind::BrokenPipe, InputClosed)
    }
}

/// One terminal's ordered input queue. Dropping it closes the queue: unwritten input is
/// discarded and the writer thread exits once any in-progress PTY write returns.
pub struct InputQueue {
    shared: Arc<Shared>,
}

impl InputQueue {
    /// Start the writer thread that owns `writer`.
    pub fn spawn(writer: Box<dyn Write + Send>) -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::new(State::default()),
            cvar: Condvar::new(),
            drained: tokio::sync::Notify::new(),
        });
        let drain = shared.clone();
        std::thread::Builder::new()
            .name("hs-pty-input".into())
            .spawn(move || run_writer(&drain, writer))
            .expect("spawn the PTY input writer thread");
        Self { shared }
    }

    /// Queue `bytes` behind earlier input and return once it is accepted, without waiting for
    /// the PTY to take it. Waits (parking no thread) only while the queue is over budget.
    pub async fn queue(&self, bytes: Vec<u8>) -> Result<(), InputClosed> {
        self.push_async(bytes, Ack::None).await
    }

    /// Queue `bytes` and wait, parking no thread, until the PTY accepted or rejected them.
    pub async fn write_async(&self, bytes: Vec<u8>) -> std::io::Result<()> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.push_async(bytes, Ack::Async(tx)).await?;
        rx.await.unwrap_or_else(|_| Err(InputClosed.into()))
    }

    /// The synchronous form of [`write_async`](Self::write_async); blocks the calling thread.
    pub fn write(&self, bytes: Vec<u8>) -> std::io::Result<()> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        {
            let mut state = self.shared.state.lock_or_recover();
            loop {
                if state.closed {
                    return Err(InputClosed.into());
                }
                if state.has_room(bytes.len()) {
                    break;
                }
                state = self
                    .shared
                    .cvar
                    .wait(state)
                    .unwrap_or_else(|e| e.into_inner());
            }
            enqueue(&mut state, bytes, Ack::Sync(tx));
        }
        self.shared.cvar.notify_all();
        rx.recv().unwrap_or_else(|_| Err(InputClosed.into()))
    }

    /// Unwritten bytes, including a write in progress.
    pub fn queued_bytes(&self) -> usize {
        self.shared.state.lock_or_recover().queued_bytes
    }

    async fn push_async(&self, bytes: Vec<u8>, ack: Ack) -> Result<(), InputClosed> {
        let mut pending = Some((bytes, ack));
        loop {
            // Register before checking so a drain between the check and the await is not lost.
            let drained = self.shared.drained.notified();
            tokio::pin!(drained);
            drained.as_mut().enable();
            {
                let mut state = self.shared.state.lock_or_recover();
                if state.closed {
                    return Err(InputClosed);
                }
                let len = pending.as_ref().map_or(0, |(bytes, _)| bytes.len());
                if state.has_room(len) {
                    let (bytes, ack) = pending.take().expect("pending input");
                    enqueue(&mut state, bytes, ack);
                    drop(state);
                    self.shared.cvar.notify_all();
                    return Ok(());
                }
            }
            drained.await;
        }
    }
}

fn enqueue(state: &mut State, bytes: Vec<u8>, ack: Ack) {
    state.queued_bytes += bytes.len();
    state.chunks.push_back(Chunk { bytes, ack });
}

impl Drop for InputQueue {
    fn drop(&mut self) {
        let discarded: Vec<Chunk> = {
            let mut state = self.shared.state.lock_or_recover();
            state.closed = true;
            let chunks: Vec<Chunk> = state.chunks.drain(..).collect();
            state.queued_bytes -= chunks.iter().map(|c| c.bytes.len()).sum::<usize>();
            chunks
        };
        for chunk in discarded {
            chunk.ack.send(Err(InputClosed.into()));
        }
        self.shared.cvar.notify_all();
        self.shared.drained.notify_waiters();
    }
}

fn run_writer(shared: &Shared, mut writer: Box<dyn Write + Send>) {
    loop {
        let chunk = {
            let mut state = shared.state.lock_or_recover();
            loop {
                if let Some(chunk) = state.chunks.pop_front() {
                    break chunk;
                }
                if state.closed {
                    return;
                }
                state = shared.cvar.wait(state).unwrap_or_else(|e| e.into_inner());
            }
        };
        let len = chunk.bytes.len();
        let result = writer.write_all(&chunk.bytes).and_then(|()| writer.flush());
        {
            let mut state = shared.state.lock_or_recover();
            state.queued_bytes -= len;
        }
        chunk.ack.send(result);
        shared.cvar.notify_all();
        shared.drained.notify_waiters();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    /// A PTY stand-in whose writes block until the test opens the gate, recording what landed.
    #[derive(Clone)]
    struct GatedWriter {
        open: Arc<(Mutex<bool>, Condvar)>,
        written: Arc<Mutex<Vec<u8>>>,
        entered: mpsc::SyncSender<usize>,
        fail: bool,
    }

    impl Write for GatedWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let _ = self.entered.try_send(buf.len());
            let (lock, cvar) = &*self.open;
            let mut open = lock.lock_or_recover();
            while !*open {
                open = cvar.wait(open).unwrap();
            }
            if self.fail {
                return Err(std::io::Error::other("pty gone"));
            }
            self.written.lock_or_recover().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn gated(open: bool, fail: bool) -> (GatedWriter, mpsc::Receiver<usize>) {
        let (entered, rx) = mpsc::sync_channel(64);
        (
            GatedWriter {
                open: Arc::new((Mutex::new(open), Condvar::new())),
                written: Arc::default(),
                entered,
                fail,
            },
            rx,
        )
    }

    fn open_gate(w: &GatedWriter) {
        *w.open.0.lock_or_recover() = true;
        w.open.1.notify_all();
    }

    // A current-thread runtime: if queueing blocked the worker on the PTY, the timer below
    // could never fire and the test would hang instead of passing.
    #[tokio::test(flavor = "current_thread")]
    async fn a_blocked_pty_never_blocks_the_async_caller_and_order_is_kept() {
        let (writer, entered) = gated(false, false);
        let queue = InputQueue::spawn(Box::new(writer.clone()));
        queue.queue(b"first ".to_vec()).await.unwrap();
        // The writer thread is now parked inside the PTY write.
        tokio::task::spawn_blocking(move || entered.recv_timeout(Duration::from_secs(5)))
            .await
            .unwrap()
            .unwrap();
        for word in ["second ", "third"] {
            tokio::time::timeout(
                Duration::from_secs(1),
                queue.queue(word.as_bytes().to_vec()),
            )
            .await
            .expect("queueing waited for the PTY")
            .unwrap();
        }
        tokio::time::sleep(Duration::from_millis(5)).await; // the worker is still free
        assert_eq!(queue.queued_bytes(), "first second third".len());
        open_gate(&writer);
        let w = &queue;
        tokio::time::timeout(Duration::from_secs(5), w.write_async(b"!".to_vec()))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&*writer.written.lock_or_recover(), b"first second third!");
        assert_eq!(queue.queued_bytes(), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn over_budget_input_waits_without_a_thread_and_resumes_on_drain() {
        let (writer, entered) = gated(false, false);
        let queue = Arc::new(InputQueue::spawn(Box::new(writer.clone())));
        // An oversize chunk is accepted into an empty queue.
        queue
            .queue(vec![b'a'; INPUT_BUDGET_BYTES + 1])
            .await
            .unwrap();
        tokio::task::spawn_blocking(move || entered.recv_timeout(Duration::from_secs(5)))
            .await
            .unwrap()
            .unwrap();
        let waiting = {
            let queue = queue.clone();
            tokio::spawn(async move { queue.queue(b"b".to_vec()).await })
        };
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(
            !waiting.is_finished(),
            "over budget input waits for a drain"
        );
        open_gate(&writer);
        tokio::time::timeout(Duration::from_secs(5), waiting)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        queue.write_async(Vec::new()).await.unwrap();
        let written = writer.written.lock_or_recover();
        assert_eq!(written.len(), INPUT_BUDGET_BYTES + 2);
        assert_eq!(written.last(), Some(&b'b'));
    }

    #[test]
    fn sync_and_async_writes_each_report_their_pty_error() {
        let (writer, _entered) = gated(true, true);
        let queue = InputQueue::spawn(Box::new(writer));
        assert_eq!(
            queue.write(b"x".to_vec()).unwrap_err().to_string(),
            "pty gone"
        );
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        assert_eq!(
            runtime
                .block_on(queue.write_async(b"y".to_vec()))
                .unwrap_err()
                .to_string(),
            "pty gone"
        );
        assert_eq!(queue.queued_bytes(), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn dropping_the_queue_discards_unwritten_input_and_wakes_waiters() {
        let (writer, entered) = gated(false, false);
        let queue = InputQueue::spawn(Box::new(writer.clone()));
        queue.queue(b"in flight".to_vec()).await.unwrap();
        tokio::task::spawn_blocking(move || entered.recv_timeout(Duration::from_secs(5)))
            .await
            .unwrap()
            .unwrap();
        let (tx, rx) = tokio::sync::oneshot::channel();
        {
            let mut state = queue.shared.state.lock_or_recover();
            enqueue(&mut state, b"discarded".to_vec(), Ack::Async(tx));
        }
        let shared = queue.shared.clone();
        drop(queue);
        let discarded = rx.await.unwrap().unwrap_err();
        assert_eq!(discarded.kind(), std::io::ErrorKind::BrokenPipe);
        assert!(shared.state.lock_or_recover().closed);
        open_gate(&writer);
        // The writer thread finishes the in-flight write, then exits instead of writing more.
        for _ in 0..200 {
            if Arc::strong_count(&shared) == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert_eq!(Arc::strong_count(&shared), 1, "the writer thread exited");
        assert_eq!(&*writer.written.lock_or_recover(), b"in flight");
    }
}
