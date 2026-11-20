//! HS2-D9ZGAE end to end: a CLI write to a ticket store whose checkout is registered with
//! the running host is published by the host's sync loop without any client opening the
//! project. The store's remote has moved on, so the write's own best-effort `git push` is
//! refused and only the host's fetch, rebase, and push can publish it. The CLI library
//! kicks the host over a real HTTP listener, found through an instance record in a
//! temporary `HOTSHEET_HOME`; no real host or store is touched. All content is synthetic.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use hotsheet_server::sync_loop::{PassScope, spawn_sync_loop, sync_registered};
use hotsheet_server::{AppState, app};
use hotsheet_ticketing::sync::SyncReport;
use hotsheet_ticketing::{FsStore, StoreMetadata};
use http_body_util::BodyExt;
use tower::ServiceExt;

const SECRET: &str = "test-secret";

fn git(cwd: &Path, args: &[&str]) {
    let status = hotsheet_ticketing::git::command()
        .args(args)
        .current_dir(cwd)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

fn git_stdout(cwd: &Path, args: &[&str]) -> String {
    let output = hotsheet_ticketing::git::command()
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}");
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn identify(repo: &Path) {
    git(repo, &["config", "user.name", "Sync Test"]);
    git(repo, &["config", "user.email", "sync@test.invalid"]);
}

/// A machine home with a bootstrap store (the host's only hosted store), a project store
/// published to a bare remote, and a registered checkout linking that project store.
struct Fixture {
    dir: tempfile::TempDir,
    home: PathBuf,
    project: PathBuf,
    remote: PathBuf,
    state: AppState,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let bootstrap = hotsheet_cli::external_launch::machine_host_store(&home);
        let bootstrap_store = FsStore::init_git(&bootstrap, &StoreMetadata::new("HB")).unwrap();
        let remote = dir.path().join("remote.git");
        git(
            dir.path(),
            &["init", "--quiet", "--bare", remote.to_str().unwrap()],
        );
        let project = dir.path().join("project.hs2");
        FsStore::init_git(&project, &StoreMetadata::new("HS")).unwrap();
        identify(&project);
        git(&project, &["add", "-A"]);
        git(
            &project,
            &["commit", "--quiet", "--allow-empty", "-m", "init"],
        );
        git(
            &project,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git(&project, &["push", "--quiet", "-u", "origin", "HEAD"]);
        let code = dir.path().join("code");
        std::fs::create_dir_all(&code).unwrap();
        let registry = home.join("checkouts.json");
        hotsheet_ticketing::checkouts::CheckoutRegistry::new(&registry)
            .register(&code, None, None, vec![project.clone()])
            .unwrap();
        let state = AppState::new(bootstrap_store, SECRET.into())
            .unwrap()
            .with_machine_home(&home)
            .with_checkout_registry(&registry)
            .with_default_codex_home(dir.path().join("no-codex-home"));
        Self {
            dir,
            home,
            project,
            remote,
            state,
        }
    }

    /// Another clone publishes a commit, so the project store's plain push is refused.
    fn advance_remote(&self) {
        let other = self.dir.path().join("other");
        if !other.exists() {
            git(
                self.dir.path(),
                &[
                    "clone",
                    "--quiet",
                    self.remote.to_str().unwrap(),
                    other.to_str().unwrap(),
                ],
            );
            identify(&other);
        }
        let file = format!("other-{}.md", ulid::Ulid::new());
        std::fs::write(other.join(&file), "synthetic\n").unwrap();
        git(&other, &["add", &file]);
        git(&other, &["commit", "--quiet", "-m", "other clone"]);
        git(&other, &["push", "--quiet"]);
    }

    fn remote_head(&self) -> String {
        let branch = git_stdout(&self.project, &["symbolic-ref", "--short", "HEAD"]);
        git_stdout(
            &self.remote,
            &["rev-parse", &format!("refs/heads/{branch}")],
        )
    }

    /// Whether the remote's branch has the file at `path` (store-relative). A sync rebases
    /// local commits onto the remote, so the published commit is not the local one.
    fn remote_contains(&self, path: &str) -> bool {
        let head = self.remote_head();
        git_stdout(&self.remote, &["ls-tree", "-r", "--name-only", &head])
            .lines()
            .any(|line| line == path)
    }

    /// Write one ticket the way a CLI command does: a headless store that commits and
    /// starts its own best-effort push. Returns the ticket file's store-relative path.
    fn cli_write(&self, title: &str) -> String {
        let store = FsStore::open(&self.project).unwrap();
        let ticket = hotsheet_ticketing::ops::create(
            &store,
            hotsheet_model::Ulid::new(),
            "HS",
            hotsheet_model::Timestamp::new("2026-01-01T00:00:00Z"),
            hotsheet_ticketing::NewTicket {
                title: title.into(),
                category: "task".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let path = store.ticket_path(&ticket.id);
        let relative = path
            .strip_prefix(store.root())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert_eq!(
            git_stdout(&self.project, &["ls-files", "--", &relative]),
            relative,
            "the write was committed"
        );
        relative
    }

    fn hosts_project(&self) -> bool {
        let project = hotsheet_sync::paths::canonicalize(&self.project).unwrap();
        self.state.hosted_store_roots().iter().any(|(_, root)| {
            hotsheet_sync::paths::canonicalize(Path::new(root))
                .ok()
                .as_ref()
                == Some(&project)
        })
    }
}

fn kick_request(store: &Path, secret: Option<&str>) -> Request<Body> {
    let mut request = Request::builder()
        .method("POST")
        .uri("/sync/kick")
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(secret) = secret {
        request = request.header("x-hotsheet-secret", secret);
    }
    request
        .body(Body::from(
            serde_json::json!({ "store": store }).to_string(),
        ))
        .unwrap()
}

async fn body_json(response: axum::response::Response) -> serde_json::Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hs2_d9zgae_a_cli_write_to_a_registered_store_is_pushed_without_opening_the_project() {
    let fixture = Fixture::new();
    // The regular pass runs once at start; an hour-long cadence leaves only the kick.
    let _sync = spawn_sync_loop(fixture.state.clone(), Duration::from_secs(3600));
    let deadline = Instant::now() + Duration::from_secs(60);
    while hotsheet_ticketing::sync_health::read_record(&fixture.project)
        .and_then(|record| record.outcome)
        .is_none()
    {
        assert!(
            Instant::now() < deadline,
            "the first pass never synced the registered store"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let router = app(fixture.state.clone());
    tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });
    let bootstrap = hotsheet_cli::external_launch::machine_host_store(&fixture.home);
    let record = hotsheet_cli::external_launch::instance_path(&fixture.home, &bootstrap);
    std::fs::create_dir_all(record.parent().unwrap()).unwrap();
    std::fs::write(
        &record,
        serde_json::json!({
            "pid": std::process::id(),
            "url": url,
            "secret": SECRET,
            "store_path": bootstrap,
        })
        .to_string(),
    )
    .unwrap();

    fixture.advance_remote();
    let written = fixture.cli_write("Synthetic CLI write");
    let recorded = hotsheet_ticketing::take_headless_published_stores();
    let project = fixture.project.clone();
    assert!(
        recorded.iter().any(|root| root == &project),
        "the write recorded its store: {recorded:?}"
    );
    let home = fixture.home.clone();
    let messages = tokio::task::spawn_blocking(move || {
        hotsheet_cli::sync_kick::kick_after_writes(&home, vec![project])
    })
    .await
    .unwrap();
    assert!(
        messages.is_empty(),
        "the host accepted the kick: {messages:?}"
    );

    let deadline = Instant::now() + Duration::from_secs(60);
    while !fixture.remote_contains(&written) {
        assert!(
            Instant::now() < deadline,
            "the kicked host never published the CLI write"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        !fixture.hosts_project(),
        "the project was published without being opened"
    );
    // HS2-AVGCKP: the kicked pass records `Synced`, but the host's regular registered-store
    // pass may run after it and find local and remote in agreement (`UpToDate`). Either is a
    // successful outcome; `last_success_at` proves a pass succeeded.
    let health = hotsheet_ticketing::sync_health::read_record(&fixture.project).unwrap();
    assert!(
        matches!(
            health.outcome,
            Some(
                hotsheet_ticketing::sync_health::SyncState::Synced
                    | hotsheet_ticketing::sync_health::SyncState::UpToDate
            )
        ),
        "the store synced successfully: {:?}",
        health.outcome
    );
    assert!(health.last_success_at.is_some(), "a pass recorded success");
}

#[tokio::test]
async fn hs2_d9zgae_sync_kick_accepts_only_an_authenticated_request_for_a_registered_store() {
    let fixture = Fixture::new();
    let router = app(fixture.state.clone());

    // Without the loop running, a registered store is refused as not running.
    let response = router
        .clone()
        .oneshot(kick_request(&fixture.project, Some(SECRET)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body_json(response).await["code"], "sync_not_running");

    let _sync = spawn_sync_loop(fixture.state.clone(), Duration::from_secs(3600));
    let response = router
        .clone()
        .oneshot(kick_request(&fixture.project, Some(SECRET)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let body = body_json(response).await;
    assert_eq!(body["accepted"], true);
    assert_eq!(
        body["store"],
        hotsheet_sync::paths::canonicalize(&fixture.project)
            .unwrap()
            .display()
            .to_string()
    );

    // An unauthenticated or wrongly authenticated kick is refused before any lookup.
    for secret in [None, Some("wrong-secret")] {
        let response = router
            .clone()
            .oneshot(kick_request(&fixture.project, secret))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{secret:?}");
    }

    // A store no registered checkout links, a missing path, and a relative path are refused.
    let unregistered = fixture.dir.path().join("unregistered.hs2");
    FsStore::init_git(&unregistered, &StoreMetadata::new("UN")).unwrap();
    for store in [unregistered, fixture.dir.path().join("missing.hs2")] {
        let response = router
            .clone()
            .oneshot(kick_request(&store, Some(SECRET)))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{store:?}");
        assert_eq!(body_json(response).await["code"], "store_not_registered");
    }
    let response = router
        .clone()
        .oneshot(kick_request(Path::new("project.hs2"), Some(SECRET)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    // The hosted bootstrap store is accepted too.
    let bootstrap = hotsheet_cli::external_launch::machine_host_store(&fixture.home);
    let response = router
        .oneshot(kick_request(&bootstrap, Some(SECRET)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
}

/// The regular pass covers a registered store no client opened: it integrates the moved
/// remote and publishes local commits, and a scoped pass covers only the stores it names.
#[test]
fn hs2_d9zgae_the_regular_pass_syncs_registered_stores_that_are_not_hosted() {
    let fixture = Fixture::new();
    fixture.advance_remote();
    let written = fixture.cli_write("Synthetic unpublished write");
    assert!(!fixture.remote_contains(&written));

    let other = fixture.dir.path().join("elsewhere.hs2");
    let scoped = sync_registered(
        &fixture.state,
        &PassScope::Stores([other].into_iter().collect()),
    );
    assert!(scoped.is_empty(), "{scoped:?}");
    assert!(!fixture.remote_contains(&written));

    let reports = sync_registered(&fixture.state, &PassScope::All);
    let project = hotsheet_sync::paths::canonicalize(&fixture.project)
        .unwrap()
        .display()
        .to_string();
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert_eq!(reports[0].0, project);
    assert!(
        matches!(reports[0].1, SyncReport::Synced { pushed: true, .. }),
        "{reports:?}"
    );
    assert!(fixture.remote_contains(&written));
    assert!(!fixture.hosts_project());
    // The hosted bootstrap store is the hosted pass's, never the registered pass's.
    assert!(
        hotsheet_server::sync_loop::sync_all(&fixture.state)
            .iter()
            .all(|(_, report)| *report == SyncReport::NoRemote)
    );
}

/// HS2-PQ7K4T: an accepted kick announces the kicked store's current sync health at once, so an
/// open client re-reads it even while the loop is not going to run a pass (here, an hour away;
/// in production, a loop backed off by a refused push). A refused kick announces nothing.
#[tokio::test]
async fn hs2_pq7k4t_an_accepted_kick_announces_the_store_health_at_once() {
    let fixture = Fixture::new();
    let checkout =
        hotsheet_ticketing::checkouts::CheckoutRegistry::new(fixture.home.join("checkouts.json"))
            .list()
            .unwrap()
            .remove(0);
    let router = app(fixture.state.clone());
    let _sync = spawn_sync_loop(fixture.state.clone(), Duration::from_secs(3600));
    let mut events = fixture.state.subscribe();

    // Refused kicks (unregistered store, wrong secret) say nothing.
    let unregistered = fixture.dir.path().join("unregistered.hs2");
    FsStore::init_git(&unregistered, &StoreMetadata::new("UN")).unwrap();
    for (store, secret) in [
        (&unregistered, Some(SECRET)),
        (&fixture.project, Some("wrong")),
    ] {
        let response = router
            .clone()
            .oneshot(kick_request(store, secret))
            .await
            .unwrap();
        assert!(!response.status().is_success(), "{store:?}");
    }
    assert!(
        events.try_recv().is_err(),
        "a refused kick announced something"
    );

    let response = router
        .clone()
        .oneshot(kick_request(&fixture.project, Some(SECRET)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let expected = hotsheet_ticketing::sync_health::health(&fixture.project).state;
    let event = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event = events.recv().await.unwrap();
            if event.kind == "ticket_store_sync" {
                return event;
            }
        }
    })
    .await
    .expect("an accepted kick announced the store's sync health");
    assert_eq!(event.id, checkout.id);
    assert_eq!(event.slug, expected.as_str());
}
