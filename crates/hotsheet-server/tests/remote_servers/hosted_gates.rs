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
/// recorded once when the host is back. Waits on connection attempts and on the gate
/// event stream, never on a fixed delay.
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
    fleet.peer_offline().await;
    let refusing = tokio::net::TcpListener::bind(fleet.peer_address)
        .await
        .unwrap();
    let started = std::time::Instant::now();
    for _ in 0..3 {
        let (connection, _) =
            tokio::time::timeout(std::time::Duration::from_secs(60), refusing.accept())
                .await
                .expect("the controller stopped trying the host")
                .unwrap();
        drop(connection);
    }
    let elapsed = started.elapsed();
    assert!(
        elapsed >= std::time::Duration::from_millis(500),
        "three connection attempts in {elapsed:?}: the controller spins on an unreachable host"
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
