//! HS2-V6JEBR: `init`, `bootstrap`, import, and autocommit decide what to do with Git
//! from the store's owner (`FsStore::git_owner`, HS2-ABV2J3), never from the presence of
//! `<store>/.git`. Each test drives the real binary against real repositories, runs the
//! command twice to prove idempotence, and checks that the enclosing repository of a
//! designated root keeps its other paths, staged changes, and remotes untouched.

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use hotsheet_ticketing::store_git::StoreGitOwner;
use hotsheet_ticketing::{FsStore, StoreMetadata};
use predicates::prelude::*;

fn git(dir: &Path, args: &[&str]) -> String {
    let output = hotsheet_ticketing::git::command_in(dir)
        .args([
            "-c",
            "user.name=Hot Sheet Test",
            "-c",
            "user.email=test@localhost",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn head(dir: &Path) -> Option<String> {
    let output = hotsheet_ticketing::git::command_in(dir)
        .args(["rev-parse", "--verify", "-q", "HEAD"])
        .output()
        .unwrap();
    output
        .status
        .success()
        .then(|| String::from_utf8(output.stdout).unwrap().trim().to_owned())
}

fn cli(home: &Path) -> Command {
    let mut command = Command::cargo_bin("hotsheet-cli").unwrap();
    command
        .env("HOTSHEET_HOME", home)
        .env_remove("HOTSHEET_ACTOR_ROLE")
        .env_remove("HOTSHEET_ACTOR_ID")
        .env_remove("HOTSHEET_NO_AUTOCOMMIT")
        .env_remove("HOTSHEET_STORE");
    command
}

fn hs(home: &Path, store: &Path) -> Command {
    let mut command = cli(home);
    command.arg("-C").arg(store);
    command
}

fn owner(store: &Path) -> StoreGitOwner {
    FsStore::git_owner_at(store)
}

/// An enclosing code repository holding `code.txt` (committed when `born`) and a store at
/// `tickets/` whose metadata the repository tracks, so the store is a designated root.
/// `staged.txt` is a user change staged but not committed, which Hot Sheet must leave
/// exactly as it is.
struct Designated {
    _dir: tempfile::TempDir,
    home: PathBuf,
    repo: PathBuf,
    store: PathBuf,
}

fn designated(born: bool) -> Designated {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let repo = dir.path().join("code");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("code.txt"), "code\n").unwrap();
    if born {
        git(&repo, &["add", "--", "code.txt"]);
        git(&repo, &["commit", "-q", "-m", "code"]);
    }
    let store = repo.join("tickets");
    FsStore::init(&store, &StoreMetadata::new("HS")).unwrap();
    git(&repo, &["add", "--", "tickets/hotsheet-store.json"]);
    if born {
        git(&repo, &["commit", "-q", "-m", "track the ticket store"]);
    }
    std::fs::write(repo.join("staged.txt"), "user work\n").unwrap();
    git(&repo, &["add", "--", "staged.txt"]);
    let store = store.canonicalize().unwrap();
    assert!(
        matches!(owner(&store), StoreGitOwner::Designated { .. }),
        "{:?}",
        owner(&store)
    );
    Designated {
        _dir: dir,
        home,
        repo: repo.canonicalize().unwrap(),
        store,
    }
}

/// The user's staged change is still staged and never reached a commit.
fn assert_user_paths_untouched(repo: &Path) {
    let staged = git(repo, &["diff", "--cached", "--name-only"]);
    assert!(staged.lines().any(|line| line == "staged.txt"), "{staged}");
    if head(repo).is_some() {
        let committed = git(repo, &["log", "--format=", "--name-only"]);
        assert!(
            !committed.lines().any(|line| line == "staged.txt"),
            "{committed}"
        );
    }
}

/// Every path any commit since `since` touched lies inside the store.
fn assert_commits_only_store_paths(repo: &Path, since: Option<&str>) {
    let range = since.map_or_else(|| "HEAD".to_owned(), |since| format!("{since}..HEAD"));
    let touched = git(repo, &["log", "--format=", "--name-only", &range]);
    assert!(!touched.trim().is_empty(), "expected a store commit");
    for path in touched.lines().filter(|line| !line.is_empty()) {
        assert!(
            path.starts_with("tickets/"),
            "{path} committed outside the store"
        );
        assert!(
            !path.contains(".hotsheet2/"),
            "{path} is machine-local checkout state"
        );
    }
}

#[test]
fn hs2_v6jebr_init_on_a_born_designated_root_never_nests_a_repository() {
    let fixture = designated(true);
    let before = head(&fixture.repo);
    for _ in 0..2 {
        hs(&fixture.home, &fixture.store)
            .arg("init")
            .assert()
            .success()
            .stdout(predicate::str::contains("designated root"));
        assert!(!fixture.store.join(".git").exists());
        assert!(matches!(
            owner(&fixture.store),
            StoreGitOwner::Designated { .. }
        ));
        assert_eq!(head(&fixture.repo), before, "init itself commits nothing");
        assert_user_paths_untouched(&fixture.repo);
    }
    hs(&fixture.home, &fixture.store)
        .arg("doctor")
        .assert()
        .stdout(predicate::str::contains("does not track it").not());
}

#[test]
fn hs2_v6jebr_init_on_an_unborn_designated_root_keeps_it_designated() {
    let fixture = designated(false);
    for _ in 0..2 {
        hs(&fixture.home, &fixture.store)
            .arg("init")
            .assert()
            .success();
        assert!(!fixture.store.join(".git").exists());
        assert!(matches!(
            owner(&fixture.store),
            StoreGitOwner::Designated { head: None, .. }
        ));
        assert_eq!(head(&fixture.repo), None);
        assert_user_paths_untouched(&fixture.repo);
    }
}

#[test]
fn hs2_v6jebr_init_creates_a_repository_once_where_none_owns_the_store() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let store = dir.path().join("store");
    hs(&home, &store).arg("init").assert().success();
    assert!(store.join(".git").is_dir());
    assert!(matches!(owner(&store), StoreGitOwner::Owned { .. }));
    let git_dir = git(&store, &["rev-parse", "--absolute-git-dir"]);
    hs(&home, &store).arg("init").assert().success();
    assert_eq!(git(&store, &["rev-parse", "--absolute-git-dir"]), git_dir);
}

#[test]
fn hs2_v6jebr_init_gives_an_unowned_store_its_own_repository_and_leaves_the_parent_alone() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let parent = dir.path().join("parent");
    std::fs::create_dir_all(&parent).unwrap();
    git(&parent, &["init", "-q", "-b", "main"]);
    std::fs::write(parent.join(".gitignore"), "target/\n").unwrap();
    git(&parent, &["add", "--", ".gitignore"]);
    git(&parent, &["commit", "-q", "-m", "parent"]);
    let parent_head = head(&parent);
    let store = parent.join("target/store");
    std::fs::create_dir_all(&store).unwrap();
    assert!(matches!(owner(&store), StoreGitOwner::Unowned { .. }));
    for _ in 0..2 {
        hs(&home, &store).arg("init").assert().success();
        assert!(matches!(owner(&store), StoreGitOwner::Owned { .. }));
        assert_eq!(head(&parent), parent_head);
        assert_eq!(git(&parent, &["status", "--porcelain"]), "");
    }
}

