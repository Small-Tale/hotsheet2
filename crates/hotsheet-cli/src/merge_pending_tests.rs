//! Transition coverage for `merge-pending/<SLUG>` pruning over a temp repo with a bare
//! remote (HS2-ZTG8FD): merged, cherry-picked, partial, open, unknown, malformed, rerun.

use super::*;
use std::collections::BTreeMap;

fn run(dir: &Path, args: &[&str]) -> String {
    let output = hotsheet_ticketing::git::command()
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

struct Fixture {
    _root: tempfile::TempDir,
    clone: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let remote = root.path().join("remote.git");
        let clone = root.path().join("clone");
        std::fs::create_dir(&remote).unwrap();
        run(&remote, &["init", "--quiet", "--bare"]);
        run(
            root.path(),
            &["clone", "--quiet", remote.to_str().unwrap(), "clone"],
        );
        let fixture = Self { _root: root, clone };
        fixture.commit("base.txt", "base");
        run(&fixture.clone, &["push", "--quiet", "origin", "HEAD:main"]);
        fixture
    }

    fn commit(&self, file: &str, text: &str) -> String {
        std::fs::write(self.clone.join(file), text).unwrap();
        run(&self.clone, &["add", file]);
        run(&self.clone, &["commit", "--quiet", "-m", file]);
        run(&self.clone, &["rev-parse", "HEAD"]).trim().to_owned()
    }

    /// Create `merge-pending/<name>` from main with one commit per file and push it.
    fn pending(&self, name: &str, files: &[&str]) -> Vec<String> {
        run(
            &self.clone,
            &["checkout", "--quiet", "-B", name, "origin/main"],
        );
        let commits = files.iter().map(|file| self.commit(file, file)).collect();
        run(
            &self.clone,
            &[
                "push",
                "--quiet",
                "origin",
                &format!("HEAD:refs/heads/{PREFIX}{name}"),
            ],
        );
        commits
    }

    /// Put `commits` on the shipping branch by cherry-pick (new SHAs, same patches).
    fn ship_by_cherry_pick(&self, commits: &[String]) {
        run(
            &self.clone,
            &["checkout", "--quiet", "-B", "ship", "origin/main"],
        );
        for commit in commits {
            run(&self.clone, &["cherry-pick", commit]);
        }
        run(&self.clone, &["push", "--quiet", "origin", "HEAD:main"]);
    }

    /// Fast-forward-free merge of a pending branch into the shipping branch.
    fn ship_by_merge(&self, name: &str) {
        run(&self.clone, &["fetch", "--quiet", "origin"]);
        run(
            &self.clone,
            &["checkout", "--quiet", "-B", "ship", "origin/main"],
        );
        run(
            &self.clone,
            &[
                "merge",
                "--quiet",
                "--no-ff",
                "-m",
                "merge",
                &format!("origin/{PREFIX}{name}"),
            ],
        );
        run(&self.clone, &["push", "--quiet", "origin", "HEAD:main"]);
    }

    fn remote_branches(&self) -> Vec<String> {
        run(
            &self.clone,
            &["ls-remote", "--heads", "origin", "merge-pending/*"],
        )
        .lines()
        .filter_map(|line| line.split('\t').nth(1))
        .map(|name| name.trim_start_matches("refs/heads/").to_owned())
        .collect()
    }

    fn prune(&self, apply: bool, statuses: &BTreeMap<&str, Status>) -> PruneReport {
        let options = PruneOptions {
            project: self.clone.clone(),
            remote: "origin".into(),
            shipping: None,
            apply,
            fetch: true,
        };
        prune(&options, &|slug| Ok(statuses.get(slug).copied())).unwrap()
    }
}

fn verdict<'a>(report: &'a PruneReport, branch: &str) -> &'a BranchVerdict {
    report
        .branches
        .iter()
        .find(|verdict| verdict.branch == format!("{PREFIX}{branch}"))
        .unwrap_or_else(|| panic!("no verdict for {branch}: {report:#?}"))
}

#[test]
fn hs2_ztg8fd_parses_slug_label_and_rejects_malformed_names() {
    assert_eq!(
        parse_slug("merge-pending/HS2-FZAE4Y").as_deref(),
        Some("HS2-FZAE4Y")
    );
    assert_eq!(
        parse_slug("merge-pending/HS2-FZAE4Y-batch-2").as_deref(),
        Some("HS2-FZAE4Y")
    );
    assert_eq!(parse_slug("merge-pending/HS-12").as_deref(), Some("HS-12"));
    for malformed in [
        "merge-pending/",
        "merge-pending/batch-2",
        "merge-pending/hs2-fzae4y",
        "merge-pending/HS2",
        "merge-pending/HS2-",
        "merge-pending/2HS-ABC",
        "merge-pending/HS2-FZAE4Y-",
        "merge-pending/HS2-FZAE4Y-a/b",
        "merge-pending/HS2-FZ/AE",
        "feature/HS2-FZAE4Y",
    ] {
        assert_eq!(parse_slug(malformed), None, "{malformed}");
    }
}

