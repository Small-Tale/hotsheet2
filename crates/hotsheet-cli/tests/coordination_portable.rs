//! Native Windows/Unix CLI recovery and handoff transitions (HS2-9TD8Q5).
#[path = "support/coordination.rs"]
mod support;

use hotsheet_cli::coordination::Phase;
use support::{Fixture, git};

#[test]
fn hs2_h0mf53_real_cli_parsing_and_coordination_fit_a_small_entry_stack() {
    let f = Fixture::new();
    let run = |args: &[&str]| {
        let command = f.command(args);
        #[cfg(unix)]
        let mut command = {
            // Start the limit setter on its own main stack: macOS rejects
            // lowering RLIMIT_STACK after a fork from the test's worker thread.
            // Arguments remain positional data, never interpolated shell code.
            let mut limited = std::process::Command::new("/bin/sh");
            limited
                .args(["-c", "ulimit -s 1024 && exec \"$@\"", "hs2-h0mf53"])
                .arg(command.get_program())
                .args(command.get_args());
            for (key, value) in command.get_envs() {
                if let Some(value) = value {
                    limited.env(key, value);
                } else {
                    limited.env_remove(key);
                }
            }
            limited
        };
        #[cfg(not(unix))]
        let mut command = command;
        command.output().unwrap()
    };
    let help = run(&["--help"]);
    assert!(
        help.status.success(),
        "{}",
        String::from_utf8_lossy(&help.stderr)
    );
    assert!(String::from_utf8_lossy(&help.stdout).contains("coordination"));
    let invalid = run(&["--not-a-real-option"]);
    assert_eq!(invalid.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("unexpected argument"));
    for action in ["setup", "status"] {
        let args = f.coord(&[action]);
        let output = run(&args.iter().map(String::as_str).collect::<Vec<_>>());
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let snapshot: hotsheet_cli::coordination::Snapshot =
            serde_json::from_slice(&output.stdout).unwrap();
        assert!(snapshot.state.assignments.is_empty());
    }
}

#[test]
fn hs2_9td8q5_native_wait_death_reconnect_handoff_and_refill() {
    let f = Fixture::new();
    let first = f.ticket();
    let second = f.ticket();
    let initial = f.run(&[
        "register",
        "--role",
        "integrator",
        "--id",
        "integrator",
        "--session",
        "root-one",
    ]);
    let mut integrator = f.waiting(&[
        "next",
        "--role",
        "integrator",
        "--id",
        "integrator",
        "--session",
        "root-one",
        "--after",
        &initial.cursor,
    ]);
    let worker_args = [
        "next",
        "--role",
        "worker",
        "--id",
        "worker",
        "--session",
        "worker-session",
    ];
    let mut worker = f.waiting(&worker_args);
    assert_eq!(integrator.result().live_workers, ["worker"]);
    let duplicate = f.coord(&worker_args);
    let rejected = f
        .command(&duplicate.iter().map(String::as_str).collect::<Vec<_>>())
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("another active wait"));
    // Abrupt native process termination releases the wait lock on Windows too.
    worker.child.kill().unwrap();
    worker.child.wait().unwrap();
    let mut reconnected = f.waiting(&worker_args);
    let base = git(&f.project, &["rev-parse", "HEAD"]);
    f.assign("first-offer", &first, &base, "root-one");
    let offered = reconnected.result();
    assert_eq!(
        offered.state.active_assignment("worker").unwrap().id,
        "first-offer"
    );
    f.cli(&["claim", &first.slug, "--worker", "worker", "--eta", "10m"]);
    f.run(&[
        "worker",
        "--id",
        "worker",
        "--session",
        "worker-session",
        "--assignment",
        "first-offer",
        "--phase",
        "accepted",
    ]);
    let checkout = f.root.path().join("worker-ticket");
    git(
        &f.project,
        &[
            "worktree",
            "add",
            "-qb",
            "worker-ticket",
            checkout.to_str().unwrap(),
            &base,
        ],
    );
    std::fs::write(
        checkout.join("synthetic.txt"),
        "portable synthetic change\n",
    )
    .unwrap();
    git(&checkout, &["add", "synthetic.txt"]);
    git(
        &checkout,
        &["commit", "-qm", "HS-SYNTHETIC Verify portable handoff"],
    );
    let commit = git(&checkout, &["rev-parse", "HEAD"]);
    f.run(&[
        "leave",
        "--role",
        "integrator",
        "--id",
        "integrator",
        "--session",
        "root-one",
    ]);
    f.cli(&[
        "edit",
        &first.slug,
        "--note",
        &format!("## Integration handoff\nVerified synthetic commit {commit}"),
    ]);
    f.cli(&["release", &first.slug, "--worker", "worker"]);
    f.run(&[
        "worker",
        "--id",
        "worker",
        "--session",
        "worker-session",
        "--assignment",
        "first-offer",
        "--phase",
        "handoff",
        "--commit",
        &commit,
    ]);
    let mut available = f.waiting(&worker_args);
    let resumed = f.run(&[
        "register",
        "--role",
        "integrator",
        "--id",
        "integrator",
        "--session",
        "root-two",
    ]);
    assert_eq!(
        resumed.state.assignments["first-offer"].commit.as_deref(),
        Some(commit.as_str())
    );
    f.run(&[
        "mark",
        "--id",
        "integrator",
        "--session",
        "root-two",
        "--assignment",
        "first-offer",
        "--phase",
        "integrating",
    ]);
    git(&f.project, &["cherry-pick", &commit]);
    f.run(&[
        "mark",
        "--id",
        "integrator",
        "--session",
        "root-two",
        "--assignment",
        "first-offer",
        "--phase",
        "ci",
    ]);
    let shipping = git(&f.project, &["rev-parse", "HEAD"]);
    f.assign("second-offer", &second, &shipping, "root-two");
    let refill = available.result();
    assert_eq!(refill.state.assignments["first-offer"].phase, Phase::Ci);
    assert_eq!(
        refill.state.active_assignment("worker").unwrap().base,
        shipping
    );
    assert!(!f.cli(&["show", &first.slug]).contains("claimed_by:"));
}
