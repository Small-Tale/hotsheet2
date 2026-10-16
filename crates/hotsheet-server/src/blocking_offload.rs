//! Run request handlers off the async worker threads (HS2-9Z0Q8N).
//!
//! Most server handlers are `async fn`s whose bodies do synchronous store work: ticket file
//! reads and writes, `flock` waits, in-memory index mutexes, and SQLite queries. Run directly
//! on a tokio worker, a handler stuck on a contended lock or a slow disk occupies that worker,
//! and a few such requests starve every unrelated request (a ticket detail can take seconds
//! while CLI agents write).
//!
//! Rather than wrapping each handler body by hand, [`offload`] is one router-wide layer:
//! every timed request's handler future runs on tokio's blocking pool, driven there by
//! [`Handle::block_on`]. Blocking calls then block a pool thread, never a worker, while the
//! handler's own `.await`s (timers, nested `spawn_blocking`, channels) still work because the
//! multi-thread runtime keeps driving its IO and timer drivers.
//!
//! Long-lived [`RequestClass::Streaming`] routes (WebSocket upgrades, the long-poll event
//! fallback, terminal attach) stay on the async workers: they mostly wait, and pinning a pool
//! thread per open connection would waste threads. [`RequestClass::LongWait`] routes (a
//! permission ask parked on a person for up to 24 h, the GitHub device-auth wait) stay there
//! too, so pending prompts can never use up the API slots and queue every other request
//! (HS2-V3S89Q). Their synchronous parts are short in-memory work; a blocking wait inside one
//! uses its own `spawn_blocking`.
//!
//! A handler still stops when its client disconnects: dropping the request future cancels the
//! handler future on the pool at its next await point, so cleanup that relies on cancellation
//! (withdrawing a pending permission prompt, for example) behaves as before.
//!
//! At most [`MAX_OFFLOADED`] requests are offloaded at once, well under tokio's default
//! blocking-pool cap of 512 threads, so a handler's nested `spawn_blocking` always finds a
//! free pool thread and the pool can never deadlock on itself. Further requests wait
//! asynchronously for a slot. A request that had to wait carries a [`SlotWait`] on its
//! response so `request_timing` warns about the saturation, and [`OffloadStats`] (reported by
//! `GET /health`) exposes the slots in use, the requests waiting, and the long waits parked.

use crate::request_timing::{RequestClass, SlotWait, classify};
use axum::{
    extract::{MatchedPath, Request},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;
use tokio::runtime::Handle;
use tokio::sync::{Semaphore, SemaphorePermit, oneshot};

/// Upper bound on requests whose handlers run on the blocking pool at the same time.
pub(crate) const MAX_OFFLOADED: usize = 256;

static OFFLOAD_SLOTS: Semaphore = Semaphore::const_new(MAX_OFFLOADED);
/// Requests currently waiting for an offload slot.
static WAITING: AtomicUsize = AtomicUsize::new(0);
/// Long-wait requests currently parked on the async workers.
static LONG_WAITS: AtomicUsize = AtomicUsize::new(0);

/// A snapshot of the request pool, for `GET /health` (HS2-V3S89Q).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub(crate) struct OffloadStats {
    /// Offloaded requests running now.
    pub(crate) in_use: usize,
    /// [`MAX_OFFLOADED`].
    pub(crate) capacity: usize,
    /// Requests queued for a slot; nonzero means the pool is saturated.
    pub(crate) waiting: usize,
    /// Long-wait requests (pending permission asks, device-auth waits) parked off the pool.
    pub(crate) long_waits: usize,
}

pub(crate) fn stats() -> OffloadStats {
    OffloadStats {
        in_use: MAX_OFFLOADED - OFFLOAD_SLOTS.available_permits(),
        capacity: MAX_OFFLOADED,
        waiting: WAITING.load(Ordering::Relaxed),
        long_waits: LONG_WAITS.load(Ordering::Relaxed),
    }
}

/// Whether a matched route's handler runs on the blocking pool.
#[cfg(test)]
pub(crate) fn offloads(route: &str) -> bool {
    !matches!(
        classify(route),
        RequestClass::Streaming | RequestClass::LongWait
    )
}

