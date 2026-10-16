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
//! thread per open connection would waste threads.
//!
//! A handler still stops when its client disconnects: dropping the request future cancels the
//! handler future on the pool at its next await point, so cleanup that relies on cancellation
//! (withdrawing a pending permission prompt, for example) behaves as before.
//!
//! At most [`MAX_OFFLOADED`] requests are offloaded at once, well under tokio's default
//! blocking-pool cap of 512 threads, so a handler's nested `spawn_blocking` always finds a
//! free pool thread and the pool can never deadlock on itself. Further requests wait
//! asynchronously for a slot.

use crate::request_timing::{RequestClass, classify};
use axum::{
    extract::{MatchedPath, Request},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use tokio::runtime::Handle;
use tokio::sync::{Semaphore, oneshot};

/// Upper bound on requests whose handlers run on the blocking pool at the same time.
pub(crate) const MAX_OFFLOADED: usize = 256;

static OFFLOAD_SLOTS: Semaphore = Semaphore::const_new(MAX_OFFLOADED);

/// Whether a matched route's handler runs on the blocking pool.
pub(crate) fn offloads(route: &str) -> bool {
    classify(route) != RequestClass::Streaming
}

/// Axum middleware: run the rest of the request (auth and handler) on the blocking pool for
/// every non-streaming route.
pub(crate) async fn offload(request: Request, next: Next) -> Response {
    let streaming = request
        .extensions()
        .get::<MatchedPath>()
        .is_some_and(|path| !offloads(path.as_str()));
    if streaming {
        return next.run(request).await;
    }
    let Ok(slot) = OFFLOAD_SLOTS.acquire().await else {
        // The semaphore is static and never closed.
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
    match result {
        Ok(Some(response)) => response,
        // Only reachable after this future was dropped, so nobody receives this response.
        Ok(None) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        Err(error) => {
            tracing::error!(%error, "request handler failed on the blocking pool");
            (StatusCode::INTERNAL_SERVER_ERROR, "request handler failed").into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, body::Body, routing::get};
    use std::time::Duration;
    use tower::ServiceExt;

    #[test]
    fn streaming_routes_stay_on_the_async_workers() {
        for route in ["/ws/sync", "/ws/poll", "/terminals/{id}/attach"] {
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
                "/permissions/ask",
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
        let request = tokio::spawn(app.oneshot(get_request("/permissions/ask")));
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
