//! HS2-9EGWC4: registry publication against temporary repositories and local bare remotes
//! only. Each test builds its own checkout; nothing touches the developer's repository.

use super::*;
use crate::groups::GroupRegistry;

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

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// A bare remote and one clone of it with a first commit on `main`, tracking
/// `origin/main`.
struct World {
    dir: tempfile::TempDir,
    remote: PathBuf,
}

impl World {
    fn new() -> (Self, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let remote = dir.path().join("remote.git");
        git_in(
            dir.path(),
            &["init", "-q", "--bare", "-b", "main", "remote.git"],
        );
        let world = Self { dir, remote };
        let seed = world.dir.path().join("seed");
        git_in(world.dir.path(), &["init", "-q", "-b", "main", "seed"]);
        write(&seed, ".gitignore", "/.hotsheet2/*\n!/.hotsheet2/groups/\n");
        write(&seed, "README.md", "code\n");
        git_in(&seed, &["add", "."]);
        git_in(&seed, &["commit", "-q", "-m", "Seed"]);
        git_in(
            &seed,
            &["remote", "add", "origin", world.remote.to_str().unwrap()],
        );
        git_in(&seed, &["push", "-q", "-u", "origin", "main"]);
        let clone = world.clone("one");
        (world, clone)
    }

    fn clone(&self, name: &str) -> PathBuf {
        git_in(
            self.dir.path(),
            &["clone", "-q", self.remote.to_str().unwrap(), name],
        );
        self.dir.path().join(name)
    }

