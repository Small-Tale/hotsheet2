//! HS2-TRP2MJ: which worktrees of a target checkout hold a hosted session's own
//! work, and the pool assignment's `worktree` field. Synthetic paths and ids only.
use std::path::PathBuf;

use hotsheet_ticketing::orchestration::SessionRole;

use super::super::session_worktrees::{Worktree, owned_worktrees, parse_worktrees};
use super::sessions::assignment;
use crate::coordination::{Assignment, Phase};

const LISTING: &str = "worktree /repo\nHEAD 1111111111111111111111111111111111111111\nbranch refs/heads/main\n\n\
worktree /repo-wt/hs2-aaa111-fix\nHEAD 2222222222222222222222222222222222222222\nbranch refs/heads/hs2-aaa111-fix\n\n\
worktree /repo-wt/other\nHEAD 3333333333333333333333333333333333333333\ndetached\n\n\
worktree /repo-wt/gone\nHEAD 4444444444444444444444444444444444444444\nbranch refs/heads/gone\nprunable gitdir file points to non-existent location\n\n\
worktree /repo-bare\nbare\n";

/// One case: its name, the session's role and worker, the pool's assignments, and the
/// worktrees it owns.
type Case<'a> = (&'a str, SessionRole, &'a str, Vec<Assignment>, Vec<&'a str>);

fn worker_assignment(worker: &str, slug: &str, worktree: Option<&str>) -> Assignment {
    Assignment {
        id: format!("{worker}-{slug}"),
        slug: slug.into(),
        worktree: worktree.map(str::to_owned),
        ..assignment(worker, Phase::Accepted)
    }
}

fn owned(
    worktrees: &[Worktree],
    role: SessionRole,
    worker: &str,
    assignments: &[Assignment],
) -> Vec<String> {
    let refs: Vec<&Assignment> = assignments.iter().collect();
    owned_worktrees(worktrees, role, worker, &refs)
        .into_iter()
        .map(|worktree| worktree.path.display().to_string())
        .collect()
}

#[test]
fn hs2_trp2mj_the_listing_keeps_worktrees_with_files_and_marks_the_main_one() {
    let worktrees = parse_worktrees(LISTING);
    assert_eq!(
        worktrees,
        [
            Worktree {
                path: PathBuf::from("/repo"),
                head: Some("1".repeat(40)),
                branch: Some("main".into()),
                main: true,
            },
            Worktree {
                path: PathBuf::from("/repo-wt/hs2-aaa111-fix"),
                head: Some("2".repeat(40)),
                branch: Some("hs2-aaa111-fix".into()),
                main: false,
            },
            Worktree {
                path: PathBuf::from("/repo-wt/other"),
                head: Some("3".repeat(40)),
                branch: None,
                main: false,
            },
        ]
    );
    assert!(parse_worktrees("").is_empty());
}

#[test]
fn hs2_trp2mj_each_role_owns_its_own_worktrees() {
    let worktrees = parse_worktrees(LISTING);
    let mut handed = worker_assignment("w-b", "TGT-9", None);
    handed.commit = Some("3".repeat(40));
    handed.phase = Phase::Handoff;
    let mut done = worker_assignment("w-a", "HS2-AAA111", Some("/repo"));
    done.phase = Phase::Done;
    // (case, role, worker, assignments, owned)
    let cases: Vec<Case<'_>> = vec![
        (
            "the field names the worktree",
            SessionRole::Worker,
            "w-a",
            vec![worker_assignment("w-a", "TGT-1", Some("/repo-wt/other"))],
            vec!["/repo-wt/other"],
        ),
        (
            "the field wins over a branch that would be inferred",
            SessionRole::Worker,
            "w-a",
            vec![worker_assignment(
                "w-a",
                "HS2-AAA111",
                Some("/repo-wt/other"),
            )],
            vec!["/repo-wt/other"],
        ),
        (
            "absent: inferred from the branch, whatever its case",
            SessionRole::Worker,
            "w-a",
            vec![worker_assignment("w-a", "HS2-AAA111", None)],
            vec!["/repo-wt/hs2-aaa111-fix"],
        ),
        (
            "absent: inferred from the handoff commit",
            SessionRole::Worker,
            "w-b",
            vec![handed.clone()],
            vec!["/repo-wt/other"],
        ),
        (
            "neither: nothing is this worker's",
            SessionRole::Worker,
            "w-a",
            vec![worker_assignment("w-a", "TGT-404", None)],
            vec![],
        ),
        (
            "a worker never owns the main checkout by inference",
            SessionRole::Worker,
            "w-a",
            vec![worker_assignment("w-a", "MAIN", None)],
            vec![],
        ),
        (
            "another worker's assignment is not this worker's",
            SessionRole::Worker,
            "w-c",
            vec![worker_assignment("w-a", "HS2-AAA111", None), handed.clone()],
            vec![],
        ),
        (
            "a done assignment owns nothing",
            SessionRole::Worker,
            "w-a",
            vec![done],
            vec![],
        ),
        (
            "no pool: nothing",
            SessionRole::Worker,
            "w-a",
            vec![],
            vec![],
        ),
        (
            "an integrator sees the main checkout only",
            SessionRole::Integrator,
            "w-a",
            vec![worker_assignment("w-a", "HS2-AAA111", None)],
            vec!["/repo"],
        ),
        (
            "a dispatcher sees nothing",
            SessionRole::Dispatcher,
            "w-a",
            vec![worker_assignment("w-a", "HS2-AAA111", None)],
            vec![],
        ),
        (
            "a planner sees nothing",
            SessionRole::Planner,
            "w-a",
            vec![],
            vec![],
        ),
    ];
    for (case, role, worker, assignments, expected) in cases {
        assert_eq!(
            owned(&worktrees, role, worker, &assignments),
            expected,
            "{case}"
        );
    }
    // Two workers on one checkout each own only their own worktree.
    let both = [
        worker_assignment("w-a", "HS2-AAA111", None),
        worker_assignment("w-b", "TGT-2", Some("/repo-wt/other")),
    ];
    assert_eq!(
        owned(&worktrees, SessionRole::Worker, "w-a", &both),
        ["/repo-wt/hs2-aaa111-fix"]
    );
    assert_eq!(
        owned(&worktrees, SessionRole::Worker, "w-b", &both),
        ["/repo-wt/other"]
    );
}

#[test]
fn hs2_trp2mj_the_assignment_worktree_is_optional_on_the_wire() {
    let without = assignment("w-a", Phase::Accepted);
    let json = serde_json::to_value(&without).unwrap();
    // Absent, it is not written, so an older record reads and writes unchanged.
    assert!(json.get("worktree").is_none(), "{json}");
    let read: Assignment = serde_json::from_value(json).unwrap();
    assert_eq!(read.worktree, None);
    let with = Assignment {
        worktree: Some("/repo-wt/x".into()),
        ..without
    };
    let json = serde_json::to_value(&with).unwrap();
    assert_eq!(json["worktree"], "/repo-wt/x");
    assert_eq!(serde_json::from_value::<Assignment>(json).unwrap(), with);
}
