//! HS2-K2YE6A: a real second server hosting a target watches a CI gate only for a
//! repository its own checkout names (HS2-WDQZ37). The controller and the peer are
//! two servers with their own homes, joined over authenticated TLS. Synthetic data.
use super::e2e::{Fleet, Side, TARGET_B};
use super::*;
use hotsheet_jobs::JobStore;
use hotsheet_ticketing::orchestration::{AuditJournal, AuditOutcome};

fn audit_reasons(home: &std::path::Path, checkout: &str) -> Vec<(String, AuditOutcome, String)> {
    AuditJournal::new(home, checkout)
        .unwrap()
        .read(0, 200)
        .unwrap()
        .entries
        .into_iter()
        .map(|entry| {
            (
                entry.record.action,
                entry.record.outcome,
                entry.record.reason.unwrap_or_default(),
            )
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_k2ye6a_a_host_refuses_to_watch_a_repository_its_checkout_does_not_name() {
    let fleet = Fleet::new().await;
    let status = hotsheet_ticketing::git::command_in(&fleet.b_project)
        .args([
            "remote",
            "add",
            "origin",
            "https://github.com/acme/widgets.git",
        ])
        .status()
        .unwrap();
    assert!(status.success());
    let orchestration = fleet.orchestration_ticket("Gate the remote work");
    let ticket = fleet.target_ticket(Side::Peer, "Remote work");
    let run = fleet
        .ok(
            "POST",
            "/runs",
            json!({"actor":Fleet::ai(),"ticket":{"native_id":orchestration.slug},
                "target_id":TARGET_B,"connection_id":fleet.connection(Side::Peer),
                "native_id":ticket.slug,"operation_id":"gated-run","mode":"dispatcher"}),
        )
        .await;
    let gates = format!("/runs/{}/gates", run["run_id"].as_str().unwrap());
    let peer_jobs = || {
        JobStore::at(&fleet.peer_home)
            .list(&fleet.b_checkout.id)
            .unwrap()
            .jobs
    };
    let own_jobs = || {
        JobStore::at(&fleet.home)
            .list(&fleet.owner.id)
            .unwrap()
            .jobs
    };

    // A repository the peer's checkout does not name: the peer refuses with the gate's
    // own code and starts nothing, so its credentials never read that repository.
    let (status, refused) = fleet
        .call(
            "POST",
            &gates,
            json!({"commit":"c".repeat(40),"repository":"intruder/elsewhere","actor":Fleet::ai()}),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["code"], "gate_repository_mismatch");
    assert!(peer_jobs().is_empty());
    assert!(
        own_jobs().is_empty(),
        "nor does the controller watch it itself"
    );
    // Both machines keep the refusal in their audit journals, by code.
    assert!(
        audit_reasons(&fleet.peer_home, &fleet.b_checkout.id).contains(&(
            "hosted_gate_watch".into(),
            AuditOutcome::Denied,
            "gate_repository_mismatch".into()
        )),
        "{:?}",
        audit_reasons(&fleet.peer_home, &fleet.b_checkout.id)
    );
    assert!(
        audit_reasons(&fleet.home, &fleet.owner.id)
            .iter()
            .any(|(_, _, reason)| reason == "gate_repository_mismatch"),
        "{:?}",
        audit_reasons(&fleet.home, &fleet.owner.id)
    );

    // The repository the checkout names (any spelling of it) is watched on the peer.
    let linked = fleet
        .ok(
            "POST",
            &gates,
            json!({"commit":"d".repeat(40),"repository":"Acme/Widgets","actor":Fleet::ai()}),
        )
        .await;
    assert_eq!(linked["gate"]["state"], "pending", "{linked}");
    let jobs = peer_jobs();
    assert_eq!(jobs.len(), 1);
    assert_eq!(linked["gate"]["watch_ref"], jobs[0].id.as_str());
    assert!(own_jobs().is_empty());
}

fn git(root: &std::path::Path, args: &[&str]) -> String {
    let output = hotsheet_ticketing::git::command_in(root)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}");
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

/// HS2-G3VAK4: an authorized deployment of a target on the peer runs the target's own
/// tracked deploy script on the peer, in a worktree of the gate's commit, and nothing
/// of it on the controller.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_g3vak4_a_remote_targets_deploy_script_runs_on_the_peer() {
    use std::os::unix::fs::PermissionsExt;
    let fleet = Fleet::new().await;
    let project = fleet.b_project.clone();
    git(
        &project,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/acme/widgets.git",
        ],
    );
    let log = fleet.peer_home.join("deploys.log");
    let script = project.join("scripts/deploy.sh");
    std::fs::create_dir_all(script.parent().unwrap()).unwrap();
    std::fs::write(
        &script,
        format!("#!/bin/sh\necho \"deployed $1\" >> '{}'\n", log.display()),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    git(&project, &["add", "scripts/deploy.sh"]);
    git(
        &project,
        &[
            "-c",
            "user.name=Synthetic",
            "-c",
            "user.email=synthetic@example.invalid",
            "commit",
            "-q",
            "-m",
            "deploy script",
        ],
    );
    let commit = git(&project, &["rev-parse", "HEAD"]);
    let orchestration = fleet.orchestration_ticket("Deploy the remote work");
    let ticket = fleet.target_ticket(Side::Peer, "Remote work");
    let run = fleet
        .ok(
            "POST",
            "/runs",
            json!({"actor":Fleet::ai(),"ticket":{"native_id":orchestration.slug},
                "target_id":TARGET_B,"connection_id":fleet.connection(Side::Peer),
                "native_id":ticket.slug,"operation_id":"deployed-run","mode":"dispatcher"}),
        )
        .await;
    let run_id = run["run_id"].as_str().unwrap().to_owned();
    // CI is green on the peer (HS2-WDQZ37): its watch Job is judged there.
    let linked = fleet
        .ok(
            "POST",
            &format!("/runs/{run_id}/gates"),
            json!({"commit":commit,"repository":"acme/widgets","actor":Fleet::ai()}),
        )
        .await;
    super::gates::judge(
        &fleet.peer_home,
        &fleet.b_checkout.id,
        linked["gate"]["watch_ref"].as_str().unwrap(),
        "green",
        &commit,
    );
    let synced = fleet
        .ok(
            "POST",
            &format!("/runs/{run_id}/gate-sync"),
            json!({"actor":Fleet::ai()}),
        )
        .await;
    assert_eq!(synced["ci"], "passed", "{synced}");
    // A person remembers that deployments may run on target B; then it is requested.
    fleet
        .ok(
            "POST",
            "/permission-rules",
            json!({"target_id":TARGET_B,"action":"deployments_run","actor":Fleet::person()}),
        )
        .await;
    let deployed = fleet
        .ok(
            "POST",
            &format!("/runs/{run_id}/deployments"),
            json!({"environment":"beta","script":"scripts/deploy.sh",
                "operation_id":"deploy-b","actor":Fleet::ai()}),
        )
        .await;
    let gate_id = deployed["gate"]["gate_id"].as_str().unwrap().to_owned();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let gate = loop {
        fleet
            .ok(
                "POST",
                &format!("/runs/{run_id}/gate-sync"),
                json!({"actor":Fleet::ai(),"wait_ms":8000}),
            )
            .await;
        let gate = fleet
            .ok("GET", &format!("/gates/{gate_id}"), Value::Null)
            .await;
        if gate["state"] != "pending" {
            break gate;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the deployment never ended: {gate}"
        );
    };
    assert_eq!(gate["state"], "passed", "{gate}");
    assert_eq!(gate["evidence"]["exit_code"], 0);
    assert_eq!(gate["evidence"]["commit"], commit.as_str());
    assert_eq!(
        std::fs::read_to_string(&log).unwrap().trim(),
        "deployed beta"
    );
    // Nothing of it ran on, or was checked out on, the controller.
    assert!(!fleet.home.join("orchestration-deployments").exists());
    assert!(
        !fleet
            .peer_home
            .join("orchestration-deployments")
            .join(&fleet.b_checkout.id)
            .join(&gate_id)
            .exists(),
        "the worktree is removed"
    );
}

/// Finish a watch Job on `home` the way that machine's Job monitor would: a green run
/// `run` built `head`.
fn judge_green(home: &std::path::Path, checkout: &str, job_id: &str, head: &str, run: u64) {
    use hotsheet_jobs::{Acquire, ActiveStatus, FinalStatus, Outcome};
    let store = JobStore::at(home);
    let (_, token) = store
        .acquire(
            checkout,
            job_id,
            Acquire {
                owner: "monitor:synthetic".into(),
                status: ActiveStatus::Running,
                launch_id: None,
                external: None,
            },
        )
        .unwrap();
    let refs = [
        ("gate", "green".to_owned()),
        ("run_id", run.to_string()),
        ("run_attempt", "1".to_owned()),
        ("run_head_sha", head.to_owned()),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value))
    .collect();
    store
        .finish(
            checkout,
            job_id,
            &token,
            FinalStatus::Succeeded,
            Outcome {
                summary: None,
                refs,
            },
        )
        .unwrap();
}

/// A dispatcher run on the peer's target with one CI gate its host watches: the run,
/// the commit, the gate and the host's watch Job.
async fn hosted_gate(fleet: &Fleet, operation: &str) -> (Value, String, String, String) {
    git(
        &fleet.b_project,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/acme/widgets.git",
        ],
    );
    let orchestration = fleet.orchestration_ticket("Gate the remote work");
    let ticket = fleet.target_ticket(Side::Peer, "Remote work");
    let run = fleet
        .ok(
            "POST",
            "/runs",
            json!({"actor":Fleet::ai(),"ticket":{"native_id":orchestration.slug},
                "target_id":TARGET_B,"connection_id":fleet.connection(Side::Peer),
                "native_id":ticket.slug,"operation_id":operation,"mode":"dispatcher"}),
        )
        .await;
    let commit = "e".repeat(40);
    let linked = fleet
        .ok(
            "POST",
            &format!("/runs/{}/gates", run["run_id"].as_str().unwrap()),
            json!({"commit":commit,"repository":"acme/widgets","actor":Fleet::ai()}),
        )
        .await;
    assert_eq!(linked["gate"]["state"], "pending", "{linked}");
    let gate_id = linked["gate"]["gate_id"].as_str().unwrap().to_owned();
    let job_id = linked["gate"]["watch_ref"].as_str().unwrap().to_owned();
    let jobs = JobStore::at(&fleet.peer_home)
        .list(&fleet.b_checkout.id)
        .unwrap()
        .jobs;
    assert_eq!(jobs.len(), 1, "the watch runs on the host");
    assert_eq!(jobs[0].id, job_id);
    (run, commit, gate_id, job_id)
}