    fn remote_head(&self) -> String {
        git_in(&self.remote, &["rev-parse", "refs/heads/main"])
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

fn ahead(root: &Path) -> usize {
    git_in(root, &["rev-list", "--count", "origin/main..HEAD"])
        .parse()
        .unwrap()
}

#[test]
fn hs2_9egwc4_a_write_commits_only_registry_paths_and_leaves_other_work_alone() {
    let (_world, root) = World::new();
    // Unrelated work: a staged edit, a different unstaged edit to the same file, an
    // unstaged edit, and an untracked file.
    write(&root, "README.md", "staged\n");
    git_in(&root, &["add", "README.md"]);
    write(&root, "README.md", "staged then edited\n");
    write(&root, "src.rs", "untracked\n");
    let status_before = git_in(&root, &["status", "--porcelain"]);
    let staged_before = git_in(&root, &["diff", "--cached"]);

    let (id, _, created) = GroupRegistry::for_checkout(&root)
        .find_or_create("Sprint")
        .unwrap();
    assert!(created);

    assert_eq!(subjects(&root)[0], "Create group Sprint");
    assert_eq!(
        git_in(&root, &["show", "--name-only", "--format=", "HEAD"]),
        format!(".hotsheet2/groups/{id}.json")
    );
    assert_eq!(git_in(&root, &["status", "--porcelain"]), status_before);
    assert_eq!(git_in(&root, &["diff", "--cached"]), staged_before);
    assert_eq!(
        fs::read_to_string(root.join("README.md")).unwrap(),
        "staged then edited\n"
    );
    // The lock and status files stay machine-local.
    assert!(git_in(&root, &["ls-files", ".hotsheet2"]).ends_with(&format!("{id}.json")));
    let status = read_status(&root).unwrap();
    assert_eq!(status.state, RegistrySyncState::Synced, "{status:?}");
    assert!(status.committed);
}

#[test]
fn hs2_9egwc4_write_commit_push_then_rename_then_noop_across_the_sequence() {
    let (world, root) = World::new();
    let registry = GroupRegistry::for_checkout(&root);
    let (id, _, _) = registry.find_or_create("Sprint").unwrap();
    // Pushed as a fast-forward of main.
    assert_eq!(world.remote_log()[0], "Create group Sprint");
    assert_eq!(ahead(&root), 0);
    // A rename is a second commit and push.
    registry.set_name(&id, "Release train").unwrap();
    assert_eq!(
        world.remote_log()[0],
        format!("Rename group {id} to Release train")
    );
    // Reusing an existing name and renaming to the same name commit nothing.
    let head = world.remote_head();
    assert!(!registry.find_or_create("release TRAIN").unwrap().2);
    registry.set_name(&id, "Release train").unwrap();
    assert_eq!(world.remote_head(), head);
    let status = read_status(&root).unwrap();
    assert_eq!(
        (status.state, status.committed),
        (RegistrySyncState::Synced, false)
    );
    // A migration commits once however many entries it creates.
    let spellings = [("a", "Alpha"), ("b", "Beta")]
        .into_iter()
        .map(|(key, name)| (key.to_string(), name.to_string()))
        .collect();
    registry.assign_ids(&spellings).unwrap();
    assert_eq!(
        world.remote_log()[0],
        "Migrate 2 group names to the group registry"
    );
    // Seed, create, rename, migrate.
    assert_eq!(subjects(&root).len(), 4);
}

#[test]
fn hs2_9egwc4_no_upstream_keeps_the_commit_local() {
    let (_world, root) = World::new();
    git_in(&root, &["checkout", "-q", "-b", "feature"]);
    GroupRegistry::for_checkout(&root)
        .find_or_create("Feature work")
        .unwrap();
    assert_eq!(subjects(&root)[0], "Create group Feature work");
    let status = read_status(&root).unwrap();
    assert_eq!(status.state, RegistrySyncState::LocalOnly, "{status:?}");
    assert!(status.committed);
}

#[test]
fn hs2_9egwc4_other_unpublished_commits_block_the_push() {
    let (world, root) = World::new();
    write(&root, "README.md", "my unpublished work\n");
    git_in(&root, &["commit", "-q", "-am", "Work in progress"]);
    let before = world.remote_head();
    GroupRegistry::for_checkout(&root)
        .find_or_create("Sprint")
        .unwrap();
    assert_eq!(
        world.remote_head(),
        before,
        "the user's commit was not pushed"
    );
    let status = read_status(&root).unwrap();
    assert_eq!(status.state, RegistrySyncState::Blocked, "{status:?}");
    assert!(status.state.needs_attention());
    // Once the user publishes their work, the next pass publishes the registry too.
    git_in(&root, &["push", "-q"]);
    assert_eq!(sync_checkout(&root).state, RegistrySyncState::Synced);
    assert_eq!(ahead(&root), 0);
}

#[test]
fn hs2_9egwc4_an_unpublished_merge_blocks_the_push() {
    let (world, root) = World::new();
    git_in(&root, &["checkout", "-q", "-b", "side"]);
    let registry = GroupRegistry::for_checkout(&root);
    registry.find_or_create("Side").unwrap();
    git_in(&root, &["checkout", "-q", "main"]);
    registry.find_or_create("Main").unwrap();
    let before = world.remote_head();
    // Pushed already; now merge the registry-only side branch.
    git_in(
        &root,
        &["merge", "-q", "--no-ff", "-m", "Merge side", "side"],
    );
    assert_eq!(sync_checkout(&root).state, RegistrySyncState::Blocked);
    assert_eq!(world.remote_head(), before);
}

#[test]
fn hs2_9egwc4_behind_or_moved_upstream_leaves_the_commit_local() {
    let (world, root) = World::new();
    let other = world.clone("two");
    write(&other, "lib.rs", "theirs\n");
    git_in(&other, &["add", "lib.rs"]);
    git_in(&other, &["commit", "-q", "-m", "Their code"]);
    git_in(&other, &["push", "-q"]);
    let theirs = world.remote_head();

    // Not fetched yet: the push itself is refused as non-fast-forward.
    GroupRegistry::for_checkout(&root)
        .find_or_create("Sprint")
        .unwrap();
    let status = read_status(&root).unwrap();
    assert_eq!(status.state, RegistrySyncState::Blocked, "{status:?}");
    assert!(status.detail.contains("upstream moved"), "{status:?}");
    assert_eq!(world.remote_head(), theirs);
    // After a fetch it is known to be behind, and nothing is pushed.
    let status = sync_checkout(&root);
    assert_eq!(status.state, RegistrySyncState::Blocked, "{status:?}");
    assert!(status.detail.contains("behind"), "{status:?}");
    assert_eq!(world.remote_head(), theirs);
    // The user's pull --rebase brings it back to a fast-forward.
    git_in(&root, &["pull", "-q", "--rebase"]);
    assert_eq!(sync_checkout(&root).state, RegistrySyncState::Synced);
    assert_eq!(world.remote_log()[0], "Create group Sprint");
}

#[test]
fn hs2_9egwc4_a_failing_hook_leaves_the_commit_local_and_never_skips_hooks() {
    let (world, root) = World::new();
    let hooks = root.join(".git/hooks");
    write(
        &hooks,
        "pre-push",
        "#!/bin/sh\necho 'gate refused this push' >&2\nexit 1\n",
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(hooks.join("pre-push"), fs::Permissions::from_mode(0o755)).unwrap();
    }
    let before = world.remote_head();
    GroupRegistry::for_checkout(&root)
        .find_or_create("Sprint")
        .unwrap();
    assert_eq!(subjects(&root)[0], "Create group Sprint");
    assert_eq!(world.remote_head(), before);
    let status = read_status(&root).unwrap();
    assert_eq!(status.state, RegistrySyncState::Failed, "{status:?}");
    assert!(
        status.detail.contains("gate refused this push"),
        "{status:?}"
    );
    // The next pass retries; once the hook passes, the commit is published.
    fs::remove_file(hooks.join("pre-push")).unwrap();
    assert_eq!(sync_checkout(&root).state, RegistrySyncState::Synced);
    assert_eq!(world.remote_log()[0], "Create group Sprint");
}

#[test]
fn hs2_9egwc4_a_failing_commit_hook_keeps_the_entry_and_retries() {
    let (world, root) = World::new();
    let other = world.clone("two");
    let hooks = root.join(".git/hooks");
    write(
        &hooks,
        "pre-commit",
        "#!/bin/sh\necho 'format check failed' >&2\nexit 1\n",
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(hooks.join("pre-commit"), fs::Permissions::from_mode(0o755)).unwrap();
    }
    let registry = GroupRegistry::for_checkout(&root);
    let (id, _, _) = registry.find_or_create("Sprint").unwrap();
    assert_eq!(registry.name(&id).unwrap().as_deref(), Some("Sprint"));
    assert_eq!(subjects(&root), ["Seed"]);
    let status = read_status(&root).unwrap();
    assert_eq!(status.state, RegistrySyncState::Failed, "{status:?}");
    assert!(status.detail.contains("format check failed"), "{status:?}");
    // A pass while the hook still refuses still shows groups other clones published.
    let (theirs, _, _) = GroupRegistry::for_checkout(&other)
        .find_or_create("Elsewhere")
        .unwrap();
    assert_eq!(sync_checkout(&root).state, RegistrySyncState::Failed);
    assert_eq!(
        registry.name(&theirs).unwrap().as_deref(),
        Some("Elsewhere")
    );
    // Hook fixed and the user pulled: the retry commits and publishes.
    fs::remove_file(hooks.join("pre-commit")).unwrap();
    git_in(&root, &["pull", "-q", "--ff-only"]);
    let status = sync_checkout(&root);
    assert_eq!(
        (status.state, status.committed),
        (RegistrySyncState::Synced, true)
    );
    assert_eq!(subjects(&root)[0], PENDING_COMMIT_MESSAGE);
}

#[test]
fn hs2_9egwc4_busy_git_states_wait_and_the_next_pass_commits() {
    let (world, root) = World::new();
    let registry = GroupRegistry::for_checkout(&root);
    let git_dir = PathBuf::from(git_in(&root, &["rev-parse", "--absolute-git-dir"]));
    for (marker, what) in [
        ("MERGE_HEAD", "merge"),
        ("rebase-merge", "rebase"),
        ("rebase-apply", "rebase"),
        ("CHERRY_PICK_HEAD", "cherry-pick"),
        ("REVERT_HEAD", "revert"),
    ] {
        let path = git_dir.join(marker);
        if marker.starts_with("rebase") {
            fs::create_dir(&path).unwrap();
        } else {
            fs::write(&path, format!("{}\n", world.remote_head())).unwrap();
        }
        registry
            .find_or_create(&format!("During {marker}"))
            .unwrap();
        let status = read_status(&root).unwrap();
        assert_eq!(status.state, RegistrySyncState::Waiting, "{marker}");
        assert!(status.detail.contains(what), "{status:?}");
        assert_eq!(subjects(&root), ["Seed"], "{marker}");
        if path.is_dir() {
            fs::remove_dir(&path).unwrap();
        } else {
            fs::remove_file(&path).unwrap();
        }
    }
    // Detached HEAD waits too.
    git_in(&root, &["checkout", "-q", "--detach"]);
    registry.find_or_create("Detached").unwrap();
    assert_eq!(
        read_status(&root).unwrap().state,
        RegistrySyncState::Waiting
    );
    git_in(&root, &["checkout", "-q", "main"]);
    // Every write made while busy goes out in one retry commit.
    let status = sync_checkout(&root);
    assert_eq!(
        (status.state, status.committed),
        (RegistrySyncState::Synced, true)
    );
    assert_eq!(
        git_in(&root, &["show", "--name-only", "--format=", "HEAD"])
            .lines()
            .count(),
        6
    );
    assert_eq!(world.remote_log()[0], PENDING_COMMIT_MESSAGE);
}

#[test]
fn hs2_9egwc4_non_git_and_ignored_checkouts_are_untracked() {
    let dir = tempfile::tempdir().unwrap();
    let registry = GroupRegistry::for_checkout(dir.path());
    registry.find_or_create("Loose").unwrap();
    assert_eq!(
        read_status(dir.path()),
        None,
        "no status for an unpublished registry"
    );
    assert_eq!(
        publish(dir.path(), None).state,
        RegistrySyncState::Untracked
    );
    assert!(!RegistrySyncState::Untracked.needs_attention());

    let (_world, root) = World::new();
    // A status left from when the registry was published is dropped once it is ignored.
    GroupRegistry::for_checkout(&root)
        .find_or_create("Published")
        .unwrap();
    assert!(read_status(&root).is_some());
    write(&root, ".gitignore", "/.hotsheet2/\n");
    git_in(
        &root,
        &["commit", "-q", "-am", "Ignore everything Hot Sheet"],
    );
    GroupRegistry::for_checkout(&root)
        .find_or_create("Hidden")
        .unwrap();
    assert_eq!(read_status(&root), None);
    let status = sync_checkout(&root);
    assert_eq!(status.state, RegistrySyncState::Untracked, "{status:?}");
    assert!(status.detail.contains("ignores"), "{status:?}");
    assert_eq!(subjects(&root)[0], "Ignore everything Hot Sheet");
}

/// HS2-1CWD0H: a listener hears each change of recorded state once, from a write and from
/// sync passes, and never a repeat of the same state; dropping its guard stops it.
#[test]
fn hs2_1cwd0h_listeners_hear_each_state_change_once_and_not_repeats() {
    let (_world, root) = World::new();
    let heard: Arc<Mutex<Vec<Option<RegistrySyncState>>>> = Arc::default();
    let listener = {
        let heard = Arc::clone(&heard);
        let root = root.clone();
        // Other tests record statuses concurrently in this process; keep this checkout's.
        on_status_change(move |changed, state| {
            if changed == root {
                heard.lock().unwrap().push(state);
            }
        })
    };
    let take = || std::mem::take(&mut *heard.lock().unwrap());

    let hooks = root.join(".git/hooks");
    write(
        &hooks,
        "pre-push",
        "#!/bin/sh\necho 'gate refused this push' >&2\nexit 1\n",
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(hooks.join("pre-push"), fs::Permissions::from_mode(0o755)).unwrap();
    }
    let registry = GroupRegistry::for_checkout(&root);
    registry.find_or_create("Sprint").unwrap();
    assert_eq!(take(), [Some(RegistrySyncState::Failed)]);
    // Failing again, on a pass and on another write, is no change.
    assert_eq!(sync_checkout(&root).state, RegistrySyncState::Failed);
    registry.find_or_create("Later").unwrap();
    assert_eq!(take(), []);
    // A recovery by a sync pass is a change; a further synced pass is not.
    fs::remove_file(hooks.join("pre-push")).unwrap();
    assert_eq!(sync_checkout(&root).state, RegistrySyncState::Synced);
    assert_eq!(sync_checkout(&root).state, RegistrySyncState::Synced);
    assert_eq!(take(), [Some(RegistrySyncState::Synced)]);
    // Once the registry is ignored its status disappears, which is a change too.
    write(&root, ".gitignore", "/.hotsheet2/\n");
    git_in(&root, &["commit", "-q", "-am", "Ignore Hot Sheet"]);
    assert_eq!(sync_checkout(&root).state, RegistrySyncState::Untracked);
    assert_eq!(sync_checkout(&root).state, RegistrySyncState::Untracked);
    assert_eq!(take(), [None]);

    // After the guard drops, a change is not heard.
    drop(listener);
    write(&root, ".gitignore", "/.hotsheet2/*\n!/.hotsheet2/groups/\n");
    git_in(&root, &["commit", "-q", "-am", "Publish groups again"]);
    assert_ne!(sync_checkout(&root).state, RegistrySyncState::Untracked);
    assert!(read_status(&root).is_some());
    assert_eq!(take(), []);
}

#[test]
fn hs2_1cwd0h_state_names_match_their_serialized_form() {
    use RegistrySyncState::{Blocked, Failed, LocalOnly, Synced, Untracked, Waiting};
    for state in [Synced, LocalOnly, Waiting, Blocked, Failed, Untracked] {
        assert_eq!(
            serde_json::to_value(state).unwrap(),
            serde_json::Value::String(state.as_str().into())
        );
    }
}

#[test]
fn hs2_9egwc4_a_burst_of_background_writes_is_one_commit() {
    assert_eq!(coalesced_message(&[]), PENDING_COMMIT_MESSAGE);
    assert_eq!(coalesced_message(&["one".into()]), "one");
    assert_eq!(
        coalesced_message(&["one".into(), "two".into(), "three".into()]),
        "one (+2 more)"
    );
    let (world, root) = World::new();
    // A private committer, so the process-wide mode of other tests is untouched.
    let background: &'static Background = Box::leak(Box::new(Background {
        state: Mutex::new(BackgroundState::default()),
        changed: Condvar::new(),
    }));
    std::thread::spawn(move || background.work());
    // Write three entries directly, announcing each like the registry does.
    for name in ["Alpha", "Beta", "Gamma"] {
        let id = hotsheet_model::new_group_id();
        write(
            &root,
            &format!(".hotsheet2/groups/{id}.json"),
            &format!("{{\n  \"name\": \"{name}\"\n}}\n"),
        );
        background.submit(&root, &format!("Create group {name}"));
    }
    background.flush();
    assert_eq!(subjects(&root)[0], "Create group Alpha (+2 more)");
    assert_eq!(subjects(&root).len(), 2);
    assert_eq!(world.remote_log()[0], "Create group Alpha (+2 more)");
    // An empty flush returns at once, and a later burst is its own commit.
    background.flush();
    let id = hotsheet_model::new_group_id();
    write(
        &root,
        &format!(".hotsheet2/groups/{id}.json"),
        "{\"name\":\"Delta\"}\n",
    );
    background.submit(&root, "Create group Delta");
    background.flush();
    assert_eq!(subjects(&root)[0], "Create group Delta");
}

