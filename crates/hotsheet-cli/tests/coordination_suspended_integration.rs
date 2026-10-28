//! HS2-2D3099: real CLI processes preserve an unpushed integration through an
//! independent failed gate, then reacquire the serial main-mutation fence.

#[allow(dead_code)] // HS2-2D3099: shared process fixture also provides unused waiter helpers.
#[path = "support/coordination.rs"]
mod support;

use std::fs;

use hotsheet_cli::coordination::{Phase, TransitionKind};
use hotsheet_model::NoteKind;
use hotsheet_ticketing::git_connection_id;
use serde_json::json;
use support::{Fixture, git};

fn command_fails(f: &Fixture, args: &[&str]) -> bool {
    let args = f.coord(args);
    !f.command(&args.iter().map(String::as_str).collect::<Vec<_>>())
        .output()
        .unwrap()
        .status
        .success()
}

fn addressed_note(
    f: &Fixture,
    ticket: &hotsheet_model::Ticket,
    heading: &str,
    body: String,
) -> String {
    f.cli(&[
        "edit",
        &ticket.slug,
        "--note",
        &format!("{heading}\n{body}"),
        "--actor-role",
        "ai",
        "--actor-id",
        "integrator",
    ]);
    f.store
        .read_ticket(&ticket.id)
        .unwrap()
        .notes
        .iter()
        .rev()
        .find(|note| note.kind == NoteKind::Regular && note.text.starts_with(heading))
        .unwrap()
        .id
        .to_string()
}

fn code_handoff(
    f: &Fixture,
    ticket: &hotsheet_model::Ticket,
    operation: &str,
    base: &str,
    commit: &str,
) {
    f.assign(operation, ticket, base, "root");
    f.cli(&["claim", &ticket.slug, "--worker", "worker", "--eta", "10m"]);
    f.run(&[
        "worker",
        "--id",
        "worker",
        "--session",
        "worker-session",
        "--assignment",
        operation,
        "--phase",
        "accepted",
    ]);
    f.cli(&["release", &ticket.slug, "--worker", "worker"]);
    f.cli(&[
        "edit",
        &ticket.slug,
        "--note",
        &format!("## Integration handoff\nVerified synthetic commit {commit}"),
    ]);
    f.run(&[
        "worker",
        "--id",
        "worker",
        "--session",
        "worker-session",
        "--assignment",
        operation,
        "--phase",
        "handoff",
        "--commit",
        commit,
    ]);
}

