//! Ticket milestone links and due dates over the real router (HS2-A820C1, docs/38 §38.2-§38.6):
//! the `milestone` and `due_on` fields on create, update and batch through a checkout (titles
//! resolved by its registry) and a store (ids only); the list filters; roll-ups in the
//! milestone list and show; Delete Milestone… (clears members first) and Move Open Tickets
//! to…; the explicit refusal from a provider without the capability before any request; and
//! ticket change events naming the old and new milestone. Synthetic tickets only.

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

async fn send(
    router: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("x-hotsheet-secret", SECRET)
        .header("content-type", "application/json")
        .body(body.map_or_else(Body::empty, |value| Body::from(value.to_string())))
        .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
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

async fn refused(
    router: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
    status: StatusCode,
    code: &str,
) {
    let (got, value) = send(router, method, uri, body).await;
    assert_eq!(got, status, "{method} {uri}: {value}");
    assert_eq!(value["code"], code, "{value}");
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

/// A GitHub fake that serves one issue and counts writes.
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
    /// `/checkouts/{id}`.
    base: String,
    root: std::path::PathBuf,
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
    let state = AppState::new(FsStore::open(&store_roots[0]).unwrap(), SECRET.into())
        .unwrap()
        .with_default_codex_home(dir.path().join("no-codex-home"))
        .with_checkout_registry(&registry)
        .with_ticket_provider(Arc::new(provider));
    Project {
        _dir: dir,
        router: app(state),
        base: format!("/checkouts/{}", registered.id),
        root,
        git,
        transport,
    }
}

impl Project {
    async fn create(&self, source: usize, title: &str, extra: Value) -> Value {
        let mut body = json!({ "title": title, "details": "Synthetic details." });
        for (key, value) in extra.as_object().unwrap() {
            body[key] = value.clone();
        }
        let (status, value) = send(
            &self.router,
            "POST",
            &format!("{}/tickets?source={}", self.base, self.git[source]),
            Some(body),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{value}");
        ticket(&value).clone()
    }

    async fn patch(&self, t: &Value, body: Value) -> Value {
        let uri = format!(
            "{}/tickets/{}",
            self.base,
            t["qualified_id"].as_str().unwrap()
        );
        ticket(&ok(&self.router, "PATCH", &uri, Some(body)).await).clone()
    }

    async fn milestone(&self, title: &str, due: Option<&str>) -> String {
        let (status, created) = send(
            &self.router,
            "POST",
            &format!("{}/milestones", self.base),
            Some(json!({ "title": title, "due_on": due })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{created}");
        created["milestone"]["id"].as_str().unwrap().to_owned()
    }

    async fn list(&self, query: &str) -> Vec<String> {
        slugs(
            &ok(
                &self.router,
                "GET",
                &format!("{}/tickets?{query}", self.base),
                None,
            )
            .await,
        )
    }

    /// The ticket events after `cursor` with a milestone move, and the new cursor.
    async fn moves(&self, cursor: u64) -> (Vec<(String, Value)>, u64) {
        let polled = ok(
            &self.router,
            "GET",
            &format!("/ws/poll?since={cursor}&timeout_ms=0"),
            None,
        )
        .await;
        let moves = polled["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| event.get("milestone").is_some())
            .map(|event| {
                (
                    event["slug"].as_str().unwrap().to_owned(),
                    event["milestone"].clone(),
                )
            })
            .collect();
        (moves, polled["cursor"].as_u64().unwrap())
    }
}

fn slug(t: &Value) -> String {
    t["slug"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn hs2_a820c1_checkout_links_resolve_titles_keep_updated_at_and_announce_moves() {
    let p = project();
    let router = &p.router;
    let alpha = p.milestone("Alpha Release", Some("2026-12-04")).await;
    let beta = p.milestone("Beta", None).await;
    let (_, cursor) = p.moves(0).await;

    // Create into a milestone by title, in either store; a title never creates one.
    let a = p
        .create(
            0,
            "a",
            json!({ "milestone": "alpha  release", "due_on": "2026-11-20" }),
        )
        .await;
    assert_eq!(a["milestone"], alpha.as_str());
    assert_eq!(a["due_on"], "2026-11-20");
    let b = p.create(1, "b", json!({ "milestone": alpha })).await;
    let c = p.create(1, "c", json!({})).await;
    assert!(
        c.get("milestone").is_none(),
        "an unlinked ticket omits the key"
    );
    refused(
        router,
        "POST",
        &format!("{}/tickets?source={}", p.base, p.git[0]),
        Some(json!({ "title": "x", "milestone": "No Such Milestone" })),
        StatusCode::NOT_FOUND,
        "milestone_not_found",
    )
    .await;
    refused(
        router,
        "POST",
        &format!("{}/tickets?source={}", p.base, p.git[0]),
        Some(json!({ "title": "x", "due_on": "2026-02-30" })),
        StatusCode::BAD_REQUEST,
        "invalid_due_on",
    )
    .await;
    assert_eq!(
        ok(
            router,
            "GET",
            &format!("{}/milestones?all=true", p.base),
            None
        )
        .await["milestones"]
            .as_array()
            .unwrap()
            .len(),
        2,
        "no milestone was created by a link"
    );

    // Relink c, then b; neither changes updated_at; each change event names from and to.
    let (created_moves, cursor) = p.moves(cursor).await;
    assert_eq!(
        created_moves,
        vec![
            (slug(&a), json!({ "from": null, "to": alpha })),
            (slug(&b), json!({ "from": null, "to": alpha })),
        ]
    );
    let linked = p.patch(&c, json!({ "milestone": "Beta" })).await;
    assert_eq!(linked["milestone"], beta.as_str());
    assert_eq!(
        linked["updated_at"], c["updated_at"],
        "a link is not content"
    );
    let moved = p
        .patch(&b, json!({ "milestone": beta, "due_on": "2026-12-01" }))
        .await;
    assert_eq!(moved["updated_at"], b["updated_at"]);
    let (moves, cursor) = p.moves(cursor).await;
    assert_eq!(
        moves,
        vec![
            (slug(&c), json!({ "from": null, "to": beta })),
            (slug(&b), json!({ "from": alpha, "to": beta })),
        ]
    );
    // Repeating a link changes nothing and names no move; a title edit names none either.
    p.patch(&b, json!({ "milestone": "beta" })).await;
    p.patch(&b, json!({ "title": "b renamed" })).await;
    let (moves, cursor) = p.moves(cursor).await;
    assert!(moves.is_empty(), "{moves:?}");
    // Clearing with null names the move to none.
    p.patch(&c, json!({ "milestone": null })).await;
    let (moves, _) = p.moves(cursor).await;
    assert_eq!(moves, vec![(slug(&c), json!({ "from": beta, "to": null }))]);

    // List filters across both stores: by title, by id, has/none, effective due.
    assert_eq!(p.list("milestone=alpha%20release").await, vec![slug(&a)]);
    assert_eq!(p.list(&format!("milestone={beta}")).await, vec![slug(&b)]);
    // Unlinked: c, and the GitHub issue (a source without the capability reads unlinked).
    let unlinked = p.list("has_milestone=false").await;
    assert!(unlinked.contains(&slug(&c)), "{unlinked:?}");
    assert!(!unlinked.contains(&slug(&a)) && !unlinked.contains(&slug(&b)));
    assert_eq!(unlinked.len(), 2, "{unlinked:?}");
    assert!(p.list("milestone=nothing").await.is_empty());
    // a is due 2026-11-20 (own date, earlier than Alpha's); b is due 2026-12-01.
    assert_eq!(p.list("due_before=2026-11-30").await, vec![slug(&a)]);
    let mut both = vec![slug(&a), slug(&b)];
    both.sort();
    assert_eq!(p.list("due_before=2026-12-01").await, both);
    refused(
        router,
        "GET",
        &format!("{}/tickets?due_before=soon", p.base),
        None,
        StatusCode::BAD_REQUEST,
        "invalid_due_on",
    )
    .await;
    // The milestone's own date lends itself to an undated member.
    let d = p
        .create(0, "d", json!({ "milestone": "Alpha Release" }))
        .await;
    assert!(
        p.list("due_before=2026-12-04").await.contains(&slug(&d)),
        "effective due from the open milestone"
    );
    ok(
        router,
        "POST",
        &format!("{}/milestones/{alpha}/close", p.base),
        None,
    )
    .await;
    assert!(
        !p.list("due_before=2026-12-04").await.contains(&slug(&d)),
        "a closed milestone lends no date"
    );
}

#[tokio::test]
async fn hs2_a820c1_rollups_close_move_open_and_delete_through_the_routes() {
    let p = project();
    let router = &p.router;
    let base = format!("{}/milestones", p.base);
    let alpha = p.milestone("Alpha", Some("2026-01-15")).await;
    let beta = p.milestone("Beta", None).await;
    let a = p.create(0, "a", json!({ "milestone": alpha })).await;
    let b = p.create(1, "b", json!({ "milestone": alpha })).await;
    let done = p.create(1, "done", json!({ "milestone": alpha })).await;
    p.patch(&done, json!({ "status": "completed" })).await;
    // An id another project's registry holds: listed as unknown, never dropped.
    // (the default store is the checkout's first store and takes ids as given).
    let (status, foreign) = send(
        router,
        "POST",
        "/tickets",
        Some(json!({ "title": "foreign", "details": "x", "milestone": "ZZZZZZZZZZ" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{foreign}");
    refused(
        router,
        "POST",
        &format!("{}/tickets?source={}", p.base, p.git[0]),
        Some(json!({ "title": "x", "milestone": "ZZZZZZZZZY" })),
        StatusCode::NOT_FOUND,
        "milestone_not_found",
    )
    .await;

    let listed = ok(router, "GET", &base, None).await;
    assert_eq!(listed["time_zone"], "UTC");
    let rows = listed["milestones"].as_array().unwrap();
    let ids: Vec<&str> = rows.iter().map(|row| row["id"].as_str().unwrap()).collect();
    assert_eq!(ids, vec![alpha.as_str(), beta.as_str(), "ZZZZZZZZZZ"]);
    let rollup = &rows[0]["rollup"];
    assert_eq!(rollup["progress"]["members"], 3);
    assert_eq!(rollup["progress"]["finished"], 1);
    assert_eq!(rollup["progress"]["percent"], 33);
    assert_eq!(rollup["health"], "off_track", "{rollup}");
    assert_eq!(rollup["overdue"], true);
    assert_eq!(rows[1]["rollup"]["progress"]["members"], 0);
    assert!(rows[1]["rollup"]["progress"].get("percent").is_none());
    assert_eq!(rows[2]["state"], "unknown");
    assert_eq!(rows[2]["named"], false);
    assert_eq!(rows[2]["title"], "Milestone ZZZZZZZZZZ");
    let shown = ok(router, "GET", &format!("{base}/ZZZZZZZZZZ"), None).await;
    assert_eq!(shown["rollup"]["progress"]["members"], 1);

    // Closing with open members is allowed; the response names them.
    let closed = ok(router, "POST", &format!("{base}/{alpha}/close"), None).await;
    let open: Vec<&str> = closed["open_members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|member| member["slug"].as_str().unwrap())
        .collect();
    let mut expected = vec![a["slug"].as_str().unwrap(), b["slug"].as_str().unwrap()];
    expected.sort_unstable();
    assert_eq!(open, expected);
    assert_eq!(closed["milestone"]["rollup"]["health"], "done");

    // Move Open Tickets to…: the open ones move, the finished one stays.
    let moved = ok(
        router,
        "POST",
        &format!("{base}/{alpha}/move-open"),
        Some(json!({ "to": "beta" })),
    )
    .await;
    assert_eq!(
        (moved["updated"].as_u64(), moved["changed"].as_bool()),
        (Some(2), Some(true))
    );
    assert_eq!(p.list(&format!("milestone={beta}")).await, expected);
    assert_eq!(
        p.list(&format!("milestone={alpha}")).await,
        vec![slug(&done)]
    );
    let again = ok(
        router,
        "POST",
        &format!("{base}/{alpha}/move-open"),
        Some(json!({ "to": beta })),
    )
    .await;
    assert_eq!(again["changed"], false);
    refused(
        router,
        "POST",
        &format!("{base}/{alpha}/move-open"),
        Some(json!({ "to": "gamma" })),
        StatusCode::NOT_FOUND,
        "milestone_not_found",
    )
    .await;

    // Delete Milestone…: every member is cleared first, then the file goes.
    let deleted = ok(router, "DELETE", &format!("{base}/beta"), None).await;
    assert_eq!(deleted["updated"], 2);
    assert!(
        !p.root
            .join(format!(".hotsheet2/milestones/{beta}.json"))
            .exists()
    );
    assert!(
        p.list("has_milestone=true").await.len() == 2,
        "done and foreign remain"
    );
    refused(
        router,
        "DELETE",
        &format!("{base}/{beta}"),
        None,
        StatusCode::NOT_FOUND,
        "milestone_not_found",
    )
    .await;
    // Deleting an unknown id clears its members only.
    let cleared = ok(router, "DELETE", &format!("{base}/ZZZZZZZZZZ"), None).await;
    assert_eq!(cleared["updated"], 1);
    let refreshed = ok(
        router,
        "GET",
        &format!("/tickets/{}", foreign["id"].as_str().unwrap()),
        None,
    )
    .await;
    assert!(ticket(&refreshed).get("milestone").is_none());
    // A conflicted file refuses delete before any ticket changes.
    std::fs::write(
        p.root.join(format!(".hotsheet2/milestones/{alpha}.json")),
        "{\n<<<<<<< HEAD\n  \"title\": \"A\",\n=======\n  \"title\": \"B\",\n>>>>>>> x\n}\n",
    )
    .unwrap();
    refused(
        router,
        "DELETE",
        &format!("{base}/{alpha}"),
        None,
        StatusCode::CONFLICT,
        "milestone_registry_conflict",
    )
    .await;
    assert_eq!(
        p.list(&format!("milestone={alpha}")).await,
        vec![slug(&done)]
    );
}

#[tokio::test]
async fn hs2_a820c1_stores_take_ids_and_external_sources_refuse_before_any_request() {
    let p = project();
    let router = &p.router;
    // The default store (no registry) takes ids and refuses titles.
    let (status, created) = send(
        router,
        "POST",
        "/tickets",
        Some(json!({ "title": "store", "milestone": "7K3M9P2QXA", "due_on": "2026-12-04" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["milestone"], "7K3M9P2QXA");
    let id = created["id"].as_str().unwrap().to_owned();
    refused(
        router,
        "PATCH",
        &format!("/tickets/{id}"),
        Some(json!({ "milestone": "Alpha" })),
        StatusCode::BAD_REQUEST,
        "milestone_registry_required",
    )
    .await;
    let cleared = ok(
        router,
        "PATCH",
        &format!("/tickets/{id}"),
        Some(json!({ "milestone": null, "due_on": null })),
    )
    .await;
    assert!(cleared.get("milestone").is_none() && cleared.get("due_on").is_none());
    // The batch route validates once for the selection.
    refused(
        router,
        "POST",
        "/batch",
        Some(json!({ "ids": [id], "milestone": "Alpha" })),
        StatusCode::BAD_REQUEST,
        "milestone_registry_required",
    )
    .await;
    // A connection route takes ids, as a store does.
    let linked = p.create(0, "connection", json!({})).await;
    refused(
        router,
        "PATCH",
        &format!(
            "/providers/{}/tickets/{}",
            p.git[0],
            linked["id"].as_str().unwrap()
        ),
        Some(json!({ "milestone": "Alpha" })),
        StatusCode::BAD_REQUEST,
        "milestone_registry_required",
    )
    .await;
    let relinked = ok(
        router,
        "PATCH",
        &format!(
            "/providers/{}/tickets/{}",
            p.git[0],
            linked["id"].as_str().unwrap()
        ),
        Some(json!({ "milestone": "7K3M9P2QXA", "due_on": "2027-01-04" })),
    )
    .await;
    assert_eq!(ticket(&relinked)["milestone"], "7K3M9P2QXA");
    assert_eq!(ticket(&relinked)["due_on"], "2027-01-04");

    // Capabilities: Git sources have both; GitHub has neither and refuses before a request.
    let providers = ok(router, "GET", "/providers", None).await;
    let capabilities = |connection: &str| {
        providers
            .as_array()
            .or_else(|| providers["providers"].as_array())
            .unwrap()
            .iter()
            .find(|provider| provider["connection_id"] == connection)
            .map(|provider| provider["capabilities"].clone())
            .unwrap_or_else(|| panic!("{connection}: {providers}"))
    };
    let github = capabilities("github-main");
    assert_eq!(
        (github["milestones"].clone(), github["due_dates"].clone()),
        (json!(false), json!(false))
    );
    for (body, capability) in [
        (json!({ "milestone": "7K3M9P2QXA" }), "milestones"),
        (json!({ "milestone": null }), "milestones"),
        (json!({ "due_on": "2026-12-04" }), "due_dates"),
    ] {
        let (status, value) = send(
            router,
            "PATCH",
            "/providers/github-main/tickets/42",
            Some(body.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "{body}: {value}");
        assert!(
            value["error"]
                .as_str()
                .unwrap()
                .contains(&format!("does not support '{capability}'")),
            "{value}"
        );
    }
    assert_eq!(
        *p.transport.writes.lock().unwrap(),
        0,
        "no request was made"
    );
}
