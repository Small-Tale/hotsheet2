//! HS2-A8TTD7: `POST /checkouts/{reference}/batch` lands on every listed ticket or on
//! none. A failing item, a stale token on a middle item, a duplicate id, an empty batch,
//! and a concurrent writer each leave every ticket file byte-identical, publish nothing,
//! and add no commit. Synthetic tickets in temporary git stores only.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use hotsheet_model::Ulid;
use hotsheet_server::{AppState, app};
use hotsheet_ticketing::{FsStore, StoreMetadata, ops};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const SECRET: &str = "synthetic-secret";
const CHECKOUT: &str = "/checkouts/atomic";

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

struct Fixture {
    _dir: tempfile::TempDir,
    store: PathBuf,
    router: axum::Router,
    /// `(qualified id, ticket)` for the three synthetic tickets, in creation order.
    tickets: Vec<(String, Value)>,
}

async fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("store");
    let checkout = dir.path().join("checkout");
    std::fs::create_dir_all(&checkout).unwrap();
    let fs_store = FsStore::init_git(&store, &StoreMetadata::new("HS")).unwrap();
    let router = app(AppState::new(fs_store, SECRET.into())
        .unwrap()
        .with_default_codex_home(dir.path().join("no-codex-home"))
        .with_checkout_registry(dir.path().join("checkouts.json")));
    let (status, value) = send(
        &router,
        "POST",
        "/checkouts",
        Some(json!({ "root": checkout, "alias": "atomic", "stores": [store] })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{value}");
    let mut tickets = Vec::new();
    for title in ["First", "Second", "Third"] {
        let (status, created) = send(
            &router,
            "POST",
            &format!("{CHECKOUT}/tickets"),
            Some(json!({ "title": title })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{created}");
        let qualified = created["qualified_id"].as_str().unwrap().to_owned();
        tickets.push((qualified, created));
    }
    hotsheet_ticketing::flush_background_commits();
    Fixture {
        _dir: dir,
        store,
        router,
        tickets,
    }
}

impl Fixture {
    fn id(&self, index: usize) -> &str {
        &self.tickets[index].0
    }

    fn token(&self, index: usize) -> &str {
        self.tickets[index].1["updated_at"].as_str().unwrap()
    }

    fn ulid(&self, index: usize) -> Ulid {
        Ulid::from_string(self.tickets[index].1["id"].as_str().unwrap()).unwrap()
    }

    async fn batch(&self, updates: Value) -> (StatusCode, Value) {
        send(
            &self.router,
            "POST",
            &format!("{CHECKOUT}/batch"),
            Some(json!({ "updates": updates })),
        )
        .await
    }

    async fn title(&self, index: usize) -> String {
        let (status, value) = send(
            &self.router,
            "GET",
            &format!("{CHECKOUT}/tickets/{}", self.id(index)),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{value}");
        // Checkout reads may wrap the ticket as `{store, ticket}`.
        let ticket = value.get("ticket").unwrap_or(&value);
        ticket["title"]
            .as_str()
            .unwrap_or_else(|| panic!("{value}"))
            .to_owned()
    }

    /// Every ticket file's exact bytes, keyed by path.
    fn files(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        let mut files = BTreeMap::new();
        collect(&self.store.join("tickets"), &mut files);
        files
    }

    fn git(&self, args: &[&str]) -> String {
        let output = hotsheet_ticketing::git::command()
            .arg("-C")
            .arg(&self.store)
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "git {args:?}");
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    fn commits(&self) -> String {
        hotsheet_ticketing::flush_background_commits();
        self.git(&["rev-list", "--count", "HEAD"])
    }

    /// The refused batch changed nothing: same bytes, same commits, a clean tree.
    async fn assert_untouched(&self, files: &BTreeMap<PathBuf, Vec<u8>>, commits: &str) {
        assert_eq!(
            &self.files(),
            files,
            "a refused batch must not change any file"
        );
        assert_eq!(self.commits(), commits, "a refused batch must not commit");
        assert_eq!(self.git(&["status", "--porcelain"]), "");
        for (index, title) in ["First", "Second", "Third"].into_iter().enumerate() {
            assert_eq!(self.title(index).await, title);
        }
    }
}

fn collect(dir: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect(&path, files);
        } else {
            files.insert(path.clone(), std::fs::read(&path).unwrap());
        }
    }
}

#[tokio::test]
async fn hs2_a8ttd7_an_invalid_later_item_rolls_back_the_items_already_written() {
    let fx = fixture().await;
    let files = fx.files();
    let commits = fx.commits();
    // The second item's title writes before its note is refused, so the rollback has to
    // restore the failing item itself as well as the first one.
    let (status, error) = fx
        .batch(json!([
            { "id": fx.id(0), "title": "First updated" },
            {
                "id": fx.id(1),
                "title": "Second updated",
                "note": "Synthetic answer",
                "response_to": Ulid::new().to_string(),
            },
            { "id": fx.id(2), "title": "Third updated" },
        ]))
        .await;
    assert!(status.is_client_error(), "{status} {error}");
    fx.assert_untouched(&files, &commits).await;

    // An invalid field value on the last item refuses the batch the same way.
    let (status, error) = fx
        .batch(json!([
            { "id": fx.id(0), "title": "First updated" },
            { "id": fx.id(1), "title": "Second updated" },
            { "id": fx.id(2), "status": "not-a-status" },
        ]))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
    fx.assert_untouched(&files, &commits).await;

    // The same selection without the bad item lands whole, in one commit.
    let (status, updated) = fx
        .batch(json!([
            { "id": fx.id(0), "title": "First updated" },
            { "id": fx.id(1), "title": "Second updated" },
        ]))
        .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated.as_array().unwrap().len(), 2);
    assert_eq!(fx.title(0).await, "First updated");
    assert_eq!(fx.title(1).await, "Second updated");
    assert_eq!(fx.title(2).await, "Third");
    assert_eq!(
        fx.commits().parse::<usize>().unwrap(),
        commits.parse::<usize>().unwrap() + 1
    );
}

#[tokio::test]
async fn hs2_a8ttd7_a_stale_token_on_a_middle_item_writes_nothing() {
    let fx = fixture().await;
    let files = fx.files();
    let commits = fx.commits();
    let (status, error) = fx
        .batch(json!([
            { "id": fx.id(0), "title": "First updated", "expected_token": fx.token(0) },
            { "id": fx.id(1), "title": "Second updated", "expected_token": "stale" },
            { "id": fx.id(2), "title": "Third updated", "expected_token": fx.token(2) },
        ]))
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{error}");
    assert_eq!(error["code"], "ticket_changed");
    fx.assert_untouched(&files, &commits).await;
}

#[tokio::test]
async fn hs2_a8ttd7_a_ticket_listed_twice_refuses_the_batch() {
    let fx = fixture().await;
    let files = fx.files();
    let commits = fx.commits();
    let slug = fx.tickets[0].1["slug"].as_str().unwrap().to_owned();
    // The same ticket under its qualified id and under its slug is still one ticket.
    for second in [fx.id(0).to_owned(), slug] {
        let (status, error) = fx
            .batch(json!([
                { "id": fx.id(0), "title": "First updated" },
                { "id": fx.id(1), "title": "Second updated" },
                { "id": second, "category": "bug" },
            ]))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
        assert!(
            error["error"].as_str().unwrap().contains("more than once"),
            "{error}"
        );
        fx.assert_untouched(&files, &commits).await;
    }
}

#[tokio::test]
async fn hs2_a8ttd7_an_empty_batch_is_refused() {
    let fx = fixture().await;
    let files = fx.files();
    let commits = fx.commits();
    let (status, error) = fx.batch(json!([])).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
    assert!(
        error["error"].as_str().unwrap().contains("at least one"),
        "{error}"
    );
    fx.assert_untouched(&files, &commits).await;
}

/// Another writer holds the store lock while the batch is sent and changes the middle
/// ticket before releasing it. Whether the batch reads that ticket before or after the
/// change, it must refuse whole: only the other writer's change remains.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hs2_a8ttd7_a_concurrent_writer_never_sees_or_leaves_a_partial_batch() {
    let fx = fixture().await;
    let commits = fx.commits();
    let store_root = fx.store.clone();
    let middle = fx.ulid(1);
    let (locked_tx, locked_rx) = std::sync::mpsc::channel();
    let (go_tx, go_rx) = std::sync::mpsc::channel::<()>();
    let writer = std::thread::spawn(move || {
        let store = FsStore::open(&store_root).unwrap();
        store
            .with_ticket_transaction(|| {
                locked_tx.send(()).unwrap();
                go_rx.recv().unwrap();
                // Re-enters the lock this thread already holds.
                ops::update(
                    &store,
                    &middle,
                    hotsheet_model::Timestamp::from("2099-01-01T00:00:00Z"),
                    ops::TicketPatch {
                        title: Some("Concurrent".into()),
                        ..ops::TicketPatch::default()
                    },
                )
            })
            .unwrap();
    });
    locked_rx.recv().unwrap();
    let router = fx.router.clone();
    let body = json!({ "updates": [
        { "id": fx.id(0), "title": "First updated", "expected_token": fx.token(0) },
        { "id": fx.id(1), "title": "Second updated", "expected_token": fx.token(1) },
        { "id": fx.id(2), "title": "Third updated", "expected_token": fx.token(2) },
    ] });
    let batch = tokio::spawn(async move {
        send(&router, "POST", &format!("{CHECKOUT}/batch"), Some(body)).await
    });
    go_tx.send(()).unwrap();
    writer.join().unwrap();
    let (status, error) = batch.await.unwrap();
    assert_eq!(status, StatusCode::CONFLICT, "{error}");
    assert_eq!(error["code"], "ticket_changed");
    assert_eq!(fx.title(0).await, "First");
    assert_eq!(fx.title(1).await, "Concurrent");
    assert_eq!(fx.title(2).await, "Third");
    // Only the other writer's own commit landed.
    assert_eq!(
        fx.commits().parse::<usize>().unwrap(),
        commits.parse::<usize>().unwrap() + 1
    );
}
