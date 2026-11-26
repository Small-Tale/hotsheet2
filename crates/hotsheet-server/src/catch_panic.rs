//! Router-wide panic boundary (HS2-GSW2AA).
//!
//! A handler that panics on the async workers (streaming and long-wait routes, which
//! [`crate::blocking_offload`] leaves there) would otherwise unwind through hyper, which drops
//! the connection without a response. [`catch_panic`] turns that into a logged `500`,
//! the same answer an offloaded handler's panic already gets.
//!
//! Unwinding past the handler is safe to continue from: server state sits behind mutexes that
//! follow the recover-on-poison policy (`hotsheet_sync::LockExt`, HS2-ZGQJZP).

use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use futures_util::FutureExt;
use std::panic::AssertUnwindSafe;

/// Axum middleware: answer `500` when the rest of the request panics.
pub(crate) async fn catch_panic(request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    match AssertUnwindSafe(next.run(request)).catch_unwind().await {
        Ok(response) => response,
        Err(panic) => {
            let message = panic
                .downcast_ref::<&str>()
                .map(|text| (*text).to_owned())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "non-string panic payload".to_owned());
            tracing::error!(%method, %path, panic = %message, "request handler panicked");
            (StatusCode::INTERNAL_SERVER_ERROR, "request handler failed").into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, body::Body, middleware, routing::get};
    use tower::ServiceExt;

    fn router() -> Router {
        Router::new()
            .route("/ok", get(|| async { "fine" }))
            .route(
                "/panics",
                get(|| async {
                    if true {
                        panic!("handler bug");
                    }
                    "unreachable"
                }),
            )
            .layer(middleware::from_fn(catch_panic))
    }

    async fn status(router: Router, path: &str) -> StatusCode {
        router
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap()
            .status()
    }

    #[tokio::test]
    async fn hs2_gsw2aa_a_panicking_handler_answers_500_and_the_router_keeps_serving() {
        let router = router();
        assert_eq!(
            status(router.clone(), "/panics").await,
            StatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!(status(router.clone(), "/ok").await, StatusCode::OK);
        assert_eq!(
            status(router, "/panics").await,
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }
}
