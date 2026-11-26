//! HS2-VVHVF0: a CI gate linked through the server is observed by a real Job monitor
//! process the server started, against a loopback provider, with no CLI call; and a
//! server that starts over a home with a waiting gate (of a finished run) starts the
//! monitor again by itself. Synthetic repository, commits and token only.
use super::orchestration::{api, shared_runtime};
use super::*;
use hotsheet_cli::orchestration::{DetachedJobMonitor, OrchestrationRuntime};
use hotsheet_jobs::{MonitorSlot, SlotState};
use hotsheet_ticketing::checkouts::CheckoutRegistry;
use hotsheet_ticketing::{NewTicket, ops};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

const TARGET: &str = "01ARZ3NDEKTSV4RRFFQ69G5FE9";
const NOW: &str = "2026-10-15T00:00:00Z";
const TOKEN: &str = "synthetic-token-for-tests";
pub(super) const RUNS: &str = "/repos/acme/widgets/actions/runs?branch=main&event=push&per_page=30";

/// The etag version, the routes by path (etag, body), the number of requests, and a
/// status every request is answered with instead (HS2-YADH9C: a provider outage).
type Routes = (u64, HashMap<String, (String, String)>, usize, Option<u16>);
/// A loopback provider answering the REST shapes the watch source reads.
#[derive(Clone, Default)]
pub(super) struct Provider(Arc<(std::sync::Mutex<Routes>, std::sync::Condvar)>);
impl Provider {
    pub(super) fn start(&self) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let state = self.0.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() {
                    continue;
                }
                let mut matched_etag = None;
                loop {
                    let mut header = String::new();
                    if reader.read_line(&mut header).is_err() || header.trim().is_empty() {
                        break;
                    }
                    if let Some((name, value)) = header.split_once(':') {
                        if name.trim().eq_ignore_ascii_case("if-none-match") {
                            matched_etag = Some(value.trim().to_owned());
                        }
                    }
                }
                let path = line.split_whitespace().nth(1).unwrap_or("").to_owned();
                let mut provider = state.0.lock_or_recover();
                provider.2 += 1;
                state.1.notify_all();
                let (status, etag, body) = match (provider.3, provider.1.get(&path)) {
                    (Some(status), _) => {
                        (status, None, json!({"message": "Unavailable"}).to_string())
                    }
                    (None, Some((etag, _))) if matched_etag.as_ref() == Some(etag) => {
                        (304, Some(etag.clone()), String::new())
                    }
                    (None, Some((etag, body))) => (200, Some(etag.clone()), body.clone()),
                    (None, None) => (404, None, json!({"message": "Not Found"}).to_string()),
                };
                drop(provider);
                let etag = etag.map_or(String::new(), |etag| format!("ETag: {etag}\r\n"));
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\n{etag}\
                     Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        url
    }
    pub(super) fn serve(&self, path: &str, body: Value) {
        let mut provider = self.0.0.lock_or_recover();
        provider.0 += 1;
        let etag = format!("W/\"v{}\"", provider.0);
        provider.1.insert(path.to_owned(), (etag, body.to_string()));
    }
    /// The only run of the workflow on the branch: `id` built `sha`.
    pub(super) fn run(&self, id: u64, sha: &str, conclusion: Option<&str>) {
        self.serve(
            RUNS,
            json!({"total_count": 1, "workflow_runs": [{
                "id": id, "name": "CI", "head_branch": "main", "head_sha": sha,
                "event": "push", "status": if conclusion.is_some() { "completed" } else { "in_progress" },
                "conclusion": conclusion, "run_attempt": 1,
                "html_url": format!("https://github.example.test/acme/widgets/actions/runs/{id}"),
                "created_at": "2026-11-01T10:09:00Z", "updated_at": "2026-11-01T10:09:30Z",
                "run_started_at": "2026-11-01T10:09:00Z"
            }]}),
        );
        self.serve(
            &format!("/repos/acme/widgets/actions/runs/{id}/jobs?per_page=100"),
            json!({"total_count": 0, "jobs": []}),
        );
    }
    pub(super) fn requests(&self) -> usize {
        self.0.0.lock_or_recover().2
    }
    /// Answer every request with `status` (a provider outage), or as routed again.
    fn fail_with(&self, status: Option<u16>) {
        self.0.0.lock_or_recover().3 = status;
    }
    /// Wait until the provider has answered more than `count` requests in all.
    fn requested_beyond(&self, count: usize) -> bool {
        let (provider, signal) = &*self.0;
        let (provider, timeout) = signal
            .wait_timeout_while(
                provider.lock_or_recover(),
                std::time::Duration::from_secs(60),
                |provider| provider.2 <= count,
            )
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        drop(provider);
        !timeout.timed_out()
    }
}

