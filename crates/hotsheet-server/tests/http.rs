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

#[tokio::test]
async fn store_scoped_writes_are_isolated_to_their_store() {
    let (_d, st) = state();
    let app = app(st);

    // Register a second store, empty to start.
    let dir2 = tempfile::tempdir().unwrap();
    FsStore::init(dir2.path(), &StoreMetadata::new("BB")).unwrap();
    let body = format!(r#"{{"path":"{}"}}"#, dir2.path().display());
    let resp = app
        .clone()
        .oneshot(authed("POST", "/stores", Some(&body)))
        .await
        .unwrap();
    let id2 = body_json(resp).await["id"].as_str().unwrap().to_string();

    // Create a ticket in the SCOPED store.
    let resp = app
        .clone()
        .oneshot(authed(
            "POST",
            &format!("/stores/{id2}/tickets"),
            Some(r#"{"title":"scoped work","priority":"high"}"#),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let created = body_json(resp).await;
    let slug = created["slug"].as_str().unwrap().to_string();
    assert!(
        slug.starts_with("BB-"),
        "minted with the scoped store's prefix"
    );

    // It shows in the scoped list but NOT in the default store's list (isolation).
    let resp = app
        .clone()
        .oneshot(authed("GET", &format!("/stores/{id2}/tickets"), None))
        .await
        .unwrap();
    assert_eq!(body_json(resp).await.as_array().unwrap().len(), 1);
    let resp = app
        .clone()
        .oneshot(authed("GET", "/tickets", None))
        .await
        .unwrap();
    assert_eq!(
        body_json(resp).await.as_array().unwrap().len(),
        0,
        "default store untouched"
    );

    // PATCH via the scoped route.
    let resp = app
        .clone()
        .oneshot(authed(
            "PATCH",
            &format!("/stores/{id2}/tickets/{slug}"),
            Some(r#"{"status":"started"}"#),
        ))
        .await
        .unwrap();
    assert_eq!(body_json(resp).await["status"], "started");

    // Close via the scoped route.
    let resp = app
        .clone()
        .oneshot(authed(
            "POST",
            &format!("/stores/{id2}/tickets/{slug}/close"),
            Some(r#"{"reason":"completed"}"#),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(body_json(resp).await["close_reason"], "completed");

    // GET one scoped ticket.
    let resp = app
        .clone()
        .oneshot(authed(
            "GET",
            &format!("/stores/{id2}/tickets/{slug}"),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(body_json(resp).await["slug"], slug);

    // Writing to an unknown store id → 404.
    let resp = app
        .clone()
        .oneshot(authed(
            "POST",
            "/stores/deadbeefdeadbeef/tickets",
            Some(r#"{"title":"x"}"#),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_registered_store_gets_its_own_watcher() {
    use hotsheet_model::{Timestamp, Ulid};
    use hotsheet_ticketing::{NewTicket, ops};

    let (_d, st) = state();
    let app = app(st);

    // Register an empty second store.
    let dir2 = tempfile::tempdir().unwrap();
    let store2 = FsStore::init(dir2.path(), &StoreMetadata::new("BB")).unwrap();
    let body = format!(r#"{{"path":"{}"}}"#, dir2.path().display());
    let resp = app
        .clone()
        .oneshot(authed("POST", "/stores", Some(&body)))
        .await
        .unwrap();
    let id2 = body_json(resp).await["id"].as_str().unwrap().to_string();

    // Write a ticket to the registered store's disk WITHOUT the API (a CLI/git edit).
    ops::create(
        &store2,
        Ulid::new(),
        "BB",
        Timestamp::new("2026-08-19T00:00:00Z"),
        NewTicket {
            title: "external to store2".into(),
            category: "task".into(),
            ..Default::default()
        },
    )
    .unwrap();

    // The per-store watcher should reindex it; poll the scoped list until it appears.
    for _ in 0..40 {
        let resp = app
            .clone()
            .oneshot(authed(
                "GET",
                &format!("/stores/{id2}/tickets?text=external"),
                None,
            ))
            .await
            .unwrap();
        if body_json(resp).await.as_array().unwrap().len() == 1 {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    panic!("the registered store's watcher did not reindex the external write within 4s");
}

#[tokio::test]
async fn resolve_finds_a_ulid_in_whichever_hosted_store_holds_it() {
    let (_d, st) = state();
    let app = app(st);

    // A ticket in the default store.
    let resp = app
        .clone()
        .oneshot(authed(
            "POST",
            "/tickets",
            Some(r#"{"title":"in default"}"#),
        ))
        .await
        .unwrap();
    let default_id = body_json(resp).await["id"].as_str().unwrap().to_string();

    // A second store with its own ticket.
    let dir2 = tempfile::tempdir().unwrap();
    FsStore::init(dir2.path(), &StoreMetadata::new("BB")).unwrap();
    let body = format!(r#"{{"path":"{}"}}"#, dir2.path().display());
    let resp = app
        .clone()
        .oneshot(authed("POST", "/stores", Some(&body)))
        .await
        .unwrap();
    let store2 = body_json(resp).await["id"].as_str().unwrap().to_string();
    let resp = app
        .clone()
        .oneshot(authed(
            "POST",
            &format!("/stores/{store2}/tickets"),
            Some(r#"{"title":"in store2"}"#),
        ))
        .await
        .unwrap();
    let ulid2 = body_json(resp).await["id"].as_str().unwrap().to_string();

    // Resolving each ULID reports the correct hosting store + the ticket.
    let resp = app
        .clone()
        .oneshot(authed("GET", &format!("/resolve/{default_id}"), None))
        .await
        .unwrap();
    let got = body_json(resp).await;
    assert_eq!(got["title"], "in default");
    assert!(got["store"].is_string());

    let resp = app
        .clone()
        .oneshot(authed("GET", &format!("/resolve/{ulid2}"), None))
        .await
        .unwrap();
    let got = body_json(resp).await;
    assert_eq!(got["title"], "in store2");
    assert_eq!(got["store"], store2, "resolved to the store that hosts it");

    // Unknown ULID → 404; a non-ULID → 400.
    let resp = app
        .clone()
        .oneshot(authed("GET", "/resolve/01ARZ3NDEKTSV4RRFFQ69G5FAV", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let resp = app
        .clone()
        .oneshot(authed("GET", "/resolve/not-a-ulid", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn permission_round_trip_lists_answers_and_unblocks_the_tool() {
    let (_d, st) = state();
    let bridge = st.permission_bridge();
    let app = app(st);

    // A driven "tool" thread blocks on an unruled request (real blocking round-trip).
    let bt = bridge.clone();
    let waiter = std::thread::spawn(move || bt.request_blocking("conn-1", "Bash", "rm -rf build"));

    // It shows up on GET /permissions once queued.
    let id = loop {
        let resp = app
            .clone()
            .oneshot(authed("GET", "/permissions", None))
            .await
            .unwrap();
        let list = body_json(resp).await;
        if let Some(first) = list.as_array().unwrap().first() {
            assert_eq!(first["connection"], "conn-1");
            assert_eq!(first["tool"], "Bash");
            assert_eq!(first["action"], "rm -rf build");
            break first["id"].as_u64().unwrap();
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    };

    // Answer it — allow, once (no rule persisted).
    let resp = app
        .clone()
        .oneshot(authed(
            "POST",
            &format!("/permissions/{id}"),
            Some(r#"{"decision":"allow","scope":"once"}"#),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let ack = body_json(resp).await;
    assert_eq!(ack["connection"], "conn-1");
    assert_eq!(ack["decision"], "allow");
    assert_eq!(ack["persisted"], false);

    // The blocked tool received the human's answer.
    assert_eq!(format!("{:?}", waiter.join().unwrap()), "Allow");

    // The queue is now empty, and answering an unknown id is a 404.
    let resp = app
        .clone()
        .oneshot(authed(
            "POST",
            "/permissions/999",
            Some(r#"{"decision":"deny"}"#),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn permissions_require_the_secret() {
    let (_d, st) = state();
    let resp = app(st)
        .oneshot(
            Request::builder()
                .uri("/permissions")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn an_always_answer_persists_a_rule_that_auto_resolves_next_time() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    let rules = dir.path().join("permissions.json");
    let st = AppState::new(store, SECRET.into())
        .unwrap()
        .with_permission_rules(&rules);
    let bridge = st.permission_bridge();
    let app = app(st);

    // Block on a request, then answer it with scope=always.
    let bt = bridge.clone();
    let waiter = std::thread::spawn(move || bt.request_blocking("c", "Bash", "ls"));
    let id = loop {
        let resp = app
            .clone()
            .oneshot(authed("GET", "/permissions", None))
            .await
            .unwrap();
        if let Some(first) = body_json(resp).await.as_array().unwrap().first() {
            break first["id"].as_u64().unwrap();
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    };
    let resp = app
        .clone()
        .oneshot(authed(
            "POST",
            &format!("/permissions/{id}"),
            Some(r#"{"decision":"allow","scope":"always"}"#),
        ))
        .await
        .unwrap();
    let ack = body_json(resp).await;
    assert_eq!(ack["persisted"], true, "an always rule was written to disk");
    assert_eq!(format!("{:?}", waiter.join().unwrap()), "Allow");
    assert!(rules.exists(), "the rule file was created");

    // The same (tool, action) now auto-resolves — no human, nothing pending.
    let bt = bridge.clone();
    let auto = std::thread::spawn(move || bt.request_blocking("c", "Bash", "ls"));
    assert_eq!(format!("{:?}", auto.join().unwrap()), "Allow");
    let resp = app
        .clone()
        .oneshot(authed("GET", "/permissions", None))
        .await
        .unwrap();
    assert!(
        body_json(resp).await.as_array().unwrap().is_empty(),
        "auto-resolved: nothing queued"
    );
}

#[tokio::test]
async fn long_poll_hands_back_a_cursor_then_replays_events_since_it() {
    let (_d, st) = state();
    let app = app(st);

    // Handshake: no `since` → the current cursor (0) + no backlog.
    let resp = app
        .clone()
        .oneshot(authed("GET", "/ws/poll?secret=test-secret", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let hs = body_json(resp).await;
    assert_eq!(hs["cursor"], 0);
    assert_eq!(hs["events"].as_array().unwrap().len(), 0);
    assert_eq!(hs["overflow"], false);

    // Two writes emit two change events.
    for t in [r#"{"title":"one"}"#, r#"{"title":"two"}"#] {
        app.clone()
            .oneshot(authed("POST", "/tickets", Some(t)))
            .await
            .unwrap();
    }

    // Poll since the handshake cursor → both events, cursor advanced to 2.
    let resp = app
        .clone()
        .oneshot(authed("GET", "/ws/poll?secret=test-secret&since=0", None))
        .await
        .unwrap();
    let got = body_json(resp).await;
    assert_eq!(got["cursor"], 2);
    let evs = got["events"].as_array().unwrap();
    assert_eq!(evs.len(), 2);
    assert_eq!(evs[0]["kind"], "created");
    assert!(evs[0]["slug"].as_str().unwrap().starts_with("HS-"));

    // Caught up: a poll at the current cursor with a tiny timeout returns empty, same cursor.
    let resp = app
        .clone()
        .oneshot(authed(
            "GET",
            "/ws/poll?secret=test-secret&since=2&timeout_ms=1",
            None,
        ))
        .await
        .unwrap();
    let caught_up = body_json(resp).await;
    assert_eq!(caught_up["cursor"], 2);
    assert_eq!(caught_up["events"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn long_poll_requires_the_secret() {
    let (_d, st) = state();
    let resp = app(st)
        .oneshot(authed("GET", "/ws/poll?secret=wrong&since=0", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn long_poll_blocks_then_wakes_on_the_next_event() {
    let (_d, st) = state();
    let app = app(st);

    // Start a poll that's caught up (since=0, cursor=0) with a generous timeout — it blocks.
    let poll_app = app.clone();
    let poll = tokio::spawn(async move {
        poll_app
            .oneshot(authed(
                "GET",
                "/ws/poll?secret=test-secret&since=0&timeout_ms=5000",
                None,
            ))
            .await
            .unwrap()
    });

    // Give the poll a moment to subscribe, then write — which should wake it.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    app.clone()
        .oneshot(authed("POST", "/tickets", Some(r#"{"title":"live"}"#)))
        .await
        .unwrap();

    let resp = poll.await.unwrap();
    let got = body_json(resp).await;
    assert_eq!(got["cursor"], 1, "the write advanced the cursor");
    let evs = got["events"].as_array().unwrap();
    assert_eq!(evs.len(), 1, "the blocked poll woke on the new event");
    assert_eq!(evs[0]["kind"], "created");
}

#[tokio::test]
async fn cross_store_copy_and_move_between_hosted_stores() {
    let (_d, st) = state();
    let app = app(st);

    // A ticket in the default store, and a second hosted store to copy/move into.
    let resp = app
        .clone()
        .oneshot(authed("POST", "/tickets", Some(r#"{"title":"portable"}"#)))
        .await
        .unwrap();
    let id = body_json(resp).await["id"].as_str().unwrap().to_string();

    let dir2 = tempfile::tempdir().unwrap();
    FsStore::init(dir2.path(), &StoreMetadata::new("DS")).unwrap();
    let body = format!(r#"{{"path":"{}"}}"#, dir2.path().display());
    let resp = app
        .clone()
        .oneshot(authed("POST", "/stores", Some(&body)))
        .await
        .unwrap();
    let store2 = body_json(resp).await["id"].as_str().unwrap().to_string();

    // copy → 201, NEW ULID in DS with copied_from provenance; source still resolves.
    let resp = app
        .clone()
        .oneshot(authed(
            "POST",
            &format!("/tickets/{id}/copy"),
            Some(&format!(r#"{{"to":"{store2}"}}"#)),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let copied = body_json(resp).await;
    assert_ne!(copied["id"].as_str().unwrap(), id, "copy mints a new ULID");
    assert!(copied["slug"].as_str().unwrap().starts_with("DS-"));
    assert_eq!(copied["copied_from"], id);
    assert_eq!(copied["store"], store2);

    // move without confirm → 400 naming the caveat.
    let resp = app
        .clone()
        .oneshot(authed(
            "POST",
            &format!("/tickets/{id}/move"),
            Some(&format!(r#"{{"to":"{store2}"}}"#)),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // move with confirm → same ULID now hosted by store2 (resolve follows the tombstone).
    let resp = app
        .clone()
        .oneshot(authed(
            "POST",
            &format!("/tickets/{id}/move"),
            Some(&format!(r#"{{"to":"{store2}","confirm":true}}"#)),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let moved = body_json(resp).await;
    assert_eq!(moved["id"], id, "move keeps the ULID");
    assert_eq!(moved["store"], store2);
    assert!(moved["tombstone"].as_str().unwrap().starts_with("HS-"));

    // The global resolve now lands in store2 (the live instance, past the tombstone).
    let resp = app
        .clone()
        .oneshot(authed("GET", &format!("/resolve/{id}"), None))
        .await
        .unwrap();
    let got = body_json(resp).await;
    assert_eq!(got["store"], store2, "resolves to the destination store");
    assert_ne!(
        got["status"], "moved",
        "the live instance, not the tombstone"
    );
}

#[tokio::test]
async fn persistent_mode_writes_a_file_backed_index_for_registered_stores() {
    // Hermetic: point HOTSHEET_HOME at a tempdir (nextest isolates each test process).
    let home = tempfile::tempdir().unwrap();
    unsafe {
        std::env::set_var("HOTSHEET_HOME", home.path());
    }

    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    let st = AppState::new(store, SECRET.into())
        .unwrap()
        .with_persistent_registered_indexes();
    let app = app(st);

    let dir2 = tempfile::tempdir().unwrap();
    FsStore::init(dir2.path(), &StoreMetadata::new("BB")).unwrap();
    let body = format!(r#"{{"path":"{}"}}"#, dir2.path().display());
    let resp = app
        .clone()
        .oneshot(authed("POST", "/stores", Some(&body)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let id2 = body_json(resp).await["id"].as_str().unwrap().to_string();

    // The registered store's index was written under ${HOTSHEET_HOME}/index/<id>.sqlite.
    let index_file = home.path().join("index").join(format!("{id2}.sqlite"));
    assert!(
        index_file.is_file(),
        "file-backed index persisted: {}",
        index_file.display()
    );

    unsafe {
        std::env::remove_var("HOTSHEET_HOME");
    }
}

#[tokio::test]
async fn startup_discovery_hosts_stores_from_stores_json() {
    let home = tempfile::tempdir().unwrap();
    unsafe {
        std::env::set_var("HOTSHEET_HOME", home.path());
    }

    // Two stores on disk; one listed in stores.json, one bogus path (must be skipped).
    let good = tempfile::tempdir().unwrap();
    FsStore::init(good.path(), &StoreMetadata::new("BB")).unwrap();
    let stores_json = serde_json::json!({
        "stores": [good.path().to_string_lossy(), "/nonexistent/nope"]
    });
    std::fs::write(home.path().join("stores.json"), stores_json.to_string()).unwrap();

    // Primary store + discovery.
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    let st = AppState::new(store, SECRET.into()).unwrap();
    let hosted = st.host_configured_stores();
    assert_eq!(hosted, 1, "one good store hosted, the bogus path skipped");

    // /stores now lists the primary + the discovered one.
    let app = app(st);
    let resp = app
        .clone()
        .oneshot(authed("GET", "/stores", None))
        .await
        .unwrap();
    let stores = body_json(resp).await;
    let arr = stores.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert!(
        arr.iter().any(|s| s["prefix"] == "BB"),
        "the discovered store is served"
    );

    unsafe {
        std::env::remove_var("HOTSHEET_HOME");
    }
}

#[tokio::test]
async fn one_machine_server_is_discoverable_for_every_hosted_store() {
    use hotsheet_server::lifecycle;

    let home = tempfile::tempdir().unwrap();
    unsafe {
        std::env::set_var("HOTSHEET_HOME", home.path());
    }

    // Primary store + machine server.
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    let st = AppState::new(store, SECRET.into()).unwrap();

    // Publish the machine server's coordinates → the primary gets a discovery file.
    st.publish_instances(
        "http://127.0.0.1:9999".into(),
        "2026-08-22T00:00:00Z".into(),
    );
    let app = app(st);

    // Register a second store at runtime; it advertises the same machine server.
    let dir2 = tempfile::tempdir().unwrap();
    FsStore::init(dir2.path(), &StoreMetadata::new("BB")).unwrap();
    let body = format!(r#"{{"path":"{}"}}"#, dir2.path().display());
    let resp = app
        .clone()
        .oneshot(authed("POST", "/stores", Some(&body)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // "Who serves project X?" resolves to the ONE machine server for BOTH projects.
    let primary = lifecycle::find_instance(dir.path()).expect("primary discoverable");
    assert_eq!(primary.url, "http://127.0.0.1:9999");
    let second = lifecycle::find_instance(dir2.path()).expect("registered store discoverable");
    assert_eq!(second.url, "http://127.0.0.1:9999");
    assert_eq!(
        second.pid, primary.pid,
        "same machine server process hosts both"
    );

    unsafe {
        std::env::remove_var("HOTSHEET_HOME");
    }
}

#[tokio::test]
async fn background_sync_covers_every_hosted_store() {
    use hotsheet_server::sync_loop::sync_all;
    use hotsheet_ticketing::sync::SyncReport;

    let (_d, st) = state();
    let app = app(st.clone());

    // Register a second (local-only, no-remote) store.
    let dir2 = tempfile::tempdir().unwrap();
    FsStore::init(dir2.path(), &StoreMetadata::new("BB")).unwrap();
    let body = format!(r#"{{"path":"{}"}}"#, dir2.path().display());
    app.clone()
        .oneshot(authed("POST", "/stores", Some(&body)))
        .await
        .unwrap();

    // One sync pass reports on BOTH hosted stores; a local-only store is NoRemote.
    let reports = sync_all(&st);
    assert_eq!(reports.len(), 2, "both hosted stores synced");
    assert!(reports.iter().all(|(_, r)| *r == SyncReport::NoRemote));
}

#[tokio::test]
async fn claim_next_release_renew_over_http() {
    let (_d, st) = state();
    let app = app(st);

    // A claimable Up Next ticket.
    let resp = app
        .clone()
        .oneshot(authed(
            "POST",
            "/tickets",
            Some(r#"{"title":"drain","up_next":true}"#),
        ))
        .await
        .unwrap();
    let slug = body_json(resp).await["slug"].as_str().unwrap().to_string();

    // Claim it.
    let resp = app
        .clone()
        .oneshot(authed(
            "POST",
            "/claim-next",
            Some(r#"{"worker":"w1","lease_minutes":15}"#),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let claimed = body_json(resp).await;
    assert_eq!(claimed["slug"], slug);
    assert_eq!(claimed["claimed_by"], "w1");

    // Renew (holder).
    let resp = app
        .clone()
        .oneshot(authed(
            "POST",
            &format!("/tickets/{slug}/renew"),
            Some(r#"{"worker":"w1"}"#),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // A non-holder release without force → 4xx (wrong worker).
    let resp = app
        .clone()
        .oneshot(authed(
            "POST",
            &format!("/tickets/{slug}/release"),
            Some(r#"{"worker":"w2"}"#),
        ))
        .await
        .unwrap();
    assert!(resp.status().is_client_error());

    // Holder releases → claimed_by cleared.
    let resp = app
        .clone()
        .oneshot(authed(
            "POST",
            &format!("/tickets/{slug}/release"),
            Some(r#"{"worker":"w1"}"#),
        ))
        .await
        .unwrap();
    assert!(body_json(resp).await["claimed_by"].is_null());

    // Nothing claimable now (it's claimable again but let's assert null when queue drained):
    // claim it again then complete, then claim-next returns null.
    app.clone()
        .oneshot(authed("POST", "/claim-next", Some(r#"{"worker":"w1"}"#)))
        .await
        .unwrap();
    app.clone()
        .oneshot(authed(
            "PATCH",
            &format!("/tickets/{slug}"),
            Some(r#"{"status":"completed"}"#),
        ))
        .await
        .unwrap();
    let resp = app
        .clone()
        .oneshot(authed("POST", "/claim-next", Some(r#"{}"#)))
        .await
        .unwrap();
    assert!(
        body_json(resp).await.is_null(),
        "no claimable tickets → null"
    );
}