/// HS2-1P1HJT: a remote target's CI gate is watched on its host (HS2-WDQZ37), and the
/// host drops off the network while the gate waits. The controller's gate follower keeps
/// the gate `pending` (never a verdict, never lost work) and backs off instead of
/// spinning on the unreachable host; the verdict the host's watch reached meanwhile is
/// recorded once when the host is back. Waits on connection attempts (counted over a
/// bounded window, HS2-Q9SPRV) and on the gate event stream, never on a fixed delay.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_1p1hjt_a_hosted_ci_watch_waits_out_its_host_going_offline() {
    let mut fleet = Fleet::new().await;
    let (run, commit, gate_id, job_id) = hosted_gate(&fleet, "offline-gated-run").await;

    // 1. The host drops off the network. Its address now refuses every connection at
    // once, and counts them: the controller keeps trying the host, but backs off.
    //
    // HS2-Q9SPRV: every controller loop that contacts the host connects here (the
    // target follower, the gate follower, the identity check before each call, a
    // request already in flight when the host left), and TLS hides which loop a
    // connection belongs to, so attempts from different loops arrive within
    // milliseconds of each other even while each loop backs off. Counting the first
    // three attempts therefore failed about half the time. The rate is measured over
    // `WINDOW` from the first attempt instead: the target follower backs off 1 s, 2 s,
    // 4 s between failed steps (two connections each), and the gate follower, whose
    // long poll the host's leaving cut short, spends the rest of its 8 s wait backing
    // off. Seven to nine attempts are seen in the window; `MAX_IN_WINDOW` leaves more
    // than four times that for a busy host, while a loop that spun would connect
    // hundreds of times. At least three show the controller keeps trying the host.
    const WINDOW: std::time::Duration = std::time::Duration::from_millis(3500);
    const MAX_IN_WINDOW: usize = 40;
    fleet.peer_offline().await;
    let refusing = tokio::net::TcpListener::bind(fleet.peer_address)
        .await
        .unwrap();
    let (first, _) = tokio::time::timeout(std::time::Duration::from_secs(60), refusing.accept())
        .await
        .expect("the controller never tried the host")
        .unwrap();
    drop(first);
    let window_end = tokio::time::Instant::now() + WINDOW;
    let mut attempts = 1;
    while let Ok(accepted) = tokio::time::timeout_at(window_end, refusing.accept()).await {
        drop(accepted.unwrap());
        attempts += 1;
        assert!(
            attempts <= MAX_IN_WINDOW,
            "over {MAX_IN_WINDOW} connection attempts within {WINDOW:?}: the controller spins \
             on an unreachable host"
        );
    }
    assert!(
        attempts >= 3,
        "{attempts} connection attempts in {WINDOW:?}: the controller stopped trying the host"
    );
    // The gate still waits: an unreachable host is neither a verdict nor lost work.
    let gate = fleet
        .ok("GET", &format!("/gates/{gate_id}"), Value::Null)
        .await;
    assert_eq!(gate["state"], "pending", "{gate}");
    assert_eq!(gate["watch_ref"], job_id.as_str(), "{gate}");
    assert!(gate["evidence"].is_null(), "{gate}");

    // 2. Meanwhile the host's own watch reaches a green verdict.
    judge_green(&fleet.peer_home, &fleet.b_checkout.id, &job_id, &commit, 77);
    let still = fleet
        .ok("GET", &format!("/gates/{gate_id}"), Value::Null)
        .await;
    assert_eq!(
        still["state"], "pending",
        "nothing is read from an offline host"
    );

    // 3. The host is back: the follower reads the verdict it reached and records it
    // once.
    drop(refusing);
    fleet.peer_online().await;
    let passed = fleet.gate_until(&gate_id, "passed").await;
    assert_eq!(passed["evidence"]["reference"], "77", "{passed}");
    assert_eq!(passed["evidence"]["commit"], commit.as_str(), "{passed}");
    let events = fleet.ok("GET", "/run-events?after=0", Value::Null).await;
    let verdicts = events["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry.get("record").unwrap_or(entry))
        .filter(|record| record["kind"] == "gate" && record["gate_id"] == gate_id.as_str())
        .filter(|record| record["gate_state"] == "passed")
        .count();
    assert_eq!(verdicts, 1, "{events:#}");
    let history: Vec<&str> = passed["history"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["to"].as_str().unwrap())
        .collect();
    assert_eq!(history, ["unknown", "pending", "passed"], "{passed}");
    // A later sync reads the same verdict again and records nothing new.
    let synced = fleet
        .ok(
            "POST",
            &format!("/runs/{}/gate-sync", run["run_id"].as_str().unwrap()),
            json!({"actor":Fleet::ai()}),
        )
        .await;
    assert_eq!(synced["changed"], json!([]), "{synced}");
    fleet.shut_down().await;
}

