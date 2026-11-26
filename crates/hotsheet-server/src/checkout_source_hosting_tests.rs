//! HS2-7AK1Q7: `PUT /checkouts/{reference}/sources/{connection_id}` hosts a linked Git
//! store as a project store, so the unhost sweep releases it once no open project
//! references it, while an explicitly pinned store stays pinned.

use super::*;
use axum::body::Body;
use axum::http::Request;
use axum::response::Response;
use hotsheet_model::Ulid;
use hotsheet_sync::LockExt;
use hotsheet_ticketing::{NewTicket, StoreMetadata, ops};
use serde_json::Value;
use tower::ServiceExt;

const SECRET: &str = "secret";

struct Fixture {
    _root: tempfile::TempDir,
    root: std::path::PathBuf,
    state: AppState,
    router: Router,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = hotsheet_sync::paths::canonicalize(dir.path()).unwrap();
        let primary =
            FsStore::init_git(root.join("primary.hs2"), &StoreMetadata::new("HS")).unwrap();
        let state = AppState::new(primary, SECRET.into())
            .unwrap()
            .with_machine_home(root.join("machine-home"))
            .with_default_codex_home(root.join("no-codex-home"))
            .with_checkout_registry(root.join("checkouts.json"));
        let router = app(state.clone());
        Self {
            _root: dir,
            root,
            state,
            router,
        }
    }

    async fn send(&self, method: &str, uri: &str, body: Option<Value>) -> Response {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header("x-hotsheet-secret", SECRET);
        if body.is_some() {
            request = request.header(axum::http::header::CONTENT_TYPE, "application/json");
        }
        let body = body.map_or(Body::empty(), |value| Body::from(value.to_string()));
        self.router
            .clone()
            .oneshot(request.body(body).unwrap())
            .await
            .unwrap()
    }

    async fn json(&self, method: &str, uri: &str, body: Option<Value>) -> Value {
        let response = self.send(method, uri, body).await;
        assert!(response.status().is_success(), "{method} {uri}");
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    /// Open a project at `name` linked to `stores`; a client tab polls it so it is live.
    async fn open_project(&self, name: &str, stores: &[&FsStore]) -> String {
        let checkout = self.root.join(name);
        std::fs::create_dir_all(&checkout).unwrap();
        let roots: Vec<_> = stores.iter().map(|store| store.root()).collect();
        let opened = self
            .json(
                "POST",
                "/projects/open",
                Some(serde_json::json!({"root": checkout, "stores": roots})),
            )
            .await;
        let id = opened["checkout"]["id"].as_str().unwrap().to_owned();
        let poll = self
            .send(
                "GET",
                &format!("/ws/poll?since=0&timeout_ms=0&checkout={id}&client=tab"),
                None,
            )
            .await;
        assert_eq!(poll.status(), StatusCode::OK);
        id
    }

    /// A linked Git store holding one ticket.
    fn linked_store(&self, name: &str, prefix: &str) -> FsStore {
        let store = FsStore::init_git(self.root.join(name), &StoreMetadata::new(prefix)).unwrap();
        ops::create(
            &store,
            Ulid::new(),
            prefix,
            now(),
            NewTicket {
                title: format!("{prefix} ticket"),
                ..Default::default()
            },
        )
        .unwrap();
        store
    }

    async fn add_source(&self, checkout: &str, store: &FsStore) -> Response {
        let source = hotsheet_ticketing::checkouts::TicketSource::git(store.root());
        self.send(
            "PUT",
            &format!("/checkouts/{checkout}/sources/{}", source.connection_id),
            Some(serde_json::json!({"provider": "git", "locator": store.root()})),
        )
        .await
    }

    /// The client closes the project and the unhost sweep runs to completion.
    async fn close_and_sweep(&self, checkout: &str) {
        let closed = self
            .send(
                "POST",
                &format!("/checkouts/{checkout}/close?client=tab"),
                None,
            )
            .await;
        assert_eq!(closed.status(), StatusCode::NO_CONTENT);
        crate::routes::store_scoped::sweep_unhosted(&self.state).await;
    }

    async fn ticket_titles(&self, checkout: &str) -> Vec<String> {
        let listed = self
            .json("GET", &format!("/checkouts/{checkout}/tickets"), None)
            .await;
        let rows = listed
            .get("tickets")
            .unwrap_or(&listed)
            .as_array()
            .unwrap_or_else(|| panic!("ticket list shape: {listed}"));
        let mut titles: Vec<String> = rows
            .iter()
            .map(|row| row["title"].as_str().unwrap().to_owned())
            .collect();
        titles.sort();
        titles
    }

    fn hosted(&self) -> Vec<String> {
        let mut ids: Vec<String> = self
            .state
            .host
            .locations()
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        ids.sort();
        ids
    }

    fn watchers(&self) -> usize {
        self.state.watchers.lock_or_recover().len()
    }

    fn project_hosted(&self, id: &str) -> bool {
        self.state.project_hosted.lock_or_recover().contains(id)
    }
}

