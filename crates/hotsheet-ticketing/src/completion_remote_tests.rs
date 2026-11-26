//! HS2-R8ZH4K transition matrix over a temp clone and bare remote: commit on main, on
//! `merge-pending/`, local only, no commits with and without the opt-out, offline, a
//! cherry-picked (patch-equivalent) commit, explicit commits, and non-AI actors.

use super::*;
use crate::actor::ActorRole;

const SLUG: &str = "HS-R8ZH";

fn run(dir: &Path, args: &[&str]) -> String {
    let output = crate::git::command()
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
    root: tempfile::TempDir,
    clone: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let remote = root.path().join("remote.git");
        std::fs::create_dir(&remote).unwrap();
        run(&remote, &["init", "--quiet", "--bare"]);
        run(
            root.path(),
            &["clone", "--quiet", remote.to_str().unwrap(), "clone"],
        );
        let clone = root.path().join("clone");
        let fixture = Self { root, clone };
        fixture.commit("base.txt", "base");
        run(&fixture.clone, &["push", "--quiet", "origin", "HEAD:main"]);
        fixture
    }

    fn commit(&self, file: &str, subject: &str) -> String {
        std::fs::write(self.clone.join(file), subject).unwrap();
        run(&self.clone, &["add", file]);
        run(&self.clone, &["commit", "--quiet", "-m", subject]);
        run(&self.clone, &["rev-parse", "HEAD"]).trim().to_owned()
    }

    fn check(&self, claim: &CommitClaim, fetch: bool) -> Result<Outcome, RuleViolation> {
        check(Some(&ai()), true, SLUG, Some(&self.clone), claim, fetch)
    }
}

fn ai() -> MutationActor {
    MutationActor {
        role: ActorRole::Ai,
        id: Some("worker".into()),
    }
}

fn none() -> CommitClaim {
    CommitClaim::default()
}

#[test]
fn hs2_r8zh4k_commit_on_main_verifies() {
    let fixture = Fixture::new();
    let sha = fixture.commit("a.txt", &format!("{SLUG} ship it"));
    run(&fixture.clone, &["push", "--quiet", "origin", "HEAD:main"]);
    assert_eq!(
        fixture.check(&none(), true).unwrap(),
        Outcome::Verified { commits: vec![sha] }
    );
}

#[test]
fn hs2_r8zh4k_commit_on_merge_pending_verifies() {
    let fixture = Fixture::new();
    run(&fixture.clone, &["checkout", "--quiet", "-b", "side"]);
    let sha = fixture.commit("a.txt", &format!("HS-OTHER {SLUG} combined"));
    run(
        &fixture.clone,
        &[
            "push",
            "--quiet",
            "origin",
            &format!("side:merge-pending/{SLUG}"),
        ],
    );
    assert_eq!(
        fixture.check(&none(), true).unwrap(),
        Outcome::Verified { commits: vec![sha] }
    );
}

#[test]
fn hs2_r8zh4k_local_only_commit_is_rejected_with_the_push_to_run() {
    let fixture = Fixture::new();
    run(&fixture.clone, &["checkout", "--quiet", "-b", "batch-2"]);
    let sha = fixture.commit("a.txt", &format!("{SLUG}: local only"));
    let error = fixture.check(&none(), true).unwrap_err();
    assert_eq!(error.code, COMMIT_NOT_ON_REMOTE);
    assert!(error.message.contains(&sha[..12]), "{}", error.message);
    assert!(error.message.contains("local branches: batch-2"));
    assert!(
        error
            .message
            .contains(&format!("git push origin batch-2:merge-pending/{SLUG}"))
    );
    assert!(error.message.contains("Nothing was changed"));
    // Once pushed under merge-pending, the same completion passes.
    run(
        &fixture.clone,
        &[
            "push",
            "--quiet",
            "origin",
            &format!("batch-2:merge-pending/{SLUG}"),
        ],
    );
    assert!(matches!(
        fixture.check(&none(), true),
        Ok(Outcome::Verified { .. })
    ));
}

