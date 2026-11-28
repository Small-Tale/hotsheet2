//! HS2-9PYNCN: milestone registry publication through the group registry machinery
//! (docs/38 §38.2 "Publishing and sync", §38.10 "Registry Git"), against temporary
//! repositories and local bare remotes only.

use super::*;
use crate::groups::git_sync::{RegistrySyncState, sync_registry};

fn git_in(root: &Path, args: &[&str]) -> String {
    let output = crate::git::command()
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// `git` that may fail; returns whether it succeeded.
fn git_ok(root: &Path, args: &[&str]) -> bool {
    crate::git::command()
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap()
        .status
        .success()
}

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

const GITIGNORE: &str = "/.hotsheet2/*\n!/.hotsheet2/groups/\n!/.hotsheet2/milestones/\n";

/// A bare remote seeded with one commit on `main`; clones track `origin/main`.
struct World {
    dir: tempfile::TempDir,
    remote: PathBuf,
}

impl World {
    fn new(gitignore: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let remote = dir.path().join("remote.git");
        git_in(
            dir.path(),
            &["init", "-q", "--bare", "-b", "main", "remote.git"],
        );
        let seed = dir.path().join("seed");
        git_in(dir.path(), &["init", "-q", "-b", "main", "seed"]);
        write(&seed, ".gitignore", gitignore);
        write(&seed, "README.md", "code\n");
        git_in(&seed, &["add", "."]);
        git_in(&seed, &["commit", "-q", "-m", "Seed"]);
        git_in(
            &seed,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git_in(&seed, &["push", "-q", "-u", "origin", "main"]);
        Self { dir, remote }
    }

    fn clone(&self, name: &str) -> PathBuf {
        git_in(
            self.dir.path(),
            &["clone", "-q", self.remote.to_str().unwrap(), name],
        );
        let root = self.dir.path().join(name);
        git_in(&root, &["config", "user.email", "dev@example.invalid"]);
        git_in(&root, &["config", "user.name", "Dev"]);
        root
    }

    fn remote_log(&self) -> Vec<String> {
        git_in(&self.remote, &["log", "--format=%s", "refs/heads/main"])
            .lines()
            .map(str::to_string)
            .collect()
    }
}

fn subjects(root: &Path) -> Vec<String> {
    git_in(root, &["log", "--format=%s"])
        .lines()
        .map(str::to_string)
        .collect()
}

fn status(root: &Path) -> RegistrySyncStatus {
    MilestoneRegistry::for_checkout(root).sync_status().unwrap()
}

#[test]
fn hs2_9pyncn_a_write_commits_only_its_milestone_file_and_publishes_it() {
    let world = World::new(GITIGNORE);
    let root = world.clone("one");
    // Unrelated work: staged, unstaged, and untracked.
    write(&root, "README.md", "staged\n");
    git_in(&root, &["add", "README.md"]);
    write(&root, "README.md", "staged then edited\n");
    write(&root, "src.rs", "untracked\n");
    let status_before = git_in(&root, &["status", "--porcelain"]);
    let staged_before = git_in(&root, &["diff", "--cached"]);

    let registry = MilestoneRegistry::for_checkout(&root);
    let milestone = registry
        .create("Toolchain 2.0", None, Some("2026-12-04"))
        .unwrap();

    assert_eq!(subjects(&root)[0], "Create milestone Toolchain 2.0");
    assert_eq!(
        git_in(&root, &["show", "--name-only", "--format=", "HEAD"]),
        format!(".hotsheet2/milestones/{}.json", milestone.id)
    );
    assert_eq!(git_in(&root, &["status", "--porcelain"]), status_before);
    assert_eq!(git_in(&root, &["diff", "--cached"]), staged_before);
    // The lock and the status stay machine-local; only the entry is tracked.
    assert_eq!(
        git_in(&root, &["ls-files", ".hotsheet2"]),
        format!(".hotsheet2/milestones/{}.json", milestone.id)
    );
    assert!(root.join(MILESTONE_REGISTRY_LOCK).exists());
    let published = status(&root);
    assert_eq!(published.state, RegistrySyncState::Synced, "{published:?}");
    assert!(published.committed);
    assert_eq!(world.remote_log()[0], "Create milestone Toolchain 2.0");
    // The group registry's status is untouched by a milestone write.
    assert!(crate::groups::git_sync::read_status(&root).is_none());
}

#[test]
fn hs2_9pyncn_each_write_is_one_commit_and_repeats_commit_nothing() {
    let world = World::new(GITIGNORE);
    let root = world.clone("one");
    let registry = MilestoneRegistry::for_checkout(&root);
    let id = registry.create("Beta", None, None).unwrap().id;
    let due = |date: Option<&str>| MilestoneEdit {
        due_on: Some(date.map(str::to_string)),
        ..MilestoneEdit::default()
    };
    assert!(
        registry
            .edit(&id, &due(Some("2026-12-04")))
            .unwrap()
            .changed
    );
    assert!(
        !registry
            .edit(&id, &due(Some("2026-12-04")))
            .unwrap()
            .changed
    );
    assert!(registry.close(&id, CloseReason::Completed).unwrap().changed);
    assert!(!registry.close(&id, CloseReason::Completed).unwrap().changed);
    assert!(registry.reopen(&id).unwrap().changed);
    assert!(!registry.reopen(&id).unwrap().changed);
    assert!(registry.edit(&id, &due(None)).unwrap().changed);
    let rename = MilestoneEdit {
        title: Some("Beta 2".into()),
        description: Some(Some("Notes".into())),
        ..MilestoneEdit::default()
    };
    assert!(registry.edit(&id, &rename).unwrap().changed);
    registry.close(&id, CloseReason::NotPlanned).unwrap();
    let expected = vec![
        format!("Close milestone {id} as not planned"),
        format!("Edit milestone {id}"),
        format!("Clear due date of milestone {id}"),
        format!("Reopen milestone {id}"),
        format!("Close milestone {id}"),
        format!("Set due date of milestone {id} to 2026-12-04"),
        "Create milestone Beta".to_string(),
        "Seed".to_string(),
    ];
    assert_eq!(subjects(&root), expected);
    assert_eq!(world.remote_log(), expected);
    assert_eq!(status(&root).state, RegistrySyncState::Synced);
    // A sync pass with nothing pending commits nothing.
    let pass = sync_registry(RegistryKind::Milestones, &root);
    assert_eq!(pass.state, RegistrySyncState::Synced);
    assert!(!pass.committed);
}

#[test]
fn hs2_9pyncn_waiting_states_and_untracked_checkouts_follow_the_group_rules() {
    // A checkout whose ignore rules predate milestones does not commit them.
    let world = World::new("/.hotsheet2/*\n!/.hotsheet2/groups/\n");
    let root = world.clone("old");
    MilestoneRegistry::for_checkout(&root)
        .create("Ignored", None, None)
        .unwrap();
    assert_eq!(subjects(&root), vec!["Seed"]);
    assert_eq!(
        sync_registry(RegistryKind::Milestones, &root).detail,
        "Git ignores .hotsheet2/milestones/ in this checkout"
    );
    assert!(
        MilestoneRegistry::for_checkout(&root)
            .sync_status()
            .is_none()
    );

    // Detached HEAD waits, and the next pass commits on the branch.
    let world = World::new(GITIGNORE);
    let root = world.clone("one");
    let registry = MilestoneRegistry::for_checkout(&root);
    git_in(&root, &["checkout", "-q", "--detach"]);
    let id = registry.create("Detached", None, None).unwrap().id;
    let waiting = status(&root);
    assert_eq!(waiting.state, RegistrySyncState::Waiting, "{waiting:?}");
    assert!(waiting.detail.contains("detached"));
    assert_eq!(subjects(&root), vec!["Seed"]);
    git_in(&root, &["checkout", "-q", "main"]);
    let pass = sync_registry(RegistryKind::Milestones, &root);
    assert_eq!(pass.state, RegistrySyncState::Synced, "{pass:?}");
    assert!(pass.committed);
    assert_eq!(subjects(&root)[0], "Update milestone registry");
    assert_eq!(
        git_in(&root, &["show", "--name-only", "--format=", "HEAD"]),
        format!(".hotsheet2/milestones/{id}.json")
    );

    // No upstream keeps the commit local.
    git_in(&root, &["checkout", "-q", "-b", "local"]);
    registry.close(&id, CloseReason::Completed).unwrap();
    assert_eq!(status(&root).state, RegistrySyncState::LocalOnly);
    assert_eq!(subjects(&root)[0], format!("Close milestone {id}"));
}

#[test]
fn hs2_9pyncn_group_and_milestone_commits_publish_together() {
    let world = World::new(GITIGNORE);
    let root = world.clone("one");
    // Both registries changed while publishing was impossible (detached HEAD), then the
    // branch returns: each registry's pass commits its own files, and the pushes accept
    // the other registry's unpublished commit as registry-only.
    git_in(&root, &["checkout", "-q", "--detach"]);
    let (group, _, _) = crate::groups::GroupRegistry::for_checkout(&root)
        .find_or_create("Sprint")
        .unwrap();
    let milestone = MilestoneRegistry::for_checkout(&root)
        .create("Alpha", None, None)
        .unwrap();
    git_in(&root, &["checkout", "-q", "main"]);
    // Commit the group file by hand, without publishing it.
    git_in(&root, &["add", &format!(".hotsheet2/groups/{group}.json")]);
    git_in(
        &root,
        &[
            "commit",
            "-q",
            "--only",
            "-m",
            "Create group Sprint",
            "--",
            &format!(".hotsheet2/groups/{group}.json"),
        ],
    );
    let pass = sync_registry(RegistryKind::Milestones, &root);
    assert_eq!(pass.state, RegistrySyncState::Synced, "{pass:?}");
    assert_eq!(
        world.remote_log(),
        vec!["Update milestone registry", "Create group Sprint", "Seed"]
    );
    assert!(git_in(&root, &["ls-files", ".hotsheet2/milestones"]).contains(&milestone.id));
    // A code commit in the range still blocks a milestone push.
    write(&root, "code.rs", "code\n");
    git_in(&root, &["add", "code.rs"]);
    git_in(&root, &["commit", "-q", "-m", "Code"]);
    MilestoneRegistry::for_checkout(&root)
        .close(&milestone.id, CloseReason::Completed)
        .unwrap();
    let blocked = status(&root);
    assert_eq!(blocked.state, RegistrySyncState::Blocked);
    assert!(
        blocked
            .detail
            .starts_with("milestone registry commits are waiting behind other unpublished commits"),
        "{}",
        blocked.detail
    );
}

#[test]
fn hs2_9pyncn_two_clones_creating_different_milestones_merge_clean() {
    let world = World::new(GITIGNORE);
    let one = world.clone("one");
    let two = world.clone("two");
    let alpha = MilestoneRegistry::for_checkout(&one)
        .create("Alpha", None, None)
        .unwrap();
    // The second clone is behind: its commit stays local until the user pulls.
    let beta = MilestoneRegistry::for_checkout(&two)
        .create("Beta", None, None)
        .unwrap();
    assert_eq!(status(&two).state, RegistrySyncState::Blocked);
    assert!(git_ok(&two, &["pull", "-q", "--rebase"]));
    let pass = sync_registry(RegistryKind::Milestones, &two);
    assert_eq!(pass.state, RegistrySyncState::Synced, "{pass:?}");
    assert!(git_ok(&one, &["pull", "-q", "--ff-only"]));
    for root in [&one, &two] {
        let ids: Vec<String> = MilestoneRegistry::for_checkout(root)
            .list()
            .unwrap()
            .into_iter()
            .map(|milestone| milestone.id)
            .collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&alpha.id) && ids.contains(&beta.id));
    }
    // Both clones creating one title leave two ids, flagged duplicate on both sides.
    MilestoneRegistry::for_checkout(&one)
        .create("Gamma", None, None)
        .unwrap();
    // Before pulling, `two` does not know Gamma, so its create succeeds too.
    MilestoneRegistry::for_checkout(&two)
        .create("gamma", None, None)
        .unwrap();
    assert!(git_ok(&two, &["pull", "-q", "--rebase"]));
    let gammas: Vec<Milestone> = MilestoneRegistry::for_checkout(&two)
        .list()
        .unwrap()
        .into_iter()
        .filter(|milestone| milestone_title_key(&milestone.title) == "gamma")
        .collect();
    assert_eq!(gammas.len(), 2);
    assert!(gammas.iter().all(|milestone| milestone.duplicate));
}

