//! HS2-NNWG8H: a GitHub device flow blocks a safe restart and a repository compaction only
//! while it is in flight. Denied, expired, failed, and cancelled flows stop blocking, and a
//! flow whose `expires_in` deadline passes is pruned on the next check with no further
//! GitHub poll. The device endpoints are a local fake with GitHub's response shapes; no
//! real GitHub call is made and the native credential store is denied.

use axum::body::Body;
use axum::extract::State;
use axum::http::{Request, StatusCode, header};
use axum::routing::post;
use axum::{Json, Router};
use hotsheet_server::remote_auth::ManualClock;
use hotsheet_server::{AppState, app};
use hotsheet_ticketing::{FsStore, StoreMetadata};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tower::ServiceExt;

const SECRET: &str = "synthetic-secret";
const CLIENT_ID: &str = "Iv1synthetic0nnwg8h";
const START: i64 = 1_800_000_000;
const EXPIRES_IN: i64 = 900;

/// GitHub's device endpoints: `POST /login/device/code` issues a fresh device code and
/// `POST /login/oauth/access_token` answers 200 with `authorization_pending` until the
/// test sets an outcome for that device code.
#[derive(Default)]
struct FakeDeviceFlow {
    issued: AtomicU64,
    outcomes: Mutex<HashMap<String, Value>>,
    polls: Mutex<HashMap<String, u64>>,
}

struct Upstream {
    origin: String,
    fake: Arc<FakeDeviceFlow>,
}

impl Upstream {
    fn finish(&self, device_code: &str, error: &str) {
        self.fake.outcomes.lock().unwrap().insert(
            device_code.to_owned(),
            json!({
                "error": error,
                "error_description": format!("synthetic {error}"),
                "error_uri": "https://docs.github.com/developers/apps/authorizing-oauth-apps#error-codes-for-the-device-flow"
            }),
        );
    }

    fn polls(&self, device_code: &str) -> u64 {
        *self
            .fake
            .polls
            .lock()
            .unwrap()
            .get(device_code)
            .unwrap_or(&0)
    }
}

async fn device_code(
    State(fake): State<Arc<FakeDeviceFlow>>,
    Json(body): Json<Value>,
) -> Json<Value> {
    assert_eq!(body["client_id"], CLIENT_ID);
    let index = fake.issued.fetch_add(1, Ordering::SeqCst) + 1;
    Json(json!({
        "device_code": format!("synthetic-device-{index}"),
        "user_code": format!("SYNT-{index:04}"),
        "verification_uri": "https://github.example/login/device",
        "expires_in": EXPIRES_IN,
        "interval": 1
    }))
}

async fn access_token(
    State(fake): State<Arc<FakeDeviceFlow>>,
    Json(body): Json<Value>,
) -> Json<Value> {
    assert_eq!(
        body["grant_type"],
        "urn:ietf:params:oauth:grant-type:device_code"
    );
    let code = body["device_code"].as_str().unwrap().to_owned();
    *fake.polls.lock().unwrap().entry(code.clone()).or_default() += 1;
    let outcome = fake.outcomes.lock().unwrap().get(&code).cloned();
    Json(outcome.unwrap_or_else(|| {
        json!({
            "error": "authorization_pending",
            "error_description": "The authorization request is still pending.",
            "error_uri": "https://docs.github.com/developers/apps/authorizing-oauth-apps#error-codes-for-the-device-flow"
        })
    }))
}

/// One upstream per test process, on its own runtime, configured before any state reads
/// the environment. Device codes are unique across tests, so they never share outcomes.
fn upstream() -> &'static Upstream {
    static UPSTREAM: OnceLock<Upstream> = OnceLock::new();
    UPSTREAM.get_or_init(|| {
        let fake = Arc::new(FakeDeviceFlow::default());
        let router = Router::new()
            .route("/login/device/code", post(device_code))
            .route("/login/oauth/access_token", post(access_token))
            .with_state(fake.clone());
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async move {
                    let listener = tokio::net::TcpListener::from_std(listener).unwrap();
                    axum::serve(listener, router).await.unwrap();
                });
        });
        // SAFETY: set once, before any test state reads them, and never changed again.
        unsafe {
            std::env::set_var("HOTSHEET_NATIVE_SECRETS", "deny");
            std::env::set_var(
                "HOTSHEET_GITHUB_ENTERPRISE_APP_CLIENT_IDS",
                json!({ &origin: CLIENT_ID }).to_string(),
            );
        }
        Upstream { origin, fake }
    })
}

