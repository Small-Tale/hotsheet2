//! Back-pressure-safe output for one attached terminal viewer (HS2-J52ZJ7).
//!
//! An attach used to send output and read input in one `select!` loop, so while a send waited
//! on a slow viewer (a busy browser tab under heavy TUI redraws) that viewer's keystrokes were
//! not read, and a viewer that fell 256 chunks behind was resynced with a full scrollback
//! replay. Attaches now split into three parts joined by bounded queues:
//!
//! - the **output pump** ([`pump_viewer_output`]) drains the terminal's live fan-out promptly
//!   into a small frame queue. When the queue is full it coalesces further output into one
//!   pending frame ([`PendingOutput`]) instead of letting the fan-out lag, and falls back to a
//!   scrollback [`ViewerFrame::Replace`] only once the coalesced bytes exceed
//!   [`MAX_COALESCED_BYTES`] (the scrollback size, so the replay is never larger);
//! - a **writer** owns the transport's send half and writes queued frames;
//! - the attach task itself only **reads input**, so typing reaches the PTY no matter how far
//!   behind the viewer's output is.

use crate::sizing::Decision;
use crate::terminal::{SCROLLBACK_BYTES, Terminal};
use std::sync::Arc;
use tokio::sync::broadcast::{self, error::RecvError};
use tokio::sync::mpsc::{self, error::TrySendError};

/// Frames queued between an output pump and its writer. Small, so coalescing starts soon after
/// a viewer stops keeping up rather than after a long backlog of tiny frames.
pub const VIEWER_QUEUE_FRAMES: usize = 32;

/// Coalesced output beyond this is replaced by a fresh scrollback snapshot, which holds at
/// most [`SCROLLBACK_BYTES`] and so is never larger than what it replaces.
pub const MAX_COALESCED_BYTES: usize = SCROLLBACK_BYTES;

/// One frame for an attached viewer, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewerFrame {
    /// Live PTY output, possibly several chunks coalesced, appended to the viewer's emulator.
    Output(Vec<u8>),
    /// A scrollback snapshot that replaces the viewer's emulator state.
    Replace(Vec<u8>),
    /// The PTY size the arbiter applied.
    Size(Decision),
}

/// The viewer went away (its frame queue closed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewerClosed;

/// Output waiting for room in a viewer's frame queue.
#[derive(Debug, Default)]
pub struct PendingOutput {
    bytes: Vec<u8>,
    replace: bool,
}

impl PendingOutput {
    /// Nothing is waiting. A pending replacement counts even when its snapshot is empty: an
    /// empty replay still clears the viewer's stale state.
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty() && !self.replace
    }

    /// Pending bytes.
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Too much has coalesced to keep appending.
    pub fn over_cap(&self) -> bool {
        self.bytes.len() > MAX_COALESCED_BYTES
    }

    /// Queue live output: straight to the viewer when nothing is pending and the queue has
    /// room, otherwise appended to the pending frame so order is kept.
    pub fn push<F: From<ViewerFrame>>(
        &mut self,
        chunk: Vec<u8>,
        frames: &mpsc::Sender<F>,
    ) -> Result<(), ViewerClosed> {
        if self.is_empty() {
            match frames.try_reserve() {
                Ok(permit) => {
                    permit.send(ViewerFrame::Output(chunk).into());
                    return Ok(());
                }
                Err(TrySendError::Closed(())) => return Err(ViewerClosed),
                Err(TrySendError::Full(())) => {}
            }
        }
        self.bytes.extend_from_slice(&chunk);
        Ok(())
    }

    /// Replace everything pending with a scrollback snapshot, which already contains it.
    pub fn replace_with(&mut self, snapshot: Vec<u8>) {
        self.bytes = snapshot;
        self.replace = true;
    }

    /// The pending frame, leaving nothing pending.
    pub fn take(&mut self) -> Option<ViewerFrame> {
        if self.is_empty() {
            return None;
        }
        let bytes = std::mem::take(&mut self.bytes);
        let frame = if std::mem::take(&mut self.replace) {
            ViewerFrame::Replace(bytes)
        } else {
            ViewerFrame::Output(bytes)
        };
        Some(frame)
    }

    /// Wait for room and queue the pending frame, if any.
    pub async fn flush<F: From<ViewerFrame>>(
        &mut self,
        frames: &mpsc::Sender<F>,
    ) -> Result<(), ViewerClosed> {
        if let Some(frame) = self.take() {
            frames.send(frame.into()).await.map_err(|_| ViewerClosed)?;
        }
        Ok(())
    }
}

/// Resubscribe from a fresh snapshot that supersedes everything pending.
fn resync(term: &Terminal, output: &mut broadcast::Receiver<Vec<u8>>, pending: &mut PendingOutput) {
    let (snapshot, replacement) = term.subscribe_with_scrollback();
    *output = replacement;
    pending.replace_with(snapshot);
}