#[test]
fn hs2_r8zh4k_no_commits_needs_the_opt_out() {
    let fixture = Fixture::new();
    // A different ticket's commit and a mention outside the subject do not count.
    fixture.commit("a.txt", &format!("HS-OTHER mentions {SLUG} later"));
    run(&fixture.clone, &["push", "--quiet", "origin", "HEAD:main"]);
    let error = fixture.check(&none(), true).unwrap_err();
    assert_eq!(error.code, COMMITS_NOT_FOUND);
    assert!(error.message.contains("--no-commit"));
    let opted = fixture
        .check(
            &CommitClaim {
                no_commit: true,
                ..CommitClaim::default()
            },
            true,
        )
        .unwrap();
    assert_eq!(opted, Outcome::OptedOut);
    assert!(opted.history_note().unwrap().contains("no-commit opt-out"));
}

#[test]
fn hs2_r8zh4k_offline_unproven_commit_is_refused_with_its_own_code() {
    let fixture = Fixture::new();
    let pushed = fixture.commit("a.txt", &format!("{SLUG} pushed before going offline"));
    run(&fixture.clone, &["push", "--quiet", "origin", "HEAD:main"]);
    run(
        &fixture.clone,
        &["remote", "set-url", "origin", "/nonexistent/remote.git"],
    );
    // Offline, a commit already on the last fetched refs is still proven.
    assert_eq!(
        fixture.check(&none(), true).unwrap(),
        Outcome::Verified {
            commits: vec![pushed]
        }
    );
    fixture.commit("b.txt", &format!("{SLUG} made offline"));
    let error = fixture.check(&none(), true).unwrap_err();
    assert_eq!(error.code, COMMIT_REMOTE_UNVERIFIED);
    assert!(error.message.contains("could not be fetched"));
}

#[test]
fn hs2_r8zh4k_cherry_picked_or_rebased_commit_is_patch_equivalent() {
    let fixture = Fixture::new();
    run(&fixture.clone, &["checkout", "--quiet", "-b", "worker"]);
    let local = fixture.commit("a.txt", &format!("{SLUG} worker commit"));
    run(
        &fixture.clone,
        &["checkout", "--quiet", "-B", "ship", "origin/main"],
    );
    fixture.commit("other.txt", "unrelated shipping work");
    run(&fixture.clone, &["cherry-pick", &local]);
    run(&fixture.clone, &["push", "--quiet", "origin", "HEAD:main"]);
    // The worker branch still holds the original SHA, which is not on the remote, but its
    // patch is: the completion passes.
    assert!(!on_remote(&fixture.clone, &local));
    assert!(matches!(
        fixture.check(&none(), true),
        Ok(Outcome::Verified { commits }) if commits.contains(&local)
    ));
}

#[test]
fn hs2_r8zh4k_explicit_commits_replace_the_search_and_are_validated() {
    let fixture = Fixture::new();
    let pushed = fixture.commit("a.txt", "subject without the slug");
    run(&fixture.clone, &["push", "--quiet", "origin", "HEAD:main"]);
    fixture.commit("b.txt", &format!("{SLUG} stale local attempt"));
    let named = CommitClaim {
        commits: vec![pushed[..10].to_owned()],
        no_commit: false,
    };
    assert_eq!(
        fixture.check(&named, false).unwrap(),
        Outcome::Verified {
            commits: vec![pushed.clone()]
        }
    );
    let unknown = CommitClaim {
        commits: vec!["0123456789abcdef0123456789abcdef01234567".into()],
        no_commit: false,
    };
    assert_eq!(
        fixture.check(&unknown, false).unwrap_err().code,
        COMMIT_INVALID
    );
    let both = CommitClaim {
        commits: vec![pushed],
        no_commit: true,
    };
    assert_eq!(
        fixture.check(&both, false).unwrap_err().code,
        COMMIT_INVALID
    );
}

#[test]
fn hs2_r8zh4k_applies_only_to_ai_completions_with_a_checkout() {
    let fixture = Fixture::new();
    fixture.commit("a.txt", &format!("{SLUG} local only"));
    let human = MutationActor {
        role: ActorRole::Human,
        id: None,
    };
    for (actor, completes) in [(Some(&human), true), (None, true), (Some(&ai()), false)] {
        assert_eq!(
            check(actor, completes, SLUG, Some(&fixture.clone), &none(), false).unwrap(),
            Outcome::NotApplicable
        );
    }
    let skipped = check(Some(&ai()), true, SLUG, None, &none(), false).unwrap();
    assert_eq!(skipped, Outcome::NoCheckout);
    assert!(skipped.warning().is_some());
}

