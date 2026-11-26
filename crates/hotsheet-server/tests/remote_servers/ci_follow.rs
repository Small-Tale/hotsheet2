//! HS2-CC7TCB: a reported GitHub Actions run still in flight is followed to its end. The
//! server hands it to the GitHub Actions watch Job, a real Job monitor process observes
//! a loopback provider answering GitHub's REST shapes, and the run's failure reaches the
//! main CI status and the live state with no further report. A repeated report starts
//! nothing new, a run without a token or on another CI is not followed, and a restarted
//! server follows a run left in flight. Synthetic repository, commits and token only;
//! no request leaves this machine.
use super::job_monitor::Provider;
use super::orchestration::api;
use super::*;
use hotsheet_cli::orchestration::{DetachedJobMonitor, JobMonitor};
use hotsheet_jobs::{JobStore, MonitorSlot, SlotState};
use hotsheet_server::ChangeEvent;
use hotsheet_server::ci_follow::{CiFollow, TokenProbe};
use hotsheet_ticketing::checkouts::CheckoutRegistry;
use std::path::Path;

const TOKEN: &str = "synthetic-token-for-tests";
/// The page `ci-gate.mjs` reports for run 9 of the synthetic repository.
const RUN_PAGE: &str = "https://github.com/acme/widgets/actions/runs/9";
/// `created_at` of the provider's run, 2026-11-01T10:09:00Z, and its `updated_at`.
const CREATED_MS: i64 = 1_793_527_740_000;
const UPDATED_MS: i64 = 1_793_527_770_000;

struct Token(bool);
impl TokenProbe for Token {
    fn available(&self, _home: &Path) -> bool {
        self.0
    }
}

/// The monitor a server on this machine starts, pointed at the loopback provider.
fn monitor(url: &str) -> Arc<dyn JobMonitor> {
    Arc::new(
        DetachedJobMonitor::new("hotsheet-server")
            .flags(
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
    )
}

struct Fixture {
    _root: tempfile::TempDir,
    home: std::path::PathBuf,
    source: std::path::PathBuf,
    checkout: String,
}

fn fixture() -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let source = root.path().join("store");
    // Creates the store, so the checkout can link it.
    drop(state(&source, &home));
    let project = root.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let checkout = CheckoutRegistry::new(home.join("checkouts.json"))
        .register(&project, None, None, vec![source.clone()])
        .unwrap()
        .id;
    Fixture {
        _root: root,
        home,
        source,
        checkout,
    }
}

fn in_flight(sha: &str, run_url: &str) -> Value {
    json!({"status": "in_progress", "branch": "main", "sha": sha, "workflow": "CI",
        "run_url": run_url, "started_at_ms": CREATED_MS})
}