fn ticket(store: &FsStore, title: &str) -> hotsheet_model::Ticket {
    ops::create(
        store,
        hotsheet_model::Ulid::new(),
        "TST",
        NOW.into(),
        NewTicket {
            title: title.into(),
            ..Default::default()
        },
    )
    .unwrap()
}

/// The runtime a server on this machine uses, with the monitor it starts pointed at
/// the loopback provider.
pub(super) fn with_monitor(source: &Path, home: &Path, url: &str) -> OrchestrationRuntime {
    shared_runtime(source, home, &Arc::new(Memory::default())).with_job_monitor(Arc::new(
        DetachedJobMonitor::new("hotsheet-server")
            .flags(
                // HS2-R49D43: the unchanged-poll backoff capped, so a verdict after
                // a slow step is seen at the next poll, not minutes later.
                [
                    "--poll-ms",
                    "40",
                    "--max-poll-ms",
                    "500",
                    "--idle-exit-ms",
                    "3000",
                ]
                .map(String::from),
            )
            .env("HOTSHEET_GITHUB_API_URL", url)
            .env("GH_TOKEN", TOKEN),
    ))
}

/// Wait, by cursor replay on the run event stream, until the gate reaches `state`.
async fn until_gate(
    waiter: &OrchestrationRuntime,
    owner: &str,
    gate_id: &str,
    state: &str,
) -> bool {
    let mut after = 0;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while std::time::Instant::now() < deadline {
        let (waiter, owner_id) = (waiter.clone(), owner.to_owned());
        let page = tokio::task::spawn_blocking(move || {
            waiter.run_events(&owner_id, after, 200, None, 10_000)
        })
        .await
        .unwrap()
        .unwrap();
        after = page.cursor;
        let reached = page.entries.iter().any(|entry| {
            let record = serde_json::to_value(&entry.record).unwrap();
            record["kind"] == "gate"
                && record["gate_id"] == gate_id
                && record["gate_state"] == state
        });
        if reached {
            return true;
        }
    }
    false
}

/// A bound self-target, a run dispatched on it and remembered rules, served by a
/// router whose runtime starts monitors pointed at `url`.
struct Watched {
    root: tempfile::TempDir,
    home: std::path::PathBuf,
    source: std::path::PathBuf,
    checkout: hotsheet_ticketing::checkouts::Checkout,
    router: axum::Router,
    base: String,
    run_id: String,
    ai: Value,
}