#[test]
fn hs2_ztg8fd_prunes_only_integrated_branches_of_closed_tickets_and_is_idempotent() {
    let fixture = Fixture::new();
    fixture.pending("HS-MERGED", &["merged.txt"]);
    let picked = fixture.pending("HS-PICKED-batch-2", &["picked-a.txt", "picked-b.txt"]);
    let partial = fixture.pending("HS-PARTIAL", &["partial-a.txt", "partial-b.txt"]);
    fixture.pending("HS-OPEN", &["open.txt"]);
    fixture.pending("HS-UNKNOWN", &["unknown.txt"]);
    fixture.pending("batch-2", &["malformed.txt"]);
    fixture.pending("HS-MOVED", &["moved.txt"]);
    fixture.ship_by_merge("HS-MERGED");
    fixture.ship_by_cherry_pick(&picked);
    fixture.ship_by_cherry_pick(&partial[..1]);
    fixture.ship_by_merge("HS-OPEN");
    fixture.ship_by_merge("HS-MOVED");
    let statuses = BTreeMap::from([
        ("HS-MERGED", Status::Completed),
        ("HS-PICKED", Status::Verified),
        ("HS-PARTIAL", Status::Completed),
        ("HS-OPEN", Status::Started),
        ("HS-MOVED", Status::Moved),
    ]);

    let dry = fixture.prune(false, &statuses);
    assert_eq!(dry.shipping, "origin/main");
    assert_eq!(dry.fetch_error, None);
    let expect = |report: &PruneReport, branch, action, reason, unintegrated| {
        let verdict = verdict(report, branch);
        assert_eq!(
            (verdict.action, verdict.reason, verdict.unintegrated),
            (action, reason, unintegrated),
            "{branch}"
        );
    };
    expect(&dry, "HS-MERGED", Action::WouldDelete, None, 0);
    expect(&dry, "HS-PICKED-batch-2", Action::WouldDelete, None, 0);
    expect(
        &dry,
        "HS-PARTIAL",
        Action::Kept,
        Some(KeepReason::UnintegratedCommits),
        1,
    );
    expect(
        &dry,
        "HS-OPEN",
        Action::Kept,
        Some(KeepReason::TicketOpen),
        0,
    );
    expect(
        &dry,
        "HS-MOVED",
        Action::Kept,
        Some(KeepReason::TicketMoved),
        0,
    );
    expect(
        &dry,
        "HS-UNKNOWN",
        Action::Kept,
        Some(KeepReason::UnknownSlug),
        1,
    );
    expect(
        &dry,
        "batch-2",
        Action::Kept,
        Some(KeepReason::MalformedName),
        1,
    );
    assert_eq!(
        verdict(&dry, "HS-PICKED-batch-2").slug.as_deref(),
        Some("HS-PICKED")
    );
    assert_eq!(
        fixture.remote_branches().len(),
        7,
        "a dry run deletes nothing"
    );

    let applied = fixture.prune(true, &statuses);
    expect(&applied, "HS-MERGED", Action::Deleted, None, 0);
    expect(&applied, "HS-PICKED-batch-2", Action::Deleted, None, 0);
    let mut remaining = fixture.remote_branches();
    remaining.sort();
    assert_eq!(
        remaining,
        [
            "merge-pending/HS-MOVED",
            "merge-pending/HS-OPEN",
            "merge-pending/HS-PARTIAL",
            "merge-pending/HS-UNKNOWN",
            "merge-pending/batch-2",
        ]
    );

    // A rerun neither deletes nor reports the pruned branches again.
    let rerun = fixture.prune(true, &statuses);
    assert_eq!(rerun.branches.len(), 5);
    assert!(
        rerun
            .branches
            .iter()
            .all(|verdict| verdict.action == Action::Kept)
    );

    // Once the open ticket closes and the rest of the partial branch ships, both go.
    fixture.ship_by_cherry_pick(&partial[1..]);
    let mut closed = statuses.clone();
    closed.insert("HS-OPEN", Status::Archive);
    let later = fixture.prune(true, &closed);
    expect(&later, "HS-OPEN", Action::Deleted, None, 0);
    expect(&later, "HS-PARTIAL", Action::Deleted, None, 0);
    assert_eq!(fixture.remote_branches().len(), 3);
}

#[test]
fn hs2_ztg8fd_reports_an_unreachable_remote_and_a_failed_delete() {
    let fixture = Fixture::new();
    fixture.pending("HS-DONE", &["done.txt"]);
    fixture.ship_by_merge("HS-DONE");
    run(&fixture.clone, &["fetch", "--quiet", "origin"]);
    // The remote disappears: the fetch fails, verdicts use the last fetched refs, and
    // the eligible branch's delete is reported rather than silently skipped.
    run(
        &fixture.clone,
        &["remote", "set-url", "origin", "/nonexistent/remote.git"],
    );
    let statuses = BTreeMap::from([("HS-DONE", Status::Completed)]);
    let report = fixture.prune(true, &statuses);
    assert!(report.fetch_error.is_some());
    let done = verdict(&report, "HS-DONE");
    assert_eq!(done.action, Action::DeleteFailed);
    assert!(done.error.is_some());
}

#[test]
fn hs2_ztg8fd_rejects_a_shipping_ref_that_does_not_resolve() {
    let fixture = Fixture::new();
    let options = PruneOptions {
        project: fixture.clone.clone(),
        remote: "origin".into(),
        shipping: Some("origin/no-such-branch".into()),
        apply: false,
        fetch: false,
    };
    let error = prune(&options, &|_| Ok(None)).unwrap_err();
    assert!(error.to_string().contains("origin/no-such-branch"));
}