/// The next pushed live state whose `ci` satisfies `want`, waiting on the change bus.
async fn next_ci(
    events: &mut tokio::sync::broadcast::Receiver<ChangeEvent>,
    want: impl Fn(&Value) -> bool,
) -> Value {
    tokio::time::timeout(std::time::Duration::from_secs(60), async {
        loop {
            let event = match events.recv().await {
                Ok(event) => event,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(error) => panic!("change bus closed: {error}"),
            };
            let Some(live) = event.live.filter(|_| event.kind == "live_state") else {
                continue;
            };
            let ci = serde_json::to_value(live).unwrap()["ci"].clone();
            if want(&ci) {
                return ci;
            }
        }
    })
    .await
    .expect("the followed run's verdict was never pushed")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_cc7tcb_a_reported_run_in_flight_is_followed_to_its_failure() {
    assert!(hotsheet_aitools::test_permission_vault_available());
    hotsheet_aitools::enable_test_permission_vault();
    let provider = Provider::default();
    let url = provider.start();
    let fx = fixture();
    let sha = "1".repeat(40);
    provider.run(9, &sha, None);
    let host = state(&fx.source, &fx.home)
        .with_ci_follow(CiFollow::new(Some(monitor(&url)), Arc::new(Token(true))));
    let router = app(host.clone());
    let mut events = host.subscribe();
    let route = format!("/checkouts/{}/ci-status", fx.checkout);

    // The reporter sees the run running and reports it once. The server starts the
    // watch and the monitor.
    let (status, started) = api(
        router.clone(),
        "POST",
        &route,
        in_flight(&sha, RUN_PAGE),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{started}");
    assert_eq!(started["applied"], true, "{started}");
    assert_eq!(started["follow"], "started", "{started}");
    assert_eq!(started["ci"]["status"], "in_progress");
    assert!(matches!(
        MonitorSlot::at(&fx.home).state().unwrap(),
        SlotState::Held(_)
    ));

    // Reporting it again starts nothing new: the same follower and one watch Job.
    let (status, again) = api(
        router.clone(),
        "POST",
        &route,
        in_flight(&sha, RUN_PAGE),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again["follow"], "following", "{again}");
    assert_eq!(host.ci_follow().following(), 1);
    let jobs = JobStore::at(&fx.home).list(&fx.checkout).unwrap().jobs;
    assert_eq!(jobs.len(), 1, "one watch Job for the run");
    assert_eq!(jobs[0].kind.name, "github-actions");

    // GitHub finishes the run red. Nobody reports again; the monitor observes it and
    // the server records the finished run.
    provider.run(9, &sha, Some("failure"));
    let ci = next_ci(&mut events, |ci| ci["status"] == "failure").await;
    assert_eq!(ci["checkout"], fx.checkout.as_str());
    assert_eq!(ci["sha"], sha.as_str());
    assert_eq!(ci["branch"], "main");
    assert_eq!(ci["workflow"], "CI");
    assert_eq!(
        ci["run_url"],
        "https://github.example.test/acme/widgets/actions/runs/9"
    );
    assert_eq!(ci["started_at_ms"], CREATED_MS);
    assert_eq!(ci["finished_at_ms"], UPDATED_MS);
    let stored = hotsheet_server::ci_status::load_runs(
        &hotsheet_server::ci_status::ci_status_path(&fx.home),
    );
    assert_eq!(
        stored[&fx.checkout].status,
        hotsheet_server::ci_status::CiStatus::Failure
    );
    assert!(provider.requests() > 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_cc7tcb_no_token_another_ci_or_no_monitor_follows_nothing_and_a_restart_resumes() {
    assert!(hotsheet_aitools::test_permission_vault_available());
    hotsheet_aitools::enable_test_permission_vault();
    let provider = Provider::default();
    let url = provider.start();
    let fx = fixture();
    let sha = "2".repeat(40);
    provider.run(9, &sha, None);
    let route = format!("/checkouts/{}/ci-status", fx.checkout);
    let slot = MonitorSlot::at(&fx.home);
    let no_jobs = || {
        JobStore::at(&fx.home)
            .list(&fx.checkout)
            .unwrap()
            .jobs
            .is_empty()
    };

    // Without a GitHub token the run is stored but not followed: no Job, no monitor.
    let tokenless = app(state(&fx.source, &fx.home)
        .with_ci_follow(CiFollow::new(Some(monitor(&url)), Arc::new(Token(false)))));
    let (status, answer) = api(tokenless, "POST", &route, in_flight(&sha, RUN_PAGE), true).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(answer["applied"], true);
    assert_eq!(answer["follow"], "no_token", "{answer}");
    assert!(no_jobs());
    assert_eq!(slot.state().unwrap(), SlotState::Free);

    // Another CI's run page is never followed (a finished run neither: unit tests).
    let following = app(state(&fx.source, &fx.home)
        .with_ci_follow(CiFollow::new(Some(monitor(&url)), Arc::new(Token(true)))));
    let gitlab = in_flight(&sha, "https://gitlab.com/acme/widgets/-/pipelines/9");
    let (_, answer) = api(following, "POST", &route, gitlab, true).await;
    assert_eq!(answer["applied"], true);
    assert_eq!(answer["follow"], "none", "{answer}");
    assert!(no_jobs());

    // A server that starts no monitor stores the run without following it...
    let offline =
        app(state(&fx.source, &fx.home).with_ci_follow(CiFollow::new(None, Arc::new(Token(true)))));
    let (_, answer) = api(offline, "POST", &route, in_flight(&sha, RUN_PAGE), true).await;
    assert_eq!(answer["applied"], true);
    assert_eq!(answer["follow"], "off", "{answer}");
    assert!(no_jobs());
    assert_eq!(slot.state().unwrap(), SlotState::Free);

    // ...and a server that starts later follows the run that was left in flight.
    let restarted = state(&fx.source, &fx.home)
        .with_ci_follow(CiFollow::new(Some(monitor(&url)), Arc::new(Token(true))));
    let _router = app(restarted.clone());
    let mut events = restarted.subscribe();
    restarted.spawn_ci_follow_recovery();
    provider.run(9, &sha, Some("success"));
    let ci = next_ci(&mut events, |ci| ci["status"] == "success").await;
    assert_eq!(ci["sha"], sha.as_str());
    assert_eq!(ci["finished_at_ms"], UPDATED_MS);
}
