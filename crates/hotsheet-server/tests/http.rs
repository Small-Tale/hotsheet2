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
    (dir, AppState::new(store, SECRET.into()).unwrap())
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
async fn blocked_by_set_clear_and_reject() {
    let (_d, st) = state();
    let app = app(st);

    let mk = |title: &str| format!(r#"{{"title":"{title}"}}"#);
    let a = body_json(
        app.clone()
            .oneshot(authed("POST", "/tickets", Some(&mk("blocker"))))
            .await
            .unwrap(),
    )
    .await;
    let a_slug = a["slug"].as_str().unwrap().to_string();
    let a_id = a["id"].as_str().unwrap().to_string();

    // create with a blocker (by slug) resolves to the ULID on the wire
    let b = body_json(
        app.clone()
            .oneshot(authed(
                "POST",
                "/tickets",
                Some(&format!(
                    r#"{{"title":"blocked","blocked_by":["{a_slug}"]}}"#
                )),
            ))
            .await
            .unwrap(),
    )
    .await;
    let b_slug = b["slug"].as_str().unwrap().to_string();
    assert_eq!(b["blocked_by"], serde_json::json!([a_id]));

    // PATCH with [] clears
    let cleared = body_json(
        app.clone()
            .oneshot(authed(
                "PATCH",
                &format!("/tickets/{b_slug}"),
                Some(r#"{"blocked_by":[]}"#),
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(cleared["blocked_by"], serde_json::json!([]));

    // self-reference → 400
    let resp = app
        .clone()
        .oneshot(authed(
            "PATCH",
            &format!("/tickets/{b_slug}"),
            Some(&format!(r#"{{"blocked_by":["{b_slug}"]}}"#)),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // unknown blocker → 404
    let resp = app
        .clone()
        .oneshot(authed(
            "PATCH",
            &format!("/tickets/{b_slug}"),
            Some(r#"{"blocked_by":["HS-NOPE00"]}"#),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn list_is_compact_by_default_and_supports_limit() {
    let (_d, st) = state();
    let app = app(st);

    // Two tickets, one with a Markdown body.
    for body in [
        r#"{"title":"has body","details":"a long markdown body"}"#,
        r#"{"title":"no body"}"#,
    ] {
        let resp = app
            .clone()
            .oneshot(authed("POST", "/tickets", Some(body)))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    // Default list omits the body (the key is absent on every row).
    let resp = app
        .clone()
        .oneshot(authed("GET", "/tickets", None))
        .await
        .unwrap();
    let rows = body_json(resp).await;
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|r| r.get("details").is_none()));

    // compact=false keeps the body.
    let resp = app
        .clone()
        .oneshot(authed("GET", "/tickets?compact=false&text=markdown", None))
        .await
        .unwrap();
    let full = body_json(resp).await;
    assert_eq!(
        full.as_array().unwrap()[0]["details"],
        "a long markdown body"
    );

    // limit caps the rows.
    let resp = app
        .clone()
        .oneshot(authed("GET", "/tickets?limit=1", None))
        .await
        .unwrap();
    assert_eq!(body_json(resp).await.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn watcher_reindexes_an_external_write() {
    use hotsheet_model::{Timestamp, Ulid};
    use hotsheet_ticketing::{NewTicket, ops};

    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    let state = AppState::new(store.clone(), SECRET.into()).unwrap();
    let _watch = hotsheet_server::spawn_watcher(state.clone()).unwrap();
    let app = app(state);

    // Write a ticket to disk WITHOUT going through the server (a CLI/git-style edit).
    ops::create(
        &store,
        Ulid::new(),
        "HS",
        Timestamp::new("2026-08-19T00:00:00Z"),
        NewTicket {
            title: "external ticket".into(),
            category: "task".into(),
            ..Default::default()
        },
    )
    .unwrap();

    // The watcher should reindex it; poll the HTTP query until it appears.
    for _ in 0..40 {
        let resp = app
            .clone()
            .oneshot(authed("GET", "/tickets?text=external", None))
            .await
            .unwrap();
        if body_json(resp).await.as_array().unwrap().len() == 1 {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    panic!("watcher did not reindex the external write within 4s");
}

/// The watcher also regenerates the derived `worklist.md` on change (HS2-90) — once per
/// debounced batch, from the store root (outside the watched tickets/ dir).
#[tokio::test]
async fn watcher_regenerates_the_worklist() {
    use hotsheet_model::{Timestamp, Ulid};
    use hotsheet_ticketing::{NewTicket, ops};

    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    let state = AppState::new(store.clone(), SECRET.into()).unwrap();
    let _watch = hotsheet_server::spawn_watcher(state).unwrap();

    ops::create(
        &store,
        Ulid::new(),
        "HS",
        Timestamp::new("2026-08-19T00:00:00Z"),
        NewTicket {
            title: "worklist me".into(),
            category: "task".into(),
            up_next: true,
            ..Default::default()
        },
    )
    .unwrap();

    let worklist = dir.path().join("worklist.md");
    for _ in 0..40 {
        if let Ok(body) = std::fs::read_to_string(&worklist) {
            if body.contains("worklist me") && body.contains("## Up Next") {
                return;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    panic!("watcher did not regenerate worklist.md within 4s");
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

#[tokio::test]
async fn update_can_append_a_note() {
    let (_d, st) = state();
    let app = app(st);

    let created = body_json(
        app.clone()
            .oneshot(authed(
                "POST",
                "/tickets",
                Some(r#"{"title":"Fix flicker"}"#),
            ))
            .await
            .unwrap(),
    )
    .await;
    let id = created["id"].as_str().unwrap().to_string();

    // PATCH with a status change AND a note in the same call.
    let updated = body_json(
        app.clone()
            .oneshot(authed(
                "PATCH",
                &format!("/tickets/{id}"),
                Some(r#"{"status":"started","note":"kicked it off"}"#),
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(updated["status"], "started");
    assert_eq!(updated["notes"][0]["text"], "kicked it off");

    // The note persisted (a fresh GET sees it).
    let got = body_json(
        app.oneshot(authed("GET", &format!("/tickets/{id}"), None))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(got["notes"][0]["text"], "kicked it off");
}

#[tokio::test]
async fn setup_endpoint_prepares_the_project_like_the_cli() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    let st = AppState::new(store, SECRET.into()).unwrap();

    let resp = app(st)
        .oneshot(authed("POST", "/setup/codex", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let reports = body_json(resp).await;
    assert_eq!(reports[0]["tool"], "Codex CLI");

    // Same artifacts the CLI's `setup codex` writes, into the served store.
    let agents = std::fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
    assert!(agents.contains("<!-- BEGIN hotsheet:codex -->"));
    let cfg = std::fs::read_to_string(dir.path().join(".codex/config.toml")).unwrap();
    assert!(cfg.contains("mcp_servers") && cfg.contains("hotsheet"));
}

#[tokio::test]
async fn setup_endpoint_rejects_an_unknown_tool() {
    let (_d, st) = state();
    let resp = app(st)
        .oneshot(authed("POST", "/setup/nope", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert!(
        body_json(resp).await["error"]
            .as_str()
            .unwrap()
            .contains("unknown tool 'nope'")
    );
}

#[tokio::test]
async fn setup_endpoint_needs_the_secret() {
    let (_d, st) = state();
    let resp = app(st)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/setup/codex")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

/// The `close_reason` / `closed` list filters are served through the **index**
/// (the server lists via `index.query`, not `ops::query`), so this pins that the
/// structured close tag actually round-trips to SQLite and back (HS2-61 / HS2-20).
#[tokio::test]
async fn list_filters_by_close_reason_and_closed_through_the_index() {
    let (_d, st) = state();
    let app = app(st);

    // One open ticket, one closed as `duplicate`, one closed as `completed`.
    let mut slugs = vec![];
    for title in ["still open", "a dup", "done deal"] {
        let resp = app
            .clone()
            .oneshot(authed(
                "POST",
                "/tickets",
                Some(&format!(r#"{{"title":"{title}"}}"#)),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        slugs.push(body_json(resp).await["slug"].as_str().unwrap().to_string());
    }
    // Need a real duplicate target for reason=duplicate.
    let dup_target = &slugs[0];
    for (slug, body) in [
        (
            &slugs[1],
            format!(r#"{{"reason":"duplicate","duplicate_of":"{dup_target}"}}"#),
        ),
        (&slugs[2], r#"{"reason":"completed"}"#.to_string()),
    ] {
        let resp = app
            .clone()
            .oneshot(authed(
                "POST",
                &format!("/tickets/{slug}/close"),
                Some(&body),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // closed=true → the two closed ones; closed=false → only the open one.
    let resp = app
        .clone()
        .oneshot(authed("GET", "/tickets?closed=true", None))
        .await
        .unwrap();
    assert_eq!(body_json(resp).await.as_array().unwrap().len(), 2);

    let resp = app
        .clone()
        .oneshot(authed("GET", "/tickets?closed=false", None))
        .await
        .unwrap();
    let open = body_json(resp).await;
    let open = open.as_array().unwrap();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0]["title"], "still open");

    // close_reason=completed → just the one.
    let resp = app
        .clone()
        .oneshot(authed("GET", "/tickets?close_reason=completed", None))
        .await
        .unwrap();
    let done = body_json(resp).await;
    let done = done.as_array().unwrap();
    assert_eq!(done.len(), 1);
    assert_eq!(done[0]["title"], "done deal");
}

// ---- multi-store (HS2-87) --------------------------------------------------------

#[tokio::test]
async fn hosts_multiple_stores_and_serves_a_scoped_list() {
    use hotsheet_model::{Timestamp, Ulid};
    use hotsheet_ticketing::{NewTicket, ops};

    let (_d, st) = state();
    let app = app(st);

    // The primary store is the single hosted entry to start.
    let resp = app
        .clone()
        .oneshot(authed("GET", "/stores", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let stores = body_json(resp).await;
    assert_eq!(stores.as_array().unwrap().len(), 1, "primary store hosted");

    // A second store on disk with one ticket.
    let dir2 = tempfile::tempdir().unwrap();
    let store2 = FsStore::init(dir2.path(), &StoreMetadata::new("BB")).unwrap();
    ops::create(
        &store2,
        Ulid::new(),
        "BB",
        Timestamp::new("2026-08-19T00:00:00Z"),
        NewTicket {
            title: "second-store ticket".into(),
            category: "task".into(),
            ..Default::default()
        },
    )
    .unwrap();

    // Register it.
    let body = format!(r#"{{"path":"{}"}}"#, dir2.path().display());
    let resp = app
        .clone()
        .oneshot(authed("POST", "/stores", Some(&body)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let info = body_json(resp).await;
    let id2 = info["id"].as_str().unwrap().to_string();
    assert_eq!(info["prefix"], "BB");
    assert_eq!(info["tickets"], 1);

    // Now two stores are hosted.
    let resp = app
        .clone()
        .oneshot(authed("GET", "/stores", None))
        .await
        .unwrap();
    assert_eq!(body_json(resp).await.as_array().unwrap().len(), 2);

    // The store-scoped list serves the second store's ticket from its own index.
    let resp = app
        .clone()
        .oneshot(authed("GET", &format!("/stores/{id2}/tickets"), None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let rows = body_json(resp).await;
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["title"], "second-store ticket");

    // Re-registering the same store is idempotent (200, not a duplicate).
    let resp = app
        .clone()
        .oneshot(authed("POST", "/stores", Some(&body)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let resp = app
        .clone()
        .oneshot(authed("GET", "/stores", None))
        .await
        .unwrap();
    assert_eq!(
        body_json(resp).await.as_array().unwrap().len(),
        2,
        "no duplicate"
    );

    // Unknown store id → 404; a non-store path → 400.
    let resp = app
        .clone()
        .oneshot(authed("GET", "/stores/deadbeefdeadbeef/tickets", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let resp = app
        .clone()
        .oneshot(authed(
            "POST",
            "/stores",
            Some(r#"{"path":"/nonexistent/nope"}"#),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}