/// HS2-C5TYA9: the gate follower's own back-off, measured on a real server. In
/// `hs2_1p1hjt` the host's address refuses every connection, so TLS and the identity
/// check before each peer call hide which controller loop connects, and the gate
/// follower makes no connection in the measured window (HS2-Q9SPRV). Here the host's
/// address is held by a stand-in with the host's own TLS identity: it answers
/// `/remote-identity` as the host does and refuses only the checkout calls, at once,
/// counting each read of `target-gates`. The gate follower then reaches the host,
/// is refused without waiting, and spends the rest of its wait backing off (on this
/// machine's adapter records): it reads the host once per back-off period (HS2-CB5YEQ:
/// no second read after the wait, from the first refusal on), the reads about one wait
/// (`MAX_HOSTED_GATE_WAIT_MS`) apart and never less than half of it, over three
/// back-off periods, and the gate stays `pending`. Waits on the reads themselves,
/// never on a fixed delay.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_c5tya9_the_gate_follower_backs_off_from_a_host_that_refuses_its_reads() {
    use axum::response::IntoResponse;
    use hotsheet_cli::orchestration::MAX_HOSTED_GATE_WAIT_MS;
    use std::time::Duration;
    let mut fleet = Fleet::new().await;
    let (_, _, gate_id, job_id) = hosted_gate(&fleet, "refused-gated-run").await;

    fleet.peer_offline().await;
    let (read_tx, mut reads) = tokio::sync::mpsc::unbounded_channel::<tokio::time::Instant>();
    let stand_in = app(fleet.peer_state.clone()).layer(axum::middleware::from_fn(
        move |request: axum::extract::Request, next: axum::middleware::Next| {
            let read_tx = read_tx.clone();
            async move {
                let path = request.uri().path().to_owned();
                if !path.contains("/checkouts/") {
                    return next.run(request).await;
                }
                if request.method() == axum::http::Method::GET && path.ends_with("/target-gates") {
                    let _ = read_tx.send(tokio::time::Instant::now());
                }
                StatusCode::SERVICE_UNAVAILABLE.into_response()
            }
        },
    ));
    let listener = tokio::net::TcpListener::bind(fleet.peer_address)
        .await
        .unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let serving = tokio::spawn(hotsheet_server::tls::serve_tls_with_acl(
        listener,
        stand_in,
        hotsheet_server::tls::build_server_config(&fleet.paths).unwrap(),
        Some(fleet.paths.acl()),
        async move {
            let _ = stopped.await;
        },
    ));

    // The follower's step reads the gate and, refused, waits out its back-off on this
    // machine's own adapter records and ends; the next step reads again. HS2-CB5YEQ: so
    // each burst is a single read, a wait apart. Reads less than a second apart are one
    // burst; four bursts span three back-off periods.
    let wait = Duration::from_millis(MAX_HOSTED_GATE_WAIT_MS);
    let mut bursts: Vec<Vec<tokio::time::Instant>> = Vec::new();
    while bursts.len() < 4 {
        let read = tokio::time::timeout(4 * wait, reads.recv())
            .await
            .expect("the gate follower stopped asking its host")
            .expect("the stand-in serves");
        match bursts.last_mut() {
            Some(burst) if read - *burst.last().unwrap() < Duration::from_secs(1) => {
                burst.push(read);
            }
            _ => bursts.push(vec![read]),
        }
    }
    let sizes: Vec<usize> = bursts.iter().map(Vec::len).collect();
    let gaps: Vec<Duration> = bursts
        .windows(2)
        .map(|pair| pair[1][0] - pair[0][0])
        .collect();
    assert!(
        sizes.iter().all(|size| *size == 1),
        "bursts of {sizes:?} reads: the follower asks a refusing host more than once per back-off"
    );
    for gap in &gaps {
        assert!(
            *gap >= wait / 2,
            "gate read bursts {gaps:?} apart: the follower asks a refusing host again \
             without backing off"
        );
        assert!(
            *gap <= 3 * wait,
            "gate read bursts {gaps:?} apart: the follower waits far past its back-off"
        );
    }
    // A refusal is neither a verdict nor lost work.
    let gate = fleet
        .ok("GET", &format!("/gates/{gate_id}"), Value::Null)
        .await;
    assert_eq!(gate["state"], "pending", "{gate}");
    assert_eq!(gate["watch_ref"], job_id.as_str(), "{gate}");

    let _ = stop.send(());
    serving.await.unwrap().unwrap();
    fleet.shut_down().await;
}

