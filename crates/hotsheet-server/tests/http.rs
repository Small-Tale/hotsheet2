//! HTTP E2E for the server, driven through the router in-process (no real socket).

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use hotsheet_server::{AppState, app};
use hotsheet_ticketing::{FsStore, StoreMetadata};
use http_body_util::BodyExt;
use tower::ServiceExt;

const SECRET: &str = "test-secret";

fn state() -> (tempfile::TempDir, AppState) {
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    (dir, AppState::new(store, SECRET.into()))
}

fn authed(method: &str, uri: &str, body: Option<&str>) -> Request<Body> {
    let mut b = Request::builder()
        .method(method)
        .uri(uri)
        .header("x-hotsheet-secret", SECRET);
    if body.is_some() {
        b = b.header(header::CONTENT_TYPE, "application/json");
    }
    b.body(body.map_or(Body::empty(), |s| Body::from(s.to_string())))
        .unwrap()
}

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn health_needs_no_secret() {
    let (_d, st) = state();
    let resp = app(st)
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(body_json(resp).await["status"], "ok");
}

#[tokio::test]
async fn tickets_require_the_secret() {
    let (_d, st) = state();
    let resp = app(st)
        .oneshot(
            Request::builder()
                .uri("/tickets")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn create_get_update_close_and_query() {
    let (_d, st) = state();
    let app = app(st);

    // create
    let resp = app
        .clone()
        .oneshot(authed(
            "POST",
            "/tickets",
            Some(r#"{"title":"Fix flicker","category":"bug","priority":"high","up_next":true}"#),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let created = body_json(resp).await;
    let slug = created["slug"].as_str().unwrap().to_string();
    assert!(slug.starts_with("HS-"));
    assert_eq!(created["title"], "Fix flicker");
    assert_eq!(created["priority"], "high");
    assert_eq!(created["up_next"], true);

    // get (by slug)
    let resp = app
        .clone()
        .oneshot(authed("GET", &format!("/tickets/{slug}"), None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(body_json(resp).await["slug"], slug);

    // update status -> stamps completed_at
    let resp = app
        .clone()
        .oneshot(authed(
            "PATCH",
            &format!("/tickets/{slug}"),
            Some(r#"{"status":"completed","details":"root-caused"}"#),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let updated = body_json(resp).await;
    assert_eq!(updated["status"], "completed");
    assert_eq!(updated["details"], "root-caused");
    assert!(updated["completed_at"].is_string());

    // close
    let resp = app
        .clone()
        .oneshot(authed(
            "POST",
            &format!("/tickets/{slug}/close"),
            Some(r#"{"reason":"completed"}"#),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(body_json(resp).await["close_reason"], "completed");

    // query with filters
    let resp = app
        .clone()
        .oneshot(authed("GET", "/tickets?status=completed&text=flick", None))
        .await
        .unwrap();
    assert_eq!(body_json(resp).await.as_array().unwrap().len(), 1);

    // unknown ticket -> 404
    let resp = app
        .clone()
        .oneshot(authed("GET", "/tickets/HS-NOPE00", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn close_duplicate_without_target_is_a_400() {
    let (_d, st) = state();
    let app = app(st);
    let created = body_json(
        app.clone()
            .oneshot(authed("POST", "/tickets", Some(r#"{"title":"t"}"#)))
            .await
            .unwrap(),
    )
    .await;
    let slug = created["slug"].as_str().unwrap();

    let resp = app
        .oneshot(authed(
            "POST",
            &format!("/tickets/{slug}/close"),
            Some(r#"{"reason":"duplicate"}"#),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}
