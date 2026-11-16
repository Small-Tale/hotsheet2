//! HS2-TRP2MJ: a worker records the worktree it works an assignment in through the
//! real CLI. Synthetic tickets and a disposable project only.

#[allow(dead_code)] // HS2-TRP2MJ: shared process fixture also provides unused helpers.
#[path = "support/coordination.rs"]
mod support;

use support::{Fixture, git};

fn strs(args: &[String]) -> Vec<&str> {
    args.iter().map(String::as_str).collect()
}

fn rejects(f: &Fixture, args: &[&str], expected: &str) {
    let args = f.coord(args);
    let out = f
        .command(&args.iter().map(String::as_str).collect::<Vec<_>>())
        .output()
        .unwrap();
    assert!(!out.status.success(), "unexpected success");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains(expected), "{stderr}");
}

#[test]
fn hs2_trp2mj_a_worker_records_its_worktree_with_the_assignment() {
    let f = Fixture::new();
    for (role, id, session) in [
        ("integrator", "integrator", "root"),
        ("worker", "worker", "worker-session"),
    ] {
        f.run(&["register", "--role", role, "--id", id, "--session", session]);
    }
    let ticket = f.ticket();
    let base = git(&f.project, &["rev-parse", "HEAD"]);
    f.assign("work", &ticket, &base, "root");
    f.cli(&["claim", &ticket.slug, "--worker", "worker", "--eta", "10m"]);
    let tree = f.project.join("synthetic-tree");
    std::fs::create_dir(&tree).unwrap();
    let accept = |session: &str, worktree: &str| {
        vec![
            "worker".to_owned(),
            "--id".into(),
            "worker".into(),
            "--session".into(),
            session.into(),
            "--assignment".into(),
            "work".into(),
            "--phase".into(),
            "accepted".into(),
            "--worktree".into(),
            worktree.into(),
        ]
    };
    // A path that is not an existing directory is refused before anything changes.
    let missing = f.project.join("no-such-tree");
    rejects(
        &f,
        &strs(&accept("worker-session", missing.to_str().unwrap())),
        "worktree must be an existing directory",
    );
    assert_eq!(f.run(&["status"]).state.assignments["work"].worktree, None);
    // Accepted without the flag: absent.
    f.run(&[
        "worker",
        "--id",
        "worker",
        "--session",
        "worker-session",
        "--assignment",
        "work",
        "--phase",
        "accepted",
    ]);
    assert_eq!(f.run(&["status"]).state.assignments["work"].worktree, None);
    // Recorded by its resolved path; the same again changes nothing.
    let resolved = std::fs::canonicalize(&tree).unwrap().display().to_string();
    f.run(&strs(&accept("worker-session", tree.to_str().unwrap())));
    let first = f.run(&["status"]);
    assert_eq!(
        first.state.assignments["work"].worktree.as_deref(),
        Some(resolved.as_str())
    );
    f.run(&strs(&accept("worker-session", tree.to_str().unwrap())));
    let again = f.run(&["status"]);
    assert_eq!(
        again.state.assignments["work"],
        first.state.assignments["work"]
    );
    // Another session of the worker may not record it.
    rejects(
        &f,
        &strs(&accept("stale-session", tree.to_str().unwrap())),
        "another session",
    );
    assert_eq!(
        f.run(&["status"]).state.assignments["work"]
            .worktree
            .as_deref(),
        Some(resolved.as_str())
    );
}