/// Decrements a counter when dropped, so a cancelled request still leaves it accurate.
struct Counted(&'static AtomicUsize);

impl Counted {
    fn new(counter: &'static AtomicUsize) -> Self {
        counter.fetch_add(1, Ordering::Relaxed);
        Self(counter)
    }
}

impl Drop for Counted {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Take a slot, waiting asynchronously when all are in use. Reports how long it waited.
async fn acquire_slot() -> (Option<SemaphorePermit<'static>>, Option<SlotWait>) {
    if let Ok(slot) = OFFLOAD_SLOTS.try_acquire() {
        return (Some(slot), None);
    }
    let started = Instant::now();
    let slot = {
        let _waiting = Counted::new(&WAITING);
        // The semaphore is static and never closed.
        OFFLOAD_SLOTS.acquire().await.ok()
    };
    let wait = SlotWait {
        waited: started.elapsed(),
        in_use: MAX_OFFLOADED - OFFLOAD_SLOTS.available_permits(),
        capacity: MAX_OFFLOADED,
    };
    (slot, Some(wait))
}

/// Axum middleware: run the rest of the request (auth and handler) on the blocking pool for
/// every non-streaming route.
pub(crate) async fn offload(request: Request, next: Next) -> Response {
    let class = request
        .extensions()
        .get::<MatchedPath>()
        .map(|path| classify(path.as_str()));
    match class {
        Some(RequestClass::Streaming) => return next.run(request).await,
        Some(RequestClass::LongWait) => {
            let _parked = Counted::new(&LONG_WAITS);
            return next.run(request).await;
        }
        _ => {}
    }
    let (slot, wait) = acquire_slot().await;
    let Some(slot) = slot else {
        return next.run(request).await;
    };
    let handle = Handle::current();
    // Dropping this future (the client disconnected) drops `_cancel`, which ends the handler on
    // the pool at its next await point, exactly as dropping an async handler would. Handlers
    // rely on that, for example to withdraw a pending permission prompt.
    let (_cancel, cancelled) = oneshot::channel::<()>();
    let result = tokio::task::spawn_blocking(move || {
        let _slot = slot;
        handle.block_on(async move {
            tokio::select! {
                biased;
                response = next.run(request) => Some(response),
                _ = cancelled => None,
            }
        })
    })
    .await;
    let mut response = match result {
        Ok(Some(response)) => response,
        // Only reachable after this future was dropped, so nobody receives this response.
        Ok(None) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        Err(error) => {
            tracing::error!(%error, "request handler failed on the blocking pool");
            (StatusCode::INTERNAL_SERVER_ERROR, "request handler failed").into_response()
        }
    };
    if let Some(wait) = wait {
        response.extensions_mut().insert(wait);
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, body::Body, routing::get};
    use std::time::Duration;
    use tower::ServiceExt;

    #[test]
    fn streaming_routes_stay_on_the_async_workers() {
        for route in [
            "/ws/sync",
            "/ws/poll",
            "/terminals/{id}/attach",
            "/permissions/ask",
            "/github-auth/device/{session_id}",
        ] {
            assert!(!offloads(route), "{route}");
        }
        for route in [
            "/tickets",
            "/checkouts/{reference}/tickets/{id}",
            "/tickets/{id}/attachments",
            "/health",
        ] {
            assert!(offloads(route), "{route}");
        }
    }

    fn get_request(uri: &str) -> Request {
        Request::builder().uri(uri).body(Body::empty()).unwrap()
    }

    /// With a single async worker, a handler that blocks its thread must not stop an
    /// unrelated request from completing.
    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn a_blocking_handler_does_not_starve_another_request() {
        let (release, released) = std::sync::mpsc::channel::<()>();
        let released = std::sync::Arc::new(std::sync::Mutex::new(released));
        let app = Router::new()
            .route(
                "/tickets",
                get(move || {
                    let released = released.clone();
                    async move {
                        // A synchronous wait standing in for a contended flock / mutex.
                        let _ = released
                            .lock()
                            .unwrap()
                            .recv_timeout(Duration::from_secs(10));
                        "slow"
                    }
                }),
            )
            .route("/fast", get(|| async { "fast" }))
            .layer(axum::middleware::from_fn(offload));
        let slow = tokio::spawn(app.clone().oneshot(get_request("/tickets")));
        tokio::time::sleep(Duration::from_millis(50)).await;
        let fast = tokio::spawn(app.clone().oneshot(get_request("/fast")));
        let fast = tokio::time::timeout(Duration::from_secs(2), fast)
            .await
            .expect("an unrelated request was starved by a blocking handler")
            .unwrap()
            .unwrap();
        assert_eq!(fast.status(), StatusCode::OK);
        assert!(!slow.is_finished());
        release.send(()).unwrap();
        assert_eq!(slow.await.unwrap().unwrap().status(), StatusCode::OK);
    }

    async fn wait_until(what: &str, mut ready: impl FnMut() -> bool) {
        tokio::time::timeout(Duration::from_secs(10), async {
            while !ready() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for {what}: {:?}", stats()));
    }

    /// More pending permission asks than there are API slots must not delay an unrelated
    /// request, and they are counted as long waits, not slots in use (HS2-V3S89Q).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pending_long_waits_never_take_api_slots() {
        let asks = MAX_OFFLOADED + 16;
        let app = Router::new()
            .route(
                "/permissions/ask",
                get(|| async {
                    std::future::pending::<()>().await;
                    "never"
                }),
            )
            .route("/tickets", get(|| async { "fast" }))
            .layer(axum::middleware::from_fn(offload));
        let pending: Vec<_> = (0..asks)
            .map(|_| tokio::spawn(app.clone().oneshot(get_request("/permissions/ask"))))
            .collect();
        wait_until("every ask to park", || stats().long_waits == asks).await;
        assert_eq!(stats().in_use, 0, "a parked ask holds an API slot");
        let started = Instant::now();
        let response = tokio::time::timeout(
            Duration::from_secs(2),
            app.clone().oneshot(get_request("/tickets")),
        )
        .await
        .expect("pending asks delayed an unrelated request")
        .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(
            response.extensions().get::<SlotWait>().is_none(),
            "the unrelated request did not wait for a slot"
        );
        for ask in &pending {
            ask.abort();
        }
        for ask in pending {
            let _ = ask.await;
        }
        wait_until("cancelled asks to be uncounted", || stats().long_waits == 0).await;
    }

    /// When every slot is busy a further request waits, is counted as waiting, and its
    /// response carries the wait so `request_timing` reports the saturation (HS2-V3S89Q).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_request_that_waits_for_a_slot_reports_the_wait() {
        let (release, _) = tokio::sync::watch::channel(false);
        let gate = release.subscribe();
        let app = Router::new()
            .route(
                "/busy",
                get(move || {
                    let mut gate = gate.clone();
                    async move {
                        let _ = gate.wait_for(|open| *open).await;
                        "busy"
                    }
                }),
            )
            .route("/tickets", get(|| async { "queued" }))
            .layer(axum::middleware::from_fn(offload));
        let busy: Vec<_> = (0..MAX_OFFLOADED)
            .map(|_| tokio::spawn(app.clone().oneshot(get_request("/busy"))))
            .collect();
        wait_until("every slot to fill", || stats().in_use == MAX_OFFLOADED).await;
        let queued = tokio::spawn(app.clone().oneshot(get_request("/tickets")));
        wait_until("the request to queue", || stats().waiting == 1).await;
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(!queued.is_finished(), "no slot was free");
        release.send(true).unwrap();
        let response = tokio::time::timeout(Duration::from_secs(5), queued)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let wait = response
            .extensions()
            .get::<SlotWait>()
            .copied()
            .expect("a queued request reports its slot wait");
        assert!(wait.waited >= Duration::from_millis(30), "{wait:?}");
        assert_eq!(wait.capacity, MAX_OFFLOADED);
        for request in busy {
            assert_eq!(request.await.unwrap().unwrap().status(), StatusCode::OK);
        }
        wait_until("the pool to drain", || {
            stats().in_use == 0 && stats().waiting == 0
        })
        .await;
    }

    /// Handlers keep full async behavior on the pool: timers and nested `spawn_blocking`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn offloaded_handlers_still_await_timers_and_nested_blocking_work() {
        let app = Router::new()
            .route(
                "/tickets",
                get(|| async {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                    tokio::task::spawn_blocking(|| "nested").await.unwrap()
                }),
            )
            .layer(axum::middleware::from_fn(offload));
        let response = app.oneshot(get_request("/tickets")).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    /// Dropping the request (a disconnected client) cancels the handler on the pool.
    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn a_dropped_request_cancels_its_offloaded_handler() {
        struct DropFlag(std::sync::Arc<std::sync::atomic::AtomicBool>);
        impl Drop for DropFlag {
            fn drop(&mut self) {
                self.0.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }
        let dropped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (entered_tx, entered) = tokio::sync::oneshot::channel::<()>();
        let entered_tx = std::sync::Arc::new(std::sync::Mutex::new(Some(entered_tx)));
        let flag = dropped.clone();
        let app = Router::new()
            .route(
                "/tickets",
                get(move || {
                    let flag = DropFlag(flag.clone());
                    let entered_tx = entered_tx.clone();
                    async move {
                        let _flag = flag;
                        if let Some(tx) = entered_tx.lock().unwrap().take() {
                            let _ = tx.send(());
                        }
                        std::future::pending::<()>().await;
                        "never"
                    }
                }),
            )
            .layer(axum::middleware::from_fn(offload));
        let request = tokio::spawn(app.oneshot(get_request("/tickets")));
        entered.await.unwrap();
        assert!(!dropped.load(std::sync::atomic::Ordering::SeqCst));
        request.abort();
        let _ = request.await;
        tokio::time::timeout(Duration::from_secs(2), async {
            while !dropped.load(std::sync::atomic::Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("the handler kept running after its request was dropped");
    }

    /// A panicking handler becomes a 500 instead of tearing down the connection task.
    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn a_panicking_handler_becomes_a_server_error() {
        let app = Router::new()
            .route(
                "/tickets",
                get(|| async {
                    if true {
                        panic!("handler bug");
                    }
                    "unreachable"
                }),
            )
            .layer(axum::middleware::from_fn(offload));
        let response = app.oneshot(get_request("/tickets")).await.unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
