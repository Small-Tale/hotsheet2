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
    assert_eq!(returned.state.version, 1);
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