#[test]
fn hs2_r8zh4k_code_checkout_excludes_the_ticket_store_repository() {
    let fixture = Fixture::new();
    let store = fixture.root.path().join("store");
    std::fs::create_dir(&store).unwrap();
    run(&store, &["init", "--quiet"]);
    let outside = fixture.root.path().join("not-a-repo");
    std::fs::create_dir(&outside).unwrap();
    let nested = fixture.clone.join("nested");
    std::fs::create_dir(&nested).unwrap();
    let clone = hotsheet_sync::paths::canonicalize(&fixture.clone).unwrap();
    assert_eq!(
        code_checkout(&[store.clone(), outside, nested], &store),
        Some(clone)
    );
    assert_eq!(code_checkout(std::slice::from_ref(&store), &store), None);
}

#[test]
fn hs2_r8zh4k_slug_tokens_lead_the_subject() {
    assert!(names_slug("HS-R8ZH fix", SLUG));
    assert!(names_slug("HS-R8ZH: fix", SLUG));
    assert!(names_slug("HS-A, HS-R8ZH fix both", SLUG));
    assert!(!names_slug("Fix HS-R8ZH later", SLUG));
    assert!(!names_slug("HS-R8ZHX fix", SLUG));
}

/// Link an external provider connection to a registered checkout, as `provider connect`
/// does, by writing the registry's source list.
fn link_connection(registry: &Path, checkout_id: &str, connection: &str, provider: &str) {
    let mut saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(registry).unwrap()).unwrap();
    saved["checkouts"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entry| entry["id"] == checkout_id)
        .unwrap()["sources"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "connection_id": connection, "provider": provider, "locator": "acme/repo"
        }));
    std::fs::write(registry, serde_json::to_vec(&saved).unwrap()).unwrap();
}

#[test]
fn hs2_z0vyz5_github_and_gitlab_slugs_match_their_short_issue_form() {
    let github = "acme/repo#7";
    assert!(names_slug("acme/repo#7 fix the bug", github));
    assert!(names_slug("#7 fix the bug", github));
    assert!(names_slug("#7: fix the bug", github));
    assert!(names_slug("HS-1 #7 fix both", github));
    assert!(!names_slug("#70 another issue", github));
    assert!(!names_slug("Fix the bug (#7)", github));
    assert!(!names_slug("other/repo#7 fix", github));
    assert_eq!(
        slug_aliases(github),
        vec![github.to_owned(), "#7".to_owned()]
    );
    // Jira keys and Hot Sheet slugs have no short form.
    assert_eq!(slug_aliases("PROJ-12"), vec!["PROJ-12".to_owned()]);
    assert_eq!(slug_aliases("weird#"), vec!["weird#".to_owned()]);
    assert_eq!(slug_aliases("#7"), vec!["#7".to_owned()]);
}