#[test]
fn hs2_9egwc4_upstream_changes_arrive_through_the_overlay_and_local_edits_win() {
    let (world, mine) = World::new();
    let theirs = world.clone("two");
    let shared = GroupRegistry::for_checkout(&mine)
        .find_or_create("Shared")
        .unwrap()
        .0;
    git_in(&theirs, &["pull", "-q"]);
    // Elsewhere: a new group, and a rename of the shared one.
    let their_registry = GroupRegistry::for_checkout(&theirs);
    let (created, _, _) = their_registry.find_or_create("From elsewhere").unwrap();
    their_registry.set_name(&shared, "Shared renamed").unwrap();
    assert_eq!(
        world.remote_log()[0],
        format!("Rename group {shared} to Shared renamed")
    );

    // Before a sync pass this checkout knows neither change.
    let registry = GroupRegistry::for_checkout(&mine);
    assert_eq!(registry.name(&created).unwrap(), None);
    // A pass fetches and shows both, without moving the branch or the working tree.
    let head = git_in(&mine, &["rev-parse", "HEAD"]);
    assert_eq!(sync_checkout(&mine).state, RegistrySyncState::Synced);
    assert_eq!(git_in(&mine, &["rev-parse", "HEAD"]), head);
    assert_eq!(git_in(&mine, &["status", "--porcelain"]), "");
    let names = registry.names().unwrap();
    assert_eq!(names[&created], "From elsewhere");
    assert_eq!(names[&shared], "Shared renamed");
    assert_eq!(
        registry.name(&created).unwrap().as_deref(),
        Some("From elsewhere")
    );
    // Find-or-create sees the upstream group instead of making a duplicate.
    assert_eq!(
        registry.find_or_create("from ELSEWHERE").unwrap(),
        (created.clone(), "From elsewhere".into(), false)
    );

    // A local rename wins over the overlay at once, and is committed (locally: the branch
    // is behind, so it waits for the user's pull).
    registry.set_name(&shared, "Ours").unwrap();
    assert_eq!(registry.names().unwrap()[&shared], "Ours");
    assert_eq!(
        read_status(&mine).unwrap().state,
        RegistrySyncState::Blocked
    );
    // A later pass does not resurrect the upstream name over the committed local change.
    sync_checkout(&mine);
    assert_eq!(registry.names().unwrap()[&shared], "Ours");
    assert_eq!(registry.names().unwrap()[&created], "From elsewhere");

    // The user's pull brings the files; the overlay empties and the push goes out.
    git_in(
        &mine,
        &[
            "-c",
            "core.editor=true",
            "pull",
            "-q",
            "--no-rebase",
            "-X",
            "ours",
        ],
    );
    let status = sync_checkout(&mine);
    assert_eq!(
        status.state,
        RegistrySyncState::Blocked,
        "a merge is never auto-pushed"
    );
    assert!(!mine.join(GROUP_REMOTE_OVERLAY_FILE).exists());
    assert_eq!(registry.names().unwrap()[&created], "From elsewhere");
    assert_eq!(registry.names().unwrap()[&shared], "Ours");
}