#[test]
fn hs2_2d3099_suspended_integration_preserves_evidence_and_reacquires_the_fence() {
    let f = Fixture::new();
    let source = f.ticket();
    let prerequisite = f.ticket();
    let second_prerequisite = f.ticket();
    f.run(&[
        "register",
        "--role",
        "integrator",
        "--id",
        "integrator",
        "--session",
        "root",
    ]);
    f.run(&[
        "register",
        "--role",
        "worker",
        "--id",
        "worker",
        "--session",
        "worker-session",
    ]);
    let base = git(&f.project, &["rev-parse", "HEAD"]);
    let remote = f.root.path().join("remote.git");
    git(
        &f.project,
        &["init", "-q", "--bare", remote.to_str().unwrap()],
    );
    git(
        &f.project,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&f.project, &["push", "-q", "-u", "origin", "main"]);
    let source_worktree = f.root.path().join("source-worktree");
    git(
        &f.project,
        &[
            "worktree",
            "add",
            "-qb",
            "source-work",
            source_worktree.to_str().unwrap(),
            &base,
        ],
    );
    fs::write(source_worktree.join("synthetic.txt"), "source change\n").unwrap();
    git(&source_worktree, &["add", "synthetic.txt"]);
    git(
        &source_worktree,
        &["commit", "-qm", "HS-SYNTHETIC Source change"],
    );
    let source_commit = git(&source_worktree, &["rev-parse", "HEAD"]);
    code_handoff(&f, &source, "source-op", &base, &source_commit);
    f.run(&[
        "mark",
        "--id",
        "integrator",
        "--session",
        "root",
        "--assignment",
        "source-op",
        "--phase",
        "integrating",
    ]);
    git(&f.project, &["cherry-pick", &source_commit]);
    let integrated_main = git(&f.project, &["rev-parse", "HEAD"]);
    assert_ne!(
        git(&f.project, &["rev-parse", "origin/main"]),
        integrated_main
    );
    let connection = git_connection_id(&f.store);
    let prerequisite_ref = format!("{connection}:{}", prerequisite.id);
    let failure_note = addressed_note(
        &f,
        &source,
        "## Integration prerequisite",
        json!({
            "operation": "source-op",
            "source_commit": source_commit,
            "main_commit": integrated_main,
            "prerequisite": prerequisite_ref,
        })
        .to_string(),
    );
    let suspend = [
        "suspend-integration",
        "--id",
        "integrator",
        "--session",
        "root",
        "--assignment",
        "source-op",
        "--main-commit",
        &integrated_main,
        "--prerequisite-connection",
        &connection,
        "--prerequisite-native-id",
        &prerequisite.id.to_string(),
        "--failure-note",
        &failure_note,
    ];
    assert!(
        command_fails(&f, &suspend),
        "schema-six rollout must be explicit"
    );
    let mut upgraded = suspend.to_vec();
    upgraded.push("--all-participants-upgraded");
    let parked = f.run(&upgraded);
    assert_eq!(parked.state.version, 6);
    assert_eq!(
        parked.state.assignments["source-op"].phase,
        Phase::SuspendedIntegration
    );
    assert_eq!(
        parked.state.assignments["source-op"].commit.as_deref(),
        Some(source_commit.as_str())
    );
    assert_eq!(
        parked.state.assignments["source-op"]
            .suspension
            .as_ref()
            .unwrap()
            .main_commit,
        integrated_main
    );
    assert!(
        command_fails(
            &f,
            &[
                "mark",
                "--id",
                "integrator",
                "--session",
                "root",
                "--assignment",
                "source-op",
                "--phase",
                "ci",
            ]
        ),
        "an unpushed suspension cannot be called CI"
    );

    let prerequisite_worktree = f.root.path().join("prerequisite-worktree");
    git(
        &f.project,
        &[
            "worktree",
            "add",
            "-qb",
            "prerequisite-work",
            prerequisite_worktree.to_str().unwrap(),
            &integrated_main,
        ],
    );
    fs::write(
        prerequisite_worktree.join("prerequisite.txt"),
        "synthetic fix\n",
    )
    .unwrap();
    git(&prerequisite_worktree, &["add", "prerequisite.txt"]);
    git(
        &prerequisite_worktree,
        &["commit", "-qm", "HS-SYNTHETIC Fix independent gate"],
    );
    let prerequisite_commit = git(&prerequisite_worktree, &["rev-parse", "HEAD"]);
    code_handoff(
        &f,
        &prerequisite,
        "prerequisite-op",
        &integrated_main,
        &prerequisite_commit,
    );
    f.run(&[
        "mark",
        "--id",
        "integrator",
        "--session",
        "root",
        "--assignment",
        "prerequisite-op",
        "--phase",
        "integrating",
    ]);
    git(&f.project, &["cherry-pick", &prerequisite_commit]);
    let gated_main = git(&f.project, &["rev-parse", "HEAD"]);
    let gated_tree = git(&f.project, &["rev-parse", "HEAD^{tree}"]);
    let gate_note = addressed_note(
        &f,
        &prerequisite,
        "## Prerequisite gate",
        json!({
            "operation": "source-op",
            "source_commit": source_commit,
            "main_commit": gated_main,
            "prerequisite": prerequisite_ref,
            "tree": gated_tree,
        })
        .to_string(),
    );
    let resume = [
        "resume-integration",
        "--id",
        "integrator",
        "--session",
        "root",
        "--assignment",
        "source-op",
        "--main-commit",
        &gated_main,
        "--gate-tree",
        &gated_tree,
        "--evidence-note",
        &gate_note,
    ];
    assert!(
        command_fails(&f, &resume),
        "a missing gate receipt cannot resume"
    );
    let cache = f.project.join(".git/hotsheet-root-gate");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join(&gated_tree), format!("{gated_tree}\n")).unwrap();
    assert!(
        command_fails(&f, &resume),
        "another integration still owns the main fence"
    );
    assert!(
        command_fails(
            &f,
            &[
                "mark",
                "--id",
                "integrator",
                "--session",
                "root",
                "--assignment",
                "prerequisite-op",
                "--phase",
                "ci",
            ]
        ),
        "the prerequisite cannot enter CI before push"
    );
    git(&f.project, &["push", "-q", "origin", "main"]);
    assert_eq!(git(&f.project, &["rev-parse", "origin/main"]), gated_main);
    f.run(&[
        "mark",
        "--id",
        "integrator",
        "--session",
        "root",
        "--assignment",
        "prerequisite-op",
        "--phase",
        "ci",
    ]);
    let resumed = f.run(&resume);
    assert_eq!(
        resumed.state.assignments["source-op"].phase,
        Phase::Integrating
    );
    let evidence = resumed.state.assignments["source-op"]
        .suspension
        .as_ref()
        .unwrap();
    assert_eq!(evidence.prerequisite.qualified(), prerequisite_ref);
    assert_eq!(evidence.failure_note, failure_note);
    assert_eq!(evidence.resumed.as_ref().unwrap().main_commit, gated_main);
    assert_eq!(evidence.resumed.as_ref().unwrap().tree, gated_tree);
    assert!(
        command_fails(
            &f,
            &[
                "mark",
                "--id",
                "integrator",
                "--session",
                "root",
                "--assignment",
                "prerequisite-op",
                "--phase",
                "integrating",
            ]
        ),
        "resumed integration must exclusively own the main fence"
    );
    let replayed = f.run(&resume);
    assert_eq!(
        replayed.state.assignments["source-op"].transitions.len(),
        resumed.state.assignments["source-op"].transitions.len()
    );
    assert_eq!(
        replayed.state.assignments["source-op"]
            .transitions
            .iter()
            .map(|event| event.kind)
            .collect::<Vec<_>>(),
        [
            TransitionKind::Offered,
            TransitionKind::Accepted,
            TransitionKind::Handoff,
            TransitionKind::Integrating,
            TransitionKind::SuspendedIntegration,
            TransitionKind::Integrating
        ],
    );
    fs::write(
        f.project.join("source-followup.txt"),
        "additional source fix\n",
    )
    .unwrap();
    git(&f.project, &["add", "source-followup.txt"]);
    git(
        &f.project,
        &["commit", "-qm", "HS-SYNTHETIC Additional source fix"],
    );
    let second_unpushed_main = git(&f.project, &["rev-parse", "HEAD"]);
    assert_ne!(
        git(&f.project, &["rev-parse", "origin/main"]),
        second_unpushed_main
    );
    let second_prerequisite_ref = format!("{connection}:{}", second_prerequisite.id);
    let second_failure_note = addressed_note(
        &f,
        &source,
        "## Integration prerequisite",
        json!({
            "operation": "source-op",
            "source_commit": source_commit,
            "main_commit": second_unpushed_main,
            "prerequisite": second_prerequisite_ref,
        })
        .to_string(),
    );
    let second_parked = f.run(&[
        "suspend-integration",
        "--id",
        "integrator",
        "--session",
        "root",
        "--assignment",
        "source-op",
        "--main-commit",
        &second_unpushed_main,
        "--prerequisite-connection",
        &connection,
        "--prerequisite-native-id",
        &second_prerequisite.id.to_string(),
        "--failure-note",
        &second_failure_note,
    ]);
    let original = &second_parked.state.assignments["source-op"];
    assert_eq!(original.phase, Phase::SuspendedIntegration);
    assert_eq!(original.suspension_history.len(), 1);
    assert_eq!(original.suspension_history[0].failure_note, failure_note);
    assert_eq!(
        original.suspension_history[0].prerequisite.qualified(),
        prerequisite_ref
    );
    assert_eq!(
        original.suspension_history[0]
            .resumed
            .as_ref()
            .unwrap()
            .tree,
        gated_tree
    );
    assert_eq!(
        original.suspension.as_ref().unwrap().failure_note,
        second_failure_note
    );

    let second_worktree = f.root.path().join("second-prerequisite-worktree");
    git(
        &f.project,
        &[
            "worktree",
            "add",
            "-qb",
            "second-prerequisite-work",
            second_worktree.to_str().unwrap(),
            &second_unpushed_main,
        ],
    );
    fs::write(
        second_worktree.join("second-prerequisite.txt"),
        "second gate fix\n",
    )
    .unwrap();
    git(&second_worktree, &["add", "second-prerequisite.txt"]);
    git(
        &second_worktree,
        &["commit", "-qm", "HS-SYNTHETIC Second gate fix"],
    );
    let second_prerequisite_commit = git(&second_worktree, &["rev-parse", "HEAD"]);
    code_handoff(
        &f,
        &second_prerequisite,
        "second-prerequisite-op",
        &second_unpushed_main,
        &second_prerequisite_commit,
    );
    f.run(&[
        "mark",
        "--id",
        "integrator",
        "--session",
        "root",
        "--assignment",
        "second-prerequisite-op",
        "--phase",
        "integrating",
    ]);
    git(&f.project, &["cherry-pick", &second_prerequisite_commit]);
    let second_gated_main = git(&f.project, &["rev-parse", "HEAD"]);
    let second_gated_tree = git(&f.project, &["rev-parse", "HEAD^{tree}"]);
    let second_gate_note = addressed_note(
        &f,
        &second_prerequisite,
        "## Prerequisite gate",
        json!({
            "operation": "source-op",
            "source_commit": source_commit,
            "main_commit": second_gated_main,
            "prerequisite": second_prerequisite_ref,
            "tree": second_gated_tree,
        })
        .to_string(),
    );
    fs::write(
        cache.join(&second_gated_tree),
        format!("{second_gated_tree}\n"),
    )
    .unwrap();
    assert!(
        command_fails(
            &f,
            &[
                "mark",
                "--id",
                "integrator",
                "--session",
                "root",
                "--assignment",
                "second-prerequisite-op",
                "--phase",
                "ci",
            ]
        ),
        "a second prerequisite cannot enter CI before push"
    );
    git(&f.project, &["push", "-q", "origin", "main"]);
    f.run(&[
        "mark",
        "--id",
        "integrator",
        "--session",
        "root",
        "--assignment",
        "second-prerequisite-op",
        "--phase",
        "ci",
    ]);
    let second_resume = [
        "resume-integration",
        "--id",
        "integrator",
        "--session",
        "root",
        "--assignment",
        "source-op",
        "--main-commit",
        &second_gated_main,
        "--gate-tree",
        &second_gated_tree,
        "--evidence-note",
        &second_gate_note,
    ];
    let twice_resumed = f.run(&second_resume);
    let original = &twice_resumed.state.assignments["source-op"];
    assert_eq!(original.phase, Phase::Integrating);
    assert_eq!(original.suspension_history[0].failure_note, failure_note);
    assert_eq!(
        original.suspension_history[0]
            .resumed
            .as_ref()
            .unwrap()
            .tree,
        gated_tree
    );
    assert_eq!(
        original.suspension.as_ref().unwrap().failure_note,
        second_failure_note
    );
    assert_eq!(
        original
            .suspension
            .as_ref()
            .unwrap()
            .resumed
            .as_ref()
            .unwrap()
            .tree,
        second_gated_tree
    );
    let replayed = f.run(&second_resume);
    assert_eq!(
        replayed.state.assignments["source-op"]
            .suspension_history
            .len(),
        1
    );
    assert_eq!(
        replayed.state.assignments["source-op"].transitions.len(),
        original.transitions.len()
    );
    let ci = f.run(&[
        "mark",
        "--id",
        "integrator",
        "--session",
        "root",
        "--assignment",
        "source-op",
        "--phase",
        "ci",
    ]);
    assert_eq!(ci.state.assignments["source-op"].phase, Phase::Ci);
    fs::write(f.project.join("after-ci.txt"), "unrelated next work\n").unwrap();
    git(&f.project, &["add", "after-ci.txt"]);
    git(&f.project, &["commit", "-qm", "HS-SYNTHETIC Later work"]);
    let ci_replay = f.run(&[
        "mark",
        "--id",
        "integrator",
        "--session",
        "root",
        "--assignment",
        "source-op",
        "--phase",
        "ci",
    ]);
    assert_eq!(
        ci_replay.state.assignments["source-op"].transitions.len(),
        ci.state.assignments["source-op"].transitions.len()
    );
}
