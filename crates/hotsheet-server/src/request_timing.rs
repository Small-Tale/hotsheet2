//! Server-side API request timing budget (HS2-0DH8ZS).
//!
//! Everything the web client calls is local, so an ordinary API request should finish
//! well under [`API_BUDGET`]. The middleware times every routed request and logs a
//! `warn` with the method, matched route template, status, and duration when it
//! exceeds its class budget, so a slow note save (or anything else) is visible in the
//! server log instead of only as a stalled UI.
//!
//! Budget classes:
//! - [`RequestClass::Api`]: every ordinary JSON request, [`API_BUDGET`] (100 ms).
//! - [`RequestClass::Transfer`]: attachment uploads/downloads, media, and speech
//!   synthesis move file bodies, so they get [`TRANSFER_BUDGET`].
//! - [`RequestClass::Streaming`]: WebSocket upgrades, the long-poll event fallback, and
//!   terminal attach are intentionally long-lived and are never timed.
//! - [`RequestClass::LongWait`]: an ordinary request whose response waits for a human or an
//!   external event (a permission ask parks for up to 24 h; the GitHub device-auth wait parks
//!   until the person authorizes). Never timed, and never offloaded (HS2-V3S89Q).
//!
//! A request that had to wait for an API offload slot (the blocking pool was saturated) is
//! also reported, with a `queue` entry in `Server-Timing` and a warning naming the wait and
//! the slots in use (HS2-V3S89Q).

use axum::{
    extract::{MatchedPath, Request},
    http::{HeaderName, HeaderValue, Method},
    middleware::Next,
    response::Response,
};
use std::time::{Duration, Instant};

const SERVER_TIMING: HeaderName = HeaderName::from_static("server-timing");

/// Budget for an ordinary local API request.
pub(crate) const API_BUDGET: Duration = Duration::from_millis(100);
/// Budget for a request that transfers a file body (attachments, media, speech).
pub(crate) const TRANSFER_BUDGET: Duration = Duration::from_millis(2_000);

/// How a route is timed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RequestClass {
    Api,
    Transfer,
    Streaming,
    LongWait,
}

impl RequestClass {
    /// The warning threshold for this class; `None` means the request is not timed.
    pub(crate) fn budget(self) -> Option<Duration> {
        match self {
            Self::Api => Some(API_BUDGET),
            Self::Transfer => Some(TRANSFER_BUDGET),
            Self::Streaming | Self::LongWait => None,
        }
    }
}

/// Route templates whose response waits on a human or an external event rather than on
/// local work (HS2-V3S89Q). Keep every such route here: an offloaded long wait would hold an
/// API slot and a pool thread for its whole wait.
pub(crate) const LONG_WAIT_ROUTES: &[&str] = &[
    // Parks until a person answers the prompt, up to `DEFAULT_PERMISSION_TIMEOUT` (24 h).
    "/permissions/ask",
    // `GET` parks until the person finishes GitHub device authorization.
    "/github-auth/device/{session_id}",
];

/// Classify a matched route template (for example `/checkouts/{reference}/tickets/{id}`).
pub(crate) fn classify(route: &str) -> RequestClass {
    if route.starts_with("/ws/") || route.ends_with("/attach") {
        RequestClass::Streaming
    } else if LONG_WAIT_ROUTES.contains(&route) {
        RequestClass::LongWait
    } else if route.contains("attachments") || route.starts_with("/tts/") {
        RequestClass::Transfer
    } else {
        RequestClass::Api
    }
}

/// A request that exceeded its budget.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SlowRequest {
    pub(crate) method: Method,
    pub(crate) route: String,
    pub(crate) status: u16,
    pub(crate) elapsed: Duration,
    pub(crate) budget: Duration,
}

/// The over-budget report for one finished request, if any.
pub(crate) fn evaluate(
    method: &Method,
    route: &str,
    status: u16,
    elapsed: Duration,
) -> Option<SlowRequest> {
    let budget = classify(route).budget()?;
    (elapsed > budget).then(|| SlowRequest {
        method: method.clone(),
        route: route.to_string(),
        status,
        elapsed,
        budget,
    })
}

/// The `Server-Timing` entry for a handler duration, in milliseconds with microsecond precision.
pub(crate) fn server_timing(elapsed: Duration) -> String {
    format!("handler;dur={:.3}", elapsed.as_secs_f64() * 1_000.0)
}