#[test]
fn hs2_9egwc4_a_fresh_clone_sees_upstream_groups_and_then_its_own_pull_clears_the_overlay() {
    let (world, origin) = World::new();
    let fresh = world.clone("fresh");
    let (id, _, _) = GroupRegistry::for_checkout(&origin)
        .find_or_create("Arrived")
        .unwrap();
    // The fresh clone has no registry directory at all yet.
    fs::create_dir_all(fresh.join(".hotsheet2")).unwrap();
    let registry = GroupRegistry::for_checkout(&fresh);
    assert!(registry.names().unwrap().is_empty());
    sync_checkout(&fresh);
    assert_eq!(registry.names().unwrap()[&id], "Arrived");
    git_in(&fresh, &["pull", "-q", "--ff-only"]);
    assert_eq!(sync_checkout(&fresh).state, RegistrySyncState::Synced);
    assert!(!fresh.join(GROUP_REMOTE_OVERLAY_FILE).exists());
    assert_eq!(registry.names().unwrap()[&id], "Arrived");
    // An overlay entry for an id with a hand-made local file does not apply once the file
    // differs from what the overlay recorded.
    write(
        &fresh,
        GROUP_REMOTE_OVERLAY_FILE,
        &format!("{{\"entries\":{{\"{id}\":{{\"name\":\"Stale\",\"local\":null}}}}}}"),
    );
    assert_eq!(registry.names().unwrap()[&id], "Arrived");
    // A corrupt overlay is ignored.
    write(&fresh, GROUP_REMOTE_OVERLAY_FILE, "{ not json");
    assert_eq!(registry.names().unwrap()[&id], "Arrived");
}