/// Waits, woken by the controller's machine home (where the audit journal lives)
/// changing, until its audit holds `wanted` denials of `action` as `target_unavailable`.
async fn denied_unavailable(fleet: &Fleet, action: &str, wanted: usize) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        let waiter = hotsheet_jobs::ChangeWaiter::starting(&[fleet.home.as_path()]).unwrap();
        let reasons = audit_reasons(&fleet.home, &fleet.owner.id);
        let denied = reasons
            .iter()
            .filter(|(code, outcome, reason)| {
                code == action && *outcome == AuditOutcome::Denied && reason == "target_unavailable"
            })
            .count();
        if denied >= wanted {
            return;
        }
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        assert!(
            !remaining.is_zero(),
            "no {action} denial as target_unavailable: {reasons:?}"
        );
        tokio::task::block_in_place(|| {
            waiter.wait(remaining.min(std::time::Duration::from_secs(5)))
        });
    }
}

/// HS2-K41M77: every follower that remembers an authorization answer (the target
/// follower, the gate follower) starts while the target's host is offline, so its very
/// first check is a `target_unavailable` denial. That denial is not kept for the
/// ten-minute re-authorization period: once the host is back, the run learns of the
/// claim made there meanwhile and the gate reads the verdict the host's watch reached,
/// well inside the bounded backoff. The capacity and remote-release follows end on a
/// denial and are started again by the next pass, so they keep none. Waits on the audit
/// journal and the run and gate event streams, never on a fixed delay.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_k41m77_followers_started_while_the_host_is_offline_ask_again_when_it_returns() {
    let mut fleet = Fleet::new().await;
    git(
        &fleet.b_project,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/acme/widgets.git",
        ],
    );
    let orchestration = fleet.orchestration_ticket("Follow the remote work");
    let ticket = fleet.target_ticket(Side::Peer, "Remote work followed after an outage");
    let run = fleet
        .ok(
            "POST",
            "/runs",
            json!({"actor":Fleet::ai(),"ticket":{"native_id":orchestration.slug},
                "target_id":TARGET_B,"connection_id":fleet.connection(Side::Peer),
                "native_id":ticket.slug,"operation_id":"first-check-offline-run",
                "mode":"dispatcher"}),
        )
        .await;
    let run_id = run["run_id"].as_str().unwrap().to_owned();
    let commit = "f".repeat(40);
    let linked = fleet
        .ok(
            "POST",
            &format!("/runs/{run_id}/gates"),
            json!({"commit":commit,"repository":"acme/widgets","actor":Fleet::ai()}),
        )
        .await;
    assert_eq!(linked["gate"]["state"], "pending", "{linked}");
    let gate_id = linked["gate"]["gate_id"].as_str().unwrap().to_owned();
    let job_id = linked["gate"]["watch_ref"].as_str().unwrap().to_owned();
    let before = fleet
        .ok("GET", &format!("/runs/{run_id}/status"), Value::Null)
        .await;
    assert_ne!(before["state"], "dispatched", "{before}");
    let denied = |action: &str| {
        audit_reasons(&fleet.home, &fleet.owner.id)
            .iter()
            .filter(|(code, outcome, reason)| {
                code == action && *outcome == AuditOutcome::Denied && reason == "target_unavailable"
            })
            .count()
    };
    let (tickets_denied, gates_denied) = (denied("tickets_read"), denied("gates_observe"));

    // 1. The host is offline, and a new controller process starts: each follower's
    // first check is made against an unreachable host.
    fleet.peer_offline().await;
    fleet.restart_controller().await;
    denied_unavailable(&fleet, "tickets_read", tickets_denied + 1).await;
    denied_unavailable(&fleet, "gates_observe", gates_denied + 1).await;

    // 2. Meanwhile, on the host, a worker claims the ticket and the watch turns green.
    fleet.claim(Side::Peer, &ticket, "synthetic-worker").await;
    judge_green(&fleet.peer_home, &fleet.b_checkout.id, &job_id, &commit, 91);
    let gate = fleet
        .ok("GET", &format!("/gates/{gate_id}"), Value::Null)
        .await;
    assert_eq!(
        gate["state"], "pending",
        "nothing is read from an offline host"
    );

    // 3. The host is back. Both followers ask again rather than keep the denial: the
    // run learns of the claim and the gate records the verdict, each once.
    fleet.peer_online().await;
    let claimed = fleet
        .run_until(
            &run_id,
            "synced with the claim made while offline",
            |status| status["worker_id"] == "synthetic-worker",
        )
        .await;
    assert_eq!(claimed["reachability"], "reachable", "{claimed}");
    let passed = fleet.gate_until(&gate_id, "passed").await;
    assert_eq!(passed["evidence"]["reference"], "91", "{passed}");
    let events = fleet.ok("GET", "/run-events?after=0", Value::Null).await;
    let verdicts = events["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry.get("record").unwrap_or(entry))
        .filter(|record| record["kind"] == "gate" && record["gate_id"] == gate_id.as_str())
        .filter(|record| record["gate_state"] == "passed")
        .count();
    assert_eq!(verdicts, 1, "{events:#}");
    fleet.shut_down().await;
}

