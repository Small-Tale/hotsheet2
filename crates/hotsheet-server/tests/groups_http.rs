//! Ticket groups over the real router (HS2-PDSHNN, HS2-44W3TP, HS2-8CDFKR): the `group`
//! field on create, update, and batch; the `group` / `has_group` list filters; the
//! `/groups` routes for the default store, a hosted store, a provider connection, and a
//! checkout (one namespace across its Git sources); and the explicit refusal from a
//! provider without the `groups` capability. Synthetic tickets only.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use hotsheet_extsync::{GitHubConfig, GitHubProvider, GitHubTransport, HttpResponse};
use hotsheet_server::{AppState, app};
use hotsheet_ticketing::checkouts::CheckoutRegistry;
use hotsheet_ticketing::{FsStore, StoreMetadata};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

const SECRET: &str = "synthetic-secret";

fn authed(method: &str, uri: &str, body: Option<Value>) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("x-hotsheet-secret", SECRET)
        .header("content-type", "application/json")
        .body(body.map_or_else(Body::empty, |value| Body::from(value.to_string())))
        .unwrap()
}

async fn send(
    router: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let response = router
        .clone()
        .oneshot(authed(method, uri, body))
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn ok(router: &axum::Router, method: &str, uri: &str, body: Option<Value>) -> Value {
    let (status, value) = send(router, method, uri, body).await;
    assert!(status.is_success(), "{method} {uri}: {status} {value}");
    value
}

/// Checkout responses wrap the ticket as `{store, ticket}`; store routes return it bare.
fn ticket(value: &Value) -> &Value {
    value.get("ticket").unwrap_or(value)
}

fn slugs(rows: &Value) -> Vec<String> {
    let rows = rows
        .as_array()
        .or_else(|| rows["items"].as_array())
        .unwrap_or_else(|| panic!("rows: {rows}"));
    let mut slugs: Vec<String> = rows
        .iter()
        .map(|row| ticket(row)["slug"].as_str().unwrap().to_owned())
        .collect();
    slugs.sort();
    slugs
}

fn sorted(mut values: Vec<String>) -> Vec<String> {
    values.sort();
    values
}

fn default_store() -> (tempfile::TempDir, axum::Router) {
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init_git(dir.path().join("store"), &StoreMetadata::new("HS")).unwrap();
    let router = app(AppState::new(store, SECRET.into())
        .unwrap()
        .with_default_codex_home(dir.path().join("no-codex-home"))
        .with_checkout_registry(dir.path().join("checkouts.json")));
    (dir, router)
}

async fn create(router: &axum::Router, uri: &str, title: &str, group: Option<&str>) -> Value {
    let mut body = json!({ "title": title });
    if let Some(group) = group {
        body["group"] = json!(group);
    }
    let (status, value) = send(router, "POST", uri, Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "{value}");
    value
}

#[tokio::test]
async fn hs2_pdshnn_default_store_group_field_filters_and_batch() {
    let (dir, router) = default_store();
    let first = create(&router, "/tickets", "first", Some("  Board   Drag & Drop ")).await;
    assert_eq!(first["group"], "Board Drag & Drop");
    let second = create(&router, "/tickets", "second", None).await;
    assert_eq!(
        second["group"],
        Value::Null,
        "detail serializes null like blocked_reason"
    );
    assert!(second.as_object().unwrap().contains_key("group"));
    let third = create(&router, "/tickets", "third", Some("alpha")).await;
    let slug = |value: &Value| value["slug"].as_str().unwrap().to_owned();
    let patch = |id: String, body: Value| {
        let router = router.clone();
        async move { send(&router, "PATCH", &format!("/tickets/{id}"), Some(body)).await }
    };

    // Set reuses the stored spelling; the detail and the list row both carry the group.
    let (status, joined) = patch(slug(&second), json!({ "group": "BOARD drag &  drop" })).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(joined["group"], "Board Drag & Drop");
    let detail = ok(&router, "GET", &format!("/tickets/{}", slug(&second)), None).await;
    assert_eq!(detail["group"], "Board Drag & Drop");
    assert_eq!(
        detail["updated_at"], second["updated_at"],
        "regrouping does not look like a content edit"
    );
    let rows = ok(&router, "GET", "/tickets", None).await;
    let row = |wanted: &Value| {
        rows.as_array()
            .unwrap()
            .iter()
            .find(|row| row["slug"] == wanted["slug"])
            .unwrap()
            .clone()
    };
    assert_eq!(row(&second)["group"], "Board Drag & Drop");
    assert_eq!(row(&third)["group"], "alpha");

    // Absent leaves it, null clears it, and a cleared row omits the field.
    let (_, retitled) = patch(slug(&second), json!({ "title": "second again" })).await;
    assert_eq!(retitled["group"], "Board Drag & Drop");
    let (status, cleared) = patch(slug(&second), json!({ "group": null })).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cleared["group"], Value::Null);
    let rows = ok(&router, "GET", "/tickets", None).await;
    let cleared_row = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["slug"] == second["slug"])
        .unwrap();
    assert!(!cleared_row.as_object().unwrap().contains_key("group"));
    let (_, emptied) = patch(slug(&third), json!({ "group": "   " })).await;
    assert_eq!(
        emptied["group"],
        Value::Null,
        "an empty name clears the group"
    );
    patch(slug(&third), json!({ "group": "alpha" })).await;

    // Invalid names are a 400 with a stable code and write nothing.
    for bad in [
        json!("line\nbreak"),
        json!("tab\there"),
        json!("x".repeat(121)),
    ] {
        let (status, error) = patch(slug(&first), json!({ "group": bad })).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
        assert_eq!(error["code"], "invalid_group_name");
        assert!(error["error"].as_str().unwrap().contains("group name"));
    }
    let (status, error) = send(
        &router,
        "POST",
        "/tickets",
        Some(json!({ "title": "bad", "group": "a\nb" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
    let (status, _) = patch(slug(&first), json!({ "group": 7 })).await;
    assert!(status.is_client_error(), "a non-string group is refused");
    let detail = ok(&router, "GET", &format!("/tickets/{}", slug(&first)), None).await;
    assert_eq!(detail["group"], "Board Drag & Drop");

    // List filters: exact group by normalized name, and has_group both ways.
    let list = |query: &'static str| {
        let router = router.clone();
        async move { slugs(&ok(&router, "GET", &format!("/tickets?{query}"), None).await) }
    };
    assert_eq!(
        list("group=board%20%20DRAG%20%26%20drop").await,
        vec![slug(&first)]
    );
    assert!(list("group=board").await.is_empty());
    assert!(list("group=%20").await.is_empty());
    assert_eq!(
        list("has_group=true").await,
        sorted(vec![slug(&first), slug(&third)])
    );
    assert_eq!(list("has_group=false").await, vec![slug(&second)]);
    assert_eq!(list("has_group=true&group=ALPHA").await, vec![slug(&third)]);

    // The batch route sets and clears the group, choosing one spelling for the selection.
    let batch = ok(
        &router,
        "POST",
        "/batch",
        Some(json!({ "ids": [slug(&first), slug(&second)], "group": "board drag & drop" })),
    )
    .await;
    assert_eq!(batch["updated"].as_array().unwrap().len(), 2, "{batch}");
    assert!(batch["errors"].as_array().unwrap().is_empty());
    let groups = ok(&router, "GET", "/groups", None).await;
    assert_eq!(
        groups,
        json!({ "groups": [
            { "name": "alpha", "count": 1 },
            { "name": "board drag & drop", "count": 2 },
        ] }),
        "the selection held every member, so its new spelling wins"
    );
    let (status, error) = send(
        &router,
        "POST",
        "/batch",
        Some(json!({ "ids": [slug(&first)], "group": "a\nb" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
    let cleared = ok(
        &router,
        "POST",
        "/batch",
        Some(json!({ "ids": [slug(&first), slug(&second), "HS-NOPE"], "group": null })),
    )
    .await;
    assert_eq!(cleared["updated"].as_array().unwrap().len(), 2);
    assert_eq!(cleared["errors"].as_array().unwrap().len(), 1);
    assert_eq!(
        ok(&router, "GET", "/groups", None).await,
        json!({ "groups": [{ "name": "alpha", "count": 1 }] })
    );

    // The file is the source of truth: the field is in the frontmatter.
    let store = FsStore::open(dir.path().join("store")).unwrap();
    let on_disk = store
        .list_tickets()
        .unwrap()
        .into_iter()
        .find(|ticket| ticket.slug == slug(&third))
        .unwrap();
    assert_eq!(on_disk.group.as_deref(), Some("alpha"));
    assert!(
        std::fs::read_to_string(store.ticket_path(&on_disk.id))
            .unwrap()
            .contains("\ngroup: alpha\n")
    );
    // Capabilities advertise the feature.
    let providers = ok(&router, "GET", "/providers", None).await;
    assert_eq!(providers[0]["capabilities"]["groups"], true, "{providers}");
}

#[tokio::test]
async fn hs2_8cdfkr_default_store_groups_list_rename_and_ungroup() {
    let (_dir, router) = default_store();
    assert_eq!(
        ok(&router, "GET", "/groups", None).await,
        json!({ "groups": [] })
    );
    let a = create(&router, "/tickets", "a", Some("Old")).await;
    let b = create(&router, "/tickets", "b", Some("old")).await;
    let other = create(&router, "/tickets", "other", Some("Taken")).await;
    let trashed = create(&router, "/tickets", "trashed", Some("Old")).await;
    let id = |value: &Value| value["slug"].as_str().unwrap().to_owned();
    ok(
        &router,
        "PATCH",
        &format!("/tickets/{}", id(&trashed)),
        Some(json!({ "status": "deleted" })),
    )
    .await;
    assert_eq!(
        ok(&router, "GET", "/groups", None).await,
        json!({ "groups": [{ "name": "Old", "count": 2 }, { "name": "Taken", "count": 1 }] }),
        "sorted by name without regard to case; a trashed ticket is not counted"
    );

    let rename = |from: &'static str, to: &'static str| {
        let router = router.clone();
        async move {
            send(
                &router,
                "POST",
                "/groups/rename",
                Some(
                    json!({ "from": from, "to": to, "actor": { "role": "human", "id": "tester" } }),
                ),
            )
            .await
        }
    };
    let (status, error) = rename("missing", "New").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        error,
        json!({ "error": "no group named 'missing'", "code": "group_not_found" })
    );
    let (status, error) = rename("Old", " TAKEN ").await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        error,
        json!({ "error": "a group named 'Taken' already exists", "code": "group_name_taken" })
    );
    for bad in ["   ", "a\nb"] {
        let (status, error) = send(
            &router,
            "POST",
            "/groups/rename",
            Some(json!({ "from": "Old", "to": bad })),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
        assert_eq!(error["code"], "invalid_group_name");
    }
    let (status, error) = send(
        &router,
        "POST",
        "/groups/rename",
        Some(json!({ "from": "Old", "to": "New", "actor": { "role": "robot" } })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
    let (status, _) = send(
        &router,
        "POST",
        "/groups/rename",
        Some(json!({ "from": "Old" })),
    )
    .await;
    assert!(status.is_client_error());
    assert_eq!(
        ok(&router, "GET", &format!("/tickets/{}", id(&a)), None).await["group"],
        "Old",
        "a refused rename writes nothing"
    );

    // A change of only case or spacing of the same group is allowed.
    assert_eq!(
        rename("old", "  OLD ").await,
        (StatusCode::OK, json!({ "name": "OLD", "updated": 2 }))
    );
    assert_eq!(
        rename("OLD", "New   name").await,
        (StatusCode::OK, json!({ "name": "New name", "updated": 2 }))
    );
    // The index follows the write: rows, filters, and the listing agree at once.
    for ticket in [&a, &b, &trashed] {
        assert_eq!(
            ok(&router, "GET", &format!("/tickets/{}", id(ticket)), None).await["group"],
            "New name"
        );
    }
    assert_eq!(
        slugs(
            &ok(
                &router,
                "GET",
                "/tickets?group=new%20NAME&collection=all",
                None
            )
            .await
        ),
        sorted(vec![id(&a), id(&b)])
    );
    assert_eq!(
        ok(&router, "GET", &format!("/tickets/{}", id(&other)), None).await["group"],
        "Taken"
    );
    assert_eq!(rename("Old", "Else").await.0, StatusCode::NOT_FOUND);

    let ungroup = |name: &'static str| {
        let router = router.clone();
        async move {
            send(
                &router,
                "POST",
                "/groups/ungroup",
                Some(json!({ "name": name })),
            )
            .await
        }
    };
    let (status, error) = ungroup("missing").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error["code"], "group_not_found");
    assert_eq!(
        ungroup(" NEW  name ").await,
        (StatusCode::OK, json!({ "updated": 2 }))
    );
    assert_eq!(ungroup("new name").await.0, StatusCode::NOT_FOUND);
    assert_eq!(
        ok(&router, "GET", "/groups", None).await,
        json!({ "groups": [{ "name": "Taken", "count": 1 }] })
    );
    assert_eq!(
        slugs(
            &ok(
                &router,
                "GET",
                "/tickets?has_group=false&collection=all",
                None
            )
            .await
        ),
        sorted(vec![id(&a), id(&b)])
    );
    // The group routes need the secret like every other route.
    let unauthenticated = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/groups")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);
}

/// A GitHub issue the routes can resolve; no group write may reach it.
#[derive(Default)]
struct OneIssue {
    writes: Mutex<usize>,
}

impl GitHubTransport for OneIssue {
    fn request(
        &self,
        method: &str,
        url: &str,
        _: &[(&str, String)],
        _: Option<&Value>,
    ) -> Result<HttpResponse, String> {
        let path = url.split('?').next().unwrap_or(url);
        let reply = |status: u16, body: Value| HttpResponse {
            status,
            headers: HashMap::new(),
            body: body.to_string(),
        };
        if method != "GET" {
            *self.writes.lock().unwrap() += 1;
            return Ok(reply(500, json!({ "message": "no write expected" })));
        }
        let issue = json!({
            "number": 42, "title": "Synthetic issue", "body": "body",
            "state": "open", "state_reason": null,
            "html_url": "https://github.test/acme/repo/issues/42",
            "created_at": "2026-11-03T00:00:00Z", "updated_at": "2026-11-03T00:01:00Z",
            "closed_at": null, "labels": [], "assignees": [], "pull_request": null
        });
        if path.ends_with("/repos/acme/repo/issues/42") {
            return Ok(reply(200, issue));
        }
        if path.ends_with("/repos/acme/repo/issues") {
            return Ok(reply(200, json!([issue])));
        }
        if path.ends_with("/comments") || path.ends_with("/events") || path.ends_with("/timeline") {
            return Ok(reply(200, json!([])));
        }
        Ok(reply(404, json!({ "message": "Not Found" })))
    }
}

struct Project {
    _dir: tempfile::TempDir,
    router: axum::Router,
    checkout: String,
    /// Connection ids of the two Git sources, in link order.
    git: Vec<String>,
    transport: Arc<OneIssue>,
}

/// A checkout linking two Git stores and one GitHub connection.
fn project() -> Project {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let store_roots: Vec<_> = ["AA", "BB"]
        .into_iter()
        .map(|prefix| {
            let store_root = dir.path().join(format!("{prefix}.hs2"));
            FsStore::init_git(&store_root, &StoreMetadata::new(prefix)).unwrap();
            store_root
        })
        .collect();
    let registry = dir.path().join("checkouts.json");
    let registered = CheckoutRegistry::new(&registry)
        .register(&root, Some("project"), None, store_roots.clone())
        .unwrap();
    let git: Vec<String> = registered
        .sources
        .iter()
        .map(|source| source.connection_id.clone())
        .collect();
    assert_eq!(git.len(), 2);
    let mut saved: Value = serde_json::from_slice(&std::fs::read(&registry).unwrap()).unwrap();
    let entry = saved["checkouts"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entry| entry["id"] == registered.id)
        .unwrap();
    entry["sources"].as_array_mut().unwrap().push(
        json!({ "connection_id": "github-main", "provider": "github", "locator": "acme/repo" }),
    );
    std::fs::write(&registry, serde_json::to_vec(&saved).unwrap()).unwrap();
    let transport = Arc::new(OneIssue::default());
    let provider = GitHubProvider::new(
        GitHubConfig::new("github-main", "acme/repo", "fixture-token"),
        transport.clone(),
    );
    let router = app(
        AppState::new(FsStore::open(&store_roots[0]).unwrap(), SECRET.into())
            .unwrap()
            .with_default_codex_home(dir.path().join("no-codex-home"))
            .with_checkout_registry(&registry)
            .with_ticket_provider(Arc::new(provider)),
    );
    Project {
        _dir: dir,
        router,
        checkout: registered.id,
        git,
        transport,
    }
}

#[tokio::test]
async fn hs2_pdshnn_a_checkout_is_one_group_namespace_across_its_git_sources() {
    let project = project();
    let router = &project.router;
    let base = format!("/checkouts/{}", project.checkout);
    let create_in = |source: usize, title: &'static str, group: Option<&'static str>| {
        let uri = format!("{base}/tickets?source={}", project.git[source]);
        let router = router.clone();
        async move { create(&router, &uri, title, group).await }
    };
    let a = create_in(0, "first store", Some("Sprint")).await;
    assert_eq!(ticket(&a)["group"], "Sprint", "{a}");
    // The second store reuses the spelling the first store already carries.
    let b = create_in(1, "second store", Some("  SPRINT ")).await;
    assert_eq!(ticket(&b)["group"], "Sprint", "{b}");
    let c = create_in(1, "loose", None).await;
    let d = create_in(0, "other", Some("Backlog cleanup")).await;
    let qualified = |value: &Value| ticket(value)["qualified_id"].as_str().unwrap().to_owned();

    let patched = ok(
        router,
        "PATCH",
        &format!("{base}/tickets/{}", qualified(&c)),
        Some(json!({ "group": "sprint" })),
    )
    .await;
    assert_eq!(ticket(&patched)["group"], "Sprint", "{patched}");
    let detail = ok(
        router,
        "GET",
        &format!("{base}/tickets/{}", qualified(&c)),
        None,
    )
    .await;
    assert_eq!(ticket(&detail)["group"], "Sprint");

    assert_eq!(
        ok(router, "GET", &format!("{base}/groups"), None).await,
        json!({ "groups": [
            { "name": "Backlog cleanup", "count": 1 },
            { "name": "Sprint", "count": 3 },
        ] }),
        "counts merge by name across the Git sources"
    );
    // Checkout rows carry the group and the list filters span every source.
    let rows = ok(router, "GET", &format!("{base}/tickets?group=sprint"), None).await;
    assert_eq!(slugs(&rows).len(), 3, "{rows}");
    let rows_array = rows
        .as_array()
        .or_else(|| rows["items"].as_array())
        .unwrap();
    assert!(
        rows_array
            .iter()
            .all(|row| ticket(row)["group"] == "Sprint")
    );
    // The GitHub issue is ungrouped, so it appears only under has_group=false.
    let ungrouped = ok(
        router,
        "GET",
        &format!("{base}/tickets?has_group=false"),
        None,
    )
    .await;
    assert_eq!(
        slugs(&ungrouped),
        vec!["acme/repo#42".to_string()],
        "{ungrouped}"
    );
    let grouped = ok(
        router,
        "GET",
        &format!("{base}/tickets?has_group=true"),
        None,
    )
    .await;
    assert_eq!(slugs(&grouped).len(), 4, "{grouped}");

    // The checkout batch sets a group across stores with one spelling, and clears it.
    let batch = ok(
        router,
        "POST",
        &format!("{base}/batch"),
        Some(json!({ "updates": [
            { "id": qualified(&a), "group": "SPRINT" },
            { "id": qualified(&b), "group": "SPRINT" },
            { "id": qualified(&c), "group": "SPRINT" },
        ] })),
    )
    .await;
    assert!(
        batch
            .as_array()
            .unwrap()
            .iter()
            .all(|row| ticket(row)["group"] == "SPRINT"),
        "the selection held every member, so its spelling wins: {batch}"
    );
    let (status, error) = send(
        router,
        "POST",
        &format!("{base}/batch"),
        Some(json!({ "updates": [
            { "id": qualified(&a), "title": "must not apply" },
            { "id": qualified(&b), "group": "a\nb" },
        ] })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
    let untouched = ok(
        router,
        "GET",
        &format!("{base}/tickets/{}", qualified(&a)),
        None,
    )
    .await;
    assert_eq!(
        ticket(&untouched)["title"],
        "first store",
        "the batch stays all-or-nothing"
    );
    ok(
        router,
        "POST",
        &format!("{base}/batch"),
        Some(json!({ "updates": [{ "id": qualified(&c), "group": null }] })),
    )
    .await;

    // Rename and ungroup rewrite the members in every Git source.
    let (status, error) = send(
        router,
        "POST",
        &format!("{base}/groups/rename"),
        Some(json!({ "from": "sprint", "to": "backlog CLEANUP" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{error}");
    assert_eq!(error["code"], "group_name_taken");
    let (status, error) = send(
        router,
        "POST",
        &format!("{base}/groups/rename"),
        Some(json!({ "from": "nope", "to": "x" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{error}");
    assert_eq!(
        ok(
            router,
            "POST",
            &format!("{base}/groups/rename"),
            Some(json!({ "from": "sprint", "to": "Sprint 2" })),
        )
        .await,
        json!({ "name": "Sprint 2", "updated": 2 })
    );
    for value in [&a, &b] {
        let detail = ok(
            router,
            "GET",
            &format!("{base}/tickets/{}", qualified(value)),
            None,
        )
        .await;
        assert_eq!(ticket(&detail)["group"], "Sprint 2");
    }
    // The per-store and per-connection routes see only their own members.
    assert_eq!(
        ok(
            router,
            "GET",
            &format!("/stores/{}/groups", project.git[1]),
            None
        )
        .await,
        json!({ "groups": [{ "name": "Sprint 2", "count": 1 }] })
    );
    assert_eq!(
        ok(
            router,
            "GET",
            &format!("/providers/{}/groups", project.git[0]),
            None
        )
        .await,
        json!({ "groups": [
            { "name": "Backlog cleanup", "count": 1 },
            { "name": "Sprint 2", "count": 1 },
        ] })
    );
    assert_eq!(
        ok(
            router,
            "POST",
            &format!("/providers/{}/groups/rename", project.git[1]),
            Some(json!({ "from": "Sprint 2", "to": "Second only" })),
        )
        .await,
        json!({ "name": "Second only", "updated": 1 })
    );
    assert_eq!(
        ok(
            router,
            "POST",
            &format!("/stores/{}/groups/ungroup", project.git[1]),
            Some(json!({ "name": "second ONLY" })),
        )
        .await,
        json!({ "updated": 1 })
    );
    assert_eq!(
        ok(
            router,
            "POST",
            &format!("{base}/groups/ungroup"),
            Some(json!({ "name": "SPRINT 2" })),
        )
        .await,
        json!({ "updated": 1 })
    );
    let (status, _) = send(
        router,
        "POST",
        &format!("{base}/groups/ungroup"),
        Some(json!({ "name": "sprint 2" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        ok(router, "GET", &format!("{base}/groups"), None).await,
        json!({ "groups": [{ "name": "Backlog cleanup", "count": 1 }] })
    );
    let _ = d;
    assert_eq!(
        send(router, "GET", "/checkouts/no-such-checkout/groups", None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn hs2_pdshnn_a_provider_without_groups_refuses_explicitly() {
    let project = project();
    let router = &project.router;
    let base = format!("/checkouts/{}", project.checkout);

    // The capability is reported per connection.
    let providers = ok(router, "GET", &format!("{base}/providers"), None).await;
    let capability = |connection: &str| {
        providers
            .as_array()
            .unwrap()
            .iter()
            .find(|provider| provider["connection_id"] == connection)
            .unwrap_or_else(|| panic!("{connection} in {providers}"))["capabilities"]["groups"]
            .clone()
    };
    assert_eq!(capability("github-main"), false);
    assert_eq!(capability(&project.git[0]), true);

    let unsupported = "provider connection 'github-main' does not support 'groups'";
    for (method, uri, body) in [
        (
            "PATCH",
            format!("{base}/tickets/github-main:42"),
            json!({ "group": "Sprint" }),
        ),
        (
            "PATCH",
            format!("{base}/tickets/github-main:42"),
            json!({ "group": null }),
        ),
        (
            "PATCH",
            "/providers/github-main/tickets/42".to_string(),
            json!({ "group": "Sprint" }),
        ),
        (
            "POST",
            format!("{base}/tickets?source=github-main"),
            json!({ "title": "new", "group": "Sprint" }),
        ),
        (
            "POST",
            "/providers/github-main/groups/rename".to_string(),
            json!({ "from": "a", "to": "b" }),
        ),
        (
            "POST",
            "/providers/github-main/groups/ungroup".to_string(),
            json!({ "name": "a" }),
        ),
    ] {
        let (status, error) = send(router, method, &uri, Some(body)).await;
        assert_eq!(status, StatusCode::CONFLICT, "{method} {uri}: {error}");
        assert_eq!(error["error"], unsupported, "{method} {uri}");
    }
    let (status, error) = send(router, "GET", "/providers/github-main/groups", None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error["error"], unsupported);
    // An invalid name is the caller's error whatever the provider.
    let (status, _) = send(
        router,
        "PATCH",
        "/providers/github-main/tickets/42",
        Some(json!({ "group": "a\nb" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        *project.transport.writes.lock().unwrap(),
        0,
        "nothing was sent to the tracker"
    );

    // The checkout namespace simply excludes it, and its tickets read as ungrouped.
    assert_eq!(
        ok(router, "GET", &format!("{base}/groups"), None).await,
        json!({ "groups": [] })
    );
    let issue = ok(
        router,
        "GET",
        &format!("{base}/tickets/github-main:42"),
        None,
    )
    .await;
    assert_eq!(ticket(&issue)["group"], Value::Null);
}