// ---- HS2-NM6E27: concurrent Git work in one repository ----------------------------------

/// Write a new registry entry file directly, as the registry does before it publishes.
fn write_entry(root: &Path, name: &str) {
    let id = hotsheet_model::new_group_id();
    write(
        root,
        &format!(".hotsheet2/groups/{id}.json"),
        &format!("{{\n  \"name\": \"{name}\"\n}}\n"),
    );
}

/// A pre-commit hook that counts its runs in `counter` and refuses every commit.
fn refusing_hook(root: &Path, counter: &Path) {
    let hooks = root.join(".git/hooks");
    write(
        &hooks,
        "pre-commit",
        &format!(
            "#!/bin/sh\necho ran >> '{}'\necho 'synthetic hook refused the commit' >&2\nexit 1\n",
            counter.display()
        ),
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(hooks.join("pre-commit"), fs::Permissions::from_mode(0o755)).unwrap();
    }
}

/// Publishes, sync passes, and Hot Sheet's own status and diff reads overlap in one
/// checkout; every publication succeeds and the remote ends with every entry.
#[test]
fn hs2_nm6e27_overlapping_publish_sync_and_status_reads_all_succeed() {
    use std::sync::atomic::AtomicBool;
    let (world, root) = World::new();
    let done = Arc::new(AtomicBool::new(false));
    let syncer = {
        let (root, done) = (root.clone(), Arc::clone(&done));
        std::thread::spawn(move || {
            let mut statuses = Vec::new();
            while !done.load(Ordering::Relaxed) {
                statuses.push(sync_checkout(&root));
            }
            statuses
        })
    };
    let reader = {
        let (root, done) = (root.clone(), Arc::clone(&done));
        std::thread::spawn(move || {
            let mut reads = 0_u32;
            while !done.load(Ordering::Relaxed) {
                crate::repository_status::snapshot(&root).unwrap();
                let diff = crate::git::command()
                    .arg("-C")
                    .arg(&root)
                    .args(["diff", "--stat"])
                    .output()
                    .unwrap();
                assert!(diff.status.success());
                reads += 1;
            }
            reads
        })
    };
    let mut published = Vec::new();
    for round in 0..12 {
        write_entry(&root, &format!("Group {round}"));
        published.push(publish(&root, Some(&format!("Create group {round}"))));
    }
    done.store(true, Ordering::Relaxed);
    let synced = syncer.join().unwrap();
    assert!(reader.join().unwrap() > 0);
    for status in published.iter().chain(&synced) {
        assert_eq!(status.state, RegistrySyncState::Synced, "{status:?}");
    }
    // A sync pass may commit an entry before its own publish does; together they commit
    // every entry, each once.
    let commits = published
        .iter()
        .chain(&synced)
        .filter(|s| s.committed)
        .count();
    assert_eq!(commits, 12);
    let last = sync_checkout(&root);
    assert_eq!(last.state, RegistrySyncState::Synced, "{last:?}");
    assert_eq!(ahead(&root), 0);
    assert_eq!(world.remote_log().len(), 13);
    assert_eq!(git_in(&root, &["status", "--porcelain", ".hotsheet2"]), "");
}

