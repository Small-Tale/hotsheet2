//! Bounded, single-flight ticket listing behind `GET /health` (HS2-9PPDR1).
//!
//! `/health` reports the primary store's healthy ticket count and its unparseable files.
//! Producing them parses every ticket file, which must never occupy an async request
//! thread or make the liveness probe wait on a slow (or blocked) store. [`HealthScan`]
//! therefore runs the listing on the blocking pool, lets concurrent probes share the one
//! scan in flight instead of piling up blocked threads, and waits for it only up to a
//! short budget. Past the budget the caller answers from the last completed scan, or —
//! before any scan has completed — from the index.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::http::StatusCode;
use tokio::sync::watch;

/// How long `/health` waits for a fresh listing before answering from the last one. Well
/// under the dev bridge's 750 ms probe timeout (`HS2-TANE0V`), yet long enough that an
/// ordinary store answers with fresh counts.
pub(crate) const HEALTH_SCAN_BUDGET: Duration = Duration::from_millis(250);

/// What one completed scan found: the healthy ticket count and the corrupt-file entries
/// already shaped for the `/health` wire format.
#[derive(Debug, PartialEq)]
pub(crate) struct HealthListing {
    pub(crate) tickets: usize,
    pub(crate) corrupt: Vec<serde_json::Value>,
}

/// A scan that failed outright (for example an unreadable ticket directory).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ScanFailure {
    pub(crate) status: StatusCode,
    pub(crate) message: String,
}

pub(crate) type ScanOutcome = Result<Arc<HealthListing>, ScanFailure>;

/// How a `/health` request's listing was resolved.
#[derive(Debug)]
pub(crate) enum Resolution {
    /// A scan finished within the budget (this request's, or the one it joined).
    Fresh(ScanOutcome),
    /// The scan is still running; this is the last one that completed successfully.
    Cached(Arc<HealthListing>),
    /// The scan is still running and none has completed yet.
    Pending,
}

#[derive(Default)]
struct ScanState {
    last: Option<Arc<HealthListing>>,
    in_flight: Option<watch::Receiver<Option<ScanOutcome>>>,
}

/// Shared, single-flight scan coordinator (one per served primary store).
pub(crate) struct HealthScan {
    state: Mutex<ScanState>,
    budget: Duration,
}

impl Default for HealthScan {
    fn default() -> Self {
        Self::new(HEALTH_SCAN_BUDGET)
    }
}

impl HealthScan {
    pub(crate) fn new(budget: Duration) -> Self {
        Self {
            state: Mutex::new(ScanState::default()),
            budget,
        }
    }

    /// Resolve a listing: join the scan in flight or start one with `scan` on the blocking
    /// pool, then wait for it at most the budget.
    pub(crate) async fn resolve<F>(self: &Arc<Self>, scan: F) -> Resolution
    where
        F: FnOnce() -> ScanOutcome + Send + 'static,
    {
        let mut receiver = self.join_or_start(scan);
        let waited = tokio::time::timeout(self.budget, async {
            receiver
                .wait_for(Option::is_some)
                .await
                .ok()
                .and_then(|outcome| outcome.clone())
        })
        .await;
        if let Ok(Some(outcome)) = waited {
            return Resolution::Fresh(outcome);
        }
        // Timed out, or the scan task died without reporting (it panicked). Either way,
        // answer from what is already known.
        match self.state.lock().ok().and_then(|state| state.last.clone()) {
            Some(last) => Resolution::Cached(last),
            None => Resolution::Pending,
        }
    }