/// How long a request waited for an API offload slot, and how many slots were in use when it
/// got one. [`crate::blocking_offload::offload`] attaches it to the response when the pool was
/// saturated; [`time_request`] reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SlotWait {
    pub(crate) waited: Duration,
    pub(crate) in_use: usize,
    pub(crate) capacity: usize,
}

/// The `Server-Timing` entry for an offload-slot wait.
pub(crate) fn queue_timing(waited: Duration) -> String {
    format!("queue;dur={:.3}", waited.as_secs_f64() * 1_000.0)
}

/// Axum middleware: time the request and warn when it exceeds its class budget. The
/// route template (never the concrete path) is logged so ticket ids and checkout paths
/// do not leak into logs and slow routes aggregate cleanly.
pub(crate) async fn time_request(request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let route = request.extensions().get::<MatchedPath>().map_or_else(
        || "<unmatched>".to_string(),
        |path| path.as_str().to_string(),
    );
    let started = Instant::now();
    let mut response = next.run(request).await;
    let elapsed = started.elapsed();
    // Expose the handler time so the web client's dev latency monitor can tell server time
    // from browser/proxy queueing (HS2-ZGBQ5Q). Streams are never timed.
    if classify(&route).budget().is_some() {
        if let Ok(value) = HeaderValue::from_str(&server_timing(elapsed)) {
            response.headers_mut().append(SERVER_TIMING, value);
        }
    }
    if let Some(wait) = response.extensions().get::<SlotWait>().copied() {
        if let Ok(value) = HeaderValue::from_str(&queue_timing(wait.waited)) {
            response.headers_mut().append(SERVER_TIMING, value);
        }
        tracing::warn!(
            method = %method,
            route = %route,
            waited_ms = wait.waited.as_millis() as u64,
            in_use = wait.in_use,
            capacity = wait.capacity,
            "API request waited for an offload slot; the request pool is saturated"
        );
    }
    if let Some(slow) = evaluate(&method, &route, response.status().as_u16(), elapsed) {
        tracing::warn!(
            method = %slow.method,
            route = %slow.route,
            status = slow.status,
            elapsed_ms = slow.elapsed.as_millis() as u64,
            budget_ms = slow.budget.as_millis() as u64,
            "slow API request exceeded its budget"
        );
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, body::Body, routing::get};
    use std::sync::{Arc, Mutex};
    use tower::ServiceExt;

    #[test]
    fn routes_are_classified_into_budgets() {
        for route in [
            "/checkouts/{reference}/tickets/{id}",
            "/tickets/{id}",
            "/checkouts",
            "/batch",
        ] {
            assert_eq!(classify(route), RequestClass::Api, "{route}");
            assert_eq!(classify(route).budget(), Some(API_BUDGET));
        }
        for route in [
            "/tickets/{id}/attachments",
            "/checkouts/{reference}/tickets/{id}/attachments/{attachment_id}/original",
            "/provider-attachments/copy",
            "/tts/synthesize",
        ] {
            assert_eq!(classify(route), RequestClass::Transfer, "{route}");
            assert_eq!(classify(route).budget(), Some(TRANSFER_BUDGET));
        }
        for route in ["/ws/sync", "/ws/poll", "/terminals/{id}/attach"] {
            assert_eq!(classify(route), RequestClass::Streaming, "{route}");
            assert_eq!(classify(route).budget(), None);
        }
        // Waits on a person are neither timed nor offloaded (HS2-V3S89Q).
        for route in ["/permissions/ask", "/github-auth/device/{session_id}"] {
            assert_eq!(classify(route), RequestClass::LongWait, "{route}");
            assert_eq!(classify(route).budget(), None);
        }
        for route in [
            "/permissions",
            "/permissions/{id}",
            "/github-auth/device",
            "/github-auth/device/{session_id}/repositories",
        ] {
            assert_eq!(classify(route), RequestClass::Api, "{route}");
        }
    }

    #[test]
    fn evaluate_reports_only_over_budget_requests() {
        let route = "/checkouts/{reference}/tickets/{id}";
        assert_eq!(
            evaluate(&Method::PATCH, route, 200, Duration::from_millis(100)),
            None
        );
        let slow = evaluate(&Method::PATCH, route, 200, Duration::from_millis(101)).unwrap();
        assert_eq!(slow.route, route);
        assert_eq!(slow.budget, API_BUDGET);
        // An upload gets the larger transfer budget.
        let upload = "/checkouts/{reference}/tickets/{id}/attachments";
        assert_eq!(
            evaluate(&Method::POST, upload, 201, Duration::from_millis(1_500)),
            None
        );
        assert!(evaluate(&Method::POST, upload, 201, Duration::from_millis(2_001)).is_some());
        // Long-lived streams are never reported.
        assert_eq!(
            evaluate(&Method::GET, "/ws/poll", 200, Duration::from_secs(30)),
            None
        );
    }

    #[test]
    fn server_timing_reports_milliseconds() {
        assert_eq!(
            server_timing(Duration::from_micros(12_345)),
            "handler;dur=12.345"
        );
    }

    #[derive(Clone, Default)]
    struct Capture(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for Capture {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Capture {
        type Writer = Self;
        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    /// End to end through a real router: a slow handler is logged with its route
    /// template and duration; a fast one and a slow long-poll are not.
    #[tokio::test(flavor = "current_thread")]
    async fn middleware_warns_for_slow_routes_only() {
        let capture = Capture::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(capture.clone())
            .with_ansi(false)
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);
        let app = Router::new()
            .route(
                "/tickets/{id}",
                get(|| async {
                    tokio::time::sleep(Duration::from_millis(150)).await;
                    "slow"
                }),
            )
            .route("/fast", get(|| async { "fast" }))
            .route(
                "/ws/poll",
                get(|| async {
                    tokio::time::sleep(Duration::from_millis(150)).await;
                    "poll"
                }),
            )
            .layer(axum::middleware::from_fn(time_request));
        for uri in ["/fast", "/ws/poll", "/tickets/HS-1"] {
            let response = app
                .clone()
                .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), 200);
            // Timed routes expose their handler time; streams do not (HS2-ZGBQ5Q).
            let timing = response
                .headers()
                .get("server-timing")
                .map(|value| value.to_str().unwrap().to_string());
            if uri == "/ws/poll" {
                assert_eq!(timing, None);
            } else {
                assert!(timing.unwrap().starts_with("handler;dur="), "{uri}");
            }
        }
        let log = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
        let warnings = log
            .lines()
            .filter(|line| line.contains("slow API request"))
            .collect::<Vec<_>>();
        assert_eq!(warnings.len(), 1, "{log}");
        assert!(warnings[0].contains("route=/tickets/{id}"), "{log}");
        assert!(warnings[0].contains("method=GET"), "{log}");
        assert!(warnings[0].contains("budget_ms=100"), "{log}");
        assert!(
            !warnings[0].contains("HS-1"),
            "concrete ids stay out of logs: {log}"
        );
    }

    /// A response that carries a slot wait is reported with a `queue` timing and a
    /// saturation warning; one without is not (HS2-V3S89Q).
    #[tokio::test(flavor = "current_thread")]
    async fn middleware_reports_offload_slot_waits() {
        let capture = Capture::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(capture.clone())
            .with_ansi(false)
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);
        let app = Router::new()
            .route(
                "/tickets/{id}",
                get(|| async {
                    let mut response = axum::response::IntoResponse::into_response("queued");
                    response.extensions_mut().insert(SlotWait {
                        waited: Duration::from_millis(42),
                        in_use: 256,
                        capacity: 256,
                    });
                    response
                }),
            )
            .route("/tickets", get(|| async { "fast" }))
            .layer(axum::middleware::from_fn(time_request));
        let queued = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/tickets/HS-1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let timing: Vec<_> = queued
            .headers()
            .get_all("server-timing")
            .iter()
            .map(|value| value.to_str().unwrap().to_string())
            .collect();
        assert!(
            timing.iter().any(|entry| entry == "queue;dur=42.000"),
            "{timing:?}"
        );
        let fast = app
            .oneshot(
                Request::builder()
                    .uri("/tickets")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(
            fast.headers()
                .get_all("server-timing")
                .iter()
                .all(|value| !value.to_str().unwrap().starts_with("queue")),
        );
        let log = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
        let warnings = log
            .lines()
            .filter(|line| line.contains("waited for an offload slot"))
            .collect::<Vec<_>>();
        assert_eq!(warnings.len(), 1, "{log}");
        assert!(warnings[0].contains("route=/tickets/{id}"), "{log}");
        assert!(warnings[0].contains("waited_ms=42"), "{log}");
        assert!(warnings[0].contains("in_use=256"), "{log}");
        assert!(!warnings[0].contains("HS-1"), "{log}");
    }
}
