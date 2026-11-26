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

/// HS2-1P1HJT: a remote target's CI gate is watched on its host (HS2-WDQZ37), and the
/// host drops off the network while the gate waits. The controller's gate follower keeps
/// the gate `pending` (never a verdict, never lost work) and backs off instead of
/// spinning on the unreachable host; the verdict the host's watch reached meanwhile is
/// recorded once when the host is back. Waits on connection attempts (counted over a
/// bounded window, HS2-Q9SPRV) and on the gate event stream, never on a fixed delay.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_1p1hjt_a_hosted_ci_watch_waits_out_its_host_going_offline() {
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
    let orchestration = fleet.orchestration_ticket("Gate the remote work");
    let ticket = fleet.target_ticket(Side::Peer, "Remote work");
    let run = fleet
        .ok(
            "POST",
            "/runs",
            json!({"actor":Fleet::ai(),"ticket":{"native_id":orchestration.slug},
                "target_id":TARGET_B,"connection_id":fleet.connection(Side::Peer),
                "native_id":ticket.slug,"operation_id":"offline-gated-run","mode":"dispatcher"}),
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
