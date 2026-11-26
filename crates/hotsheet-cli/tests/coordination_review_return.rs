//! HS2-65ETRJ: real CLI correction loop, preserved worktrees and actual Git integration.
#[path = "support/coordination.rs"]
mod support;

use hotsheet_cli::coordination::Phase;
use hotsheet_model::{NoteKind, Timestamp, Ulid};
use hotsheet_ticketing::ops;
use support::{Fixture, git};

#[test]
fn hs2_65etrj_review_return_corrected_handoff_and_single_actual_integration() {
    let f = Fixture::new();
    let t = f.ticket();
    for (role, id, session) in [
        ("integrator", "integrator", "root"),
        ("worker", "worker", "worker-session"),
    ] {
        f.run(&["register", "--role", role, "--id", id, "--session", session]);
    }
    let base = git(&f.project, &["rev-parse", "HEAD"]);
    f.assign("original", &t, &base, "root");
    f.cli(&["claim", &t.slug, "--worker", "worker", "--eta", "10m"]);
    f.run(&[
        "worker",
        "--id",
        "worker",
        "--session",
        "worker-session",
        "--assignment",
        "original",
        "--phase",
        "accepted",
    ]);
    let old = f.root.path().join("original");
    git(
        &f.project,
        &[
            "worktree",
            "add",
            "-qb",
            "original",
            old.to_str().unwrap(),
            &base,
        ],
    );
    std::fs::write(old.join("synthetic.txt"), "reviewed synthetic behavior\n").unwrap();
    git(&old, &["add", "synthetic.txt"]);
    git(
        &old,
        &["commit", "-qm", "HS-SYNTHETIC Add reviewed behavior"],
    );
    let original = git(&old, &["rev-parse", "HEAD"]);
    f.cli(&[
        "edit",
        &t.slug,
        "--note",
        &format!("## Integration handoff\n{original}"),
    ]);
    f.cli(&["release", &t.slug, "--worker", "worker"]);
    f.run(&[
        "worker",
        "--id",
        "worker",
        "--session",
        "worker-session",
        "--assignment",
        "original",
        "--phase",
        "handoff",
        "--commit",
        &original,
    ]);
    let review = Ulid::new();
    ops::add_note(&f.store, &t.id, review, Timestamp::from_datetime(time::OffsetDateTime::now_utc()), NoteKind::Regular,
        format!("## Review return\noriginal {original}\nPreserve the original worktree; correct synthetic behavior.")).unwrap();
    let returned = f.run(&[
        "review-return",
        "--id",
        "integrator",
        "--session",
        "root",
        "--assignment",
        "original",
        "--evidence-note",
        &review.to_string(),
        "--inspected",
    ]);
    assert_eq!(returned.state.version, 3);
    assert_eq!(returned.state.assignments["original"].phase, Phase::Blocked);
    assert_eq!(
        returned.state.assignments["original"].commit.as_deref(),
        Some(original.as_str())
    );
    assert_eq!(git(&old, &["rev-parse", "HEAD"]), original);
    let mut waiting = f.waiting(&[
        "next",
        "--role",
        "worker",
        "--id",
        "worker",
        "--session",
        "worker-session",
    ]);
    f.assign("correction", &t, &base, "root");
    let offered = waiting.result();
    assert_eq!(
        offered.state.active_assignment("worker").unwrap().id,
        "correction"
    );
    f.assign("correction", &t, &base, "root");
    f.cli(&["claim", &t.slug, "--worker", "worker", "--eta", "10m"]);
    // Lost return acknowledgement can replay after the new reservation and claim.
    f.run(&[
        "review-return",
        "--id",
        "integrator",
        "--session",
        "root",
        "--assignment",
        "original",
        "--evidence-note",
        &review.to_string(),
        "--inspected",
    ]);
    f.run(&[
        "worker",
        "--id",
        "worker",
        "--session",
        "worker-session",
        "--assignment",
        "correction",
        "--phase",
        "accepted",
    ]);
    let new = f.root.path().join("correction");
    git(
        &f.project,
        &[
            "worktree",
            "add",
            "-qb",
            "correction",
            new.to_str().unwrap(),
            &base,
        ],
    );
    git(&new, &["cherry-pick", "--no-commit", &original]);
    std::fs::write(new.join("synthetic.txt"), "corrected synthetic behavior\n").unwrap();
    git(&new, &["add", "synthetic.txt"]);
    git(
        &new,
        &["commit", "-qm", "HS-SYNTHETIC Correct reviewed behavior"],
    );
    let corrected = git(&new, &["rev-parse", "HEAD"]);
    f.cli(&[
        "edit",
        &t.slug,
        "--note",
        &format!("## Integration handoff\n{corrected}"),
    ]);
    f.cli(&["release", &t.slug, "--worker", "worker"]);
    f.run(&[
        "worker",
        "--id",
        "worker",
        "--session",
        "worker-session",
        "--assignment",
        "correction",
        "--phase",
        "handoff",
        "--commit",
        &corrected,
    ]);
    for phase in ["integrating", "done"] {
        let args = f.coord(&[
            "mark",
            "--id",
            "integrator",
            "--session",
            "root",
            "--assignment",
            "original",
            "--phase",
            phase,
        ]);
        assert!(
            !f.command(&args.iter().map(String::as_str).collect::<Vec<_>>())
                .output()
                .unwrap()
                .status
                .success()
        );
    }
    f.run(&[
        "mark",
        "--id",
        "integrator",
        "--session",
        "root",
        "--assignment",
        "correction",
        "--phase",
        "integrating",
    ]);
    git(&f.project, &["cherry-pick", &corrected]);
    let ci = f.run(&[
        "mark",
        "--id",
        "integrator",
        "--session",
        "root",
        "--assignment",
        "correction",
        "--phase",
        "ci",
    ]);
    assert_eq!(ci.state.assignments["correction"].phase, Phase::Ci);
    assert_eq!(
        git(
            &f.project,
            &["rev-list", "--count", &format!("{base}..HEAD")]
        ),
        "1"
    );
    assert_eq!(
        std::fs::read_to_string(f.project.join("synthetic.txt")).unwrap(),
        "corrected synthetic behavior\n"
    );
    assert_eq!(git(&old, &["rev-parse", "HEAD"]), original);
    assert!(ops::is_open(&f.store.read_ticket(&t.id).unwrap()));
}

