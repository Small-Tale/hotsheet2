//! HS2-G3VAK4: an authorized deployment gate runs the target's own checked-in deploy
//! script on the machine that hosts the target, at the gate's commit, once, and reports
//! typed evidence. Synthetic repositories and scripts only.
use std::os::unix::fs::PermissionsExt;
use std::time::{Duration, Instant};

use hotsheet_ticketing::orchestration::{Coverage, GateKind, GateState, TargetAction};
use serde_json::json;

use super::gates::{CI, Scripted, deploy, judge, link, sha, verdict, with_run};
use super::hosted_gates::{git_remote, remote};
use super::tickets::worker;
use super::*;

fn git(root: &Path, args: &[&str]) -> String {
    let output = hotsheet_ticketing::git::command_in(root)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}");
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

/// Commit `script` at `scripts/deploy.sh` (executable) and return the commit.
fn commit_script(root: &Path, script: &str, exit: i32, log: &Path) -> String {
    let path = root.join("scripts/deploy.sh");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        format!(
            "#!/bin/sh\n{script} \"$1\" \"$(pwd)\" >> '{}'\nexit {exit}\n",
            log.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    git(root, &["add", "scripts/deploy.sh"]);
    git(
        root,
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
    git(root, &["rev-parse", "HEAD"])
}

fn lines(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .map(|text| text.lines().map(str::to_owned).collect())
        .unwrap_or_default()
}

#[test]
fn hs2_g3vak4_the_targets_tracked_script_runs_once_at_the_gate_commit() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let (mut m, run) = with_run(
        &[TargetAction::GatesObserve, TargetAction::DeploymentsRun],
        &mut tx,
    );
    let ci = Scripted::new(CI, GateKind::Ci);
    m.runtime = m.runtime.clone().with_gate_provider(ci.clone());
    let owner = m.owner.id.clone();
    let root = Path::new(&m.target.root).to_path_buf();
    let log = m.home.join("deploys.log");
    git(&root, &["init", "-q"]);
    let first = commit_script(&root, "echo v1", 0, &log);
    // The working tree changes after the commit; only the commit's script may run.
    std::fs::write(
        root.join("scripts/deploy.sh"),
        format!("#!/bin/sh\necho working-tree >> '{}'\n", log.display()),
    )
    .unwrap();
    let pass = |commit: &str, order: u64, tx: &mut Transaction<'_>| {
        m.runtime
            .gate_link(&owner, &run, link(commit, CI), Some(&worker()), tx)
            .unwrap();
        ci.set(
            commit,
            verdict(CI, GateState::Passed, order, commit, Coverage::Exact),
        );
        m.runtime
            .gate_sync(&owner, &run, &SyncGates::default(), Some(&worker()), tx)
            .unwrap();
    };
    let until_settled = |gate_id: &str, tx: &mut Transaction<'_>| {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            m.runtime
                .gate_sync(
                    &owner,
                    &run,
                    &SyncGates { wait_ms: 8_000 },
                    Some(&worker()),
                    tx,
                )
                .unwrap();
            let gate = m.runtime.gate(&owner, gate_id).unwrap();
            if gate.state != GateState::Pending {
                return gate;
            }
            assert!(Instant::now() < deadline, "the deployment never ended");
        }
    };
    pass(&first, 1, &mut tx);
    let started = m
        .runtime
        .gate_deploy(
            &owner,
            &run,
            deploy("deploy-1", false),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert!(started.created);
    let passed = until_settled(&started.gate.gate_id, &mut tx);
    assert_eq!(passed.state, GateState::Passed, "{passed:?}");
    let evidence = passed.evidence.unwrap();
    assert_eq!(
        (
            evidence.provider.as_str(),
            evidence.reference.as_str(),
            evidence.commit.as_str(),
            evidence.coverage,
            evidence.exit_code
        ),
        (
            "target-script",
            started.gate.gate_id.as_str(),
            first.as_str(),
            Coverage::Exact,
            Some(0)
        )
    );
    assert!(evidence.duration_ms.is_some());
    // It ran the committed script, in a worktree of that commit, with the environment
    // as its argument; the worktree is gone afterwards.
    let ran = lines(&log);
    assert_eq!(ran.len(), 1, "{ran:?}");
    assert!(ran[0].starts_with("v1 beta "), "{ran:?}");
    let worktree = m
        .home
        .join(DEPLOYMENT_WORKTREES_DIRECTORY)
        .join(&m.target.id)
        .join(&started.gate.gate_id);
    assert!(ran[0].ends_with(&worktree.display().to_string()));
    assert!(!worktree.exists());
    // The same request again runs nothing.
    m.runtime
        .gate_deploy(
            &owner,
            &run,
            deploy("deploy-1", false),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(lines(&log).len(), 1);

    // A script that exits non-zero fails the gate with its exit code.
    git(&root, &["checkout", "-q", "--", "scripts/deploy.sh"]);
    let second = commit_script(&root, "echo v2", 3, &log);
    pass(&second, 2, &mut tx);
    let failing = m
        .runtime
        .gate_deploy(
            &owner,
            &run,
            deploy("deploy-2", false),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    let failed = until_settled(&failing.gate.gate_id, &mut tx);
    assert_eq!(failed.state, GateState::Failed, "{failed:?}");
    assert_eq!(failed.evidence.unwrap().exit_code, Some(3));

    // A script the commit does not track as an executable never runs.
    git(&root, &["rm", "-q", "scripts/deploy.sh"]);
    std::fs::write(root.join("plain.sh"), "#!/bin/sh\nexit 0\n").unwrap();
    git(&root, &["add", "plain.sh"]);
    git(
        &root,
        &[
            "-c",
            "user.name=Synthetic",
            "-c",
            "user.email=synthetic@example.invalid",
            "commit",
            "-q",
            "-m",
            "no deploy script",
        ],
    );
    let third = git(&root, &["rev-parse", "HEAD"]);
    pass(&third, 3, &mut tx);
    for (operation, script) in [("deploy-3", "scripts/deploy.sh"), ("deploy-4", "plain.sh")] {
        let refused = m
            .runtime
            .gate_deploy(
                &owner,
                &run,
                serde_json::from_value(json!({"environment": "beta", "script": script,
                    "operation_id": operation}))
                .unwrap(),
                Some(&worker()),
                &mut tx,
            )
            .unwrap();
        assert_eq!(
            (refused.gate.state, refused.gate.reason.as_deref()),
            (GateState::Unknown, Some("gate_deployment_script_untracked")),
            "{script}"
        );
    }
    assert_eq!(lines(&log).len(), 2, "only the two tracked scripts ran");
}

#[test]
fn hs2_g3vak4_a_remote_targets_script_runs_on_its_host_not_here() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let r = remote(&mut tx);
    allow_on(
        &r.runtime,
        &r.owner,
        &id(1),
        &[TargetAction::DeploymentsRun],
        &[],
        &mut tx,
    );
    let host_root = Path::new(&r.peer.hosted.target.root).to_path_buf();
    git_remote(&host_root, "https://github.com/example/widgets.git");
    let log = r.peer.hosted.home.join("deploys.log");
    let commit = commit_script(&host_root, "echo remote", 0, &log);
    // CI passes on the host (HS2-WDQZ37), then the deployment runs there.
    let linked = r
        .runtime
        .gate_link(
            &r.owner,
            &r.run,
            serde_json::from_value(json!({"commit": commit})).unwrap(),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    judge(
        &r.peer.hosted.home,
        &r.peer.hosted.target.id,
        linked.gate.watch_ref.as_deref().unwrap(),
        "green",
        &commit,
        81,
    );
    super::hosted_gates::sync(&r, 0, &mut tx);
    let started = r
        .runtime
        .gate_deploy(
            &r.owner,
            &r.run,
            deploy("deploy-remote", false),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    let gate = loop {
        super::hosted_gates::sync(&r, 8_000, &mut tx);
        let gate = r.runtime.gate(&r.owner, &started.gate.gate_id).unwrap();
        if gate.state != GateState::Pending {
            break gate;
        }
        assert!(Instant::now() < deadline, "the deployment never ended");
    };
    assert_eq!(gate.state, GateState::Passed, "{gate:?}");
    assert_eq!(lines(&log).len(), 1);
    assert!(
        r.calls("/target-deployments")
            .iter()
            .any(|(method, _)| method == "POST"),
        "it was sent to the host"
    );
    // Nothing ran on, or was checked out on, the orchestrating machine.
    assert!(!r.runtime.home.join(DEPLOYMENT_WORKTREES_DIRECTORY).exists());
    let _ = sha('a');
}

#[test]
fn hs2_wz0nkh_reconciliation_asks_the_host_about_a_hosted_deployment() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let (mut m, run) = with_run(
        &[TargetAction::GatesObserve, TargetAction::DeploymentsRun],
        &mut tx,
    );
    let ci = Scripted::new(CI, GateKind::Ci);
    m.runtime = m.runtime.clone().with_gate_provider(ci.clone());
    let owner = m.owner.id.clone();
    let root = Path::new(&m.target.root).to_path_buf();
    let log = m.home.join("deploys.log");
    let release = m.home.join("release");
    git(&root, &["init", "-q"]);
    // A deployment that runs until the test releases it.
    let commit = commit_script(
        &root,
        &format!(
            "while [ ! -f '{}' ]; do sleep 0.05; done; echo held",
            release.display()
        ),
        0,
        &log,
    );
    m.runtime
        .gate_link(&owner, &run, link(&commit, CI), Some(&worker()), &mut tx)
        .unwrap();
    ci.set(
        &commit,
        verdict(CI, GateState::Passed, 1, &commit, Coverage::Exact),
    );
    m.runtime
        .gate_sync(
            &owner,
            &run,
            &SyncGates::default(),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    let started = m
        .runtime
        .gate_deploy(
            &owner,
            &run,
            deploy("deploy-held", false),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(started.gate.state, GateState::Pending);
    // While it runs, reconciliation finds it live on the host, by its operation id.
    let live = m
        .runtime
        .run_deployment_work(&owner, &run, Some(&worker()), &mut tx);
    assert!(
        matches!(&live, hotsheet_ticketing::orchestration::Observed::Verified(work)
            if work.live.as_deref() == Some("deploy-held")),
        "{live:?}"
    );
    // It ends; the next reconciliation records the host's answer on the gate.
    std::fs::write(&release, "").unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let answer = m
            .runtime
            .run_deployment_work(&owner, &run, Some(&worker()), &mut tx);
        let gate = m.runtime.gate(&owner, &started.gate.gate_id).unwrap();
        if gate.state == GateState::Passed {
            assert!(
                matches!(&answer, hotsheet_ticketing::orchestration::Observed::Verified(work)
                    if work.live.is_none()),
                "{answer:?}"
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the deployment never ended: {gate:?}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    // Asked under the original decision as a continuation: nobody was asked again.
    assert!(
        m.runtime
            .permission_list(&owner, &mut tx)
            .unwrap()
            .requests
            .is_empty()
    );
}