/// A source added to an open project is hosted and usable at once, and closing the project
/// returns the hosted set and watcher count to their baseline.
#[tokio::test]
async fn an_added_source_is_usable_and_released_when_the_project_closes() {
    let fixture = Fixture::new();
    let checkout = fixture.open_project("app", &[]).await;
    let linked = fixture.linked_store("linked.hs2", "LNK");
    let id = multistore::store_url_id(&linked);
    let (hosted_before, watchers_before) = (fixture.hosted(), fixture.watchers());

    assert_eq!(
        fixture.add_source(&checkout, &linked).await.status(),
        StatusCode::OK
    );
    assert!(fixture.state.host.contains(&id));
    assert!(
        fixture.project_hosted(&id),
        "hosted outside the unhost sweep"
    );
    assert_eq!(fixture.watchers(), watchers_before + 1);
    assert_eq!(fixture.ticket_titles(&checkout).await, ["LNK ticket"]);

    // While the project stays open, a sweep keeps the store hosted.
    crate::routes::store_scoped::sweep_unhosted(&fixture.state).await;
    assert!(fixture.state.host.contains(&id));

    fixture.close_and_sweep(&checkout).await;
    assert_eq!(fixture.hosted(), hosted_before);
    assert_eq!(fixture.watchers(), watchers_before);
    assert!(!fixture.project_hosted(&id));

    // A later request through the checkout hosts it again on demand.
    assert_eq!(fixture.ticket_titles(&checkout).await, ["LNK ticket"]);
    assert!(fixture.project_hosted(&id));
}

/// Repeated adds host the store once, and one close releases it.
#[tokio::test]
async fn repeated_adds_host_once_and_release_together() {
    let fixture = Fixture::new();
    let checkout = fixture.open_project("app", &[]).await;
    let first = fixture.linked_store("first.hs2", "ONE");
    let second = fixture.linked_store("second.hs2", "TWO");
    let (hosted_before, watchers_before) = (fixture.hosted(), fixture.watchers());

    for _ in 0..3 {
        assert_eq!(
            fixture.add_source(&checkout, &first).await.status(),
            StatusCode::OK
        );
    }
    assert_eq!(
        fixture.add_source(&checkout, &second).await.status(),
        StatusCode::OK
    );
    assert_eq!(fixture.hosted().len(), hosted_before.len() + 2);
    assert_eq!(fixture.watchers(), watchers_before + 2);
    assert_eq!(
        fixture.ticket_titles(&checkout).await,
        ["ONE ticket", "TWO ticket"]
    );

    fixture.close_and_sweep(&checkout).await;
    assert_eq!(fixture.hosted(), hosted_before);
    assert_eq!(fixture.watchers(), watchers_before);
}

/// A store pinned through `POST /stores` stays pinned when a project links it: closing the
/// project never unhosts it.
#[tokio::test]
async fn adding_a_pinned_store_keeps_it_pinned() {
    let fixture = Fixture::new();
    let checkout = fixture.open_project("app", &[]).await;
    let linked = fixture.linked_store("linked.hs2", "PIN");
    let id = multistore::store_url_id(&linked);
    let pinned = fixture
        .send(
            "POST",
            "/stores",
            Some(serde_json::json!({"path": linked.root()})),
        )
        .await;
    assert_eq!(pinned.status(), StatusCode::CREATED);
    let (hosted_before, watchers_before) = (fixture.hosted(), fixture.watchers());

    assert_eq!(
        fixture.add_source(&checkout, &linked).await.status(),
        StatusCode::OK
    );
    assert_eq!(fixture.hosted(), hosted_before);
    assert_eq!(fixture.watchers(), watchers_before);
    assert!(!fixture.project_hosted(&id));
    assert_eq!(fixture.ticket_titles(&checkout).await, ["PIN ticket"]);

    fixture.close_and_sweep(&checkout).await;
    assert!(
        fixture.state.host.contains(&id),
        "a pinned store was unhosted"
    );
    assert_eq!(fixture.watchers(), watchers_before);
}

/// A store another open project already hosts is shared, not hosted twice, and is released
/// only once neither project is open.
#[tokio::test]
async fn adding_a_store_another_project_hosts_releases_it_after_both_close() {
    let fixture = Fixture::new();
    let shared = fixture.linked_store("shared.hs2", "SHR");
    let id = multistore::store_url_id(&shared);
    let (hosted_before, watchers_before) = (fixture.hosted(), fixture.watchers());
    let owner = fixture.open_project("owner", &[&shared]).await;
    assert!(fixture.project_hosted(&id));
    let other = fixture.open_project("other", &[]).await;

    assert_eq!(
        fixture.add_source(&other, &shared).await.status(),
        StatusCode::OK
    );
    assert_eq!(fixture.watchers(), watchers_before + 1);
    assert_eq!(fixture.ticket_titles(&other).await, ["SHR ticket"]);

    fixture.close_and_sweep(&other).await;
    assert!(fixture.state.host.contains(&id), "the owner is still open");
    fixture.close_and_sweep(&owner).await;
    assert_eq!(fixture.hosted(), hosted_before);
    assert_eq!(fixture.watchers(), watchers_before);
}

/// A refused add releases the store it hosted at once, without waiting for a sweep.
#[tokio::test]
async fn a_refused_add_releases_the_store_it_hosted() {
    let fixture = Fixture::new();
    let checkout = fixture.open_project("app", &[]).await;
    let linked = fixture.linked_store("linked.hs2", "BAD");
    let (hosted_before, watchers_before) = (fixture.hosted(), fixture.watchers());

    let mismatched = fixture
        .send(
            "PUT",
            &format!("/checkouts/{checkout}/sources/not-the-id"),
            Some(serde_json::json!({"provider": "git", "locator": linked.root()})),
        )
        .await;
    assert_eq!(mismatched.status(), StatusCode::BAD_REQUEST);
    assert_eq!(fixture.hosted(), hosted_before);
    assert_eq!(fixture.watchers(), watchers_before);

    let unknown = fixture.add_source("no-such-project", &linked).await;
    assert!(unknown.status().is_client_error(), "{}", unknown.status());
    assert_eq!(fixture.hosted(), hosted_before);
    assert_eq!(fixture.watchers(), watchers_before);
}