/// HS2-ZMBMV3: one worker's work on a commit in its own worktree, handed off through
/// the real CLI: claim, accept, commit, `## Integration handoff` note, release and the
/// mailbox handoff. Returns the commit.
fn hand_off(
    f: &Fixture,
    t: &hotsheet_model::Ticket,
    base: &str,
    (assignment, worker, session): (&str, &str, &str),
    text: &str,
) -> String {
    f.cli(&["claim", &t.slug, "--worker", worker, "--eta", "10m"]);
    f.run(&[
        "worker",
        "--id",
        worker,
        "--session",
        session,
        "--assignment",
        assignment,
        "--phase",
        "accepted",
    ]);
    let tree = f.root.path().join(assignment);
    git(
        &f.project,
        &[
            "worktree",
            "add",
            "-qb",
            assignment,
            tree.to_str().unwrap(),
            base,
        ],
    );
    std::fs::write(tree.join("synthetic.txt"), text).unwrap();
    git(&tree, &["add", "synthetic.txt"]);
    git(
        &tree,
        &["commit", "-qm", &format!("HS-SYNTHETIC {assignment}")],
    );
    let commit = git(&tree, &["rev-parse", "HEAD"]);
    f.cli(&[
        "edit",
        &t.slug,
        "--note",
        &format!("## Integration handoff\n{commit}"),
    ]);
    f.cli(&["release", &t.slug, "--worker", worker]);
    f.run(&[
        "worker",
        "--id",
        worker,
        "--session",
        session,
        "--assignment",
        assignment,
        "--phase",
        "handoff",
        "--commit",
        &commit,
    ]);
    commit
}

/// HS2-ZMBMV3: the integrator's regular `## Review return` note naming the assignment
/// and its commit. Returns the note id.
fn review_note(f: &Fixture, t: &hotsheet_model::Ticket, assignment: &str, commit: &str) -> String {
    let note = Ulid::new();
    ops::add_note(
        &f.store,
        &t.id,
        note,
        Timestamp::from_datetime(time::OffsetDateTime::now_utc()),
        NoteKind::Regular,
        format!("## Review return\n{assignment} {commit}\nCorrect the synthetic behavior again."),
    )
    .unwrap();
    note.to_string()
}

fn review_return(f: &Fixture, assignment: &str, note: &str) -> std::process::Output {
    let args = f.coord(&[
        "review-return",
        "--id",
        "integrator",
        "--session",
        "root",
        "--assignment",
        assignment,
        "--evidence-note",
        note,
        "--inspected",
    ]);
    f.command(&args.iter().map(String::as_str).collect::<Vec<_>>())
        .output()
        .unwrap()
}

fn assign_to(f: &Fixture, t: &hotsheet_model::Ticket, base: &str, operation: &str, worker: &str) {
    f.run(&[
        "assign",
        "--id",
        "integrator",
        "--session",
        "root",
        "--worker",
        worker,
        "--worker-session",
        &format!("{worker}-session"),
        "--operation",
        operation,
        "--connection",
        &hotsheet_ticketing::git_connection_id(&f.store),
        "--native-id",
        &t.id.to_string(),
        "--slug",
        &t.slug,
        "--base",
        base,
    ]);
}