/// A fresh `index.lock` another program holds is waited out, and the commit lands once it
/// is released.
#[test]
fn hs2_nm6e27_a_live_index_lock_is_waited_out() {
    let (world, root) = World::new();
    let lock = root.join(".git/index.lock");
    fs::write(&lock, "").unwrap();
    let releaser = {
        let lock = lock.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            fs::remove_file(lock).unwrap();
        })
    };
    write_entry(&root, "Sprint");
    let status = publish(&root, Some("Create group Sprint"));
    releaser.join().unwrap();
    assert_eq!(
        (status.state, status.committed),
        (RegistrySyncState::Synced, true),
        "{status:?}"
    );
    assert_eq!(world.remote_log()[0], "Create group Sprint");
}

/// A stale `index.lock` (a crashed Git) is reported at once with its age and never
/// removed; once the user removes it, the next pass commits.
#[test]
fn hs2_nm6e27_a_stale_index_lock_is_reported_at_once_and_kept() {
    let (_world, root) = World::new();
    let lock = root.join(".git/index.lock");
    fs::File::create(&lock)
        .unwrap()
        .set_modified(SystemTime::now() - Duration::from_secs(2 * 60 * 60))
        .unwrap();
    write_entry(&root, "Sprint");
    let started = Instant::now();
    let status = publish(&root, Some("Create group Sprint"));
    assert!(started.elapsed() < Duration::from_secs(2), "no retries");
    assert_eq!(status.state, RegistrySyncState::Failed, "{status:?}");
    assert!(status.detail.contains("index.lock"), "{status:?}");
    assert!(status.detail.contains("120 minutes old"), "{status:?}");
    assert!(lock.exists(), "a lock is never removed");
    assert_eq!(subjects(&root), ["Seed"]);
    fs::remove_file(&lock).unwrap();
    let status = sync_checkout(&root);
    assert_eq!(
        (status.state, status.committed),
        (RegistrySyncState::Synced, true)
    );
}