#[test]
fn hs2_v6jebr_init_in_a_linked_worktree_keeps_its_git_file() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let main = dir.path().join("main");
    std::fs::create_dir_all(&main).unwrap();
    git(&main, &["init", "-q", "-b", "main"]);
    std::fs::write(main.join("README"), "x\n").unwrap();
    git(&main, &["add", "--", "README"]);
    git(&main, &["commit", "-q", "-m", "init"]);
    let linked = dir.path().join("linked");
    git(
        &main,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "tickets",
            linked.to_str().unwrap(),
        ],
    );
    for _ in 0..2 {
        hs(&home, &linked).arg("init").assert().success();
        assert!(linked.join(".git").is_file(), "no nested repository");
        assert!(matches!(owner(&linked), StoreGitOwner::Owned { .. }));
    }
}

#[test]
fn hs2_v6jebr_init_refuses_a_location_git_cannot_describe_before_writing() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let bare = dir.path().join("bare");
    std::fs::create_dir_all(&bare).unwrap();
    git(&bare, &["init", "-q", "--bare"]);
    for _ in 0..2 {
        hs(&home, &bare)
            .arg("init")
            .assert()
            .failure()
            .stderr(predicate::str::contains(
                "Git cannot describe a working tree",
            ));
        assert!(!bare.join("hotsheet-store.json").exists());
    }
}