/// A GitHub ticket's commits are searched by both spellings against the real remote:
/// local only is refused, pushed is verified, and the opt-out needs no commits.
#[test]
fn hs2_z0vyz5_provider_slug_commits_must_be_on_the_remote() {
    let fixture = Fixture::new();
    let github = "acme/repo#7";
    let check_github =
        |claim: &CommitClaim| check(Some(&ai()), true, github, Some(&fixture.clone), claim, true);
    assert_eq!(check_github(&none()).unwrap_err().code, COMMITS_NOT_FOUND);
    run(&fixture.clone, &["checkout", "--quiet", "-b", "issue-7"]);
    let short = fixture.commit("a.txt", "#7 short form");
    let long = fixture.commit("b.txt", "acme/repo#7 long form");
    let error = check_github(&none()).unwrap_err();
    assert_eq!(error.code, COMMIT_NOT_ON_REMOTE);
    assert!(error.message.contains(&short[..12]), "{}", error.message);
    assert!(error.message.contains(&long[..12]), "{}", error.message);
    run(&fixture.clone, &["push", "--quiet", "origin", "HEAD:main"]);
    match check_github(&none()).unwrap() {
        Outcome::Verified { mut commits } => {
            commits.sort();
            let mut expected = vec![short, long];
            expected.sort();
            assert_eq!(commits, expected);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        check_github(&CommitClaim {
            no_commit: true,
            ..CommitClaim::default()
        })
        .unwrap(),
        Outcome::OptedOut
    );
}

#[test]
fn hs2_z0vyz5_provider_code_checkout_is_the_checkout_linking_the_connection() {
    let fixture = Fixture::new();
    let registry_path = fixture.root.path().join("checkouts.json");
    let registry = crate::checkouts::CheckoutRegistry::new(&registry_path);
    let store = fixture.root.path().join("AA.hs2");
    crate::FsStore::init_git(&store, &crate::StoreMetadata::new("AA")).unwrap();
    let clone = hotsheet_sync::paths::canonicalize(&fixture.clone).unwrap();
    // No checkout is registered: nothing is known.
    assert_eq!(
        provider_code_checkout(&registry, "github-main", None, None),
        None
    );
    let other = fixture.root.path().join("other");
    std::fs::create_dir(&other).unwrap();
    run(&other, &["init", "--quiet"]);
    let other = hotsheet_sync::paths::canonicalize(&other).unwrap();
    let unrelated = registry
        .register(&other, Some("other"), None, vec![])
        .unwrap();
    link_connection(&registry_path, &unrelated.id, "jira-main", "jira");
    let linked = registry
        .register(&fixture.clone, Some("code"), None, vec![store.clone()])
        .unwrap();
    assert_eq!(
        provider_code_checkout(&registry, "github-main", None, None),
        None,
        "a checkout that links neither the connection nor its store does not count"
    );
    link_connection(&registry_path, &linked.id, "github-main", "github");
    assert_eq!(
        provider_code_checkout(&registry, "github-main", None, None),
        Some(clone.clone())
    );
    assert_eq!(
        provider_code_checkout(&registry, "jira-main", None, None),
        Some(other.clone())
    );
    // The route's own checkout wins over the registry order.
    assert_eq!(
        provider_code_checkout(&registry, "github-main", None, Some(&other)),
        Some(other)
    );
    // A hosted Git connection finds the checkout linked to its store, never the store.
    let connection = crate::provider::git_connection_id(&crate::FsStore::open(&store).unwrap());
    assert_eq!(
        provider_code_checkout(&registry, &connection, Some(&store), None),
        Some(clone.clone())
    );
    assert_eq!(
        provider_code_checkout(&registry, &connection, Some(&store), Some(&store)),
        Some(clone)
    );
}

#[test]
fn hs2_z0vyz5_record_provider_writes_the_opt_out_through_the_provider() {
    let dir = tempfile::tempdir().unwrap();
    let store = crate::FsStore::init_git(dir.path(), &crate::StoreMetadata::new("AA")).unwrap();
    let ticket = crate::ops::create(
        &store,
        hotsheet_model::Ulid::new(),
        "AA",
        hotsheet_model::Timestamp::new("2026-11-26T00:00:00Z"),
        crate::NewTicket {
            title: "Decision".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let provider = crate::GitProvider::new("git-test", store);
    let now = hotsheet_model::Timestamp::new("2026-11-26T00:01:00Z");
    let native = ticket.id.to_string();
    for outcome in [
        Outcome::NotApplicable,
        Outcome::NoCheckout,
        Outcome::Verified { commits: vec![] },
    ] {
        assert!(
            record_provider(&provider, &native, now.clone(), Some(&ai()), &outcome)
                .unwrap()
                .is_none()
        );
    }
    let recorded = record_provider(&provider, &native, now, Some(&ai()), &Outcome::OptedOut)
        .unwrap()
        .unwrap();
    let note = recorded.notes.last().unwrap();
    assert_eq!(
        note.summary.as_deref(),
        Some("Completed without code commits")
    );
    assert_eq!(note.kind, hotsheet_model::NoteKind::Activity);
    assert!(note.text.contains("no-commit opt-out"));
}