async fn watched_run(url: &str) -> Watched {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let source = root.path().join("store");
    let host = state(&source, &home);
    let project = root.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let checkout = CheckoutRegistry::new(home.join("checkouts.json"))
        .register(&project, None, None, vec![source.clone()])
        .unwrap();
    let connection = checkout.sources[0].connection_id.clone();
    let store = FsStore::open(&source).unwrap();
    let orchestration = ticket(&store, "Coordinate the watched release");
    let work = ticket(&store, "Work whose commit is watched");
    // HS2-507DGZ: an ended watch is linked again after 150, 300 and 600 ms here.
    let runtime = with_monitor(&source, &home, url)
        .with_gate_relink_retry(std::time::Duration::from_millis(150));
    let router = app(host.with_orchestration_runtime(runtime));
    let base = format!("/checkouts/{}/orchestration", checkout.id);
    let added = api(
        router.clone(),
        "POST",
        &format!("{base}/targets"),
        json!({"target_id":TARGET,"label":"Self","expected_revision":null}),
        true,
    )
    .await
    .1;
    let candidate = api(
        router.clone(),
        "POST",
        &format!("{base}/inspect"),
        json!({"destination":{"kind":"local","checkout_id":checkout.id}}),
        true,
    )
    .await
    .1;
    api(
        router.clone(),
        "PUT",
        &format!("{base}/targets/{TARGET}/binding"),
        json!({"target":{"owner_checkout_id":checkout.id,"target_id":TARGET},
            "expected_manifest_revision":added["revision"],"expected_binding_revision":0,
            "previous_destination":null,"destination":candidate["destination"]}),
        true,
    )
    .await;
    let expires = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
        + 3_600_000;
    let ai = json!({"role":"ai","id":"scheduler-1"});
    super::orchestration::remember_rules(
        router.clone(),
        &format!("{base}/targets/{TARGET}"),
        json!({"actor_id":"scheduler-1",
            "actions":["tickets_read","tickets_write","work_start","gates_observe"],
            "expires_at_ms":expires,"actor":{"role":"human","id":"maintainer"}}),
    )
    .await;
    let (status, run) = api(
        router.clone(),
        "POST",
        &format!("{base}/runs"),
        json!({"ticket":{"native_id":orchestration.slug},"target_id":TARGET,
            "connection_id":connection,"native_id":work.slug,"operation_id":"op-watched",
            "worker_id":"watched-worker","actor":ai}),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{run}");
    let run_id = run["run_id"].as_str().unwrap().to_owned();
    Watched {
        root,
        home,
        source,
        checkout,
        router,
        base,
        run_id,
        ai,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_vvhvf0_a_server_starts_the_job_monitor_for_its_gates_and_again_after_a_restart() {
    assert!(hotsheet_aitools::test_permission_vault_available());
    hotsheet_aitools::enable_test_permission_vault();
    let provider = Provider::default();
    let url = provider.start();
    let Watched {
        root: _root,
        home,
        source,
        checkout,
        router,
        base,
        run_id,
        ai,
    } = watched_run(&url).await;
    let slot = MonitorSlot::at(&home);
    assert_eq!(slot.state().unwrap(), SlotState::Free);
    let (a, b) = ("1".repeat(40), "2".repeat(40));
    provider.run(9, &a, None);

    // Linked over HTTP: the server starts the monitor itself and says it watches.
    let (status, linked) = api(
        router.clone(),
        "POST",
        &format!("{base}/runs/{run_id}/gates"),
        json!({"commit":a,"repository":"acme/widgets","actor":ai}),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{linked}");
    assert_eq!(linked["watching"], true, "{linked}");
    assert!(matches!(slot.state().unwrap(), SlotState::Held(_)));
    let gate_a = linked["gate"]["gate_id"].as_str().unwrap().to_owned();
    // HS2-7QNK7Y: reconciliation over HTTP finds the run's CI run in flight and the
    // same watch live: verified, and nothing names it as unverified.
    let (status, reconciled) = api(
        router.clone(),
        "POST",
        &format!("{base}/runs/{run_id}/reconcile"),
        json!({"actor":ai}),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{reconciled}");
    assert_eq!(
        reconciled["reconciliation"]["findings"][4],
        json!({"source":"ci_run","verification":"verified"})
    );
    // HS2-BYEB60: no deployment was requested, and the answer says so.
    assert_eq!(
        reconciled["reconciliation"]["findings"][5],
        json!({"source":"deployment","verification":"verified","reason":"no_deployment"})
    );
    // HS2-75Q254: with every source reported, nothing is unverified at all.
    assert_eq!(reconciled["unverified"], Value::Null, "{reconciled}");
    // The provider finishes the run green. Nobody runs a CLI command; the monitor
    // observes it and the server's follower records the verdict.
    provider.run(9, &a, Some("success"));
    let waiter = shared_runtime(&source, &home, &Arc::new(Memory::default()));
    assert!(
        until_gate(&waiter, &checkout.id, &gate_a, "passed").await,
        "the gate linked through the server never passed"
    );
    assert!(provider.requests() > 0);

    // The server stops. While nothing runs, the CLI links the next commit without a
    // monitor, the run is cancelled, and the monitor is stopped: a gate of a finished
    // run waits, and nothing observes it.
    let store_arg = source.to_str().unwrap();
    let worker = [
        "--actor-role",
        "ai",
        "--actor-id",
        "scheduler-1",
        "-C",
        store_arg,
        "orchestration",
    ];
    let moved = cli_named(
        &home,
        &worker,
        &[
            "gate",
            "link",
            &run_id,
            "--project",
            &checkout.id,
            "--commit",
            &b,
            "--repo",
            "acme/widgets",
            "--no-monitor",
        ],
    );
    assert_eq!(moved["gate"]["state"], "pending", "{moved}");
    let gate_b = moved["gate"]["gate_id"].as_str().unwrap().to_owned();
    let cancelled = cli_named(
        &home,
        &worker,
        &["run", "cancel", &run_id, "--project", &checkout.id],
    );
    assert_eq!(cancelled["state"], "cancelled", "{cancelled}");
    cli_named(&home, &["job", "watch"], &["stop"]);
    assert_eq!(slot.state().unwrap(), SlotState::Free);
    assert!(waiter.run_owners().unwrap().is_empty());
    assert_eq!(waiter.gate_owners(), std::slice::from_ref(&checkout.id));
    provider.run(10, &b, Some("success"));

    // A server starts on that home. Its start-up alone follows the waiting gate, starts
    // the monitor, and records the verdict.
    let restarted =
        state(&source, &home).with_orchestration_runtime(with_monitor(&source, &home, &url));
    restarted.spawn_run_recovery();
    assert!(
        until_gate(&waiter, &checkout.id, &gate_b, "passed").await,
        "the restarted server never observed the waiting gate"
    );
    let summary = waiter
        .gate_summary(
            &checkout.id,
            &hotsheet_ticketing::orchestration::GateFilter {
                run_id: Some(run_id.clone()),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(
        summary.ci,
        hotsheet_ticketing::orchestration::GateState::Passed
    );
    assert!(waiter.gate_owners().is_empty());
    // The token never reaches the machine-local records.
    for name in ["orchestration-gates.json", "orchestration-runs.json"] {
        let record = std::fs::read_to_string(home.join(name)).unwrap();
        assert!(!record.contains(TOKEN));
    }
    restarted.begin_stopping();
    cli_named(&home, &["job", "watch"], &["stop"]);
}

/// The gate as this machine records it: `(state, reason)`.
fn gate_record(
    runtime: &OrchestrationRuntime,
    owner: &str,
    gate_id: &str,
) -> (String, Option<String>) {
    let gate = serde_json::to_value(runtime.gate(owner, gate_id).unwrap()).unwrap();
    (
        gate["state"].as_str().unwrap().to_owned(),
        gate["reason"].as_str().map(str::to_owned),
    )
}

/// Wait, by cursor replay on the run event stream, until the gate reads `unknown` with
/// `reason`.
async fn until_reason(
    waiter: &OrchestrationRuntime,
    owner: &str,
    gate_id: &str,
    reason: &str,
) -> bool {
    let mut after = 0;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while std::time::Instant::now() < deadline {
        let (waiter, owner_id) = (waiter.clone(), owner.to_owned());
        let page = tokio::task::spawn_blocking(move || {
            waiter.run_events(&owner_id, after, 200, None, 10_000)
        })
        .await
        .unwrap()
        .unwrap();
        after = page.cursor;
        let reached = page.entries.iter().any(|entry| {
            let record = serde_json::to_value(&entry.record).unwrap();
            record["kind"] == "gate"
                && record["gate_id"] == gate_id
                && record["gate_state"] == "unknown"
                && record["reason"] == reason
        });
        if reached {
            return true;
        }
    }
    false
}

/// The watch Jobs of the checkout's store, oldest first: `(id, status)`.
fn watch_jobs(home: &Path, checkout: &str) -> Vec<(String, String)> {
    hotsheet_jobs::JobStore::at(home)
        .list(checkout)
        .unwrap()
        .jobs
        .into_iter()
        .map(|job| (job.id, job.status.as_str().to_owned()))
        .collect()
}

/// HS2-YADH9C, HS2-507DGZ: a CI watch the provider ends on a real server. The provider
/// cannot find the repository (404), so the watch ends without a verdict and the gate
/// reads `unknown` (`watch_ended`), never a verdict. The server's gate follower links
/// it again by itself on a bounded back-off: once the repository is readable the
/// re-linked watch passes with no manual link. When the 404 persists, three re-links
/// later the gate reads `watch_gone`, which waits on nothing; a link by hand after the
/// fix watches again. A later provider outage (server errors) is retried by the monitor
/// while the gate stays `pending`, and once the provider answers again the verdict is
/// recorded. No CLI call, no fixed wait: every step waits on the run event stream or on
/// the provider's own requests.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_yadh9c_a_failed_ci_watch_is_unknown_until_relinked_and_an_outage_is_waited_out() {
    assert!(hotsheet_aitools::test_permission_vault_available());
    hotsheet_aitools::enable_test_permission_vault();
    let provider = Provider::default();
    let url = provider.start();
    let Watched {
        root: _root,
        home,
        source,
        checkout,
        router,
        base,
        run_id,
        ai,
    } = watched_run(&url).await;
    let waiter = shared_runtime(&source, &home, &Arc::new(Memory::default()));
    let path = format!("{base}/runs/{run_id}/gates");
    let link = |commit: &str| {
        let body = json!({"commit":commit,"repository":"acme/widgets","actor":ai});
        api(router.clone(), "POST", &path, body, true)
    };

    // 1. The provider does not know the repository (404): the watch ends. The gate
    // says so and never turns that into a verdict.
    let first = "2".repeat(40);
    let (status, linked) = link(&first).await;
    assert_eq!(status, StatusCode::OK, "{linked}");
    assert_eq!(linked["watching"], true, "{linked}");
    let gate_id = linked["gate"]["gate_id"].as_str().unwrap().to_owned();
    assert!(
        until_reason(&waiter, &checkout.id, &gate_id, "watch_ended").await,
        "a watch the provider ended never reached the gate"
    );
    // Never a verdict: unknown, or pending again once the follower re-linked it.
    let summary = waiter.run_gate_state(&checkout.id, &run_id).unwrap();
    assert!(
        matches!(
            summary.ci,
            hotsheet_ticketing::orchestration::GateState::Unknown
                | hotsheet_ticketing::orchestration::GateState::Pending
        ),
        "{:?}",
        summary.ci
    );
    // 2. The repository is readable again with a green run. Nobody links again: the
    // follower's re-link watches it and the gate passes.
    provider.run(20, &first, Some("success"));
    assert!(
        until_gate(&waiter, &checkout.id, &gate_id, "passed").await,
        "the follower never linked the ended watch again"
    );
    assert!(watch_jobs(&home, &checkout.id).len() >= 2);

    // 3. Another commit, and the repository stays unreadable: three re-links later the
    // gate is given up with `watch_gone` and waits on nothing.
    provider.fail_with(Some(404));
    let commit = "3".repeat(40);
    let jobs_before = watch_jobs(&home, &checkout.id).len();
    let (status, linked) = link(&commit).await;
    assert_eq!(status, StatusCode::OK, "{linked}");
    let gate_id = linked["gate"]["gate_id"].as_str().unwrap().to_owned();
    assert!(
        until_reason(&waiter, &checkout.id, &gate_id, "watch_gone").await,
        "the follower never gave up a watch that kept ending"
    );
    let jobs = watch_jobs(&home, &checkout.id);
    // The linked watch and three re-links, each ended.
    assert_eq!(jobs.len() - jobs_before, 4, "{jobs:?}");
    assert!(
        jobs[jobs_before..]
            .iter()
            .all(|(_, status)| status == "failed"),
        "{jobs:?}"
    );
    assert_eq!(
        gate_record(&waiter, &checkout.id, &gate_id),
        ("unknown".into(), Some("watch_gone".into()))
    );
    let gone = waiter.gate(&checkout.id, &gate_id).unwrap();
    assert!(!gone.awaits_provider());

    // 4. The credentials are fixed and the run is in flight. Linking again is the same
    // gate, watched again.
    provider.fail_with(None);
    provider.run(21, &commit, None);
    let (status, relinked) = link(&commit).await;
    assert_eq!(status, StatusCode::OK, "{relinked}");
    assert_eq!(relinked["gate"]["gate_id"], gate_id.as_str(), "{relinked}");
    assert_eq!(relinked["watching"], true, "{relinked}");
    assert_eq!(
        gate_record(&waiter, &checkout.id, &gate_id),
        ("pending".into(), None)
    );

    // 5. The provider fails with server errors. The monitor keeps asking, and the gate
    // stays pending: an outage is neither a verdict nor an ended watch.
    let before = provider.requests();
    provider.fail_with(Some(503));
    for asked in 0..3 {
        assert!(
            provider.requested_beyond(before + asked),
            "the monitor stopped asking during the outage"
        );
    }
    assert_eq!(
        gate_record(&waiter, &checkout.id, &gate_id),
        ("pending".into(), None)
    );

    // 6. The provider answers again with the finished run: the verdict is recorded.
    provider.run(21, &commit, Some("success"));
    provider.fail_with(None);
    assert!(
        until_gate(&waiter, &checkout.id, &gate_id, "passed").await,
        "the gate never passed after the outage"
    );
    assert_eq!(
        waiter.run_gate_state(&checkout.id, &run_id).unwrap().ci,
        hotsheet_ticketing::orchestration::GateState::Passed
    );
    // The gate's history keeps each step of the way, in order: watched, ended, linked by
    // hand, passed. (A re-link whose watch the monitor ended before the follower read
    // it goes from one ended watch to the next without a `pending` entry; the Jobs
    // counted above prove the re-links.)
    let history: Vec<String> = serde_json::to_value(waiter.gate(&checkout.id, &gate_id).unwrap())
        .unwrap()["history"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["to"].as_str().unwrap_or_default().to_owned())
        .collect();
    let pending = history.iter().filter(|state| *state == "pending").count();
    let unknown = history.iter().rposition(|state| state == "unknown");
    let passed = history.iter().position(|state| state == "passed");
    assert!(
        pending >= 2 && unknown.is_some() && passed > unknown,
        "{history:?}"
    );
    cli_named(&home, &["job", "watch"], &["stop"]);
}