#[test]
fn hs2_v6jebr_autocommit_scopes_a_designated_root_to_its_own_paths() {
    let fixture = designated(true);
    let before = head(&fixture.repo).unwrap();
    hs(&fixture.home, &fixture.store)
        .args(["new", "--title", "scoped"])
        .assert()
        .success();
    assert_ne!(head(&fixture.repo).unwrap(), before, "the write committed");
    assert_commits_only_store_paths(&fixture.repo, Some(&before));
    assert_user_paths_untouched(&fixture.repo);
    // The ticket file itself is committed, and the machine-local checkout worklist the
    // CLI keeps beside it is excluded rather than left for a later commit to pick up.
    assert_eq!(
        git(&fixture.repo, &["status", "--porcelain", "--", "tickets"]),
        ""
    );
}

#[test]
fn hs2_v6jebr_autocommit_creates_an_unborn_enclosing_repositorys_first_commit_from_store_paths_only()
 {
    let fixture = designated(false);
    hs(&fixture.home, &fixture.store)
        .args(["new", "--title", "first"])
        .assert()
        .success();
    assert!(head(&fixture.repo).is_some());
    assert_commits_only_store_paths(&fixture.repo, None);
    assert_user_paths_untouched(&fixture.repo);
    // code.txt was never added by the user, so it stays untracked.
    let untracked = git(
        &fixture.repo,
        &["ls-files", "--others", "--exclude-standard"],
    );
    assert!(
        untracked.lines().any(|line| line == "code.txt"),
        "{untracked}"
    );
}

#[test]
fn hs2_v6jebr_autocommit_skips_a_store_no_repository_owns() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let parent = dir.path().join("parent");
    std::fs::create_dir_all(&parent).unwrap();
    git(&parent, &["init", "-q", "-b", "main"]);
    git(&parent, &["commit", "-q", "--allow-empty", "-m", "parent"]);
    let parent_head = head(&parent);
    let store = parent.join("loose");
    FsStore::init(&store, &StoreMetadata::new("HS")).unwrap();
    assert!(matches!(owner(&store), StoreGitOwner::Unowned { .. }));
    hs(&home, &store)
        .args(["new", "--title", "loose"])
        .assert()
        .success();
    assert_eq!(head(&parent), parent_head);
    assert_eq!(git(&parent, &["diff", "--cached", "--name-only"]), "");
}

fn bootstrap(home: &Path, project: &Path, store: &Path) -> Command {
    let mut command = cli(home);
    command.args([
        "bootstrap",
        "--project",
        project.to_str().unwrap(),
        "--store",
        store.to_str().unwrap(),
        "--tool",
        "claude",
    ]);
    command
}

#[test]
fn hs2_v6jebr_bootstrap_on_an_unborn_designated_root_commits_only_the_store() {
    let fixture = designated(false);
    bootstrap(&fixture.home, &fixture.repo, &fixture.store)
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Reused designated Hot Sheet store",
        ))
        .stdout(predicate::str::contains(
            "managed by the enclosing repository",
        ));
    let first = head(&fixture.repo).expect("the scoped initial commit");
    assert_commits_only_store_paths(&fixture.repo, None);
    assert_user_paths_untouched(&fixture.repo);
    assert!(!fixture.store.join(".git").exists());

    bootstrap(&fixture.home, &fixture.repo, &fixture.store)
        .assert()
        .success();
    assert_eq!(head(&fixture.repo).as_deref(), Some(first.as_str()));
    assert!(!fixture.store.join(".git").exists());
    assert_user_paths_untouched(&fixture.repo);
}