/// Finish a watch Job on `home` the way that machine's Job monitor does when the
/// provider ended the watch (the repository is gone): failed, with no verdict.
fn judge_gone(home: &std::path::Path, checkout: &str, job_id: &str) {
    use hotsheet_jobs::{Acquire, ActiveStatus, FinalStatus, Outcome};
    let store = JobStore::at(home);
    let (_, token) = store
        .acquire(
            checkout,
            job_id,
            Acquire {
                owner: "monitor:synthetic".into(),
                status: ActiveStatus::Running,
                launch_id: None,
                external: None,
            },
        )
        .unwrap();
    store
        .finish(
            checkout,
            job_id,
            &token,
            FinalStatus::Failed,
            Outcome {
                summary: Some("synthetic: the repository is gone".into()),
                refs: std::collections::BTreeMap::new(),
            },
        )
        .unwrap();
}

/// The peer's watch Jobs, oldest first, once there are `count`: woken by the peer's
/// machine home (where its Job store lives) changing.
async fn peer_jobs_until(fleet: &Fleet, count: usize) -> Vec<hotsheet_jobs::Job> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(90);
    loop {
        let waiter = hotsheet_jobs::ChangeWaiter::starting(&[fleet.peer_home.as_path()]).unwrap();
        let mut jobs = JobStore::at(&fleet.peer_home)
            .list(&fleet.b_checkout.id)
            .unwrap()
            .jobs;
        if jobs.len() >= count {
            jobs.sort_by(|a, b| a.id.cmp(&b.id));
            return jobs;
        }
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        assert!(
            !remaining.is_zero(),
            "the host never had {count} watch Jobs: {jobs:?}"
        );
        tokio::task::block_in_place(|| {
            waiter.wait(remaining.min(std::time::Duration::from_secs(5)));
        });
    }
}

