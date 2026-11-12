//! Ticket groups over the real router (HS2-PDSHNN, HS2-44W3TP, HS2-8CDFKR, HS2-9KZSD4):
//! the `group` field on create, update, and batch; the `group` / `has_group` list
//! filters; the `/groups` routes for the default store, a hosted store, a provider
//! connection, and a checkout (one namespace across its Git sources, named by the
//! checkout's group registry); the explicit migration; and the explicit refusal from a
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

const ID: &str = "7K3M9P2QXA";
const OTHER_ID: &str = "0000000001";

#[tokio::test]
async fn hs2_9kzsd4_a_store_takes_group_ids_and_refuses_names() {
    let (dir, router) = default_store();
    let first = create(&router, "/tickets", "first", Some(&format!(" {ID} "))).await;
    assert_eq!(first["group"], ID);
    let second = create(&router, "/tickets", "second", None).await;
    assert_eq!(
        second["group"],
        Value::Null,
        "detail serializes null like blocked_reason"
    );
    assert!(second.as_object().unwrap().contains_key("group"));
    let slug = |value: &Value| value["slug"].as_str().unwrap().to_owned();
    // A store has no registry: a name is refused, never stored as a name.
    for (method, uri, body) in [
        (
            "POST",
            "/tickets".to_string(),
            json!({ "title": "named", "group": "Sprint" }),
        ),
        (
            "PATCH",
            format!("/tickets/{}", slug(&second)),
            json!({ "group": "Sprint" }),
        ),
        (
            "POST",
            "/batch".to_string(),
            json!({ "ids": [slug(&second)], "group": "Sprint" }),
        ),
    ] {
        let (status, error) = send(&router, method, &uri, Some(body)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{method} {uri}: {error}");
        assert_eq!(error["code"], "group_registry_required", "{method} {uri}");
    }
    let (status, error) = send(
        &router,
        "PATCH",
        &format!("/tickets/{}", slug(&second)),
        Some(json!({ "group": "a\nb" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
    let joined = ok(
        &router,
        "PATCH",
        &format!("/tickets/{}", slug(&second)),
        Some(json!({ "group": ID })),
    )
    .await;
    assert_eq!(joined["group"], ID);
    assert_eq!(
        joined["updated_at"], second["updated_at"],
        "regrouping keeps updated_at"
    );
    let rows = ok(&router, "GET", &format!("/tickets?group={ID}"), None).await;
    assert_eq!(slugs(&rows), sorted(vec![slug(&first), slug(&second)]));
    let rows = ok(&router, "GET", "/tickets?group=Sprint", None).await;
    assert!(slugs(&rows).is_empty(), "a name alone finds no id: {rows}");
    let rows = ok(&router, "GET", "/tickets?has_group=false", None).await;
    assert!(slugs(&rows).is_empty());

    // The list shows the id under a placeholder; rename needs a project.
    assert_eq!(
        ok(&router, "GET", "/groups", None).await,
        json!({ "registry": false, "groups": [{
            "id": ID, "name": format!("Group {ID}"), "count": 2,
            "named": false, "legacy": false, "duplicate": false,
        }] })
    );
    let (status, error) = send(
        &router,
        "POST",
        "/groups/rename",
        Some(json!({ "group": ID, "to": "Sprint" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
    assert_eq!(error["code"], "group_registry_required");

    // Merge and ungroup work by id.
    ok(
        &router,
        "POST",
        "/batch",
        Some(json!({ "ids": [slug(&second)], "group": OTHER_ID })),
    )
    .await;
    let (status, error) = send(
        &router,
        "POST",
        "/groups/merge",
        Some(json!({ "from": ID, "into": ID })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
    assert_eq!(error["code"], "invalid_group_merge");
    assert_eq!(
        ok(
            &router,
            "POST",
            "/groups/merge",
            Some(json!({ "from": OTHER_ID, "into": ID })),
        )
        .await,
        json!({ "id": ID, "name": format!("Group {ID}"), "updated": 1 })
    );
    let rows = ok(&router, "GET", &format!("/tickets?group={ID}"), None).await;
    assert_eq!(slugs(&rows).len(), 2);
    assert_eq!(
        ok(
            &router,
            "POST",
            "/groups/ungroup",
            Some(json!({ "group": ID }))
        )
        .await,
        json!({ "updated": 2 })
    );
    let (status, error) = send(
        &router,
        "POST",
        "/groups/ungroup",
        Some(json!({ "name": ID })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{error}");
    assert_eq!(error["code"], "group_not_found");
    assert_eq!(
        ok(&router, "GET", "/groups", None).await,
        json!({ "registry": false, "groups": [] })
    );
    // Both files keep their updated_at and no registry appeared beside the store.
    assert!(!dir.path().join("store/.hotsheet2/groups").exists());
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
    /// The code checkout, whose `.hotsheet2/groups` is the project's group registry.
    root: std::path::PathBuf,
    /// Connection ids of the two Git sources, in link order.
    git: Vec<String>,
    transport: Arc<OneIssue>,
}

/// A checkout linking two Git stores and one GitHub connection.
fn project() -> Project {
    project_with(|_| {})
}

/// [`project`], with `seed` writing to the two Git stores before the server starts.
fn project_with(seed: impl FnOnce(&[std::path::PathBuf])) -> Project {
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
    seed(&store_roots);
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
        root,
        git,
        transport,
    }
}

#[tokio::test]
async fn hs2_9kzsd4_a_checkout_names_groups_through_its_registry_across_git_sources() {
    let project = project();
    let router = &project.router;
    let base = format!("/checkouts/{}", project.checkout);
    let create_in = |source: usize, title: &'static str, group: Option<&'static str>| {
        let uri = format!("{base}/tickets?source={}", project.git[source]);
        let router = router.clone();
        async move { create(&router, &uri, title, group).await }
    };
    // A name creates one registry entry; another spelling in another store reuses it.
    let a = create_in(0, "first store", Some("Sprint")).await;
    let sprint = ticket(&a)["group"].as_str().unwrap().to_owned();
    assert!(hotsheet_model::is_group_id(&sprint), "{a}");
    let entry = project
        .root
        .join(".hotsheet2/groups")
        .join(format!("{sprint}.json"));
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(&entry).unwrap()).unwrap(),
        json!({ "name": "Sprint" })
    );
    let b = create_in(1, "second store", Some("  SPRINT ")).await;
    assert_eq!(ticket(&b)["group"], sprint.as_str(), "{b}");
    let c = create_in(1, "loose", None).await;
    let d = create_in(0, "other", Some("Backlog cleanup")).await;
    let backlog = ticket(&d)["group"].as_str().unwrap().to_owned();
    let qualified = |value: &Value| ticket(value)["qualified_id"].as_str().unwrap().to_owned();

    let patched = ok(
        router,
        "PATCH",
        &format!("{base}/tickets/{}", qualified(&c)),
        Some(json!({ "group": "sprint" })),
    )
    .await;
    assert_eq!(ticket(&patched)["group"], sprint.as_str(), "{patched}");
    let summary = |id: &str, name: &str, count: usize| {
        json!({ "id": id, "name": name, "count": count,
                "named": true, "legacy": false, "duplicate": false })
    };
    assert_eq!(
        ok(router, "GET", &format!("{base}/groups"), None).await,
        json!({ "registry": true, "groups": [
            summary(&backlog, "Backlog cleanup", 1),
            summary(&sprint, "Sprint", 3),
        ] }),
        "counts merge by id across the Git sources"
    );
    // The checkout filter resolves a name to its id across every source.
    let rows = ok(router, "GET", &format!("{base}/tickets?group=SPRINT"), None).await;
    assert_eq!(slugs(&rows).len(), 3, "{rows}");
    let rows = ok(
        router,
        "GET",
        &format!("{base}/tickets?group={sprint}"),
        None,
    )
    .await;
    assert_eq!(slugs(&rows).len(), 3, "{rows}");
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

    // The checkout batch resolves one name to one id for the whole selection.
    let batch = ok(
        router,
        "POST",
        &format!("{base}/batch"),
        Some(json!({ "updates": [
            { "id": qualified(&a), "group": "Release" },
            { "id": qualified(&b), "group": "RELEASE" },
        ] })),
    )
    .await;
    let release = ticket(&batch[0])["group"].as_str().unwrap().to_owned();
    assert!(
        batch
            .as_array()
            .unwrap()
            .iter()
            .all(|row| ticket(row)["group"] == release.as_str()),
        "{batch}"
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
    assert_eq!(ticket(&untouched)["title"], "first store", "all-or-nothing");
    ok(
        router,
        "POST",
        &format!("{base}/batch"),
        Some(json!({ "updates": [
            { "id": qualified(&a), "group": sprint },
            { "id": qualified(&b), "group": sprint },
        ] })),
    )
    .await;

    // Create is find-or-create by name.
    assert_eq!(
        ok(
            router,
            "POST",
            &format!("{base}/groups"),
            Some(json!({ "name": "sprint" }))
        )
        .await,
        json!({ "id": sprint, "name": "Sprint", "created": false })
    );
    let created = ok(
        router,
        "POST",
        &format!("{base}/groups"),
        Some(json!({ "name": " Fresh " })),
    )
    .await;
    assert_eq!(
        (created["name"].clone(), created["created"].clone()),
        (json!("Fresh"), json!(true))
    );
    let (status, error) = send(
        router,
        "POST",
        &format!("{base}/groups"),
        Some(json!({ "name": " " })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
    assert_eq!(error["code"], "invalid_group_name");

    // Rename is a registry edit: no ticket changes.
    let (status, error) = send(
        router,
        "POST",
        &format!("{base}/groups/rename"),
        Some(json!({ "group": sprint, "to": "backlog CLEANUP" })),
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
    let before = ok(
        router,
        "GET",
        &format!("{base}/tickets/{}", qualified(&a)),
        None,
    )
    .await;
    assert_eq!(
        ok(
            router,
            "POST",
            &format!("{base}/groups/rename"),
            Some(json!({ "group": "sprint", "to": "Sprint 2" })),
        )
        .await,
        json!({ "id": sprint, "name": "Sprint 2", "updated": 0 })
    );
    let after = ok(
        router,
        "GET",
        &format!("{base}/tickets/{}", qualified(&a)),
        None,
    )
    .await;
    assert_eq!(ticket(&after)["group"], sprint.as_str());
    assert_eq!(ticket(&after)["updated_at"], ticket(&before)["updated_at"]);
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(&entry).unwrap()).unwrap(),
        json!({ "name": "Sprint 2" })
    );

    // A store or connection of the project sees ids only.
    assert_eq!(
        ok(
            router,
            "GET",
            &format!("/stores/{}/groups", project.git[1]),
            None
        )
        .await["groups"][0]["name"],
        format!("Group {sprint}")
    );
    let (status, error) = send(
        router,
        "POST",
        &format!("/providers/{}/groups/rename", project.git[0]),
        Some(json!({ "group": sprint, "to": "x" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
    assert_eq!(error["code"], "group_registry_required");
    let (status, error) = send(
        router,
        "PATCH",
        &format!(
            "/providers/{}/tickets/{}",
            project.git[0],
            ticket(&d)["id"].as_str().unwrap()
        ),
        Some(json!({ "group": "Sprint 2" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
    assert_eq!(error["code"], "group_registry_required");

    // Two clones created the same name: both are listed and flagged, and merge folds them.
    std::fs::write(
        project
            .root
            .join(".hotsheet2/groups")
            .join(format!("{OTHER_ID}.json")),
        "{\"name\":\"sprint 2\"}",
    )
    .unwrap();
    ok(
        router,
        "PATCH",
        &format!("{base}/tickets/{}", qualified(&c)),
        Some(json!({ "group": OTHER_ID })),
    )
    .await;
    let listed = ok(router, "GET", &format!("{base}/groups"), None).await;
    let duplicates: Vec<String> = listed["groups"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|g| g["duplicate"] == true)
        .map(|g| g["id"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        sorted(duplicates),
        sorted(vec![OTHER_ID.to_string(), sprint.clone()]),
        "{listed}"
    );
    let (status, error) = send(
        router,
        "POST",
        &format!("{base}/groups/rename"),
        Some(json!({ "group": "SPRINT 2", "to": "x" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{error}");
    assert_eq!(error["code"], "group_ambiguous");
    assert_eq!(
        ok(
            router,
            "POST",
            &format!("{base}/groups/merge"),
            Some(json!({ "from": OTHER_ID, "into": sprint })),
        )
        .await,
        json!({ "id": sprint, "name": "Sprint 2", "updated": 1 })
    );
    assert_eq!(
        ok(router, "GET", &format!("{base}/groups"), None).await,
        json!({ "registry": true, "groups": [
            summary(&backlog, "Backlog cleanup", 1),
            summary(&sprint, "Sprint 2", 3),
        ] })
    );

    // An id this project does not name shows a placeholder until a rename names it.
    ok(
        router,
        "PATCH",
        &format!("{base}/tickets/{}", qualified(&c)),
        Some(json!({ "group": ID })),
    )
    .await;
    let listed = ok(router, "GET", &format!("{base}/groups"), None).await;
    assert!(
        listed["groups"]
            .as_array()
            .unwrap()
            .iter()
            .any(|g| g["id"] == ID && g["name"] == format!("Group {ID}") && g["named"] == false),
        "{listed}"
    );
    assert_eq!(
        ok(
            router,
            "POST",
            &format!("{base}/groups/rename"),
            Some(json!({ "group": ID, "to": "Shared" })),
        )
        .await,
        json!({ "id": ID, "name": "Shared", "updated": 0 })
    );

    // Ungroup by id or name clears every member in every Git source.
    assert_eq!(
        ok(
            router,
            "POST",
            &format!("{base}/groups/ungroup"),
            Some(json!({ "group": "SPRINT 2" }))
        )
        .await,
        json!({ "updated": 2 })
    );
    let (status, _) = send(
        router,
        "POST",
        &format!("{base}/groups/ungroup"),
        Some(json!({ "name": "sprint 2" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(entry.exists(), "the registry entry is never deleted");
    assert_eq!(
        send(router, "GET", "/checkouts/no-such-checkout/groups", None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn hs2_9kzsd4_the_checkout_migration_is_explicit_and_idempotent() {
    let project = project_with(|stores| {
        // Pre-registry name values, mixed spellings, across both stores and Trash.
        let seed = |store: &FsStore, title: &str, group: &str, status| {
            let ticket = hotsheet_ticketing::ops::create(
                store,
                hotsheet_model::Ulid::new(),
                store.metadata().unwrap().ticket_prefix.as_str(),
                hotsheet_model::Timestamp::new("2026-11-01T00:00:00Z"),
                hotsheet_ticketing::ops::NewTicket {
                    title: title.into(),
                    category: "task".into(),
                    group: Some(group.into()),
                    status,
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(
                ticket.group.as_deref(),
                Some(group),
                "a legacy value is kept as is"
            );
        };
        let first = FsStore::open(&stores[0]).unwrap();
        let second = FsStore::open(&stores[1]).unwrap();
        seed(
            &first,
            "a",
            "Board Drag",
            hotsheet_model::Status::NotStarted,
        );
        seed(
            &second,
            "b",
            "board drag",
            hotsheet_model::Status::NotStarted,
        );
        seed(
            &second,
            "trashed",
            "BOARD DRAG",
            hotsheet_model::Status::Deleted,
        );
        seed(&first, "c", "Search", hotsheet_model::Status::NotStarted);
    });
    let router = &project.router;
    let base = format!("/checkouts/{}", project.checkout);
    let listed = ok(router, "GET", &format!("{base}/groups"), None).await;
    assert_eq!(
        listed["groups"],
        json!([
            { "id": "Board Drag", "name": "Board Drag", "count": 2,
              "named": true, "legacy": true, "duplicate": false },
            { "id": "Search", "name": "Search", "count": 1,
              "named": true, "legacy": true, "duplicate": false },
        ]),
        "legacy names are read and listed by name until migrated"
    );
    // The unfiltered checkout list includes Trash, whose member keeps its group.
    let rows = ok(
        router,
        "GET",
        &format!("{base}/tickets?group=board%20drag"),
        None,
    )
    .await;
    assert_eq!(slugs(&rows).len(), 3, "{rows}");
    // A store has no registry to migrate into.
    let (status, error) = send(
        router,
        "POST",
        &format!("/checkouts/{}/groups/migrate", "no-such-checkout"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{error}");

    assert_eq!(
        ok(
            router,
            "POST",
            &format!("{base}/groups/migrate"),
            Some(json!({}))
        )
        .await,
        json!({ "created": 2, "reused": 0, "updated": 4 })
    );
    let listed = ok(router, "GET", &format!("{base}/groups"), None).await;
    let groups = listed["groups"].as_array().unwrap();
    assert_eq!(groups.len(), 2, "{listed}");
    for (group, name, count) in [(&groups[0], "Board Drag", 2), (&groups[1], "Search", 1)] {
        assert!(
            hotsheet_model::is_group_id(group["id"].as_str().unwrap()),
            "{group}"
        );
        assert_eq!(
            (group["name"].as_str(), group["count"].as_u64()),
            (Some(name), Some(count))
        );
        assert_eq!(group["legacy"], false);
    }
    let board = groups[0]["id"].as_str().unwrap();
    let rows = ok(
        router,
        "GET",
        &format!("{base}/tickets?group={board}"),
        None,
    )
    .await;
    assert_eq!(slugs(&rows).len(), 3, "{rows}");
    let rows = ok(
        router,
        "GET",
        &format!("{base}/tickets?group=BOARD%20DRAG"),
        None,
    )
    .await;
    assert_eq!(
        slugs(&rows).len(),
        3,
        "a name now finds the migrated ids: {rows}"
    );
    // Rerun: nothing left to migrate, nothing written.
    assert_eq!(
        ok(router, "POST", &format!("{base}/groups/migrate"), None).await,
        json!({ "created": 0, "reused": 0, "updated": 0 })
    );
    assert_eq!(
        ok(router, "GET", &format!("{base}/groups"), None).await,
        listed
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
        (
            "POST",
            "/providers/github-main/groups/merge".to_string(),
            json!({ "from": "a", "into": "b" }),
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
        json!({ "registry": true, "groups": [] })
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