/// Forward a terminal's live output and size decisions to one viewer's frame queue until the
/// terminal ends or the viewer goes away. `output` and `sizes` must already be subscribed
/// (the caller sends the attach preamble first). Returning drops `frames`, which tells the
/// writer to finish what is queued and close the viewer.
pub async fn pump_viewer_output<F: From<ViewerFrame> + Send>(
    term: Arc<Terminal>,
    mut output: broadcast::Receiver<Vec<u8>>,
    mut sizes: broadcast::Receiver<Decision>,
    frames: mpsc::Sender<F>,
) {
    let mut pending = PendingOutput::default();
    loop {
        tokio::select! {
            biased;
            permit = frames.reserve(), if !pending.is_empty() => {
                let Ok(permit) = permit else { return };
                if let Some(frame) = pending.take() {
                    permit.send(frame.into());
                }
            }
            chunk = output.recv() => match chunk {
                Ok(chunk) => {
                    if pending.push(chunk, &frames).is_err() {
                        return;
                    }
                    if pending.over_cap() {
                        resync(&term, &mut output, &mut pending);
                    }
                }
                // Only reachable if the pump itself was starved; recover the same way.
                Err(RecvError::Lagged(_)) => resync(&term, &mut output, &mut pending),
                Err(RecvError::Closed) => break,
            },
            size = sizes.recv() => match size {
                Ok(decision) => {
                    // Keep order: output produced before the resize reaches the viewer first.
                    if pending.flush(&frames).await.is_err()
                        || frames.send(ViewerFrame::Size(decision).into()).await.is_err()
                    {
                        return;
                    }
                }
                Err(RecvError::Lagged(_)) => {} // a missed size update self-corrects on the next claim
                Err(RecvError::Closed) => break,
            },
        }
    }
    // The terminal ended: deliver what is pending before the viewer closes.
    let _ = pending.flush(&frames).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_goes_straight_through_while_the_queue_has_room() {
        let (tx, mut rx) = mpsc::channel::<ViewerFrame>(2);
        let mut pending = PendingOutput::default();
        pending.push(b"a".to_vec(), &tx).unwrap();
        pending.push(b"b".to_vec(), &tx).unwrap();
        assert!(pending.is_empty());
        assert_eq!(rx.try_recv().unwrap(), ViewerFrame::Output(b"a".to_vec()));
        assert_eq!(rx.try_recv().unwrap(), ViewerFrame::Output(b"b".to_vec()));
    }

    /// A full queue coalesces in order; once anything is pending, later chunks append even if
    /// the queue drains, so output never reorders.
    #[test]
    fn a_full_queue_coalesces_without_reordering() {
        let (tx, mut rx) = mpsc::channel::<ViewerFrame>(1);
        let mut pending = PendingOutput::default();
        pending.push(b"1".to_vec(), &tx).unwrap();
        pending.push(b"2".to_vec(), &tx).unwrap();
        pending.push(b"3".to_vec(), &tx).unwrap();
        assert_eq!(rx.try_recv().unwrap(), ViewerFrame::Output(b"1".to_vec()));
        // The queue has room again, but "4" must follow the pending "23".
        pending.push(b"4".to_vec(), &tx).unwrap();
        assert!(rx.try_recv().is_err());
        assert_eq!(pending.take(), Some(ViewerFrame::Output(b"234".to_vec())));
        assert!(pending.is_empty());
        assert_eq!(pending.take(), None);
    }

    #[test]
    fn a_replacement_supersedes_pending_output_and_survives_an_empty_snapshot() {
        let (tx, _rx) = mpsc::channel::<ViewerFrame>(1);
        let mut pending = PendingOutput::default();
        pending.push(b"fills the queue".to_vec(), &tx).unwrap();
        pending.push(b"stale".to_vec(), &tx).unwrap();
        pending.replace_with(Vec::new());
        assert!(!pending.is_empty(), "an empty replay is still a boundary");
        // Output after the snapshot appends to it.
        pending.push(b"after".to_vec(), &tx).unwrap();
        assert_eq!(
            pending.take(),
            Some(ViewerFrame::Replace(b"after".to_vec()))
        );
        assert_eq!(pending.take(), None);
    }

    #[test]
    fn the_cap_matches_the_scrollback_and_a_closed_viewer_is_reported() {
        let (tx, rx) = mpsc::channel::<ViewerFrame>(1);
        let mut pending = PendingOutput::default();
        pending.push(vec![0; 1], &tx).unwrap();
        pending.push(vec![0; MAX_COALESCED_BYTES], &tx).unwrap();
        assert!(!pending.over_cap());
        pending.push(vec![0; 1], &tx).unwrap();
        assert!(pending.over_cap());
        drop(rx);
        let mut fresh = PendingOutput::default();
        assert_eq!(fresh.push(b"x".to_vec(), &tx), Err(ViewerClosed));
    }

    #[tokio::test]
    async fn flush_waits_for_room_then_queues_the_pending_frame() {
        let (tx, mut rx) = mpsc::channel::<ViewerFrame>(1);
        let mut pending = PendingOutput::default();
        pending.push(b"a".to_vec(), &tx).unwrap();
        pending.push(b"b".to_vec(), &tx).unwrap();
        let reader = tokio::spawn(async move {
            let mut frames = Vec::new();
            while let Some(frame) = rx.recv().await {
                frames.push(frame);
            }
            frames
        });
        pending.flush(&tx).await.unwrap();
        assert!(pending.is_empty());
        drop(tx);
        assert_eq!(
            reader.await.unwrap(),
            vec![
                ViewerFrame::Output(b"a".to_vec()),
                ViewerFrame::Output(b"b".to_vec())
            ]
        );
    }
}