#[test]
fn hs2_9pyncn_two_clones_editing_one_milestone_conflict_until_resolved() {
    let world = World::new(GITIGNORE);
    let one = world.clone("one");
    let id = MilestoneRegistry::for_checkout(&one)
        .create("Shared", None, None)
        .unwrap()
        .id;
    let two = world.clone("two");
    // Both clones change the same line of the same file before syncing.
    MilestoneRegistry::for_checkout(&one)
        .edit(
            &id,
            &MilestoneEdit {
                title: Some("Shared one".into()),
                ..MilestoneEdit::default()
            },
        )
        .unwrap();
    let two_registry = MilestoneRegistry::for_checkout(&two);
    two_registry
        .edit(
            &id,
            &MilestoneEdit {
                title: Some("Shared two".into()),
                ..MilestoneEdit::default()
            },
        )
        .unwrap();
    assert_eq!(status(&two).state, RegistrySyncState::Blocked);
    // The user merges and Git stops on the conflict.
    assert!(!git_ok(&two, &["pull", "-q", "--no-rebase", "--no-edit"]));
    let file = two.join(format!(".hotsheet2/milestones/{id}.json"));
    let conflicted_text = fs::read_to_string(&file).unwrap();
    assert!(conflicted_text.contains("<<<<<<<"));
    let read = two_registry.get(&id).unwrap().unwrap();
    assert_eq!(read.state, MilestoneState::Conflicted);
    assert_eq!(read.title, format!("Milestone {id}"));
    assert_eq!(
        read.conflict.unwrap().file,
        format!(".hotsheet2/milestones/{id}.json")
    );
    for error in [
        two_registry.close(&id, CloseReason::Completed).unwrap_err(),
        two_registry.reopen(&id).unwrap_err(),
        two_registry
            .edit(
                &id,
                &MilestoneEdit {
                    due_on: Some(Some("2027-01-01".into())),
                    ..MilestoneEdit::default()
                },
            )
            .unwrap_err(),
    ] {
        assert_eq!(error.code(), "milestone_registry_conflict");
    }
    assert_eq!(fs::read_to_string(&file).unwrap(), conflicted_text);
    // A sync pass mid merge waits and never commits the markers.
    let pass = sync_registry(RegistryKind::Milestones, &two);
    assert_eq!(pass.state, RegistrySyncState::Waiting, "{pass:?}");
    assert!(pass.detail.contains("merge"));
    // The person resolves it with Git; the milestone reads and writes again.
    fs::write(
        &file,
        "{\n  \"title\": \"Shared two\",\n  \"state\": \"open\"\n}\n",
    )
    .unwrap();
    git_in(&two, &["add", &format!(".hotsheet2/milestones/{id}.json")]);
    git_in(&two, &["commit", "-q", "--no-edit"]);
    assert_eq!(
        two_registry.get(&id).unwrap().unwrap().state,
        MilestoneState::Open
    );
    assert!(
        two_registry
            .close(&id, CloseReason::Completed)
            .unwrap()
            .changed
    );
    // The merge commit is the user's to publish, as for the group registry.
    let blocked = status(&two);
    assert_eq!(blocked.state, RegistrySyncState::Blocked, "{blocked:?}");
    git_in(&two, &["push", "-q"]);
    assert_eq!(
        sync_registry(RegistryKind::Milestones, &two).state,
        RegistrySyncState::Synced
    );
}