/// The cursor after the controller recorded `gate_id` as `unknown` for `reason`,
/// woken by the owner's run event stream.
async fn gate_reason_after(fleet: &Fleet, gate_id: &str, reason: &str, mut cursor: u64) -> u64 {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(90);
    loop {
        let page = fleet
            .ok(
                "GET",
                &format!("/run-events?after={cursor}&wait_ms=5000"),
                Value::Null,
            )
            .await;
        let found = page["entries"].as_array().unwrap().iter().any(|entry| {
            let record = entry.get("record").unwrap_or(entry);
            record["kind"] == "gate"
                && record["gate_id"] == gate_id
                && record["gate_state"] == "unknown"
                && record["reason"] == reason
        });
        cursor = page["cursor"].as_u64().unwrap_or(cursor);
        if found {
            return cursor;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "gate {gate_id} never read {reason}"
        );
    }
}

/// HS2-F7ZG0F: the hosted half of HS2-507DGZ on two real servers. A remote target's CI
/// gate is watched on its host (HS2-WDQZ37); the host's watch ends without a verdict
/// (the provider no longer has the repository). The controller's gate reads
/// `watch_ended`, never a verdict, and its follower asks the host to watch again by
/// itself: a new Job on the host, whose green verdict passes the gate with no link by
/// hand. For another commit whose watches keep ending, three re-links later the gate
/// reads `watch_gone`, the host starts no more Jobs for it, and a link by hand watches
/// it again. Waits on the run event stream and the host's Job store, never on a fixed
/// delay.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_f7zg0f_a_hosted_ci_watch_that_ends_is_linked_again_on_its_host_until_gone() {
    let mut fleet = Fleet::new().await;
    // Re-links after 150, 300 and 600 ms rather than the server's 30 s back-off.
    fleet
        .relink_after(std::time::Duration::from_millis(150))
        .await;
    git(
        &fleet.b_project,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/acme/widgets.git",
        ],
    );
    let orchestration = fleet.orchestration_ticket("Gate the remote work again");
    let ticket = fleet.target_ticket(Side::Peer, "Remote work whose watch ends");
    let run = fleet
        .ok(
            "POST",
            "/runs",
            json!({"actor":Fleet::ai(),"ticket":{"native_id":orchestration.slug},
                "target_id":TARGET_B,"connection_id":fleet.connection(Side::Peer),
                "native_id":ticket.slug,"operation_id":"relinked-gated-run",
                "mode":"dispatcher"}),
        )
        .await;
    let run_id = run["run_id"].as_str().unwrap().to_owned();
    let link = |commit: &str| {
        let body = json!({"commit":commit,"repository":"acme/widgets","actor":Fleet::ai()});
        let path = format!("/runs/{run_id}/gates");
        let fleet = &fleet;
        async move { fleet.ok("POST", &path, body).await }
    };

    // 1. The host's watch ends without a verdict: the gate reads `watch_ended`, and the
    // follower asks the host to watch again. That watch's green verdict passes the gate.
    let first = "1".repeat(40);
    let linked = link(&first).await;
    assert_eq!(linked["gate"]["state"], "pending", "{linked}");
    let gate_id = linked["gate"]["gate_id"].as_str().unwrap().to_owned();
    let jobs = peer_jobs_until(&fleet, 1).await;
    assert_eq!(jobs.len(), 1, "the watch runs on the host");
    assert_eq!(linked["gate"]["watch_ref"], jobs[0].id.as_str());
    assert!(
        JobStore::at(&fleet.home)
            .list(&fleet.b_checkout.id)
            .map(|listing| listing.jobs.is_empty())
            .unwrap_or(true),
        "nothing is watched on the controller"
    );
    judge_gone(&fleet.peer_home, &fleet.b_checkout.id, &jobs[0].id);
    let cursor = gate_reason_after(&fleet, &gate_id, "watch_ended", 0).await;
    let jobs = peer_jobs_until(&fleet, 2).await;
    assert_eq!(jobs.len(), 2, "one re-link: {jobs:?}");
    let relinked = &jobs[1];
    assert!(!relinked.status.is_finished(), "{relinked:?}");
    judge_green(
        &fleet.peer_home,
        &fleet.b_checkout.id,
        &relinked.id,
        &first,
        31,
    );
    let passed = fleet.gate_until(&gate_id, "passed").await;
    assert_eq!(passed["evidence"]["reference"], "31", "{passed}");
    assert_eq!(passed["watch_ref"], relinked.id.as_str(), "{passed}");
    let history: Vec<(&str, &str)> = passed["history"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            (
                entry["to"].as_str().unwrap(),
                entry["reason"].as_str().unwrap_or_default(),
            )
        })
        .collect();
    assert!(
        history.contains(&("unknown", "watch_ended")) && history.last() == Some(&("passed", "")),
        "{history:?}"
    );
    // A passed gate is never linked again.
    assert_eq!(peer_jobs_until(&fleet, 2).await.len(), 2);

    // 2. Another commit, and every watch the host starts for it ends as well: the linked
    // watch and three re-links, then `watch_gone`, which waits on nothing.
    let second = "2".repeat(40);
    let linked = link(&second).await;
    let gone_id = linked["gate"]["gate_id"].as_str().unwrap().to_owned();
    assert_ne!(gone_id, gate_id);
    for count in 3..=6 {
        let jobs = peer_jobs_until(&fleet, count).await;
        assert_eq!(jobs.len(), count, "{jobs:?}");
        let newest = jobs.last().unwrap();
        assert!(!newest.status.is_finished(), "{newest:?}");
        judge_gone(&fleet.peer_home, &fleet.b_checkout.id, &newest.id);
    }
    gate_reason_after(&fleet, &gone_id, "watch_gone", cursor).await;
    let gone = fleet
        .ok("GET", &format!("/gates/{gone_id}"), Value::Null)
        .await;
    assert_eq!(gone["state"], "unknown", "{gone}");
    assert_eq!(gone["reason"], "watch_gone", "{gone}");
    let jobs = JobStore::at(&fleet.peer_home)
        .list(&fleet.b_checkout.id)
        .unwrap()
        .jobs;
    assert_eq!(jobs.len(), 6, "no re-link after the budget: {jobs:?}");
    assert!(jobs.iter().all(|job| job.status.is_finished()), "{jobs:?}");
    // A sync reads the given-up gate as it stands and starts nothing on the host.
    fleet
        .ok(
            "POST",
            &format!("/runs/{run_id}/gate-sync"),
            json!({"actor":Fleet::ai()}),
        )
        .await;
    let still = fleet
        .ok("GET", &format!("/gates/{gone_id}"), Value::Null)
        .await;
    assert_eq!(still["reason"], "watch_gone", "{still}");

    // 3. Linking by hand is the same gate, watched again on the host.
    let again = link(&second).await;
    assert_eq!(again["gate"]["gate_id"], gone_id.as_str(), "{again}");
    assert_eq!(again["gate"]["state"], "pending", "{again}");
    let jobs = peer_jobs_until(&fleet, 7).await;
    assert_eq!(again["gate"]["watch_ref"], jobs[6].id.as_str(), "{again}");
    fleet.shut_down().await;
}