    fn join_or_start<F>(self: &Arc<Self>, scan: F) -> watch::Receiver<Option<ScanOutcome>>
    where
        F: FnOnce() -> ScanOutcome + Send + 'static,
    {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // A live scan is joined. A receiver whose sender is gone belongs to a scan that
        // panicked before clearing itself; replace it rather than wait on it forever.
        if let Some(receiver) = &state.in_flight
            && receiver.has_changed().is_ok()
        {
            return receiver.clone();
        }
        let (sender, receiver) = watch::channel(None);
        state.in_flight = Some(receiver.clone());
        drop(state);
        let coordinator = self.clone();
        let _detached = tokio::task::spawn_blocking(move || {
            let outcome = scan();
            {
                let mut state = coordinator
                    .state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let Ok(listing) = &outcome {
                    state.last = Some(listing.clone());
                }
                state.in_flight = None;
            }
            // Every waiter may already have timed out; nobody listening is fine.
            let _ = sender.send(Some(outcome));
        });
        receiver
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    fn listing(tickets: usize) -> ScanOutcome {
        Ok(Arc::new(HealthListing {
            tickets,
            corrupt: Vec::new(),
        }))
    }

    fn tickets(resolution: &Resolution) -> Option<usize> {
        match resolution {
            Resolution::Fresh(Ok(listing)) | Resolution::Cached(listing) => Some(listing.tickets),
            Resolution::Fresh(Err(_)) | Resolution::Pending => None,
        }
    }

    /// A gate the test opens to let a blocked scan finish.
    fn gate() -> (
        std::sync::mpsc::Sender<()>,
        Arc<Mutex<std::sync::mpsc::Receiver<()>>>,
    ) {
        let (open, wait) = std::sync::mpsc::channel();
        (open, Arc::new(Mutex::new(wait)))
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_fast_scan_is_fresh_and_every_request_rescans() {
        let scan = Arc::new(HealthScan::new(Duration::from_secs(5)));
        let runs = Arc::new(AtomicUsize::new(0));
        for expected in 1..=3 {
            let counter = runs.clone();
            let resolution = scan
                .resolve(move || listing(counter.fetch_add(1, Ordering::SeqCst) + 1))
                .await;
            assert!(matches!(resolution, Resolution::Fresh(Ok(_))));
            assert_eq!(tickets(&resolution), Some(expected));
        }
        assert_eq!(runs.load(Ordering::SeqCst), 3);
    }

    /// The transition walk: pending (no scan ever completed) → fresh once it finishes →
    /// cached while the next scan is blocked → fresh again with the new result.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_blocked_scan_answers_pending_then_cached_then_fresh() {
        let scan = Arc::new(HealthScan::new(Duration::from_millis(50)));
        let (open, wait) = gate();

        let blocked = wait.clone();
        let first = scan
            .resolve(move || {
                blocked.lock().unwrap().recv().unwrap();
                listing(7)
            })
            .await;
        assert!(matches!(first, Resolution::Pending), "{first:?}");

        // A second probe joins the same scan rather than starting another blocked one.
        let started = Arc::new(AtomicUsize::new(0));
        let flag = started.clone();
        let second = scan
            .resolve(move || {
                flag.fetch_add(1, Ordering::SeqCst);
                listing(0)
            })
            .await;
        assert!(matches!(second, Resolution::Pending), "{second:?}");

        // Release it; the next probe starts a fresh scan and reports it.
        open.send(()).unwrap();
        let mut settled = None;
        for _ in 0..100 {
            let resolution = scan.resolve(|| listing(8)).await;
            if tickets(&resolution) == Some(8) {
                settled = Some(resolution);
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(matches!(settled, Some(Resolution::Fresh(Ok(_)))));

        // Block the next scan: answers come from the last completed listing (8).
        let blocked = wait.clone();
        let cached = scan
            .resolve(move || {
                blocked.lock().unwrap().recv().unwrap();
                listing(9)
            })
            .await;
        assert!(matches!(cached, Resolution::Cached(_)), "{cached:?}");
        assert_eq!(tickets(&cached), Some(8));
        let flag = started.clone();
        let joined = scan
            .resolve(move || {
                flag.fetch_add(1, Ordering::SeqCst);
                listing(0)
            })
            .await;
        assert_eq!(tickets(&joined), Some(8));
        assert_eq!(
            started.load(Ordering::SeqCst),
            0,
            "a probe during a blocked scan must join it, not start another"
        );

        // Once released, the blocked scan's own result becomes the answer.
        open.send(()).unwrap();
        let mut refreshed = false;
        for _ in 0..100 {
            let resolution = scan.resolve(|| listing(9)).await;
            if tickets(&resolution) == Some(9) {
                refreshed = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(refreshed);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_failed_scan_is_reported_fresh_and_keeps_the_last_good_listing() {
        let scan = Arc::new(HealthScan::new(Duration::from_secs(5)));
        assert_eq!(tickets(&scan.resolve(|| listing(4)).await), Some(4));
        let failure = ScanFailure {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: "unreadable".into(),
        };
        let expected = failure.clone();
        match scan.resolve(move || Err(failure)).await {
            Resolution::Fresh(Err(error)) => assert_eq!(error, expected),
            other => panic!("expected a fresh failure, got {other:?}"),
        }
        assert_eq!(scan.state.lock().unwrap().last.as_ref().unwrap().tickets, 4);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_panicked_scan_does_not_wedge_later_probes() {
        let scan = Arc::new(HealthScan::new(Duration::from_secs(5)));
        let resolution = scan.resolve(|| panic!("scan crashed")).await;
        assert!(matches!(resolution, Resolution::Pending), "{resolution:?}");
        // The dead scan's receiver is replaced; the next probe scans again.
        assert_eq!(tickets(&scan.resolve(|| listing(2)).await), Some(2));
    }
}