/// A refusing hook is reported with its own message after one attempt, even while a
/// concurrent reader runs, and is never retried as if it were lock contention.
#[test]
fn hs2_nm6e27_a_refusing_hook_reports_its_message_without_retrying() {
    let (_world, root) = World::new();
    let counter = root.join(".git/hook-runs");
    refusing_hook(&root, &counter);
    write_entry(&root, "Sprint");
    let reader = {
        let root = root.clone();
        std::thread::spawn(move || {
            for _ in 0..20 {
                crate::repository_status::snapshot(&root).unwrap();
            }
        })
    };
    let status = publish(&root, Some("Create group Sprint"));
    reader.join().unwrap();
    assert_eq!(status.state, RegistrySyncState::Failed, "{status:?}");
    assert!(
        status
            .detail
            .starts_with("could not commit the group registry: "),
        "{status:?}"
    );
    assert!(
        status.detail.contains("synthetic hook refused the commit"),
        "{status:?}"
    );
    assert!(!status.detail.contains("index.lock"), "{status:?}");
    assert_eq!(fs::read_to_string(&counter).unwrap().lines().count(), 1);
}

/// The retry policy across its transitions: a released lock is retried with doubling,
/// bounded waits; a lock still held after every retry, a failure without a lock, and a
/// stale lock each return without further attempts.
#[test]
fn hs2_nm6e27_retry_policy_transitions() {
    let failure = |locks: Vec<PathBuf>| CommitFailure {
        status: RegistrySyncStatus::new(RegistrySyncState::Failed, "could not commit"),
        locks,
    };
    let gone = PathBuf::from("/nonexistent/hs2-nm6e27/index.lock");
    // Contended twice, then committed.
    let mut attempts = 0;
    let mut waits = Vec::new();
    let result = retry_live_lock(
        || {
            attempts += 1;
            if attempts < 3 {
                Err(failure(vec![gone.clone()]))
            } else {
                Ok(true)
            }
        },
        |delay| waits.push(delay),
        SystemTime::now,
    );
    assert!(result.unwrap());
    assert_eq!(
        waits,
        [Duration::from_millis(25), Duration::from_millis(50)]
    );
    // Held through every retry: bounded, capped waits, then the last failure.
    let mut attempts = 0;
    let mut waits = Vec::new();
    let result = retry_live_lock(
        || {
            attempts += 1;
            Err(failure(vec![gone.clone()]))
        },
        |delay| waits.push(delay),
        SystemTime::now,
    );
    assert_eq!(result.unwrap_err().detail, "could not commit");
    assert_eq!(attempts, LOCK_RETRIES + 1);
    assert_eq!(waits.len(), LOCK_RETRIES);
    assert!(waits.iter().all(|delay| *delay <= LOCK_RETRY_MAX_DELAY));
    assert_eq!(waits.last(), Some(&LOCK_RETRY_MAX_DELAY));
    // No lock named (a hook refusal): one attempt, no wait.
    let mut attempts = 0;
    let result = retry_live_lock(
        || {
            attempts += 1;
            Err(failure(Vec::new()))
        },
        |_| panic!("a refusal is never retried"),
        SystemTime::now,
    );
    assert!(result.is_err());
    assert_eq!(attempts, 1);
    // A stale lock: one attempt, no wait, and its age in the detail.
    let dir = tempfile::tempdir().unwrap();
    let stale = dir.path().join("index.lock");
    fs::write(&stale, "").unwrap();
    let later = SystemTime::now() + STALE_LOCK_AGE + Duration::from_secs(5 * 60);
    let mut attempts = 0;
    let result = retry_live_lock(
        || {
            attempts += 1;
            Err(failure(vec![stale.clone()]))
        },
        |_| panic!("a stale lock is never retried"),
        move || later,
    );
    let detail = result.unwrap_err().detail;
    assert_eq!(attempts, 1);
    assert!(detail.contains("6 minutes old"), "{detail}");
    // The same file while it is still young is retried.
    let mut attempts = 0;
    let result = retry_live_lock(
        || {
            attempts += 1;
            if attempts == 1 {
                Err(failure(vec![stale.clone()]))
            } else {
                Ok(false)
            }
        },
        |_| {},
        SystemTime::now,
    );
    assert!(!result.unwrap());
    assert_eq!(attempts, 2);
}
