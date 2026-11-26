//! HS2-K6KG1T: the reported CLI sequence on a real binary and an isolated store. A claim
//! starts a Not Started or Backlog ticket, a phase edit keeps the claim, a phase edit outside
//! Started is refused, and an edit that ends a claim says so.

use assert_cmd::Command;
use predicates::prelude::*;

#[path = "../../test-support/workspace_bin.rs"]
mod workspace_bin;

struct Fixture {
    home: tempfile::TempDir,
    store: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let fixture = Self {
            home: tempfile::tempdir().unwrap(),
            store: tempfile::tempdir().unwrap(),
        };
        fixture.hs().arg("init").assert().success();
        fixture
    }

    fn hs(&self) -> Command {
        let mut cmd = Command::new(workspace_bin::path("hotsheet-cli"));
        cmd.arg("--test-fixture-credentials")
            .env("HOTSHEET_HOME", self.home.path())
            .env_remove("HOTSHEET_ACTOR_ROLE")
            .env_remove("HOTSHEET_ACTOR_ID")
            .env_remove("HOTSHEET_WORKER_ID")
            .arg("-C")
            .arg(self.store.path());
        cmd
    }

    fn run(&self, args: &[&str]) -> assert_cmd::assert::Assert {
        self.hs().args(args).assert()
    }

    fn new_ticket(&self, title: &str, status: Option<&str>) -> String {
        let out = self.run(&["new", "--title", title]).success();
        let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
        let slug = stdout.split_whitespace().nth(1).unwrap().to_string();
        if let Some(status) = status {
            self.run(&["edit", &slug, "--status", status]).success();
        }
        slug
    }

    fn show(&self, slug: &str) -> String {
        let out = self.run(&["show", slug]).success();
        String::from_utf8(out.get_output().stdout.clone()).unwrap()
    }

    fn claimed(&self) -> String {
        let out = self.run(&["ls", "--claimed"]).success();
        String::from_utf8(out.get_output().stdout.clone()).unwrap()
    }
}

fn field<'a>(show: &'a str, name: &str) -> Option<&'a str> {
    show.lines()
        .find_map(|line| line.strip_prefix(&format!("{name}: ")))
}

#[test]
fn hs2_k6kg1t_claim_starts_backlog_and_phase_edits_keep_the_claim() {
    let f = Fixture::new();
    for status in ["backlog", "not_started"] {
        let slug = f.new_ticket(&format!("from {status}"), Some(status));
        f.run(&["claim", &slug, "--worker", "w", "--eta", "2h"])
            .success()
            .stdout(predicate::str::contains(format!("Claimed {slug} for w")))
            .stdout(predicate::str::contains("Phase: analyzing"));
        let shown = f.show(&slug);
        assert_eq!(field(&shown, "status"), Some("started"), "{shown}");
        assert_eq!(field(&shown, "started_phase"), Some("analyzing"));

        f.run(&["edit", &slug, "--started-phase", "working"])
            .success()
            .stdout(predicate::str::contains(format!("Updated {slug}")))
            .stderr(predicate::str::contains("Released").not());
        assert!(f.claimed().contains(&slug), "the claim survives the edit");
        let shown = f.show(&slug);
        assert_eq!(field(&shown, "started_phase"), Some("working"));
        assert!(shown.contains("Started phase changed from Analyzing to Working"));
    }
}

#[test]
fn hs2_k6kg1t_phase_edit_outside_started_is_refused_without_writing() {
    let f = Fixture::new();
    for status in ["backlog", "not_started"] {
        let slug = f.new_ticket(&format!("unclaimed {status}"), Some(status));
        let before = f.show(&slug);
        f.run(&["edit", &slug, "--started-phase", "working"])
            .failure()
            .stderr(predicate::str::contains("not Started"))
            .stdout(predicate::str::contains("Updated").not());
        assert_eq!(f.show(&slug), before);
        // Status and phase together start it at that phase.
        f.run(&[
            "edit",
            &slug,
            "--status",
            "started",
            "--started-phase",
            "working",
        ])
        .success();
        assert_eq!(field(&f.show(&slug), "started_phase"), Some("working"));
    }
}

#[test]
fn hs2_k6kg1t_claim_matrix_over_states_and_repeats() {
    let f = Fixture::new();
    // Closed tickets are refused and stay closed.
    for status in ["completed", "archive"] {
        let slug = f.new_ticket(&format!("closed {status}"), Some(status));
        f.run(&["claim", &slug, "--worker", "w"])
            .failure()
            .stderr(predicate::str::contains("not open"));
        assert_eq!(field(&f.show(&slug), "status"), Some(status));
    }

    // Repeat, release, reclaim, and another worker's live claim.
    let slug = f.new_ticket("cycle", Some("backlog"));
    f.run(&["claim", &slug, "--worker", "w1"]).success();
    f.run(&["claim", &slug, "--worker", "w1"]).success();
    f.run(&["edit", &slug, "--started-phase", "planning"])
        .success();
    f.run(&["claim", &slug, "--worker", "w2"])
        .failure()
        .stderr(predicate::str::contains("claimed by 'w1'"));
    f.run(&["release", &slug, "--worker", "w1"]).success();
    f.run(&["claim", &slug, "--worker", "w2"]).success();
    let shown = f.show(&slug);
    assert_eq!(field(&shown, "status"), Some("started"));
    assert_eq!(field(&shown, "started_phase"), Some("planning"));
    assert_eq!(field(&shown, "claim_count"), Some("2"));

    // Moving the claimed ticket to Backlog ends the claim and says so.
    f.run(&["edit", &slug, "--status", "backlog"])
        .success()
        .stderr(predicate::str::contains(format!(
            "Released w2's claim on {slug}"
        )));
    assert!(!f.claimed().contains(&slug));

    // HS2-K6Q8TW: claim-next skips a Backlog ticket; an exact claim starts it.
    let g = Fixture::new();
    let parked = g.new_ticket("only backlog", Some("backlog"));
    g.run(&["claim-next", "--worker", "w"])
        .success()
        .stdout(predicate::str::contains("No claimable tickets."));
    assert_eq!(field(&g.show(&parked), "status"), Some("backlog"));
    let ready = g.new_ticket("ready", None);
    g.run(&["claim-next", "--worker", "w"])
        .success()
        .stdout(predicate::str::contains(format!("Claimed {ready}")));
    g.run(&["claim", &parked, "--worker", "w2"]).success();
    assert_eq!(field(&g.show(&parked), "status"), Some("started"));
    g.run(&["edit", &parked, "--started-phase", "working"])
        .success();
    assert!(g.claimed().contains(&parked));
}