/// HS2-ZMBMV3: two review rounds through the shipped CLI with real claims, notes and
/// Git. The original is returned, its correction is returned too, and a second
/// correction by another worker is integrated and completed. Both returned records
/// stay fenced with their own evidence, both returns replay after later work (in
/// either order), a return with the other round's note is refused, and only the last
/// correction reaches the shipping branch.
#[test]
fn hs2_zmbmv3_a_correction_is_returned_again_and_another_workers_correction_integrates() {
    let f = Fixture::new();
    let t = f.ticket();
    for (role, id, session) in [
        ("integrator", "integrator", "root"),
        ("worker", "worker", "worker-session"),
        ("worker", "other", "other-session"),
    ] {
        f.run(&["register", "--role", role, "--id", id, "--session", session]);
    }
    let base = git(&f.project, &["rev-parse", "HEAD"]);

    // Round one: the original is handed off and returned.
    assign_to(&f, &t, &base, "original", "worker");
    let original = hand_off(
        &f,
        &t,
        &base,
        ("original", "worker", "worker-session"),
        "first synthetic attempt\n",
    );
    let first_note = review_note(&f, &t, "original", &original);
    assert!(review_return(&f, "original", &first_note).status.success());

    // Round two: the same worker's correction is handed off and returned too. The
    // first round's note names the original, so it cannot return the correction.
    assign_to(&f, &t, &base, "correction-1", "worker");
    let first_correction = hand_off(
        &f,
        &t,
        &base,
        ("correction-1", "worker", "worker-session"),
        "second synthetic attempt\n",
    );
    assert!(
        !review_return(&f, "correction-1", &first_note)
            .status
            .success()
    );
    let second_note = review_note(&f, &t, "correction-1", &first_correction);
    assert!(
        review_return(&f, "correction-1", &second_note)
            .status
            .success()
    );

    // Round three: another worker's correction is integrated, its CI passes and the
    // ticket is completed.
    assign_to(&f, &t, &base, "correction-2", "other");
    let last = hand_off(
        &f,
        &t,
        &base,
        ("correction-2", "other", "other-session"),
        "corrected synthetic behavior\n",
    );
    let mark = |assignment: &str, phase: &str| {
        let args = f.coord(&[
            "mark",
            "--id",
            "integrator",
            "--session",
            "root",
            "--assignment",
            assignment,
            "--phase",
            phase,
        ]);
        f.command(&args.iter().map(String::as_str).collect::<Vec<_>>())
            .output()
            .unwrap()
    };
    assert!(mark("correction-2", "integrating").status.success());
    git(&f.project, &["cherry-pick", &last]);
    assert!(mark("correction-2", "ci").status.success());
    f.cli(&["edit", &t.slug, "--status", "completed"]);
    assert!(mark("correction-2", "done").status.success());

    // Both returned records stay fenced with their own evidence, even after the
    // ticket completed.
    for (assignment, commit, note) in [
        ("original", &original, &first_note),
        ("correction-1", &first_correction, &second_note),
    ] {
        for phase in ["verifying", "integrating", "done"] {
            assert!(
                !mark(assignment, phase).status.success(),
                "{assignment} {phase}"
            );
        }
        let state = f.run(&["status"]).state;
        let record = &state.assignments[assignment];
        assert_eq!(record.phase, Phase::Blocked, "{assignment}");
        assert_eq!(record.commit.as_deref(), Some(commit.as_str()));
        assert_eq!(record.recovery_note.as_deref(), Some(note.as_str()));
        assert!(record.returned_for_review());
    }
    // Both returns replay after the later work, the later one first, and change
    // nothing; a return with the other round's note is refused.
    let before = f.run(&["status"]).state;
    for (assignment, note) in [("correction-1", &second_note), ("original", &first_note)] {
        let replay = review_return(&f, assignment, note);
        assert!(
            replay.status.success(),
            "{assignment}: {}",
            String::from_utf8_lossy(&replay.stderr)
        );
    }
    assert!(!review_return(&f, "original", &second_note).status.success());
    let after = f.run(&["status"]).state;
    assert_eq!(after.assignments, before.assignments);
    assert_eq!(after.assignments["correction-2"].phase, Phase::Done);
    // Only the last correction reached the shipping branch.
    assert_eq!(
        git(
            &f.project,
            &["rev-list", "--count", &format!("{base}..HEAD")]
        ),
        "1"
    );
    assert_eq!(
        std::fs::read_to_string(f.project.join("synthetic.txt")).unwrap(),
        "corrected synthetic behavior\n"
    );
    assert!(!ops::is_open(&f.store.read_ticket(&t.id).unwrap()));
}