fn state(dir: &std::path::Path, clock: &ManualClock) -> AppState {
    let store = FsStore::init_git(dir, &StoreMetadata::new("HS")).unwrap();
    AppState::new(store, SECRET.into())
        .unwrap()
        .with_machine_home(dir.join(".git").join("hotsheet-test-home"))
        .with_default_codex_home(dir.join("no-codex-home"))
        .with_github_auth_clock(clock.clock())
}

fn authed(method: &str, uri: &str, body: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("x-hotsheet-secret", SECRET);
    if body.is_some() {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
    }
    builder
        .body(body.map_or(Body::empty(), |body| Body::from(body.to_owned())))
        .unwrap()
}

async fn call(router: &Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// Start a flow and return `(session_id, device_code)`.
async fn start_flow(router: &Router) -> (String, String) {
    let body = json!({ "web_base": upstream().origin }).to_string();
    let (status, started) = call(router, authed("POST", "/github-auth/device", Some(&body))).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{started}");
    assert_eq!(started["expires_in"], EXPIRES_IN);
    let index: u64 = started["user_code"].as_str().unwrap()[5..].parse().unwrap();
    (
        started["session_id"].as_str().unwrap().to_owned(),
        format!("synthetic-device-{index}"),
    )
}

/// Long-wait for a flow's terminal state; the route returns when its status changes.
async fn wait_flow(router: &Router, session_id: &str) -> Value {
    let uri = format!("/github-auth/device/{session_id}");
    let (status, body) = tokio::time::timeout(
        Duration::from_secs(30),
        call(router, authed("GET", &uri, None)),
    )
    .await
    .expect("device flow reached a terminal state");
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

async fn github_auth_blockers(router: &Router) -> u64 {
    let (status, body) = call(router, authed("GET", "/lifecycle/quiescence", None)).await;
    assert_eq!(status, StatusCode::OK);
    body["report"]["blockers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|blocker| blocker["kind"] == "github_auth")
        .map_or(0, |blocker| blocker["count"].as_u64().unwrap())
}

#[tokio::test(flavor = "multi_thread")]
async fn hs2_nnwg8h_finished_device_flows_stop_blocking_restart() {
    let upstream = upstream();
    let dir = tempfile::tempdir().unwrap();
    let clock = ManualClock::new(START);
    let state = state(dir.path(), &clock);
    let shutdown = state.clone();
    let router = app(state);

    // Five concurrent flows: four finish four different ways, one stays in flight.
    let flows = futures_util::future::join_all((0..5).map(|_| start_flow(&router))).await;
    assert_eq!(github_auth_blockers(&router).await, 5);
    let (status, blocked) = call(&router, authed("POST", "/lifecycle/restart", None)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(blocked["quiescence"]["blockers"][0]["kind"], "github_auth");

    let [denied, expired, failed, cancelled, in_flight] = <[_; 5]>::try_from(flows).unwrap();
    upstream.finish(&denied.1, "access_denied");
    upstream.finish(&expired.1, "expired_token");
    upstream.finish(&failed.1, "incorrect_device_code");
    let (status, _) = call(
        &router,
        authed(
            "DELETE",
            &format!("/github-auth/device/{}", cancelled.0),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(wait_flow(&router, &denied.0).await["state"], "denied");
    assert_eq!(wait_flow(&router, &expired.0).await["state"], "expired");
    let failure = wait_flow(&router, &failed.0).await;
    assert_eq!(failure["state"], "error");
    assert!(
        failure["message"]
            .as_str()
            .unwrap()
            .contains("synthetic incorrect_device_code")
    );
    assert_eq!(wait_flow(&router, &cancelled.0).await["state"], "cancelled");

    // Only the in-flight flow still blocks.
    assert_eq!(github_auth_blockers(&router).await, 1);
    let (status, blocked) = call(&router, authed("POST", "/lifecycle/restart", None)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(blocked["quiescence"]["blockers"][0]["count"], 1);

    // Its deadline passes while GitHub still says pending: the very next check prunes it,
    // without waiting for another GitHub poll.
    let polls = upstream.polls(&in_flight.1);
    clock.advance(EXPIRES_IN);
    assert_eq!(github_auth_blockers(&router).await, 0);
    assert_eq!(wait_flow(&router, &in_flight.0).await["state"], "expired");
    assert!(upstream.polls(&in_flight.1) <= polls + 1);

    // Finished sessions are dropped once their retention ends.
    clock.advance(15 * 60);
    let (status, _) = call(
        &router,
        authed("GET", &format!("/github-auth/device/{}", denied.0), None),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let waiter = tokio::spawn(async move { shutdown.shutdown_requested().await });
    let (status, accepted) = call(&router, authed("POST", "/lifecycle/restart", None)).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{accepted}");
    tokio::time::timeout(Duration::from_secs(5), waiter)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn hs2_nnwg8h_compaction_runs_after_a_finished_device_flow() {
    let upstream = upstream();
    let store_dir = tempfile::tempdir().unwrap();
    let checkout = tempfile::tempdir().unwrap();
    let remote = tempfile::tempdir().unwrap();
    let registry_home = tempfile::tempdir().unwrap();
    let clock = ManualClock::new(START);
    let st = state(store_dir.path(), &clock);
    let git = |root: &std::path::Path, args: &[&str]| {
        let output = hotsheet_ticketing::git::command_in(root)
            .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(store_dir.path(), &["branch", "-M", "main"]);
    git(store_dir.path(), &["config", "user.name", "Test"]);
    git(
        store_dir.path(),
        &["config", "user.email", "test@example.com"],
    );
    git(store_dir.path(), &["add", "-A"]);
    git(store_dir.path(), &["commit", "-qm", "store"]);
    let store = FsStore::open_without_maintenance(store_dir.path())
        .unwrap()
        .with_deferred_push();
    let old = hotsheet_model::Timestamp::new("2025-01-01T00:00:00Z");
    let id = hotsheet_model::Ulid::new();
    let mut ticket =
        hotsheet_model::Ticket::new(id, "HS-OLD", "Old", "task", old.clone(), old.clone());
    ticket.status = hotsheet_model::Status::Completed;
    ticket.completed_at = Some(old.clone());
    store.write_ticket_committing(&ticket).unwrap();
    store
        .write_attachment(
            &id,
            hotsheet_model::Ulid::new(),
            old,
            "large.png",
            b"synthetic payload",
        )
        .unwrap();
    git(remote.path(), &["init", "-q", "--bare"]);
    git(
        store_dir.path(),
        &["remote", "add", "origin", &remote.path().to_string_lossy()],
    );
    git(store_dir.path(), &["push", "-q", "origin", "main"]);
    let router = app(st.with_checkout_registry(registry_home.path().join("checkouts.json")));
    let create = json!({
        "root": checkout.path(), "alias": "compact", "stores": [store_dir.path()]
    })
    .to_string();
    let (status, created) = call(&router, authed("POST", "/checkouts", Some(&create))).await;
    assert_eq!(status, StatusCode::CREATED);
    let source = created["sources"][0]["connection_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let (status, preview) = call(
        &router,
        authed(
            "GET",
            &format!(
                "/checkouts/compact/repository/compaction?source={source}&min_age_days=30&oversized_bytes=10"
            ),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    let execute = json!({
        "source": source,
        "head": preview["head"],
        "branch": preview["branch"],
        "fingerprint": preview["fingerprint"],
        "min_age_days": 30,
        "oversized_bytes": 10,
        "backup": registry_home.path().join("compact-backup.git"),
        "confirm": format!(
            "rewrite {} {}",
            preview["branch"].as_str().unwrap(),
            preview["fingerprint"].as_str().unwrap()
        ),
    })
    .to_string();
    let execute_uri = "/checkouts/compact/repository/compaction";

    // A finished flow and an in-flight one: only the in-flight one refuses compaction.
    let (finished, finished_code) = start_flow(&router).await;
    let (in_flight, _) = start_flow(&router).await;
    upstream.finish(&finished_code, "access_denied");
    assert_eq!(wait_flow(&router, &finished).await["state"], "denied");
    let (status, refused) = call(&router, authed("POST", execute_uri, Some(&execute))).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert!(refused.to_string().contains("active work"));

    let (status, _) = call(
        &router,
        authed("DELETE", &format!("/github-auth/device/{in_flight}"), None),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, result) = call(&router, authed("POST", execute_uri, Some(&execute))).await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_ne!(result["old_head"], result["new_head"]);
}