#[test]
fn hs2_v6jebr_bootstrap_prints_scoped_steps_when_it_cannot_commit_a_designated_root() {
    let fixture = designated(false);
    let prefix = "'tickets'";
    bootstrap(&fixture.home, &fixture.repo, &fixture.store)
        .env("HOTSHEET_NO_AUTOCOMMIT", "1")
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "git -C '{}' add -A -- {prefix}",
            fixture.repo.display()
        )))
        .stdout(predicate::str::contains(format!("--only -- {prefix}")));
    assert_eq!(head(&fixture.repo), None);
    assert!(!fixture.store.join(".git").exists());
    assert_user_paths_untouched(&fixture.repo);
}

#[test]
fn hs2_v6jebr_bootstrap_refuses_to_add_a_remote_to_an_enclosing_repository() {
    let fixture = designated(true);
    bootstrap(&fixture.home, &fixture.repo, &fixture.store)
        .args(["--remote", "git@example.com:acme/tickets.git"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("owns its remotes"));
    assert_eq!(git(&fixture.repo, &["remote"]), "");
}

#[test]
fn hs2_v6jebr_bootstrap_repairs_an_existing_store_without_a_repository() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let project = dir.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    let store = dir.path().join("project.hs2");
    FsStore::init(&store, &StoreMetadata::new("HS")).unwrap();
    assert_eq!(owner(&store), StoreGitOwner::NoRepository);
    bootstrap(&home, &project, &store)
        .assert()
        .success()
        .stdout(predicate::str::contains("Reused standalone"));
    let first = head(&store).expect("initial commit");
    assert!(matches!(owner(&store), StoreGitOwner::Owned { .. }));
    bootstrap(&home, &project, &store).assert().success();
    assert_eq!(head(&store).as_deref(), Some(first.as_str()));
}

fn export_file(dir: &Path) -> PathBuf {
    let export = dir.join("hotsheet-export.json");
    std::fs::write(
        &export,
        r#"{
          "exportVersion": 1,
          "project": {"name":"Demo","ticketPrefix":"HS"},
          "tickets": [
            {"ticket_number":"HS-1","title":"one","status":"not_started",
             "created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-02T00:00:00Z"}
          ]
        }"#,
    )
    .unwrap();
    export
}

#[test]
fn hs2_v6jebr_import_into_a_designated_root_commits_only_store_paths() {
    let fixture = designated(true);
    let before = head(&fixture.repo).unwrap();
    // An ignored file inside the store must never be force-added.
    std::fs::write(fixture.store.join(".gitignore"), "scratch.txt\n").unwrap();
    std::fs::write(fixture.store.join("scratch.txt"), "local\n").unwrap();
    let export = export_file(fixture._dir.path());
    for expected in ["Imported 1 ticket(s)", "skipped 1 already present"] {
        hs(&fixture.home, &fixture.store)
            .args(["import", export.to_str().unwrap()])
            .assert()
            .success()
            .stdout(predicate::str::contains(expected));
        assert!(!fixture.store.join(".git").exists());
        assert_commits_only_store_paths(&fixture.repo, Some(&before));
        assert_user_paths_untouched(&fixture.repo);
    }
    let committed = git(&fixture.repo, &["log", "--format=", "--name-only"]);
    assert!(
        !committed.lines().any(|line| line.ends_with("scratch.txt")),
        "{committed}"
    );
}

#[test]
fn hs2_v6jebr_import_creates_its_own_repository_once_where_none_exists() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let store = dir.path().join("store");
    let export = export_file(dir.path());
    hs(&home, &store)
        .args(["import", export.to_str().unwrap()])
        .assert()
        .success();
    assert!(matches!(owner(&store), StoreGitOwner::Owned { .. }));
    let first = head(&store).expect("import commit");
    hs(&home, &store)
        .args(["import", export.to_str().unwrap()])
        .assert()
        .success();
    assert_eq!(head(&store).as_deref(), Some(first.as_str()));
}