/// Wait, on change events of the host's Job directory (never a fixed delay), until the
/// host's Job monitor has recorded what it read for one pipeline.
async fn until_pipeline_read(
    home: &std::path::Path,
    provider: &'static str,
    slug: &'static str,
) -> hotsheet_jobs::PipelineRepository {
    use hotsheet_jobs::{ChangeWaiter, PipelineRepositories, PipelineRepository};
    let home = home.to_owned();
    tokio::task::spawn_blocking(move || {
        let waiter = ChangeWaiter::new(&home.join("jobs")).unwrap();
        let pipelines = PipelineRepositories::at(&home);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        loop {
            let known = pipelines.lookup(provider, slug);
            if known != PipelineRepository::Unknown {
                return known;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the host's monitor never read {provider} {slug}"
            );
            waiter.wait(deadline.saturating_duration_since(std::time::Instant::now()));
        }
    })
    .await
    .unwrap()
}

/// HS2-QV00AP: HS2-YA5S82 end to end. A Buildkite slug and a CircleCI `circleci/`
/// slug name no repository, so the host checks them through the repository its own
/// Job monitor read. Here that monitor is the real detached process the peer server
/// starts, reading the pipeline from the loopback provider fake with a synthetic
/// token. The first link answers the retryable `gate_repository_unknown` (503) and
/// starts nothing; once the monitor has read the pipeline, the same link is watched
/// on the host for a pipeline of a repository the checkout names, and refused
/// `gate_repository_mismatch` for one of another repository. Synthetic data only.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_qv00ap_a_hosts_monitor_process_reads_the_pipeline_repository_it_checks() {
    use hotsheet_jobs::{BUILDKITE_KIND, CIRCLECI_KIND, PipelineRepository};
    let fleet = Fleet::with_peer_monitor().await;
    git(
        &fleet.b_project,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/acme/widgets.git",
        ],
    );
    // What the provider says each pipeline builds.
    fleet.provider.serve(
        "/organizations/acme/pipelines/widgets-ci",
        json!({"slug":"widgets-ci","repository":"git@github.com:Acme/Widgets.git"}),
    );
    fleet.provider.serve(
        "/organizations/acme/pipelines/other-ci",
        json!({"slug":"other-ci","repository":"git@github.com:acme/other.git"}),
    );
    fleet.provider.serve(
        "/project/circleci/0b1c2d3e/4f5a6b7c",
        json!({"slug":"circleci/0b1c2d3e/4f5a6b7c",
            "vcs_info":{"vcs_url":"https://github.com/acme/widgets","provider":"GitHub"}}),
    );
    let orchestration = fleet.orchestration_ticket("Gate the pipeline work");
    let ticket = fleet.target_ticket(Side::Peer, "Pipeline work");
    let run = fleet
        .ok(
            "POST",
            "/runs",
            json!({"actor":Fleet::ai(),"ticket":{"native_id":orchestration.slug},
                "target_id":TARGET_B,"connection_id":fleet.connection(Side::Peer),
                "native_id":ticket.slug,"operation_id":"pipeline-run","mode":"dispatcher"}),
        )
        .await;
    let gates = format!("/runs/{}/gates", run["run_id"].as_str().unwrap());
    let commit = "f".repeat(40);
    let link = |provider: &str, slug: &str| json!({"commit":commit,"provider":provider,"repository":slug,"actor":Fleet::ai()});
    let peer_jobs = || {
        JobStore::at(&fleet.peer_home)
            .list(&fleet.b_checkout.id)
            .unwrap()
            .jobs
    };
    // The host's watch Jobs of one pipeline (an ended watch may be linked again).
    let watched = |slug: &str| -> Vec<String> {
        peer_jobs()
            .into_iter()
            .filter(|job| serde_json::to_string(&job.external).unwrap().contains(slug))
            .map(|job| job.id)
            .collect()
    };
    let unknown = |(status, reply): (StatusCode, Value)| {
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{reply}");
        assert_eq!(reply["code"], "gate_repository_unknown", "{reply}");
    };

    // Nothing known yet, and the provider is out: the first link is retryable and
    // starts the host's monitor, whose read fails transiently; asked again after that
    // failed read, still retryable (never a mismatch), and nothing is watched.
    fleet.provider.fail_with(Some(503));
    let before = fleet.provider.requests();
    unknown(
        fleet
            .call("POST", &gates, link(BUILDKITE_KIND, "acme/widgets-ci"))
            .await,
    );
    let provider = fleet.provider.clone();
    assert!(
        tokio::task::spawn_blocking(move || provider.requested_beyond(before))
            .await
            .unwrap(),
        "the host's monitor never asked the provider"
    );
    unknown(
        fleet
            .call("POST", &gates, link(BUILDKITE_KIND, "acme/widgets-ci"))
            .await,
    );
    assert!(peer_jobs().is_empty());
    assert!(
        !audit_reasons(&fleet.peer_home, &fleet.b_checkout.id)
            .iter()
            .any(|(action, _, _)| action == "hosted_gate_watch"),
        "nothing is decided, so nothing is audited"
    );
    // The provider is back: the monitor's next read (after its back-off) records it.
    fleet.provider.fail_with(None);
    assert_eq!(
        until_pipeline_read(&fleet.peer_home, BUILDKITE_KIND, "acme/widgets-ci").await,
        PipelineRepository::Known {
            repository: "git@github.com:Acme/Widgets.git".into()
        }
    );
    // Asked again: the repository is one the checkout names (any spelling), so the
    // watch starts on the host, and asking once more is the same gate.
    let linked = fleet
        .ok("POST", &gates, link(BUILDKITE_KIND, "acme/widgets-ci"))
        .await;
    assert_eq!(linked["gate"]["state"], "pending", "{linked}");
    assert_eq!(linked["gate"]["provider"], BUILDKITE_KIND);
    let watch = linked["gate"]["watch_ref"].as_str().unwrap().to_owned();
    assert!(
        watched("acme/widgets-ci").contains(&watch),
        "the watch runs on the host"
    );
    let again = fleet
        .ok("POST", &gates, link(BUILDKITE_KIND, "acme/widgets-ci"))
        .await;
    assert_eq!(
        (&again["created"], &again["gate"]["gate_id"]),
        (&json!(false), &linked["gate"]["gate_id"])
    );

    // A pipeline of another repository: unknown first, then refused once read, and the
    // host's credentials never watch it.
    unknown(
        fleet
            .call("POST", &gates, link(BUILDKITE_KIND, "acme/other-ci"))
            .await,
    );
    assert_eq!(
        until_pipeline_read(&fleet.peer_home, BUILDKITE_KIND, "acme/other-ci").await,
        PipelineRepository::Known {
            repository: "git@github.com:acme/other.git".into()
        }
    );
    let (status, refused) = fleet
        .call("POST", &gates, link(BUILDKITE_KIND, "acme/other-ci"))
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["code"], "gate_repository_mismatch");
    assert!(watched("acme/other-ci").is_empty());
    assert!(
        audit_reasons(&fleet.peer_home, &fleet.b_checkout.id).contains(&(
            "hosted_gate_watch".into(),
            AuditOutcome::Denied,
            "gate_repository_mismatch".into()
        )),
        "{:?}",
        audit_reasons(&fleet.peer_home, &fleet.b_checkout.id)
    );

    // A CircleCI project slug: the same, through CircleCI's project read.
    let circleci = "circleci/0b1c2d3e/4f5a6b7c";
    unknown(
        fleet
            .call("POST", &gates, link(CIRCLECI_KIND, circleci))
            .await,
    );
    assert_eq!(
        until_pipeline_read(&fleet.peer_home, CIRCLECI_KIND, circleci).await,
        PipelineRepository::Known {
            repository: "https://github.com/acme/widgets".into()
        }
    );
    let linked = fleet
        .ok("POST", &gates, link(CIRCLECI_KIND, circleci))
        .await;
    assert_eq!(linked["gate"]["state"], "pending", "{linked}");
    let watch = linked["gate"]["watch_ref"].as_str().unwrap().to_owned();
    assert!(watched(circleci).contains(&watch));
    assert!(watched("acme/other-ci").is_empty());
    // The monitor read with the host's token; the token is in no machine-local record.
    let record = std::fs::read_to_string(
        fleet
            .peer_home
            .join("jobs")
            .join(hotsheet_jobs::PIPELINE_REPOSITORIES_FILE),
    )
    .unwrap();
    assert!(!record.contains(super::e2e::TOKEN));
    cli_named(&fleet.peer_home, &["job", "watch"], &["stop"]);
    cli_named(&fleet.home, &["job", "watch"], &["stop"]);
}
